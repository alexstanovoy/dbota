//! The effects playing: started and ended by what the game shows, stepped each frame, their
//! cards gathered by look into the frame the renderer draws.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::camera::primitives::{Aabb, Frustum};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;
use bevy::transform::TransformSystems;

use super::draw::{Eye, Quad, draw, from_world, turn_to_screen};
use super::eval::HeightAt;
use super::library::{Effect, Library};
use super::lights::{Light, gather};
use super::models::{self, Placed};
use super::render::{Batch, ParticleFrame};
use super::sim::Instance;
use super::store;
use crate::camera::RtsCamera;
use crate::manifest::Manifest;

/// Most effects playing at once; past it a new one is not started.
const MAX_PLAYING: usize = 2048;
/// Ended effects kept to be played again without allocating.
const MAX_SPARE: usize = 256;
/// Effects playing from which they are stepped on the compute threads together: fewer step
/// sooner than the threads would be handed them.
const PARALLEL_FROM: usize = 4;

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

    /// The playing instances of `effect`.
    #[cfg(test)]
    pub fn instances_of<'a>(
        &'a self,
        effect: &'a Arc<Effect>,
    ) -> impl Iterator<Item = &'a Instance> {
        self.slots
            .iter()
            .filter_map(|(_, i)| i.as_ref())
            .filter(move |i| Arc::ptr_eq(&i.effect, effect))
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

    /// Steps every effect by `dt`, on the compute threads when there are enough of them that
    /// the effects take turns on no single thread (each effect steps alone: the order they step
    /// in changes nothing), and retires the ones that are finished.
    fn advance(&mut self, dt: f32, ground: Option<HeightAt<'_>>) {
        self.clock += dt;
        let pool = ComputeTaskPool::get();
        let playing = self.slots.iter().filter(|(_, i)| i.is_some()).count();
        if playing >= PARALLEL_FROM && pool.thread_num() > 1 {
            // Several pieces a thread, so one heavy effect does not hold the rest up.
            let piece = self.slots.len().div_ceil(pool.thread_num() * 4).max(1);
            pool.scope(|scope| {
                for slots in self.slots.chunks_mut(piece) {
                    scope.spawn(async move {
                        for instance in slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
                            instance.advance(dt, ground);
                        }
                    });
                }
            });
        } else {
            for instance in self.slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
                instance.advance(dt, ground);
            }
        }
        for slot in 0..self.slots.len() {
            let (generation, entry) = &mut self.slots[slot];
            let Some(instance) = entry else {
                continue;
            };
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
        .add_systems(
            Update,
            load.run_if(crate::loading::requested).in_set(Loaded),
        )
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

/// Whether any of `instance`'s particles may be in `frustum`'s view: the box about them all,
/// padded by its definition's bounding box as Source pads a system's bounds (about its first
/// control point while it has none).
fn in_view(instance: &Instance, frustum: &Frustum) -> bool {
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for node in &instance.nodes {
        for p in &node.parts.vectors[store::POSITION][..node.parts.len()] {
            low = low.min(*p);
            high = high.max(*p);
        }
    }
    if low.x > high.x {
        let Some(first) = instance.nodes.first() else {
            return false;
        };
        low = first.control_points[0].position;
        high = low;
    }
    let [pad_low, pad_high] = instance.effect.defs[0].system.bounds;
    let (low, high) = (low + Vec3::from(pad_low), high + Vec3::from(pad_high));
    if !(low.is_finite() && high.is_finite()) {
        return true;
    }
    // The particles' frame (x, y, height) to the world's (x, height, -y).
    let aabb = Aabb::from_min_max(
        Vec3::new(low.x, low.z, -high.y),
        Vec3::new(high.x, high.z, -low.y),
    );
    frustum.intersects_obb(&aabb, &Affine3A::IDENTITY, true, false)
}

/// Steps every effect and gathers the frame's cards and lights.
#[allow(clippy::too_many_arguments)]
pub(super) fn tick(
    time: Res<Time>,
    session: NonSend<crate::session::Session>,
    mut playing: ResMut<Playing>,
    library: Option<Res<Library>>,
    camera: Query<(&GlobalTransform, &Frustum), With<RtsCamera>>,
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
    let dt = time.delta_secs() * pace;
    playing.advance(dt, height);
    playing.lights.clear();
    playing.placed.clear();
    let frame = &mut *frame;
    frame.quads.clear();
    frame.batches.clear();
    let (Some(library), Ok((camera, frustum))) = (library, camera.single()) else {
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
        instance.seen(in_view(instance, frustum), dt);
        turn_to_screen(instance, &eye);
    }
    for instance in playing.slots.iter().filter_map(|(_, i)| i.as_ref()) {
        // Out of view an effect draws no cards; its lights may still reach into it.
        if !instance.out_of_view() {
            draw(instance, &eye, height, &mut playing.lists);
        }
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
