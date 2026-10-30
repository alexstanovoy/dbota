//! The effects playing: started and ended by what the game shows, stepped each frame, their
//! cards gathered by look into the frame the renderer draws.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::draw::{Eye, Quad, draw, from_world, turn_to_screen};
use super::eval::HeightAt;
use super::library::{Effect, Library};
use super::lights::{Light, gather};
use super::models::{self, Placed};
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
    /// The lights the effects ask for this frame, kept for their capacity.
    pub(super) lights: Vec<Light>,
    /// The particles drawn as models this frame, kept for their capacity.
    pub(super) placed: Vec<Placed>,
    /// The world's clock, in seconds of the game's pace: what noise over the world's time reads.
    clock: f32,
    /// How many times each effect was started, for the end-to-end tests to ask what the game
    /// drew.
    started: Vec<(Arc<Effect>, u32)>,
}

impl Playing {
    /// How many times `effect` was started.
    pub fn started(&self, effect: &Arc<Effect>) -> u32 {
        self.started
            .iter()
            .find(|(e, _)| Arc::ptr_eq(e, effect))
            .map_or(0, |(_, n)| *n)
    }

    /// Starts `effect`, its draws keyed by `seed`.
    pub fn start(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        let mut instance = match self
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
        instance.world_time = self.clock;
        match self
            .started
            .iter_mut()
            .find(|(e, _)| Arc::ptr_eq(e, effect))
        {
            Some((_, n)) => *n = n.saturating_add(1),
            None => self.started.push((Arc::clone(effect), 1)),
        }
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

    /// Starts `effect` held on until it is ended (`Instance::end`), however long that is: what
    /// a unit wears or the map places.
    pub fn hold(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        let handle = self.start(effect, seed)?;
        self.get_mut(handle)?.hold();
        Some(handle)
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
        self.clock += dt;
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

/// The making of the effects' library, the frame the first match connects: what adds effects
/// of its own to it reads after.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Loaded;

pub fn plugin(app: &mut App) {
    app.init_resource::<Playing>()
        .add_systems(Update, load.run_if(crate::loading::requested).in_set(Loaded))
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

/// Steps every effect and gathers the frame's cards and lights.
#[allow(clippy::too_many_arguments)]
pub(super) fn tick(
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
    playing.lights.clear();
    playing.placed.clear();
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
    for instance in playing.slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
        turn_to_screen(instance, &eye);
    }
    for instance in playing.slots.iter().filter_map(|(_, i)| i.as_ref()) {
        draw(instance, &eye, height, &mut playing.lists);
        gather(instance, &mut playing.lights);
        models::gather(instance, &mut playing.placed);
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
