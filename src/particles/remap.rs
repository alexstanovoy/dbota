//! The operators that turn particles about an axis, remap a velocity, a share of a way, a count
//! or noise into an attribute, oscillate or ramp one, and restart their system, as Source 2
//! runs them.

use std::f32::consts::PI;

use bevy::math::{Quat, Vec3};
use heatel::particle::{Field, Operator, PercentageBetween};

use super::eval::{Scene, between, float, noise, remap_clamped, smoothstep, vector};
use super::ops::{Call, Restart};
use super::store::{self, Particles, Slot, slot};

/// Runs one of the operators of turns, remaps, ramps and restarts.
pub fn operate(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    match op {
        Operator::MovementRotateParticleAroundAxis { .. } => {
            rotate_around_axis(op, scene, parts, call)
        }
        Operator::OscillateScalarSimple {
            field,
            rate,
            frequency,
            multiplier,
            offset,
        } => {
            let target = slot(*field);
            let (min, max) = field.range();
            let wave = (PI * frequency * (multiplier * scene.age + offset)).sin();
            let delta = wave * rate * call.strength * scene.dt;
            for i in 0..parts.len() {
                let value = parts.scalar(target, i) + delta;
                parts.set_scalar(target, i, value.clamp(min, max));
            }
        }
        Operator::PercentageBetweenTransforms(p) => percentage_between(p, scene, parts),
        Operator::RampScalarSplineSimple { .. } => ramp_spline_simple(op, scene, parts, call),
        Operator::Noise { .. } => noise_op(op, scene, parts, call),
        Operator::RadiusDecay { min_radius } => {
            for i in 0..parts.len() {
                if parts.scalars[store::RADIUS][i] <= *min_radius {
                    parts.killed[i] = true;
                }
            }
        }
        Operator::RemapCpVelocityToVector {
            control_point,
            field,
            scale,
            normalize,
        } => {
            let point = scene.control_points[control_point.index()];
            let step = point.position - point.previous;
            let value = if *normalize {
                step.normalize_or_zero() * *scale
            } else {
                step * (*scale / scene.frame_len.max(1e-20))
            };
            let target = slot(*field);
            for i in 0..parts.len() {
                parts.set_vector(target, i, value);
            }
        }
        Operator::RemapVelocityToVector {
            field,
            scale,
            normalize,
        } => remap_velocity(*field, *scale, *normalize, scene, parts),
        Operator::RemapParticleCountOnScalarEndCap { .. } => {
            count_on_end_cap(op, scene, parts, call)
        }
        Operator::RestartAfterDuration { .. } => restart_after_duration(op, scene, call),
        _ => {}
    }
}

/// `C_OP_MovementRotateParticleAroundAxis`: particles (and their previous positions) turn
/// about the axis through the transform at the rate, as far as the strength says.
fn rotate_around_axis(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let Operator::MovementRotateParticleAroundAxis {
        axis,
        rate,
        transform,
        local,
    } = op
    else {
        return;
    };
    let mut axis = vector(axis, scene, parts, None, call.salt)
        .try_normalize()
        .unwrap_or(Vec3::Z);
    let frame = scene.transform_at(transform, scene.age);
    if *local {
        axis = frame.rotate(axis);
    }
    let angle = float(rate, scene, parts, None, call.salt + 3).to_radians() * scene.dt;
    let turn = Quat::from_axis_angle(axis, angle);
    let center = frame.position;
    for i in 0..parts.len() {
        for column in [store::POSITION, store::PREVIOUS] {
            let at = parts.vectors[column][i];
            let turned = center + turn * (at - center);
            parts.vectors[column][i] = at.lerp(turned, call.strength);
        }
    }
}

/// `C_OP_PercentageBetweenTransforms`: an attribute from the particle's share of the way from
/// the start to the end (its distance from the start over the way's length, or its projection
/// on the way), remapped and written by the set method.
fn percentage_between(p: &PercentageBetween, scene: &Scene<'_>, parts: &mut Particles) {
    let start = scene.transform_at(&p.start, scene.age).position;
    let end = scene.transform_at(&p.end, scene.age).position;
    let length = start.distance(end);
    let target = slot(p.field);
    let [low, high] = p.input;
    for i in 0..parts.len() {
        let position = parts.vectors[store::POSITION][i];
        let share = if p.radial {
            1.0 / ((length + f32::EPSILON) / (position.distance(start) + f32::EPSILON))
        } else {
            let axis = end - start;
            let squared = axis.length_squared();
            if squared < 1e-5 {
                0.0
            } else {
                axis.dot(position - start) / squared
            }
        };
        if p.active_range && !(low <= share && share <= high) {
            continue;
        }
        let divisor = if high == low { 1.0 } else { high - low };
        let t = ((share - low) / divisor).clamp(0.0, 1.0);
        let value = p.output[0] + (p.output[1] - p.output[0]) * t;
        let (initial, current) = (parts.initial_scalar(target, i), parts.scalar(target, i));
        let value = p.set_method.apply(value, initial, current, scene.dt);
        parts.set_scalar(target, i, value);
    }
}

/// `C_OP_RampScalarSplineSimple`: an attribute changes at the rate eased in and out over the
/// window (seconds of the particle's age, as `C_OP_RampScalarLinearSimple`'s), or eased up to
/// the window's middle and back.
fn ramp_spline_simple(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let Operator::RampScalarSplineSimple {
        rate,
        time: [start, end],
        field,
        ease_out,
    } = op
    else {
        return;
    };
    let target = slot(*field);
    let (min, max) = field.range();
    let step = call.strength * scene.dt * rate;
    let inverse = 1.0 / (end - start + f32::EPSILON);
    for i in 0..parts.len() {
        if parts.scalars[store::LIFETIME][i] <= 0.0 {
            continue;
        }
        let age = scene.age - parts.scalars[store::CREATION][i];
        if age < *start || age >= *end {
            continue;
        }
        let mut fraction = ((age - start) * inverse).clamp(0.0, 1.0);
        if *ease_out {
            fraction = if fraction > 0.5 {
                1.0 - fraction
            } else {
                fraction
            } * 2.0;
        }
        let value = parts.scalar(target, i) + smoothstep(0.0, 1.0, fraction) * step;
        parts.set_scalar(target, i, value.clamp(min, max));
    }
}

/// `C_OP_Noise`: a float attribute from noise at the particle's position (moving with the
/// system's age along X), between the output's ends; added per second when additive.
fn noise_op(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let Operator::Noise {
        field,
        min,
        max,
        scale,
        time_scale,
        additive,
    } = op
    else {
        return;
    };
    let target = slot(*field);
    let mut low = float(min, scene, parts, None, call.salt);
    let mut high = float(max, scene, parts, None, call.salt + 1);
    if field.is_angle() {
        (low, high) = (low.to_radians(), high.to_radians());
    }
    let (mut half, mut base) = (0.5 * (high - low), 0.0);
    base += low + half;
    if *additive {
        (half, base) = (half * scene.dt, base * scene.dt);
    }
    let offset = time_scale * scene.age;
    for i in 0..parts.len() {
        let at = parts.vectors[store::POSITION][i] * *scale;
        let value = base + half * noise(Vec3::new(at.x + offset, at.y, at.z));
        let value = if *additive {
            parts.scalar(target, i) + value
        } else {
            value
        };
        parts.set_scalar(target, i, value);
    }
}

/// `C_OP_RemapVelocityToVector`: a vector attribute from the particle's velocity, or its
/// direction (a particle at rest keeps the attribute); never the position itself.
fn remap_velocity(
    field: Field,
    scale: f32,
    normalize: bool,
    scene: &Scene<'_>,
    parts: &mut Particles,
) {
    if matches!(field, Field::POSITION | Field::POSITION_PREVIOUS) {
        return;
    }
    let target = slot(field);
    let per_second = scale / scene.dt.max(1e-20);
    for i in 0..parts.len() {
        let velocity = parts.vectors[store::POSITION][i] - parts.vectors[store::PREVIOUS][i];
        if normalize {
            if velocity != Vec3::ZERO {
                parts.set_vector(target, i, velocity.normalize_or_zero() * scale);
            }
        } else {
            parts.set_vector(target, i, velocity * per_second);
        }
    }
}

/// `C_OP_RemapParticleCountOnScalarEndCap`: once, when the end cap begins, the particles in
/// the input's places get an attribute from their place (counted from the last when
/// backwards), written by the set method.
fn count_on_end_cap(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::RemapParticleCountOnScalarEndCap {
        field,
        input: [min, max],
        output,
        backwards,
        set_method,
    } = op
    else {
        return;
    };
    if call.memory.done || !scene.in_end_cap {
        return;
    }
    call.memory.done = true;
    let count = parts.len() as i64;
    let (min, max) = (i64::from(*min), i64::from(*max));
    let alpha = matches!(*field, Field::ALPHA | Field::ALPHA_ALTERNATE);
    let (mut low, mut high) = (output[0], output[1]);
    if alpha {
        (low, high) = (low.clamp(0.0, 1.0), high.clamp(0.0, 1.0));
    }
    let start = (if *backwards { count - max - 1 } else { min }).clamp(0, count);
    let end = (if *backwards { count - min } else { max }).clamp(0, count);
    let (first, last) = if *backwards { (high, low) } else { (low, high) };
    let input_end = if *backwards { end - 1 } else { end };
    let target = slot(*field);
    if !matches!(target, Slot::Scalar(_)) {
        return;
    }
    for i in start..end {
        let i = i as usize;
        let value = remap_clamped(i as f32, start as f32, input_end as f32, first, last);
        let (initial, current) = (parts.initial_scalar(target, i), parts.scalar(target, i));
        let value = set_method.apply(value, initial, current, scene.dt);
        parts.set_scalar(target, i, current + (value - current) * call.strength);
    }
}

/// `C_OP_RestartAfterDuration`: while the system emits, once it is a random duration (times a
/// control point's component) past its last restart, it (or its children of a group) starts
/// over, and a new duration is drawn.
fn restart_after_duration(op: &Operator, scene: &Scene<'_>, call: &mut Call<'_>) {
    let Operator::RestartAfterDuration {
        duration: [min, max],
        control_point,
        group,
        only_children,
    } = op
    else {
        return;
    };
    if !scene.emitting {
        return;
    }
    let memory = &mut *call.memory;
    let draw = |n: u32| between(scene.seed, call.salt.wrapping_add(n), *min, *max);
    let mut duration = *memory.duration.get_or_insert_with(|| draw(0));
    if let Some((cp, component)) = control_point {
        duration *= scene.control_points[cp.index()].position[usize::from(*component)];
    }
    if scene.age <= duration + memory.restarted_at {
        return;
    }
    call.restart = Some(if *only_children {
        Restart::Children(*group)
    } else {
        Restart::System
    });
    memory.restarted_at = scene.age;
    memory.restarts = memory.restarts.wrapping_add(1);
    memory.duration = Some(draw(memory.restarts));
}
