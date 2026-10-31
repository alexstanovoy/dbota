//! A playing effect: its systems as a tree, each with its particles, clock and control
//! points, stepped as Source 2 steps them (at most `max_time_step` a step: emit, initialize,
//! operate), its end cap played when the game says the effect ends. The systems are kept in
//! depth-first order, so a system's descendants are the ones after it up to its `end`.

use std::sync::Arc;

use bevy::math::Vec3;
use heatel::particle::{Emitter, EndCap, FloatInput, Function, Timing};

use super::eval::{ControlPoint, HeightAt, Scene, float};
use super::init::{Batch, InitMemory, initialize, writes};
use super::library::Effect;
use super::ops::{Call, Memory, Restart, Target, operate};
use super::pre::{PreCall, Stop, pre_emit};
use super::store::Particles;

/// Most systems one playing effect has (a child shared by several parents counts each time).
const MAX_NODES: usize = 512;
/// Seconds an effect may play at most, end cap included: a definition that never ends stops.
/// An effect held on (worn by a unit, placed on the map) counts them from when it is ended.
const MAX_LIFE: f32 = 30.0;
/// The bit of the creation time, which every emitted particle has written.
const CREATION_WRITTEN: u64 = 1 << 8;
/// Most particles one emitter makes in one step.
const MAX_EMITTED: usize = 5000;

/// How one emitter stands: what it has made, and the system's age its timings count from (its
/// last restart).
#[derive(Clone, Copy, Default)]
struct EmitterState {
    burst_done: bool,
    accumulated: f64,
    emitted: u64,
    time_base: f32,
}

/// One system of a playing effect.
pub struct Node {
    pub def: usize,
    parent: Option<usize>,
    /// One past its last descendant.
    end: usize,
    pub children: Vec<usize>,
    end_cap_child: bool,
    delay: f32,
    last_step: f32,
    /// Whether it runs: an end cap child waits for its parent's end cap.
    running: bool,
    /// Whether it was stepped this frame (its children step only after it).
    stepped: bool,
    pub age: f32,
    in_end_cap: bool,
    end_cap_start: f32,
    emitting: bool,
    first_frame: bool,
    /// Its control points. One the game or an operator never placed stands at the world's
    /// origin, unturned, as Source's: Shadow Raze's embers and fleks take control point 3,
    /// which Dota leaves unplaced, as a velocity (`C_INIT_VelocityFromCP`), and rise in place.
    pub control_points: [ControlPoint; 64],
    /// Which control points have been placed (bit `cp`): one placed the first time has no
    /// previous place to move from.
    placed: u64,
    emitters: Vec<EmitterState>,
    /// What each pre-emission operator, initializer and operator keeps from step to step.
    pre_done: Vec<bool>,
    inits: Vec<InitMemory>,
    memories: Vec<Memory>,
    /// The age the functions' fades count from: the system's last restart.
    time_base: f32,
    /// The world's clock.
    world_time: f32,
    /// The strengths of the forces, and creation times, of a step.
    strengths: Vec<f32>,
    times: Vec<f32>,
    pub parts: Particles,
    emitted: u32,
    seed: u32,
    steps: u32,
}

/// What a system reads besides itself: the ground, and its parent's particles.
#[derive(Clone, Copy, Default)]
struct World<'a> {
    ground: Option<HeightAt<'a>>,
    parent: Option<&'a Particles>,
}

/// The frame being played: the system's age at its start, and its length. A control point
/// moves from its previous place to its place over it.
#[derive(Clone, Copy)]
struct Frame {
    start: f32,
    len: f32,
}

/// A playing effect.
pub struct Instance {
    pub effect: Arc<Effect>,
    pub nodes: Vec<Node>,
    life: f32,
    /// Held on until ended: no [`MAX_LIFE`] before that.
    held: bool,
    /// The world's clock, in seconds.
    pub world_time: f32,
    /// Control points the operators set in a step, applied after it.
    writes: Vec<(Target, usize, ControlPoint)>,
    /// Seconds since it was last in view.
    undrawn: f32,
    /// Out of view the last frame it was looked at.
    out_of_view: bool,
    /// Ended: it finishes, asleep or not.
    ending: bool,
}

impl EmitterState {
    /// The creation times of what a continuous emitter of `rate` a second releases over
    /// `from..to` (at most a step): what it has owed so far less what it made, spread evenly.
    fn release(&mut self, rate: f64, (from, to): (f64, f64), step: f32, times: &mut Vec<f32>) {
        let span = (to - from).clamp(0.0, f64::from(step));
        self.accumulated += rate * span;
        let count =
            ((self.accumulated + 0.001).floor() as u64).saturating_sub(self.emitted) as usize;
        self.emitted += count as u64;
        let spacing = if count > 0 { span / count as f64 } else { 0.0 };
        times.extend(
            (1..=count.min(MAX_EMITTED)).map(|k| (from + spacing * k as f64).min(to) as f32),
        );
    }
}

impl Instance {
    pub fn new(effect: Arc<Effect>, seed: u32) -> Self {
        let mut nodes = Vec::new();
        // Depth first, children in their definition's order.
        let mut stack = vec![(0usize, false, 0.0f32, None::<usize>)];
        while let Some((def, end_cap_child, delay, parent)) = stack.pop() {
            if nodes.len() == MAX_NODES {
                break;
            }
            let index = nodes.len();
            nodes.push(Node::new(
                &effect,
                def,
                end_cap_child,
                delay,
                parent,
                seed.wrapping_add(129 * index as u32),
            ));
            if let Some(parent) = parent {
                nodes[parent].children.push(index);
            }
            for (child, end_cap, delay) in effect.defs[def].children.iter().rev() {
                stack.push((*child, *end_cap, *delay, Some(index)));
            }
        }
        for index in (0..nodes.len()).rev() {
            let end = nodes[index]
                .children
                .last()
                .map_or(index + 1, |&last| nodes[last].end);
            nodes[index].end = end;
        }
        Self {
            effect,
            nodes,
            life: 0.0,
            held: false,
            world_time: 0.0,
            writes: Vec::new(),
            undrawn: 0.0,
            out_of_view: false,
            ending: false,
        }
    }

    /// Plays the effect again from its start, its draws keyed by `seed`, keeping what its
    /// particles' columns hold.
    pub fn restart(&mut self, seed: u32) {
        self.life = 0.0;
        self.held = false;
        self.undrawn = 0.0;
        self.out_of_view = false;
        self.ending = false;
        for (index, node) in self.nodes.iter_mut().enumerate() {
            node.reset(seed.wrapping_add(129 * index as u32));
        }
    }

    /// Sets control point `cp` of every system, facing `forward` when it has a length; one
    /// placed rather than moved (`jump`) has no previous place to move from.
    pub fn set_control_point(&mut self, cp: usize, position: Vec3, forward: Vec3, jump: bool) {
        for node in &mut self.nodes {
            let first = node.placed & 1 << cp == 0;
            node.placed |= 1 << cp;
            let point = &mut node.control_points[cp];
            point.position = position;
            if jump || first {
                point.previous = position;
            }
            point.face(forward);
        }
    }

    /// Holds the effect on until it is ended, however long that is.
    pub fn hold(&mut self) {
        self.held = true;
    }

    /// Ends the effect: emitters stop, the end cap's functions run and the end cap children
    /// start, with what runs under them.
    pub fn end(&mut self) {
        self.ending = true;
        if std::mem::take(&mut self.held) {
            self.life = 0.0;
        }
        stop(
            &mut self.nodes,
            0,
            Stop {
                destroy: false,
                play_end_cap: true,
            },
        );
    }

    /// Whether it was out of view the last frame it was looked at.
    pub fn out_of_view(&self) -> bool {
        self.out_of_view
    }

    /// Notes whether it was in view this frame, `dt` seconds after the last.
    pub fn seen(&mut self, in_view: bool, dt: f32) {
        self.out_of_view = !in_view;
        self.undrawn = if in_view { 0.0 } else { self.undrawn + dt };
    }

    /// Whether it sleeps: out of view for longer than its definition's sleep time
    /// (`m_flNoDrawTimeToGoToSleep`), as Source stops simulating a system no view draws, its
    /// particles held where they are until it is in view again. One ended never sleeps, so it
    /// finishes.
    pub fn asleep(&self) -> bool {
        !self.ending && self.undrawn > self.effect.defs[0].system.sleep_after
    }

    /// Plays `dt` seconds on the ground `ground` gives (none while the game does not know it).
    /// Asleep, only its clock and its control points move on: it wakes where it is placed then,
    /// not swept from where it fell asleep.
    pub fn advance(&mut self, dt: f32, ground: Option<HeightAt<'_>>) {
        self.life += dt;
        self.world_time += dt;
        if self.asleep() {
            for node in &mut self.nodes {
                for point in &mut node.control_points {
                    point.previous = point.position;
                }
            }
            return;
        }
        for node in &mut self.nodes {
            node.world_time = self.world_time;
        }
        for index in 0..self.nodes.len() {
            let parent_stepped = self.nodes[index]
                .parent
                .is_none_or(|p| self.nodes[p].stepped);
            self.nodes[index].stepped = parent_stepped && self.advance_node(index, dt, ground);
        }
        for node in &mut self.nodes {
            for point in &mut node.control_points {
                point.previous = point.position;
            }
        }
    }

    /// Steps system `index` over `dt`; whether it ran its steps (its children run only then).
    /// Its parent, stepped before it (the systems are in depth-first order), lends it its
    /// particles.
    fn advance_node(&mut self, index: usize, dt: f32, ground: Option<HeightAt<'_>>) -> bool {
        let Self {
            effect,
            nodes,
            writes,
            ..
        } = self;
        let (before, after) = nodes.split_at_mut(index);
        let subtree_end = after[0].end;
        let subtree = &mut after[..subtree_end - index];
        let node = &mut subtree[0];
        if !node.running {
            return false;
        }
        if node.age < 0.0 {
            node.age += dt;
            return false;
        }
        let world = World {
            ground,
            parent: node.parent.map(|p| &before[p].parts),
        };
        let max_step = effect.defs[node.def].system.max_time_step;
        let frame = Frame {
            start: node.age,
            len: dt,
        };
        if std::mem::take(&mut node.first_frame) {
            if let Some(asked) = node.first_frame(effect, world, frame, writes) {
                stop(subtree, index, asked);
            }
            for (target, cp, point) in writes.drain(..) {
                write_control_point(effect, subtree, index, target, cp, point);
            }
        }
        let mut remaining = dt.min(max_step * 10.0);
        while remaining > 1e-6 {
            let step = remaining.min(max_step);
            remaining -= step;
            let node = &mut subtree[0];
            let step_start = node.age;
            (node.age, node.last_step, node.steps) =
                (node.age + step, step, node.steps.wrapping_add(1));
            if let Some(asked) = node.pre_emit(effect, world, step, frame, false, writes) {
                stop(subtree, index, asked);
            }
            let node = &mut subtree[0];
            node.emit(effect, world, step, step_start, frame);
            if let Some(asked) = node.operate(effect, world, step, frame, writes) {
                restart(effect, subtree, index, asked);
            }
            for (target, cp, point) in writes.drain(..) {
                write_control_point(effect, subtree, index, target, cp, point);
            }
        }
        true
    }

    /// Whether it has ended: past its end cap with nothing left to draw or emit.
    pub fn finished(&self) -> bool {
        (!self.held && self.life > MAX_LIFE)
            || self.nodes.iter().all(|node| {
                (node.end_cap_child && !node.running)
                    || (node.parts.is_empty() && !node.may_emit(&self.effect))
            })
    }
}

impl Node {
    fn new(
        effect: &Effect,
        def: usize,
        end_cap_child: bool,
        delay: f32,
        parent: Option<usize>,
        seed: u32,
    ) -> Self {
        let system = &effect.defs[def].system;
        let mut parts = Particles::default();
        parts.reserve(system.max_particles.min(256) as usize);
        Self {
            def,
            parent,
            end: 0,
            children: Vec::new(),
            end_cap_child,
            delay,
            last_step: 0.0,
            running: !end_cap_child,
            stepped: false,
            age: -delay,
            in_end_cap: false,
            end_cap_start: 0.0,
            emitting: true,
            first_frame: true,
            control_points: [ControlPoint::default(); 64],
            placed: 0,
            emitters: vec![EmitterState::default(); system.emitters.len()],
            pre_done: vec![false; system.pre_emission.len()],
            inits: vec![InitMemory::default(); system.initializers.len()],
            memories: vec![Memory::default(); system.operators.len()],
            time_base: 0.0,
            world_time: 0.0,
            strengths: Vec::with_capacity(system.forces.len().max(system.initializers.len())),
            times: Vec::new(),
            parts,
            emitted: 0,
            seed,
            steps: 0,
        }
    }

    fn reset(&mut self, seed: u32) {
        (self.running, self.stepped, self.age, self.last_step) =
            (!self.end_cap_child, false, -self.delay, 0.0);
        (
            self.in_end_cap,
            self.end_cap_start,
            self.emitting,
            self.first_frame,
        ) = (false, 0.0, true, true);
        self.control_points = [ControlPoint::default(); 64];
        self.placed = 0;
        self.emitters
            .iter_mut()
            .for_each(|e| *e = EmitterState::default());
        self.pre_done.iter_mut().for_each(|d| *d = false);
        self.inits
            .iter_mut()
            .for_each(|m| *m = InitMemory::default());
        self.memories
            .iter_mut()
            .for_each(|m| *m = Memory::default());
        self.time_base = 0.0;
        self.parts.clear();
        (self.emitted, self.seed, self.steps) = (0, seed, 0);
    }

    /// Takes a control point an operator set, keeping where the old one was; one placed the
    /// first time jumps there (a missile's head locked to it does not fly in from the origin).
    fn place(&mut self, cp: usize, point: ControlPoint) {
        let previous = if self.placed & 1 << cp == 0 {
            point.position
        } else {
            self.control_points[cp].previous
        };
        self.placed |= 1 << cp;
        self.control_points[cp] = ControlPoint { previous, ..point };
    }

    /// The scene as it stands after the last step, for drawing.
    pub fn scene_now(&self) -> Scene<'_> {
        scene(
            self,
            self.last_step,
            Frame {
                start: self.age,
                len: 0.0,
            },
        )
    }

    pub fn last_step(&self) -> f32 {
        self.last_step
    }

    /// The strength a function (a renderer) runs at now.
    pub fn strength_now(&self, timing: &Timing) -> f32 {
        self.strength(timing)
    }

    /// The strength a function runs at now.
    fn strength(&self, timing: &Timing) -> f32 {
        if timing.is_always() {
            return 1.0;
        }
        let phase = match timing.end_cap {
            EndCap::Always => true,
            EndCap::Off => !self.in_end_cap,
            EndCap::On => self.in_end_cap,
        };
        if !phase {
            return 0.0;
        }
        let strength = match &timing.strength {
            FloatInput::Literal(v) => *v,
            other => float(
                other,
                &scene(
                    self,
                    0.0,
                    Frame {
                        start: self.age,
                        len: 0.0,
                    },
                ),
                &self.parts,
                None,
                0,
            ),
        };
        if strength <= 0.0 {
            return strength;
        }
        // The fades count from the last restart; an oscillation runs on the system's age.
        let age = if timing.oscillate > 0.0 {
            self.age
        } else {
            self.age - self.time_base
        };
        strength * timing.fade(timing.time(age))
    }

    /// The system's first frame: every pre-emission operator once, those that run only then
    /// included, then the initial particles. The stop a pre-emission operator asks for.
    fn first_frame(
        &mut self,
        effect: &Effect,
        world: World<'_>,
        frame: Frame,
        writes: &mut Vec<(Target, usize, ControlPoint)>,
    ) -> Option<Stop> {
        let asked = self.pre_emit(effect, world, 0.0, frame, true, writes);
        let system = &effect.defs[self.def].system;
        let first = self.parts.len();
        for _ in 0..system.initial_particles.min(system.max_particles) {
            self.spawn(effect, self.age);
        }
        self.initialize(effect, world, first, frame, 0.0);
        asked
    }

    /// Runs the pre-emission operators (in the first frame those that run only then too); the
    /// stop one asks for. What they set on their own system the functions after them read at
    /// once.
    fn pre_emit(
        &mut self,
        effect: &Effect,
        world: World<'_>,
        step: f32,
        frame: Frame,
        first_frame: bool,
        writes: &mut Vec<(Target, usize, ControlPoint)>,
    ) -> Option<Stop> {
        let system = &effect.defs[self.def].system;
        let mut asked = None;
        for (index, op) in system.pre_emission.iter().enumerate() {
            if op.timing.run_once && !first_frame {
                continue;
            }
            let strength = self.strength(&op.timing);
            if strength <= 0.0 {
                continue;
            }
            let before = writes.len();
            let scene = scene_of(&self.control_points, self.clock(), world, step, frame);
            let mut call = PreCall {
                parts: &self.parts,
                emitting: self.emitting && asked.is_none(),
                done: &mut self.pre_done[index],
                writes,
            };
            asked = asked.or(pre_emit(&op.class, &scene, &mut call));
            for &(target, cp, point) in &writes[before..] {
                if let Target::Own = target {
                    self.place(cp, point);
                }
            }
        }
        asked
    }

    fn spawn(&mut self, effect: &Effect, time: f32) {
        let def = &effect.defs[self.def];
        if self.parts.len() >= def.system.max_particles as usize {
            return;
        }
        let number = self.emitted;
        self.emitted = self.emitted.wrapping_add(1);
        let seed = self.seed.wrapping_mul(0x9E37_79B9) ^ number.wrapping_mul(0x85EB_CA6B);
        self.parts.push(&def.defaults, number, seed, time);
    }

    fn may_emit(&self, effect: &Effect) -> bool {
        if !self.emitting || !self.running {
            return false;
        }
        let system = &effect.defs[self.def].system;
        system
            .emitters
            .iter()
            .zip(&self.emitters)
            .any(|(emitter, state)| match &emitter.class {
                Emitter::Instantaneous { .. } => !state.burst_done,
                Emitter::Continuous {
                    duration,
                    start_time,
                    rate,
                } => {
                    let duration = duration.literal().unwrap_or(0.0);
                    let start = state.time_base + start_time.literal().unwrap_or(0.0);
                    rate.literal().is_none_or(|r| r > 0.0)
                        && (duration == 0.0 || self.age <= start + duration)
                }
                Emitter::Noise(noise) => {
                    let duration = noise.duration.literal().unwrap_or(0.0);
                    let start = state.time_base + noise.start_time.literal().unwrap_or(0.0);
                    duration == 0.0 || self.age - self.last_step <= start + duration
                }
                _ => false,
            })
    }

    fn emit(
        &mut self,
        effect: &Effect,
        world: World<'_>,
        step: f32,
        step_start: f32,
        frame: Frame,
    ) {
        if !self.emitting {
            return;
        }
        let system = &effect.defs[self.def].system;
        let mut times = std::mem::take(&mut self.times);
        for (index, emitter) in system.emitters.iter().enumerate() {
            let strength = self.strength(&emitter.timing);
            if strength <= 0.0 {
                continue;
            }
            times.clear();
            self.emission(emitter, index, strength, step, step_start, &mut times);
            let first = self.parts.len();
            for &time in &times {
                self.spawn(effect, time);
            }
            if self.parts.len() > first {
                self.initialize(effect, world, first, frame, step);
            }
        }
        self.times = times;
    }

    /// When the particles emitter `index` makes this step are created, into `times`.
    fn emission(
        &mut self,
        emitter: &Function<Emitter>,
        index: usize,
        strength: f32,
        step: f32,
        step_start: f32,
        times: &mut Vec<f32>,
    ) {
        let (frame, salt) = (
            Frame {
                start: step_start,
                len: step,
            },
            0x0E00 + index as u32,
        );
        let value = |input: &FloatInput, node: &Node| {
            float(input, &scene(node, step, frame), &node.parts, None, salt)
        };
        match &emitter.class {
            Emitter::Instantaneous {
                count,
                start_time,
                max_per_frame,
            } => {
                let at = self.emitters[index].time_base + value(start_time, self);
                if self.emitters[index].burst_done || at > self.age {
                    return;
                }
                self.emitters[index].burst_done = true;
                let mut n = value(count, self).max(0.0) as usize;
                if *max_per_frame >= 0 {
                    n = n.min(*max_per_frame as usize);
                }
                times.resize(
                    (((n as f32) * strength.min(1.0)) as usize).min(MAX_EMITTED),
                    at,
                );
            }
            Emitter::Continuous {
                rate,
                start_time,
                duration,
            } => {
                let start = self.emitters[index].time_base + value(start_time, self);
                let duration = value(duration, self);
                let rate = f64::from((strength * value(rate, self)).max(0.0));
                if start > self.age
                    || rate <= 0.0
                    || (duration != 0.0 && self.age - step > start + duration)
                {
                    return;
                }
                let from = f64::from(step_start.max(start));
                let to = f64::from(if duration != 0.0 {
                    self.age.min(start + duration)
                } else {
                    self.age
                });
                self.emitters[index].release(rate, (from, to), step, times);
            }
            Emitter::Noise(noise) => {
                self.noise_emission(noise, index, strength, (step, step_start), times);
            }
            _ => {}
        }
    }

    /// When the particles noise emitter `index` makes this step are created, into `times`: at
    /// its noise's rate, within its window.
    fn noise_emission(
        &mut self,
        noise: &heatel::particle::NoiseEmitter,
        index: usize,
        strength: f32,
        (step, step_start): (f32, f32),
        times: &mut Vec<f32>,
    ) {
        let rate = self.noise_rate(noise, strength, step, step_start);
        let scene = scene(
            self,
            step,
            Frame {
                start: step_start,
                len: step,
            },
        );
        let value = |input: &FloatInput| float(input, &scene, &self.parts, None, 0x0E40);
        let start = self.emitters[index].time_base + value(&noise.start_time);
        let duration = value(&noise.duration);
        if rate <= 0.0 || start > self.age || (duration != 0.0 && step_start > start + duration) {
            return;
        }
        let (from, to) = if duration != 0.0 {
            (step_start.max(start), self.age.min(start + duration))
        } else {
            (step_start, self.age)
        };
        self.emitters[index].release(
            f64::from(rate),
            (f64::from(from), f64::from(to)),
            step,
            times,
        );
    }

    /// The particles a second a noise emitter makes this step: noise over the system's age
    /// (and the world's time, and a control point's place) mapped onto its output, as Source
    /// maps it, times a control point's component and, below behaviour version 2, the
    /// highest control point.
    fn noise_rate(
        &self,
        noise: &heatel::particle::NoiseEmitter,
        strength: f32,
        step: f32,
        step_start: f32,
    ) -> f32 {
        let scene = scene(
            self,
            step,
            Frame {
                start: step_start,
                len: step,
            },
        );
        let value = |input: &FloatInput| float(input, &scene, &self.parts, None, 0x0E40);
        let (position, scale) = match noise.world_noise_point {
            Some(cp) => (
                self.control_points[cp.index()].position + Vec3::from(noise.offset_location),
                noise.world_noise_scale,
            ),
            None => (Vec3::ZERO, 1.0),
        };
        let time = (noise.offset + self.age) * value(&noise.scale);
        let world = (self.world_time * 1000.0).trunc() * noise.world_time_scale;
        let mut n = super::eval::noise(position * scale + Vec3::splat(time + world));
        if noise.absolute {
            n = n.abs();
        }
        if noise.absolute_inverted {
            n = 1.0 - n;
        }
        let amplitude = if noise.absolute { 1.0 } else { 0.5 };
        let low = value(&noise.output_min);
        let range = value(&noise.output_max) - low;
        let mut rate = (range * amplitude * n + (1.0 - amplitude) * range + low)
            .clamp(0.0, 2_147_483_600.0)
            * strength;
        if let Some((cp, component)) = noise.scale_control_point {
            rate *= self.control_points[cp.index()].position[usize::from(component)].max(0.0);
        }
        // The highest control point placed on it.
        let highest = 63u32.saturating_sub(self.placed.leading_zeros());
        let index_scale = highest as f32 * noise.index_scale;
        if index_scale != 0.0 {
            rate *= index_scale;
        }
        rate
    }

    /// Runs the initializers on particles `first..`, then records them as they start. Below
    /// behaviour version 6 an initializer writing only what earlier ones wrote is skipped
    /// (from `first_multiple_override` on it runs anyway).
    fn initialize(
        &mut self,
        effect: &Effect,
        world: World<'_>,
        first: usize,
        frame: Frame,
        step: f32,
    ) {
        let system = &effect.defs[self.def].system;
        let mut strengths = std::mem::take(&mut self.strengths);
        strengths.clear();
        strengths.extend(
            system
                .initializers
                .iter()
                .map(|init| self.strength(&init.timing)),
        );
        let dt = if step > 0.0 {
            step
        } else {
            system.max_time_step.min(0.05)
        };
        let scene = scene_of(&self.control_points, self.clock(), world, dt, frame);
        let (parts, inits) = (&mut self.parts, &mut self.inits);
        let mut written = CREATION_WRITTEN;
        for (index, init) in system.initializers.iter().enumerate() {
            let fields = writes(&init.class);
            let overridden = system.first_multiple_override >= 0
                && index as i32 >= system.first_multiple_override;
            if system.behavior_version < 6 && !overridden && fields & !written == 0 {
                continue;
            }
            if strengths[index] <= 0.0 {
                continue;
            }
            let mut batch = Batch {
                parts,
                first,
                written,
                salt: 0x1000 + (index as u32) * 64,
                memory: &mut inits[index],
            };
            initialize(&init.class, &scene, &mut batch);
            written |= fields;
        }
        self.parts.snapshot(first);
        self.strengths = strengths;
    }

    fn operate(
        &mut self,
        effect: &Effect,
        world: World<'_>,
        step: f32,
        frame: Frame,
        writes: &mut Vec<(Target, usize, ControlPoint)>,
    ) -> Option<Restart> {
        let system = &effect.defs[self.def].system;
        let mut strengths = std::mem::take(&mut self.strengths);
        strengths.clear();
        strengths.extend(system.forces.iter().map(|f| self.strength(&f.timing)));
        strengths.extend(system.constraints.iter().map(|c| self.strength(&c.timing)));
        let mut asked = None;
        for (index, op) in system.operators.iter().enumerate() {
            if self.parts.is_empty() {
                break;
            }
            let strength = self.strength(&op.timing);
            if strength <= 0.0 {
                continue;
            }
            let scene = scene_of(&self.control_points, self.clock(), world, step, frame);
            let (parts, memories) = (&mut self.parts, &mut self.memories);
            let before = writes.len();
            let mut call = Call {
                strength,
                salt: 0x8000 + (index as u32) * 64,
                memory: &mut memories[index],
                writes,
                forces: &system.forces,
                constraints: &system.constraints,
                strengths: &strengths,
                restart: None,
            };
            operate(&op.class, &scene, parts, &mut call);
            asked = asked.or(call.restart);
            parts.remove_killed();
            // What it sets on its own system the operators after it read at once.
            for &(target, cp, point) in &writes[before..] {
                if let Target::Own = target {
                    self.place(cp, point);
                }
            }
        }
        self.strengths = strengths;
        asked
    }

    /// Starts its functions over without rewinding its clock or losing its particles, as
    /// Source restarts a system: the emitters, the fades and what the functions keep count
    /// from now (or wait again, for an end cap child), the end cap ends; a system that stopped
    /// emitting stays stopped.
    fn restart(&mut self, effect: &Effect) {
        let system = &effect.defs[self.def].system;
        if self.end_cap_child {
            (self.running, self.age, self.time_base) = (false, -self.delay, 0.0);
        } else {
            self.time_base = self.age.max(0.0);
        }
        self.in_end_cap = false;
        let base = if self.end_cap_child {
            0.0
        } else {
            self.time_base
        };
        for (state, emitter) in self.emitters.iter_mut().zip(&system.emitters) {
            let fresh = EmitterState {
                time_base: base,
                ..EmitterState::default()
            };
            // An emitter that runs for ever goes on as it was.
            match &emitter.class {
                Emitter::Instantaneous { .. } => *state = fresh,
                Emitter::Continuous { duration, .. } if duration.literal() != Some(0.0) => {
                    *state = fresh;
                }
                Emitter::Noise(noise) if noise.duration.literal() != Some(0.0) => {
                    state.time_base = base;
                }
                _ => {}
            }
        }
        self.pre_done.iter_mut().for_each(|d| *d = false);
        for (memory, init) in self.inits.iter_mut().zip(&system.initializers) {
            if !super::init::keeps_on_restart(&init.class) {
                *memory = InitMemory::default();
            }
        }
        for (memory, op) in self.memories.iter_mut().zip(&system.operators) {
            if !super::ops::keeps_on_restart(&op.class) {
                *memory = Memory::default();
            }
        }
    }
}

/// Restarts what an operator of system `nodes[0]` (number `base` in the effect) asks for: it
/// and everything under it, or its children of a group and everything under them.
fn restart(effect: &Effect, nodes: &mut [Node], base: usize, asked: Restart) {
    let group = |node: &Node| effect.defs[node.def].system.group;
    match asked {
        Restart::System => nodes.iter_mut().for_each(|n| n.restart(effect)),
        Restart::Children(g) => {
            for k in 0..nodes[0].children.len() {
                let child = nodes[0].children[k] - base;
                if group(&nodes[child]) == g {
                    let end = nodes[child].end - base;
                    nodes[child..end].iter_mut().for_each(|n| n.restart(effect));
                }
            }
        }
    }
}

/// A system's clock as its inputs read it.
#[derive(Clone, Copy)]
struct Clock {
    age: f32,
    in_end_cap: bool,
    end_cap_age: f32,
    step_seed: u32,
    seed: u32,
    world_time: f32,
    emitting: bool,
}

impl Node {
    fn clock(&self) -> Clock {
        Clock {
            seed: self.seed,
            world_time: self.world_time,
            emitting: self.emitting,
            age: self.age,
            in_end_cap: self.in_end_cap,
            end_cap_age: if self.in_end_cap {
                self.age - self.end_cap_start
            } else {
                0.0
            },
            step_seed: self.steps ^ self.seed.rotate_left(13),
        }
    }
}

/// What node's inputs read: its clock and control points, without the ground or a parent.
fn scene(node: &Node, dt: f32, frame: Frame) -> Scene<'_> {
    scene_of(
        &node.control_points,
        node.clock(),
        World::default(),
        dt,
        frame,
    )
}

fn scene_of<'a>(
    control_points: &'a [ControlPoint; 64],
    clock: Clock,
    world: World<'a>,
    dt: f32,
    frame: Frame,
) -> Scene<'a> {
    Scene {
        age: clock.age,
        in_end_cap: clock.in_end_cap,
        end_cap_age: clock.end_cap_age,
        control_points,
        dt,
        step_seed: clock.step_seed,
        frame_start: frame.start,
        frame_len: frame.len,
        ground: world.ground,
        parent: world.parent,
        seed: clock.seed,
        world_time: clock.world_time,
        emitting: clock.emitting,
    }
}

/// Applies a control point an operator of system `nodes[0]` (number `base` in the effect) set:
/// to it, or to its children of a group, and everything under them; `nodes` is it and its
/// descendants.
fn write_control_point(
    effect: &Effect,
    nodes: &mut [Node],
    base: usize,
    target: Target,
    cp: usize,
    point: ControlPoint,
) {
    let group = |node: &Node| effect.defs[node.def].system.group;
    let mut matched = 0;
    for k in 0..nodes[0].children.len() {
        let child = nodes[0].children[k] - base;
        let in_group = |g: i32| group(&nodes[child]) == g;
        let hit = match target {
            Target::Own => false,
            Target::Children(g) => in_group(g),
            Target::Child(g, n) | Target::ChildPosition(g, n, _) => {
                let hit = in_group(g) && matched == n;
                matched += usize::from(in_group(g));
                hit
            }
        };
        if hit {
            let end = nodes[child].end - base;
            for node in &mut nodes[child..end] {
                let mut placed = point;
                if let Target::ChildPosition(_, _, orientation) = target {
                    let own = node.control_points[cp];
                    if !orientation {
                        (placed.forward, placed.left, placed.up) = (own.forward, own.left, own.up);
                    }
                }
                node.place(cp, placed);
            }
        }
    }
    if let Target::Own = target {
        nodes.iter_mut().for_each(|n| n.place(cp, point));
    }
}

/// Stops the emission of system `nodes[0]` (number `base` in the effect) and of everything
/// under it, `nodes` being it and its descendants: their end cap's functions run, and with the
/// end cap played the end cap children start, with what runs under them.
fn stop(nodes: &mut [Node], base: usize, asked: Stop) {
    let mut k = 0;
    while k < nodes.len() {
        let node = &mut nodes[k];
        if k > 0 && asked.play_end_cap && node.end_cap_child && !node.running {
            let end = node.end - base;
            (node.running, node.age, node.in_end_cap, node.end_cap_start) = (true, 0.0, true, 0.0);
            for below in &mut nodes[k + 1..end] {
                if !below.end_cap_child {
                    (below.running, below.in_end_cap) = (true, true);
                }
            }
            k = end;
            continue;
        }
        node.emitting = false;
        if asked.destroy {
            node.parts.clear();
        }
        if !node.end_cap_child {
            node.in_end_cap = true;
            if asked.play_end_cap {
                node.end_cap_start = node.age;
            }
        }
        k += 1;
    }
}
