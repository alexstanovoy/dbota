//! The effects playing: started and ended by what the game shows, stepped each frame, their
//! cards gathered by look into the frame the renderer draws.

use std::sync::Arc;

use bevy::camera::primitives::{Aabb, Frustum};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use super::draw::{Eye, Quad, draw, from_world, turn_to_screen};
use super::eval::HeightAt;
use super::library::{Effect, Library};
use super::lights::gather;
use super::models;
use super::render::{Batch, ParticleFrame};
use super::sim::Instance;
use super::store;

/// Most seconds the portrait's effects are owed: a portrait not drawn for longer holds them
/// where they were, as an effect out of view sleeps.
pub(super) const STAGE_OWED_MAX: f32 = 0.25;
/// Most seconds of one step of a portrait's effect: the portrait's frame.
const STAGE_STEP: f32 = 1.0 / 30.0;
/// How many times `effect` is counted in `counts`.
fn count_of(counts: &[(Arc<Effect>, u32)], effect: &Arc<Effect>) -> u32 {
    counts
        .iter()
        .find(|(e, _)| Arc::ptr_eq(e, effect))
        .map_or(0, |(_, n)| *n)
}

/// Most effects playing at once; past it a new one is not started.
const MAX_PLAYING: usize = 2048;
/// Ended effects kept to be played again without allocating.
const MAX_SPARE: usize = 256;
/// Effects playing from which they are stepped on the compute threads together: fewer step
/// sooner than the threads would be handed them.
const PARALLEL_FROM: usize = 4;

/// A playing effect, by its slot and the slot's generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    slot: usize,
    generation: u32,
}

/// A separate host view, advanced and drawn only on the frames the host supplies it.
pub struct StageView<'a> {
    pub camera: (&'a GlobalTransform, &'a Frustum),
    pub layer: usize,
}

/// The effects playing.
#[derive(Resource, Default)]
pub struct Simulation {
    slots: Vec<(u32, Option<Instance>)>,
    free: Vec<usize>,
    spare: Vec<Instance>,
    /// Each look's cards this frame, kept for their capacity.
    lists: Vec<Vec<Quad>>,
    stage_lists: Vec<Vec<Quad>>,
    stage_owed: f32,
    /// The world's clock, in seconds of the game's pace: what noise over the world's time reads.
    clock: f32,
    /// How many times each effect was started, for the end-to-end tests to ask what the game
    /// drew.
    started: Vec<(Arc<Effect>, u32)>,
    stage_started: Vec<(Arc<Effect>, u32)>,
    /// Actual instance contributions to frames, including retained ending tails.
    #[cfg(any(test, feature = "test-support"))]
    drawn: Vec<(Arc<Effect>, u32)>,
    #[cfg(any(test, feature = "test-support"))]
    drawn_endings: Vec<(Arc<Effect>, u32)>,
}

impl Simulation {
    pub fn reset(&mut self) {
        for slot in 0..self.slots.len() {
            let handle = Handle {
                slot,
                generation: self.slots[slot].0,
            };
            self.cancel(handle);
        }
        self.clock = 0.0;
        self.stage_owed = 0.0;
    }

    /// How many instance frames actually contributed cards, lights or models.
    #[cfg(any(test, feature = "test-support"))]
    pub fn drawn(&self, effect: &Arc<Effect>) -> u32 {
        self.drawn
            .iter()
            .find(|(held, _)| Arc::ptr_eq(held, effect))
            .map_or(0, |(_, count)| *count)
    }

    /// Actual contributions made after an emitter ended, while its tails remained.
    #[cfg(any(test, feature = "test-support"))]
    pub fn drawn_endings(&self, effect: &Arc<Effect>) -> u32 {
        self.drawn_endings
            .iter()
            .find(|(held, _)| Arc::ptr_eq(held, effect))
            .map_or(0, |(_, count)| *count)
    }

    /// How many times `effect` was started.
    pub fn started(&self, effect: &Arc<Effect>) -> u32 {
        count_of(&self.started, effect)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn started_on_stage(&self, effect: &Arc<Effect>) -> u32 {
        count_of(&self.stage_started, effect)
    }

    /// Leaves out the cards of the systems of `effect`'s playing instances whose path contains
    /// `matching` (none when it is empty): presentation only, the simulation runs on.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hide_systems(&mut self, effect: &Arc<Effect>, matching: &str) {
        let hidden: Vec<usize> = effect
            .defs
            .iter()
            .enumerate()
            .filter(|(_, def)| !matching.is_empty() && def.path.contains(matching))
            .map(|(index, _)| index)
            .collect();
        for instance in self.slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
            if Arc::ptr_eq(&instance.effect, effect) {
                instance.hidden_defs.clone_from(&hidden);
            }
        }
    }

    /// The playing instances of `effect`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn instances_of<'a>(
        &'a self,
        effect: &'a Arc<Effect>,
    ) -> impl Iterator<Item = &'a Instance> {
        self.slots
            .iter()
            .filter_map(|(_, i)| i.as_ref())
            .filter(move |i| Arc::ptr_eq(&i.effect, effect))
    }

    /// Live simulation instances paired with their opaque, generation-checked handles.
    #[cfg(any(test, feature = "test-support"))]
    pub fn instances(&self) -> impl Iterator<Item = (Handle, &Instance)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(slot, (generation, entry))| {
                entry.as_ref().map(|instance| {
                    (
                        Handle {
                            slot,
                            generation: *generation,
                        },
                        instance,
                    )
                })
            })
    }

    /// Starts `effect`, its draws keyed by `seed`.
    pub fn start(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        self.start_in(effect, seed, false)
    }

    /// Starts a held effect in the host's separate stage view.
    pub fn hold_on_stage(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        let handle = self.start_in(effect, seed, true)?;
        self.get_mut(handle)?.hold();
        Some(handle)
    }

    fn start_in(&mut self, effect: &Arc<Effect>, seed: u32, portrait: bool) -> Option<Handle> {
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
        instance.portrait = portrait;
        let counts = if portrait {
            &mut self.stage_started
        } else {
            &mut self.started
        };
        match counts.iter_mut().find(|(e, _)| Arc::ptr_eq(e, effect)) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => counts.push((Arc::clone(effect), 1)),
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

    /// Whether this generation of the effect is still playing, including its live tails.
    pub fn contains(&self, handle: Handle) -> bool {
        self.slots
            .get(handle.slot)
            .is_some_and(|(generation, instance)| {
                *generation == handle.generation && instance.is_some()
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

    /// Retires an instance without emitting an impact or playing its end cap.
    pub fn cancel(&mut self, handle: Handle) {
        let Some((generation, entry)) = self.slots.get_mut(handle.slot) else {
            return;
        };
        if *generation != handle.generation {
            return;
        }
        let Some(instance) = entry.take() else {
            return;
        };
        *generation = generation.wrapping_add(1);
        self.free.push(handle.slot);
        if self.spare.len() < MAX_SPARE {
            self.spare.push(instance);
        }
    }

    /// Steps authored-effect fixtures without gathering a render frame.
    #[cfg(feature = "test-support")]
    pub fn advance_for_test(
        &mut self,
        dt: f32,
        stage_dt: Option<f32>,
        ground: Option<HeightAt<'_>>,
    ) {
        self.advance(dt, stage_dt, ground);
    }

    /// Steps every effect by `dt`, on the compute threads when there are enough of them that
    /// the effects take turns on no single thread (each effect steps alone: the order they step
    /// in changes nothing), and retires the ones that are finished.
    pub(crate) fn advance(&mut self, dt: f32, stage_dt: Option<f32>, ground: Option<HeightAt<'_>>) {
        self.clock += dt;
        // The portrait's effects: what they are owed, at most STAGE_OWED_MAX, in steps of the
        // portrait's frame, so no step outruns what the simulation steps in one; on no ground,
        // as the stage has none (the map's would pull their particles down to it).
        let stage_dt = stage_dt.map(|owed| owed.clamp(0.0, STAGE_OWED_MAX));
        let advance = move |instance: &mut Instance| match (instance.portrait, stage_dt) {
            (false, _) => instance.advance(dt, ground),
            (true, Some(owed)) if owed > 0.0 => {
                // Steps of one length: movement rescales a displacement by the ratio of the steps,
                // and a remainder far shorter than the step after it (a displacement of a few
                // rounding errors) would have its rounding scaled up by that ratio.
                let steps = (owed / STAGE_STEP).ceil();
                for _ in 0..steps as u32 {
                    instance.advance(owed / steps, None);
                }
            }
            (true, _) => {}
        };
        let pool = ComputeTaskPool::get();
        let playing = self.slots.iter().filter(|(_, i)| i.is_some()).count();
        if playing >= PARALLEL_FROM && pool.thread_num() > 1 {
            // Several pieces a thread, so one heavy effect does not hold the rest up.
            let piece = self.slots.len().div_ceil(pool.thread_num() * 4).max(1);
            pool.scope(|scope| {
                for slots in self.slots.chunks_mut(piece) {
                    scope.spawn(async move {
                        for instance in slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
                            advance(instance);
                        }
                    });
                }
            });
        } else {
            for instance in self.slots.iter_mut().filter_map(|(_, i)| i.as_mut()) {
                advance(instance);
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

impl Simulation {
    /// Steps the simulation and gathers what this view draws, borrowing its height traces.
    /// Borrowed filters keep gathering code here while the host owns visibility policy.
    pub fn step(
        &mut self,
        dt: f32,
        height: Option<HeightAt<'_>>,
        library: Option<&Library>,
        camera: Option<(&GlobalTransform, &Frustum)>,
        stage: Option<StageView<'_>>,
        frame: &mut ParticleFrame,
        units_per_metre: f32,
        permitted: &dyn Fn(Handle, &Instance) -> bool,
    ) {
        let playing = self;
        frame.eye = camera.map(|(camera, _)| from_world(camera.translation()));
        playing.stage_owed = (playing.stage_owed + dt).min(STAGE_OWED_MAX);
        let stage_dt = stage
            .as_ref()
            .map(|_| std::mem::take(&mut playing.stage_owed));
        playing.advance(dt, stage_dt, height);
        frame.stage_layer = stage
            .as_ref()
            .map(|view| bevy::camera::visibility::RenderLayers::layer(view.layer));
        frame.lights.clear();
        frame.placed.clear();
        frame.quads.clear();
        frame.batches.clear();
        let (Some(library), Some((camera, frustum))) = (library, camera) else {
            return;
        };
        let eye_of = |camera: &GlobalTransform| Eye {
            position: from_world(camera.translation()),
            right: from_world(camera.right().as_vec3()),
            up: from_world(camera.up().as_vec3()),
        };
        let world = (eye_of(camera), frustum);
        let stage = stage.map(|view| (eye_of(view.camera.0), view.camera.1));
        let view_of = |instance: &Instance| match (instance.portrait, stage) {
            (false, _) => Some(world),
            (true, stage) => stage,
        };
        playing.lists.resize_with(library.looks.len(), Vec::new);
        playing
            .stage_lists
            .resize_with(library.looks.len(), Vec::new);
        for list in playing.lists.iter_mut().chain(&mut playing.stage_lists) {
            list.clear();
        }
        for (slot, (generation, entry)) in playing.slots.iter_mut().enumerate() {
            let Some(instance) = entry else {
                continue;
            };
            let handle = Handle {
                slot,
                generation: *generation,
            };
            let Some((eye, frustum)) = view_of(instance) else {
                continue;
            };
            instance.seen(
                permitted(handle, instance) && in_view(instance, frustum),
                dt,
            );
            turn_to_screen(instance, &eye);
        }
        for (slot, (generation, entry)) in playing.slots.iter().enumerate() {
            let Some(instance) = entry else {
                continue;
            };
            let handle = Handle {
                slot,
                generation: *generation,
            };
            // A host visibility rule also hides ended emitters' live tails, lights and models.
            let Some((eye, _)) = view_of(instance).filter(|_| permitted(handle, instance)) else {
                continue;
            };
            #[cfg(any(test, feature = "test-support"))]
            let before = (
                playing
                    .lists
                    .iter()
                    .chain(&playing.stage_lists)
                    .map(Vec::len)
                    .sum::<usize>(),
                frame.lights.len(),
                frame.placed.len(),
            );
            // Out of view an effect draws no cards; its lights may still reach into it.
            if !instance.out_of_view() {
                let lists = if instance.portrait {
                    &mut playing.stage_lists
                } else {
                    &mut playing.lists
                };
                let ground = if instance.portrait { None } else { height };
                draw(instance, &eye, ground, lists);
            }
            if !instance.portrait {
                gather(instance, &mut frame.lights);
                models::gather(instance, &mut frame.placed, units_per_metre);
            }
            #[cfg(any(test, feature = "test-support"))]
            if before
                != (
                    playing
                        .lists
                        .iter()
                        .chain(&playing.stage_lists)
                        .map(Vec::len)
                        .sum::<usize>(),
                    frame.lights.len(),
                    frame.placed.len(),
                )
            {
                if instance.ending() {
                    count_contribution(&mut playing.drawn_endings, &instance.effect);
                }
                count_contribution(&mut playing.drawn, &instance.effect);
            }
        }
        let world_lists = playing.lists.iter().map(|list| (list, false));
        let stage_lists = playing.stage_lists.iter().map(|list| (list, true));
        for (look, (list, on_stage)) in world_lists.enumerate().chain(stage_lists.enumerate()) {
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
                stage: on_stage,
            });
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
fn count_contribution(counts: &mut Vec<(Arc<Effect>, u32)>, effect: &Arc<Effect>) {
    if let Some((_, count)) = counts
        .iter_mut()
        .find(|(held, _)| Arc::ptr_eq(held, effect))
    {
        *count = count.saturating_add(1);
    } else {
        counts.push((Arc::clone(effect), 1));
    }
}
