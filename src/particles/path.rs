//! Particles along a path between control points (`CPathParameters`): made one after another
//! along it, kept moving with it, or pulled back onto it.

use bevy::math::Vec3;
use heatel::particle::{FloatInput, Initializer, Operator, Path, SequentialPath};

use super::eval::{Scene, between, float};
use super::init::Batch;
use super::ops::Call;
use super::store::{self, Particles};

/// A path at a moment: start, midpoint and end of a quadratic Bezier.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bezier {
    pub start: Vec3,
    pub mid: Vec3,
    pub end: Vec3,
}

impl Bezier {
    /// The point `t` of the way along it.
    pub fn at(&self, t: f32) -> Vec3 {
        self.start
            .lerp(self.mid, t)
            .lerp(self.mid.lerp(self.end, t), t)
    }
}

/// Path `path` from control point `start` to `end` where they were at system age `time`. The
/// bulge pushes the midpoint along a control point's forward, the more the more square the
/// path is to it; without a control point it is a random offset per axis fixed for the system
/// (`seed`); a mid control point instead makes the path pass through it at half way.
pub fn bezier(
    scene: &Scene<'_>,
    path: &Path,
    (start, end): (usize, usize),
    time: f32,
    seed: u32,
) -> Bezier {
    let start = scene.control_point_at(start, time).position + Vec3::from(path.start_offset);
    let end_point = scene.control_point_at(end, time);
    let end = end_point.position + Vec3::from(path.end_offset);
    if let Some(mid) = path.mid {
        let through = scene.control_point_at(mid.index(), time).position;
        let mid = 2.0 * through - 0.5 * (start + end) + Vec3::from(path.mid_offset);
        return Bezier { start, mid, end };
    }
    let mut mid = start.lerp(end, path.mid_point);
    if path.bulge_control != 0 {
        let forward = if path.bulge_control == 2 {
            end_point.forward
        } else {
            scene.control_points[path.start.index()].forward
        };
        let direction = end - start;
        let length = direction.length();
        let square = if length > 1e-6 {
            1.0 - (direction / length).dot(forward).abs()
        } else {
            0.0
        };
        if forward.length() > 1e-6 {
            mid += forward * (length * path.bulge * square / forward.length());
        }
    } else {
        let bulge = path.bulge;
        mid += Vec3::new(
            between(seed, 0x7A70, -bulge, bulge),
            between(seed, 0x7A71, -bulge, bulge),
            between(seed, 0x7A72, -bulge, bulge),
        );
    }
    Bezier {
        start,
        mid: mid + Vec3::from(path.mid_offset),
        end,
    }
}

/// Where a sequential path's walk stands from one particle to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Walk {
    parameter: f32,
    direction: f32,
    control_point: f32,
    parameter_step: f32,
    control_point_step: f32,
}

impl Walk {
    fn new(s: &SequentialPath) -> Self {
        let (start, end) = (s.path.start.index() as f32, s.path.end.index() as f32);
        let step = if s.count <= 1.0 {
            0.0
        } else {
            1.0 / (s.count - 1.0)
        };
        let span = end - start;
        let pairs = s.pairs && span > 1.0 && s.count > 1.0;
        Self {
            parameter: 0.0,
            direction: -1.0,
            control_point: if s.pairs { start } else { 0.0 },
            parameter_step: if pairs { span * step } else { step },
            control_point_step: if pairs { span * step } else { 0.0 },
        }
    }

    /// The next particle's share of the way and its segment's control points; the walk moves
    /// on past it.
    fn next(&mut self, s: &SequentialPath) -> (f32, usize, usize) {
        let (start, end) = (s.path.start.index(), s.path.end.index());
        let mut t = self.parameter;
        let (from, to) = if s.pairs {
            let cp = self.control_point;
            if (cp as usize) > end || cp < start as f32 {
                if s.looped {
                    self.control_point = start as f32;
                } else {
                    self.control_point_step = -self.control_point_step;
                    self.parameter_step = -self.parameter_step;
                    self.direction = -self.direction;
                    self.control_point = cp + 2.0 * self.control_point_step;
                    t += 2.0 * self.parameter_step;
                    self.parameter = t;
                }
            }
            let from = (self.control_point.max(0.0) as usize).min(63);
            (
                from,
                if from == end {
                    from
                } else {
                    (from + 1).min(63)
                },
            )
        } else {
            (start, end)
        };
        if !(0.0..=1.000_000_1).contains(&t) {
            if s.looped || s.pairs {
                t += self.direction;
            } else {
                self.parameter_step = -self.parameter_step;
                self.direction = -self.direction;
                t += 2.0 * self.parameter_step;
            }
            self.parameter = t;
        }
        self.parameter += self.parameter_step;
        self.control_point += self.control_point_step;
        (t, from, to)
    }
}

/// `C_INIT_CreateSequentialPath`: each new particle at the next point of the path (looping or
/// turning back at its ends), offset at random by up to the maximum distance per axis; with
/// the offset saved, its share of the way and its segment's control points go to its hitbox
/// offset.
pub fn create_sequential_path(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::CreateSequentialPath(s) = init else {
        return;
    };
    let walk = batch.memory.path.get_or_insert_with(|| Walk::new(s));
    for i in batch.first..batch.parts.len() {
        let (t, from, to) = walk.next(s);
        let creation = batch.parts.scalars[store::CREATION][i];
        let curve = bezier(scene, &s.path, (from, to), creation, scene.seed);
        let seed = batch.parts.seed[i];
        let d = s.max_distance;
        let jitter = Vec3::new(
            between(seed, batch.salt, -d, d),
            between(seed, batch.salt + 1, -d, d),
            between(seed, batch.salt + 2, -d, d),
        );
        let position = curve.at(t) + jitter;
        if s.save_offset {
            batch.parts.vectors[store::HITBOX_OFFSET][i] = Vec3::new(t, from as f32, to as f32);
        }
        batch.parts.vectors[store::POSITION][i] = position;
        batch.parts.vectors[store::PREVIOUS][i] = position;
    }
}

/// `C_OP_LockToSavedSequentialPath`: each particle moves as far as its saved point of the
/// path moved since the last step (as much of it as it has lived), its offset from the path
/// kept.
pub fn lock_to_saved_path(
    op: &Operator,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Operator::LockToSavedSequentialPath { pairs, path } = op else {
        return;
    };
    let (start, end) = (path.start.index(), path.end.index());
    let count = if *pairs {
        end.saturating_sub(start).max(1)
    } else {
        1
    };
    let segment = |k: usize| {
        if *pairs {
            ((start + k).min(63), (start + k + 1).min(63))
        } else {
            (start, end)
        }
    };
    let now: Vec<Bezier> = (0..count)
        .map(|k| bezier(scene, path, segment(k), scene.age, scene.seed))
        .collect();
    let before = std::mem::take(&mut call.memory.paths);
    let before = if before.len() == count {
        before
    } else {
        (0..count)
            .map(|k| bezier(scene, path, segment(k), scene.age - scene.dt, scene.seed))
            .collect()
    };
    for i in 0..parts.len() {
        let saved = parts.vectors[store::HITBOX_OFFSET][i];
        let k = ((saved.y - start as f32) as i64).clamp(0, count as i64 - 1) as usize;
        let delta = (now[k].at(saved.x) - before[k].at(saved.x)) * lived(scene, parts, i);
        parts.vectors[store::POSITION][i] += delta;
        parts.vectors[store::PREVIOUS][i] += delta;
    }
    call.memory.paths = now;
}

/// The share of the step particle `i` has lived.
pub fn lived(scene: &Scene<'_>, parts: &Particles, i: usize) -> f32 {
    if scene.dt <= 0.0 {
        return 1.0;
    }
    let age = scene.age - parts.scalars[store::CREATION][i];
    age.clamp(0.0, scene.dt) / scene.dt
}

/// Where `C_OP_MaintainSequentialPath` stands: the next point handed out, which way the order
/// runs, and where the control points were when the pull last ran.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    slot: i32,
    step: i32,
    start: Vec3,
    end: Vec3,
}

impl Default for Order {
    fn default() -> Self {
        Self {
            slot: 0,
            step: 1,
            start: Vec3::splat(f32::MAX),
            end: Vec3::splat(f32::MAX),
        }
    }
}

/// `C_OP_MaintainSequentialPath`: the particles in order each pulled to the next of evenly
/// spaced points of the path (the order looping or turning back), their offset from it kept
/// by one less the cohesion, and no longer than the maximum distance.
pub fn maintain_path(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::MaintainSequentialPath(m) = op else {
        return;
    };
    let order = &mut call.memory.order;
    let (start, end) = (m.path.start.index(), m.path.end.index());
    if m.tolerance > 0.0 {
        let (a, b) = (
            scene.control_points[start].position,
            scene.control_points[end].position,
        );
        // The tolerance is compared with the squared move, as Source compares it.
        if a.distance_squared(order.start) < m.tolerance
            && b.distance_squared(order.end) < m.tolerance
        {
            return;
        }
        (order.start, order.end) = (a, b);
    }
    let value = |input: &FloatInput| float(input, scene, parts, None, call.salt);
    if m.use_particle_count || !matches!(m.count, FloatInput::Literal(_)) {
        order.slot = 0;
    }
    let count = if m.use_particle_count {
        parts.len() as f32
    } else {
        value(&m.count)
    };
    let spacing = if count <= 1.0 {
        0.0
    } else {
        1.0 / (count - 1.0)
    };
    let (max, keep) = (value(&m.max_distance), 1.0 - value(&m.cohesion));
    let curve = bezier(scene, &m.path, (start, end), scene.age, scene.seed);
    for i in 0..parts.len() {
        let mut slot = order.slot;
        if slot as f32 >= count || slot < 0 {
            if m.looped {
                (slot, order.slot) = (0, 0);
            } else {
                order.step = -order.step;
                slot = slot.min(count as i32 - 1).max(1);
                order.slot = slot;
            }
        }
        let target = curve.at(slot as f32 * spacing);
        let pull = |p: Vec3| {
            let offset = p - target;
            let distance = offset.length();
            if distance <= 0.0 {
                target
            } else {
                target + offset / distance * (distance.min(max) * keep)
            }
        };
        parts.vectors[store::POSITION][i] = pull(parts.vectors[store::POSITION][i]);
        parts.vectors[store::PREVIOUS][i] = pull(parts.vectors[store::PREVIOUS][i]);
        order.slot += order.step;
    }
}
