//! The effects playing: started and ended by what the game shows, stepped each frame, their
//! cards gathered by look into the frame the renderer draws.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::draw::{Eye, Quad, draw, from_world};
use super::eval::HeightAt;
use super::library::{Effect, Library};
use super::render::{Batch, ParticleFrame};
use super::sim::Instance;
use crate::camera::RtsCamera;
use crate::manifest::Manifest;

/// Most effects playing at once; past it a new one is not started.
const MAX_PLAYING: usize = 2048;
/// Ended effects kept to be played again without allocating.
const MAX_SPARE: usize = 256;

/// A playing effect, by its slot and the slot's generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    slot: usize,
    generation: u32,
}

/// The effects playing.
#[derive(Resource, Default)]
pub struct Playing {
    slots: Vec<(u32, Option<Instance>)>,
    free: Vec<usize>,
    spare: Vec<Instance>,
    /// Each look's cards this frame, kept for their capacity.
    lists: Vec<Vec<Quad>>,
}

impl Playing {
    /// Starts `effect`, its draws keyed by `seed`.
    pub fn start(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        let instance = match self
            .spare
            .iter()
            .position(|i| Arc::ptr_eq(&i.effect, effect))
        {
            Some(at) => {
                let mut instance = self.spare.swap_remove(at);
                instance.restart(seed);
                instance
            }
            None => Instance::new(Arc::clone(effect), seed),
        };
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None if self.slots.len() < MAX_PLAYING => {
                self.slots.push((0, None));
                self.slots.len() - 1
            }
            None => return None,
        };
        self.slots[slot].1 = Some(instance);
        Some(Handle {
            slot,
            generation: self.slots[slot].0,
        })
    }

    /// The effect of `handle`, while it plays.
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Instance> {
        let (generation, instance) = self.slots.get_mut(handle.slot)?;
        if *generation != handle.generation {
            return None;
        }
        instance.as_mut()
    }

    fn advance(&mut self, dt: f32, ground: Option<HeightAt<'_>>) {
        for slot in 0..self.slots.len() {
            let (generation, entry) = &mut self.slots[slot];
            let Some(instance) = entry else {
                continue;
            };
            instance.advance(dt, ground);
            if instance.finished() {
                let instance = entry.take();
                *generation = generation.wrapping_add(1);
                self.free.push(slot);
                if let Some(instance) = instance
                    && self.spare.len() < MAX_SPARE
                {
                    self.spare.push(instance);
                }
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Playing>()
        .add_systems(Startup, load)
        .add_systems(PostUpdate, tick.after(TransformSystems::Propagate));
}

/// Reads every particle file the manifest lists; what does not read is left out, said once.
fn load(mut commands: Commands, manifest: Res<Manifest>, server: Res<AssetServer>) {
    let mut library = Library::default();
    let mut unsupported = BTreeSet::new();
    for (logical, path) in manifest.particles() {
        let Some(bytes) = manifest
            .root
            .as_deref()
            .and_then(|root| crate::cachefs::read_small(root, path))
        else {
            continue;
        };
        match library.add(logical, &bytes, &server) {
            Ok(classes) => unsupported.extend(classes),
            Err(why) => error!("particle effect {logical} ({path}): {why}; drawn without it"),
        }
    }
    if !unsupported.is_empty() {
        let list: Vec<String> = unsupported.into_iter().collect();
        warn!(
            "particle classes not simulated, left out of their systems: {}",
            list.join(", ")
        );
    }
    commands.insert_resource(library);
}

/// Steps every effect and gathers the frame's cards.
#[allow(clippy::too_many_arguments)]
fn tick(
    time: Res<Time>,
    session: NonSend<crate::session::Session>,
    mut playing: ResMut<Playing>,
    library: Option<Res<Library>>,
    camera: Query<&GlobalTransform, With<RtsCamera>>,
    ground: Res<crate::map::Ground>,
    mut frame: ResMut<ParticleFrame>,
) {
    let height = |x: f32, y: f32| ground.surface_at(x, y);
    let height: Option<HeightAt<'_>> = ground.known().then_some(&height);
    // Effects keep the game's pace: a replay held or sped up holds or speeds them.
    let pace = match &session.0.source {
        bota_client_core::Source::Replay(player) if player.paused => 0.0,
        bota_client_core::Source::Replay(player) => player.speed,
        _ => 1.0,
    };
    playing.advance(time.delta_secs() * pace, height);
    let frame = &mut *frame;
    frame.quads.clear();
    frame.batches.clear();
    let (Some(library), Ok(camera)) = (library, camera.single()) else {
        return;
    };
    let eye = Eye {
        position: from_world(camera.translation()),
        right: from_world(camera.right().as_vec3()),
        up: from_world(camera.up().as_vec3()),
    };
    let playing = &mut *playing;
    playing.lists.resize_with(library.looks.len(), Vec::new);
    for list in &mut playing.lists {
        list.clear();
    }
    for instance in playing.slots.iter().filter_map(|(_, i)| i.as_ref()) {
        draw(instance, &eye, height, &mut playing.lists);
    }
    for (look, list) in playing.lists.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let start = frame.quads.len() as u32;
        frame.quads.extend_from_slice(list);
        let sum: Vec3 = list.iter().map(|q| Vec3::from(q.head)).sum();
        frame.batches.push(Batch {
            look: look as u16,
            range: start..frame.quads.len() as u32,
            center: sum / list.len() as f32,
        });
    }
}
