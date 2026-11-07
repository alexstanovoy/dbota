//! The ground as the particles meet it: traces along a direction to the height the game gives,
//! and the functions that place particles on it.

use bevy::math::Vec3;
use heatel::particle::{Field, GroundMovement, Initializer, PlaceOnGround, TraceMiss};

use super::eval::{HeightAt, Scene, float, vector};
use super::init::Batch;
use super::ops::Call;
use super::store::{self, Particles, slot};

/// Samples a trace takes of the ground; a crossing between two is found by their heights, exact
/// for a vertical trace, and the terrain under one sample's span is about flat.
const SAMPLES: usize = 16;
/// How far behind the particle a trace starts, so one on the ground finds it (Source's).
const START_BACKOFF: f32 = 0.1;
/// Half the span the ground's slope is taken over.
const SLOPE_SPAN: f32 = 8.0;

/// Where a trace from `start` along unit `direction` meets the ground, as a share of `length`;
/// none when it does not within it. A start under the ground is in it: the ground is solid
/// below its surface.
pub fn trace(height: HeightAt<'_>, start: Vec3, direction: Vec3, length: f32) -> Option<f32> {
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let above = |t: f32| {
        let p = start + direction * (length * t);
        p.z - height(p.x, p.y)
    };
    let mut last = (0.0, above(0.0));
    if last.1 <= 0.0 {
        return Some(0.0);
    }
    for k in 1..=SAMPLES {
        let t = k as f32 / SAMPLES as f32;
        let now = above(t);
        if now <= 0.0 {
            return Some(last.0 + (t - last.0) * last.1 / (last.1 - now));
        }
        last = (t, now);
    }
    None
}

/// The ground's normal at `x`, `y`.
pub fn normal(height: HeightAt<'_>, x: f32, y: f32) -> Vec3 {
    let dx = (height(x + SLOPE_SPAN, y) - height(x - SLOPE_SPAN, y)) / (2.0 * SLOPE_SPAN);
    let dy = (height(x, y + SLOPE_SPAN) - height(x, y - SLOPE_SPAN)) / (2.0 * SLOPE_SPAN);
    Vec3::new(-dx, -dy, 1.0).normalize()
}

/// `C_INIT_PositionPlaceOnGround`: each new particle's attribute moved along its trace to the
/// ground, short of it by the offset.
pub fn place_on_ground(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let (Initializer::PositionPlaceOnGround(p), Some(height)) = (init, scene.ground) else {
        return;
    };
    let field = slot(p.field);
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let direction = vector(&p.direction, scene, parts, Some(i), batch.salt).normalize_or_zero();
        if direction == Vec3::ZERO {
            continue;
        }
        let value = parts.vector(field, i);
        let mut pull_back = float(&p.offset, scene, parts, Some(i), batch.salt + 3);
        let length = float(&p.max_trace_length, scene, parts, Some(i), batch.salt + 4);
        let start = value - direction * START_BACKOFF;
        let (fraction, ground_normal) = match trace(height, start, direction, length) {
            Some(fraction) => {
                let hit = start + direction * (length * fraction);
                (fraction, normal(height, hit.x, hit.y))
            }
            None => match p.miss {
                TraceMiss::Nothing => continue,
                TraceMiss::Kill => {
                    miss_kill(p, batch.parts, i);
                    continue;
                }
                TraceMiss::TraceEnd => (1.0, Vec3::ZERO),
            },
        };
        pull_back += p.offset_by_radius * parts.scalars[store::RADIUS][i];
        if let Some(cp) = p.preserve_offset {
            pull_back += start.z - scene.control_points[cp.index()].position.z;
        }
        let delta = direction * (length * fraction - START_BACKOFF - pull_back);
        if !p.previous_only {
            batch.parts.set_vector(field, i, value + delta);
        }
        if p.field == Field::POSITION {
            batch.parts.vectors[store::PREVIOUS][i] += delta;
        }
        if p.set_normal {
            batch.parts.set_vector(slot(p.normal), i, ground_normal);
        }
    }
}

fn miss_kill(p: &PlaceOnGround, parts: &mut Particles, i: usize) {
    parts.scalars[store::LIFETIME][i] = -1.0;
    if p.set_normal {
        parts.set_vector(slot(p.normal), i, Vec3::ZERO);
    }
}

/// What `C_OP_MovementPlaceOnGround` does this step: trace the ground again, and how far
/// particles that move to it smoothly have come.
struct Pass {
    retrace: bool,
    interpolates: bool,
    interpolation: f32,
    /// The height of the control point whose offset particles keep.
    preserve_height: f32,
}

/// `C_OP_MovementPlaceOnGround`: particles kept on the ground below them, traced again each
/// step or when reference control points move, moved to it at once or over a time.
pub fn movement_place_on_ground(
    m: &GroundMovement,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Some(height) = scene.ground else {
        return;
    };
    let Some(pass) = pass(m, scene, call) else {
        return;
    };
    for i in 0..parts.len() {
        settle(m, &pass, scene, height, parts, call.salt, i);
    }
}

fn pass(m: &GroundMovement, scene: &Scene<'_>, call: &mut Call<'_>) -> Option<Pass> {
    let memory = &mut *call.memory;
    let referenced = m.references.iter().any(Option::is_some) || m.lerp_control_point.is_some();
    let retrace_on_move = referenced && m.tolerance > 0.0;
    let interpolates = m.lerp_control_point.is_some() || m.lerp_rate > 0.0;
    let mut retrace = !retrace_on_move && !interpolates;
    if retrace_on_move {
        for (reference, last) in m.references.iter().zip(&mut memory.references) {
            if moved(scene, *reference, last, m.tolerance) {
                retrace = true;
                memory.retrace_time = scene.age;
            }
        }
        moved(
            scene,
            m.lerp_control_point,
            &mut memory.references[2],
            m.tolerance,
        );
    }
    let mut interpolation = 0.0;
    if m.lerp_rate > 0.0 {
        let since = scene.age - memory.retrace_time;
        interpolation = (since / m.lerp_rate).clamp(0.0, 1.0);
        if interpolation == 1.0 && !retrace_on_move {
            retrace = true;
            memory.retrace_time = scene.age;
        }
    } else if let (Some(cp), Some(last)) = (m.lerp_control_point, memory.references[2]) {
        let moved = scene.control_points[cp.index()].position.distance(last) / m.tolerance;
        interpolation = if moved.is_nan() { 0.0 } else { moved.min(1.0) };
    }
    (interpolates || retrace).then_some(Pass {
        retrace,
        interpolates,
        interpolation,
        preserve_height: m
            .preserve_offset
            .map_or(0.0, |cp| scene.control_points[cp.index()].position.z),
    })
}

/// Where a particle stands on the ground found at `at` (`hit`: not the trace's end), lifted by
/// `lift` back along the trace's `direction`, or along the ground's normal when it takes it.
fn ground_target(
    m: &GroundMovement,
    height: HeightAt<'_>,
    parts: &mut Particles,
    i: usize,
    (at, hit): (Vec3, bool),
    (direction, lift): (Vec3, f32),
) -> Vec3 {
    if !m.set_normal {
        return at - direction * lift;
    }
    let ground_normal = if hit {
        normal(height, at.x, at.y)
    } else {
        Vec3::ZERO
    };
    parts.vectors[store::NORMAL][i] = ground_normal;
    at + ground_normal * lift
}

/// Whether control point `cp` moved past `tolerance` from where it was last seen (never seen:
/// it did), remembering it then.
fn moved(
    scene: &Scene<'_>,
    cp: Option<heatel::particle::ControlPoint>,
    last: &mut Option<Vec3>,
    tolerance: f32,
) -> bool {
    let Some(cp) = cp else {
        return false;
    };
    let position = scene.control_points[cp.index()].position;
    if last.is_some_and(|l| l.distance_squared(position) <= tolerance * tolerance) {
        return false;
    }
    *last = Some(position);
    true
}

/// Places particle `i` on the ground for the pass. The previous position at spawn and the
/// hitbox offset hold where a smooth move goes from and to, as Source keeps them.
fn settle(
    m: &GroundMovement,
    pass: &Pass,
    scene: &Scene<'_>,
    height: HeightAt<'_>,
    parts: &mut Particles,
    salt: u32,
    i: usize,
) {
    let direction = vector(&m.direction, scene, parts, Some(i), salt).normalize_or_zero();
    let position = parts.vectors[store::POSITION][i];
    let step = position - parts.vectors[store::PREVIOUS][i];
    let previous = slot(Field::POSITION_PREVIOUS);
    let offset = slot(Field::HITBOX_OFFSET_POSITION);
    let mut from = parts.initial_vector(previous, i);
    let mut to = parts.vectors[store::HITBOX_OFFSET][i];
    let mut lift = float(&m.offset, scene, parts, Some(i), salt + 3);
    if m.scale_offset {
        lift *= parts.scalars[store::RADIUS][i];
    }
    let length = if m.miss == TraceMiss::Nothing {
        m.max_trace_length
    } else {
        m.max_trace_length.max(lift + m.trace_offset)
    };
    if m.preserve_offset.is_some() {
        let mut preserved = parts.initial_vector(offset, i);
        if preserved.x == 0.0 {
            let start = parts.initial_vector(slot(Field::POSITION), i).z;
            preserved = Vec3::new(1.0, preserved.y, start - pass.preserve_height);
            parts.set_initial_vector(offset, i, preserved);
        }
        lift += preserved.z;
    }
    let mut retrace = pass.retrace;
    if pass.interpolates && to == Vec3::ZERO {
        (retrace, from, to) = (true, position, position);
    }
    if retrace {
        let start = position - direction * m.trace_offset;
        let hit = trace(height, start, direction, length);
        if hit.is_none() && m.miss == TraceMiss::Nothing {
            return;
        }
        if hit.is_none() && m.miss == TraceMiss::Kill {
            parts.scalars[store::LIFETIME][i] = -1.0;
        } else {
            let at = start + direction * (length * hit.unwrap_or(1.0));
            let target = ground_target(m, height, parts, i, (at, hit.is_some()), (direction, lift));
            if !pass.interpolates {
                parts.vectors[store::POSITION][i] = target;
                parts.vectors[store::PREVIOUS][i] = target - step;
                return;
            }
            parts.set_initial_vector(previous, i, to);
            parts.vectors[store::HITBOX_OFFSET][i] = target;
            (from, to) = (to, target);
        }
    }
    if pass.interpolates {
        let axes = direction.abs();
        let placed = from.lerp(to, pass.interpolation) * axes + (Vec3::ONE - axes) * position;
        parts.vectors[store::POSITION][i] = placed;
        parts.vectors[store::PREVIOUS][i] = placed - step;
    }
}
