//! A system's particles as columns: one array per attribute, each also kept as it was at
//! spawn (operators read the initial values), in emission order.

use bevy::math::Vec3;
use heatel::particle::Field;

/// The float attributes a particle has, by column.
pub const SCALARS: usize = 16;
/// The vector attributes, by column.
pub const VECTORS: usize = 7;

/// Where an attribute lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Scalar(usize),
    Vector(usize),
    /// The particle's number, read only.
    Id,
    /// An attribute nothing draws or reads: writes go nowhere, reads are 0.
    None,
}

pub const LIFETIME: usize = 0;
pub const RADIUS: usize = 1;
pub const ROLL: usize = 2;
pub const ROLL_SPEED: usize = 3;
pub const ALPHA: usize = 4;
pub const CREATION: usize = 5;
pub const SEQUENCE: usize = 6;
pub const TRAIL_LENGTH: usize = 7;
pub const YAW: usize = 8;
pub const FORCE_SCALE: usize = 15;

pub const POSITION: usize = 0;
pub const PREVIOUS: usize = 1;
pub const COLOR: usize = 2;
pub const NORMAL: usize = 4;
pub const HITBOX_OFFSET: usize = 6;

/// The column of attribute `field`.
pub fn slot(field: Field) -> Slot {
    match field.id() {
        1 => Slot::Scalar(LIFETIME),
        3 => Slot::Scalar(RADIUS),
        4 => Slot::Scalar(ROLL),
        5 => Slot::Scalar(ROLL_SPEED),
        7 => Slot::Scalar(ALPHA),
        8 => Slot::Scalar(CREATION),
        9 => Slot::Scalar(SEQUENCE),
        10 => Slot::Scalar(TRAIL_LENGTH),
        12 => Slot::Scalar(YAW),
        13 => Slot::Scalar(9),
        16 => Slot::Scalar(10),
        18 => Slot::Scalar(11),
        20 => Slot::Scalar(12),
        26 => Slot::Scalar(13),
        27 => Slot::Scalar(14),
        34 => Slot::Scalar(FORCE_SCALE),
        0 => Slot::Vector(POSITION),
        2 => Slot::Vector(PREVIOUS),
        6 => Slot::Vector(COLOR),
        15 => Slot::Vector(HITBOX_OFFSET),
        17 => Slot::Vector(3),
        21 => Slot::Vector(NORMAL),
        30 => Slot::Vector(5),
        11 => Slot::Id,
        _ => Slot::None,
    }
}

/// What a particle starts with before the initializers (the system's constants).
#[derive(Clone, Copy)]
pub struct Defaults {
    pub scalars: [f32; SCALARS],
    pub vectors: [Vec3; VECTORS],
}

/// The particles of one system.
#[derive(Default)]
pub struct Particles {
    pub scalars: [Vec<f32>; SCALARS],
    pub vectors: [Vec<Vec3>; VECTORS],
    pub initial_scalars: [Vec<f32>; SCALARS],
    pub initial_vectors: [Vec<Vec3>; VECTORS],
    /// The number in emission order.
    pub number: Vec<u32>,
    /// What its constant random draws are keyed by.
    pub seed: Vec<u32>,
    /// The force gathered this step.
    pub force: Vec<Vec3>,
    pub killed: Vec<bool>,
}

impl Particles {
    pub fn len(&self) -> usize {
        self.number.len()
    }

    pub fn is_empty(&self) -> bool {
        self.number.is_empty()
    }

    pub fn reserve(&mut self, capacity: usize) {
        let more = capacity.saturating_sub(self.number.capacity());
        for column in self
            .scalars
            .iter_mut()
            .chain(self.initial_scalars.iter_mut())
        {
            column.reserve(more);
        }
        for column in self
            .vectors
            .iter_mut()
            .chain(self.initial_vectors.iter_mut())
        {
            column.reserve(more);
        }
        self.number.reserve(more);
        self.seed.reserve(more);
        self.force.reserve(more);
        self.killed.reserve(more);
    }

    pub fn clear(&mut self) {
        let _ = self.compact_with(|_| false);
    }

    /// Adds a particle with the defaults, created at `time`.
    pub fn push(&mut self, defaults: &Defaults, number: u32, seed: u32, time: f32) {
        for (column, value) in self.scalars.iter_mut().zip(defaults.scalars) {
            column.push(value);
        }
        for (column, value) in self.vectors.iter_mut().zip(defaults.vectors) {
            column.push(value);
        }
        self.scalars[CREATION][self.number.len()] = time;
        self.number.push(number);
        self.seed.push(seed);
        self.force.push(Vec3::ZERO);
        self.killed.push(false);
    }

    /// Records particles `from..` as they are now as their initial values.
    pub fn snapshot(&mut self, from: usize) {
        for (initial, column) in self.initial_scalars.iter_mut().zip(&self.scalars) {
            initial.truncate(from);
            initial.extend_from_slice(&column[from..]);
        }
        for (initial, column) in self.initial_vectors.iter_mut().zip(&self.vectors) {
            initial.truncate(from);
            initial.extend_from_slice(&column[from..]);
        }
    }

    pub fn scalar(&self, slot: Slot, i: usize) -> f32 {
        match slot {
            Slot::Scalar(s) => self.scalars[s][i],
            Slot::Id => self.number[i] as f32,
            _ => 0.0,
        }
    }

    pub fn initial_scalar(&self, slot: Slot, i: usize) -> f32 {
        match slot {
            Slot::Scalar(s) => self.initial_scalars[s]
                .get(i)
                .copied()
                .unwrap_or(self.scalars[s][i]),
            Slot::Id => self.number[i] as f32,
            _ => 0.0,
        }
    }

    pub fn set_scalar(&mut self, slot: Slot, i: usize, value: f32) {
        if let Slot::Scalar(s) = slot {
            self.scalars[s][i] = value;
        }
    }

    pub fn vector(&self, slot: Slot, i: usize) -> Vec3 {
        match slot {
            Slot::Vector(v) => self.vectors[v][i],
            _ => Vec3::ZERO,
        }
    }

    pub fn initial_vector(&self, slot: Slot, i: usize) -> Vec3 {
        match slot {
            Slot::Vector(v) => self.initial_vectors[v]
                .get(i)
                .copied()
                .unwrap_or(self.vectors[v][i]),
            _ => Vec3::ZERO,
        }
    }

    pub fn set_vector(&mut self, slot: Slot, i: usize, value: Vec3) {
        if let Slot::Vector(v) = slot {
            self.vectors[v][i] = value;
        }
    }

    /// Sets what particle `i` counts as its value at spawn; one not yet recorded has none.
    pub fn set_initial_scalar(&mut self, slot: Slot, i: usize, value: f32) {
        if let Slot::Scalar(s) = slot
            && let Some(initial) = self.initial_scalars[s].get_mut(i)
        {
            *initial = value;
        }
    }

    /// Sets what particle `i` counts as its vector at spawn, as [`Self::set_initial_scalar`].
    pub fn set_initial_vector(&mut self, slot: Slot, i: usize, value: Vec3) {
        if let Slot::Vector(v) = slot
            && let Some(initial) = self.initial_vectors[v].get_mut(i)
        {
            *initial = value;
        }
    }

    /// Removes the killed particles, keeping the order of the rest; how many went.
    pub fn remove_killed(&mut self) -> usize {
        if !self.killed.iter().any(|k| *k) {
            return 0;
        }
        let killed = std::mem::take(&mut self.killed);
        let gone = self.compact_with(|i| !killed[i]);
        self.killed = killed;
        self.killed.clear();
        self.killed.resize(self.number.len(), false);
        gone
    }

    /// Keeps the particles `keep` says, in order.
    fn compact_with(&mut self, keep: impl Fn(usize) -> bool) -> usize {
        let len = self.number.len();
        let mut kept = 0;
        for i in 0..len {
            if !keep(i) {
                continue;
            }
            if kept != i {
                for column in self
                    .scalars
                    .iter_mut()
                    .chain(self.initial_scalars.iter_mut())
                {
                    column[kept] = column[i];
                }
                for column in self
                    .vectors
                    .iter_mut()
                    .chain(self.initial_vectors.iter_mut())
                {
                    column[kept] = column[i];
                }
                self.number[kept] = self.number[i];
                self.seed[kept] = self.seed[i];
                self.force[kept] = self.force[i];
            }
            kept += 1;
        }
        for column in self
            .scalars
            .iter_mut()
            .chain(self.initial_scalars.iter_mut())
        {
            column.truncate(kept);
        }
        for column in self
            .vectors
            .iter_mut()
            .chain(self.initial_vectors.iter_mut())
        {
            column.truncate(kept);
        }
        self.number.truncate(kept);
        self.seed.truncate(kept);
        self.force.truncate(kept);
        self.killed.truncate(kept);
        len - kept
    }
}
