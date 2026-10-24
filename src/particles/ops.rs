//! The operators a particle runs each step and the forces that push it, as Source 2 runs them.

use std::f32::consts::TAU;

use bevy::math::Vec3;
use heatel::particle::{
    ControlPointsFromParticles, CpOffset, Field, Force, Operator, Oscillate, PositionLock,
};

use super::eval::{
    ControlPoint, Scene, between, between_exp, bias, float, noise3, normalized_age, smoothstep,
    vector,
};
use super::store::{self, Particles, slot};

/// A control point an operator sets: on the system's children of a group (`None` every
/// child), on the system itself, or on its `n`-th child of a group.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Children(i32),
    Own,
    Child(i32, usize),
}

/// What one operator call has: its strength, keys for draws, its state, and where it puts
/// the control points it sets.
pub struct Call<'a> {
    pub strength: f32,
    pub salt: u32,
    pub lock: &'a mut Option<ControlPoint>,
    pub writes: &'a mut Vec<(Target, usize, ControlPoint)>,
    pub forces: &'a [heatel::particle::Function<Force>],
    /// The strengths of the forces now.
    pub force_strengths: &'a [f32],
}

/// Runs `op` on every particle.
pub fn operate(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    if parts.is_empty() {
        return;
    }
    if life(op, scene, parts, call) {
        return;
    }
    match op {
        Operator::BasicMovement { gravity, drag } => movement(scene, parts, call, gravity, drag),
        Operator::PositionLock(lock) => position_lock(lock, scene, parts, call),
        Operator::SpinUpdate => {
            for i in 0..parts.len() {
                let age = scene.age - parts.scalars[store::CREATION][i];
                parts.scalars[store::ROLL][i] += parts.scalars[store::ROLL_SPEED][i]
                    * scene.dt.min(age.max(0.0))
                    * call.strength;
            }
        }
        Operator::Spin {
            rate_degrees,
            min_rate_degrees,
            stop_time,
        } => {
            spin(
                scene,
                parts,
                call.strength,
                *rate_degrees,
                *min_rate_degrees,
                *stop_time,
            );
        }
        Operator::OscillateVector(oscillate) => oscillate_vector(oscillate, scene, parts, call),
        Operator::VectorNoise { .. } => vector_noise(op, scene, parts, call),
        Operator::SetFloat { .. } => set_float(op, scene, parts, call),
        Operator::SetVec { .. } => set_vec(op, scene, parts, call),
        Operator::OrientTo2dDirection { .. } => orient_to_2d_direction(op, parts),
        Operator::MaxVelocity { max, min } => max_velocity(scene, parts, call, max, min),
        Operator::CpOffsetToPercentage(c) => cp_offset(c, scene, call),
        Operator::SetChildControlPoints(from) => {
            from_particles(from, Target::Children(from.group), scene, parts, call)
        }
        Operator::SetControlPointsToParticle(from) => {
            from_particles(from, Target::Own, scene, parts, call)
        }
        Operator::SetPerChildControlPoint { .. } => {
            set_per_child_control_point(op, scene, parts, call)
        }
        _ => {}
    }
}

/// The operators of a particle's life: its fades, ramps and death. Whether `op` was one.
fn life(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) -> bool {
    match op {
        Operator::Decay { rope } => decay(scene, parts, *rope),
        Operator::InterpolateRadius {
            time,
            start_scale,
            end_scale,
            ease,
            bias: b,
        } => {
            interpolate_radius(
                scene,
                parts,
                call,
                *time,
                (start_scale, end_scale),
                *ease,
                *b,
            );
        }
        Operator::FadeInSimple { .. } => fade_in_simple(op, scene, parts),
        Operator::FadeOutSimple { .. } => fade_out_simple(op, scene, parts),
        Operator::FadeOut { .. } => fade_out(op, scene, parts, call),
        Operator::FadeAndKill { .. } => fade_and_kill(op, scene, parts, call),
        Operator::ColorInterpolate { .. } => color_interpolate(op, scene, parts, call),
        Operator::RampScalarLinearSimple { .. } => {
            ramp_scalar_linear_simple(op, scene, parts, call)
        }
        Operator::RampScalarLinear { .. } => ramp_scalar_linear(op, scene, parts, call),
        _ => return false,
    }
    true
}

fn blend(current: f32, value: f32, strength: f32) -> f32 {
    if strength >= 1.0 {
        value
    } else {
        value * strength + current * (1.0 - strength)
    }
}

fn decay(scene: &Scene<'_>, parts: &mut Particles, rope: bool) {
    let past = |parts: &Particles, i: usize| {
        scene.age - parts.scalars[store::CREATION][i] > parts.scalars[store::LIFETIME][i]
    };
    let last = parts.len() - 1;
    for i in 0..parts.len() {
        let lifetime = parts.scalars[store::LIFETIME][i];
        let dead = if rope {
            past(parts, i) && past(parts, (i + 1).min(last)) && past(parts, (i + 2).min(last))
        } else {
            lifetime <= 0.0 || lifetime <= scene.age - parts.scalars[store::CREATION][i]
        };
        if dead {
            parts.killed[i] = true;
        }
    }
}

fn interpolate_radius(
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &Call<'_>,
    time: [f32; 2],
    scales: (&heatel::particle::FloatInput, &heatel::particle::FloatInput),
    ease: bool,
    b: f32,
) {
    if time[1] <= time[0] {
        return;
    }
    for i in 0..parts.len() {
        let lifetime = parts.scalars[store::LIFETIME][i];
        if lifetime <= 0.0 {
            continue;
        }
        let t = normalized_age(scene, parts, i);
        let frame = scene.dt / lifetime;
        let window_end = if lifetime * (time[1] - time[0]) <= scene.dt {
            time[0] + frame
        } else {
            time[1]
        };
        if t < time[0] || t > frame + window_end {
            continue;
        }
        let start = float(scales.0, scene, parts, Some(i), call.salt);
        let end = float(scales.1, scene, parts, Some(i), call.salt + 1);
        let mut k = if time[0] == time[1] {
            1.0
        } else {
            (t - time[0]) / (time[1] - time[0])
        };
        if ease {
            k = k * k * (3.0 - 2.0 * k);
        } else if b != 0.5 {
            k = bias(k, b);
        }
        let scale = (start + (end - start) * k).clamp(start.min(end), start.max(end));
        parts.scalars[store::RADIUS][i] = parts.initial_scalars[store::RADIUS][i] * scale;
    }
}

/// Verlet motion: the step repeated with drag, plus gravity and the forces.
fn movement(
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
    gravity: &heatel::particle::VectorInput,
    drag: &heatel::particle::FloatInput,
) {
    let gravity = vector(gravity, scene, parts, None, call.salt);
    for (force, strength) in call.forces.iter().zip(call.force_strengths) {
        if *strength > 0.0 {
            apply_force(&force.class, scene, parts, *strength, call.salt);
        }
    }
    let dt = scene.dt;
    let drag = float(drag, scene, parts, None, call.salt + 3).clamp(0.0, 1.0 - f32::EPSILON);
    let factor = (1.0 - drag).powf(dt * 30.0);
    for i in 0..parts.len() {
        let position = parts.vectors[store::POSITION][i];
        let acceleration = gravity + parts.force[i];
        let step = (position - parts.vectors[store::PREVIOUS][i]) * factor
            + parts.scalars[store::FORCE_SCALE][i] * dt * dt * acceleration;
        parts.force[i] = Vec3::ZERO;
        parts.vectors[store::PREVIOUS][i] = position;
        parts.vectors[store::POSITION][i] = position + step;
    }
}

fn apply_force(force: &Force, scene: &Scene<'_>, parts: &mut Particles, strength: f32, salt: u32) {
    match force {
        Force::AttractToControlPoint {
            transform,
            amount,
            falloff,
            min_distance,
            component_scale,
        } => {
            let frame = scene.transform_at(transform, scene.age);
            let scale = frame.rotate(Vec3::from(*component_scale)) * strength;
            let exponent = -(falloff.clamp(-255.0, 255.0) * 4.0).trunc() * 0.25;
            for i in 0..parts.len() {
                let a = float(amount, scene, parts, Some(i), salt + 100);
                let diff = parts.vectors[store::POSITION][i] - frame.position;
                let mut distance = diff.length();
                if distance < float(min_distance, scene, parts, Some(i), salt + 101) {
                    distance = 1.0;
                }
                if distance <= 1e-6 {
                    continue;
                }
                let magnitude = a / distance * distance.powf(exponent);
                parts.force[i] -= diff * magnitude * scale;
            }
        }
        Force::RandomForce { min, max } => {
            for i in 0..parts.len() {
                let seed = parts.seed[i] ^ scene.step_seed.wrapping_mul(0x2545_F491);
                let f = Vec3::new(
                    between(seed, salt, min[0], max[0]),
                    between(seed, salt + 1, min[1], max[1]),
                    between(seed, salt + 2, min[2], max[2]),
                );
                parts.force[i] += f * strength;
            }
        }
        _ => {}
    }
}

fn max_velocity(
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &Call<'_>,
    max: &heatel::particle::FloatInput,
    min: &heatel::particle::FloatInput,
) {
    for i in 0..parts.len() {
        let max_step = float(max, scene, parts, Some(i), call.salt) * scene.dt;
        let min_step = float(min, scene, parts, Some(i), call.salt + 1) * scene.dt;
        let previous = parts.vectors[store::PREVIOUS][i];
        let mut step = parts.vectors[store::POSITION][i] - previous;
        if min_step > 0.0 && step.length_squared() <= 0.0 {
            step = Vec3::new(
                between(parts.seed[i], call.salt + 2, -0.1, 0.1),
                between(parts.seed[i], call.salt + 3, -0.1, 0.1),
                0.0,
            );
            step.z = 0.1 - step.x - step.y;
        }
        let length = step.length();
        let mut scale = 1.0;
        if max_step < length && length > 0.0 {
            scale = max_step / length;
        }
        if length < min_step && length > 0.0 {
            scale = min_step / length;
        }
        let target = step * scale + previous;
        let current = parts.vectors[store::POSITION][i];
        parts.vectors[store::POSITION][i] = current.lerp(target, call.strength.min(1.0));
    }
}

fn position_lock(
    lock: &PositionLock,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let now = scene.transform_at(&lock.transform, scene.age);
    let before = call
        .lock
        .get_or_insert_with(|| scene.transform_at(&lock.transform, scene.frame_start));
    let delta = now.position - before.position;
    *call.lock = Some(now);
    let jump = lock.jump_threshold != 0.0
        && delta.length_squared() > lock.jump_threshold * lock.jump_threshold;
    let scale = vector(&lock.scale, scene, parts, None, call.salt);
    let range_bias = float(&lock.range_bias, scene, parts, None, call.salt + 3);
    let (output, previous) = (slot(lock.output), slot(lock.output_previous));
    for i in 0..parts.len() {
        let mut window = 1.0;
        if lock.start[0] < 1.0 {
            let seed = parts.seed[i];
            let start = between_exp(
                seed,
                call.salt + 4,
                lock.start[2],
                lock.start[0],
                lock.start[1],
            );
            let end = between_exp(seed, call.salt + 5, lock.end[2], lock.end[0], lock.end[1]);
            let life = normalized_age(scene, parts, i).clamp(0.0, 1.0);
            window = if jump {
                1.0
            } else {
                1.0 - ((life - start) / (end - start)).clamp(0.0, 1.0)
            };
            if window <= 0.0 {
                continue;
            }
        }
        let current = parts.vector(output, i);
        let age = scene.age - parts.scalars[store::CREATION][i];
        let lived = if scene.dt > 0.0 {
            age.min(scene.dt) / scene.dt
        } else {
            1.0
        };
        let mut moved = delta * (call.strength * lived * window);
        if lock.range != 0.0 {
            let distance = now.position.distance(current + moved);
            moved *= 1.0 - bias((distance / lock.range).min(1.0), range_bias);
        }
        let moved = moved * scale;
        parts.set_vector(output, i, current + moved);
        let before = parts.vector(previous, i);
        parts.set_vector(previous, i, before + moved * lock.previous_scale);
    }
}

fn spin(
    scene: &Scene<'_>,
    parts: &mut Particles,
    strength: f32,
    rate: i32,
    min_rate: i32,
    stop: f32,
) {
    let rate = strength * (rate as f32).to_radians();
    if rate == 0.0 {
        return;
    }
    let mut step = (rate * TAU).abs() * scene.dt;
    if stop == 0.0 {
        step %= TAU;
    }
    if rate < 0.0 {
        step = -step;
    }
    let least = (min_rate as f32).to_radians() * TAU * scene.dt;
    for i in 0..parts.len() {
        let stop_age = parts.scalars[store::LIFETIME][i] * stop;
        let age = scene.age - parts.scalars[store::CREATION][i];
        let left = if stop == 0.0 {
            1.0
        } else if stop_age != 0.0 {
            (1.0 - age / stop_age).max(0.0)
        } else {
            0.0
        };
        let mut delta = left * step;
        delta = if rate >= 0.0 {
            delta.max(least)
        } else {
            delta.min(least)
        };
        let mut angle = parts.scalars[store::ROLL][i] + delta;
        if angle >= TAU {
            angle -= TAU;
        }
        if angle <= -TAU {
            angle += TAU;
        }
        parts.scalars[store::ROLL][i] = angle;
    }
}

fn oscillate_vector(o: &Oscillate, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let slot = slot(o.field);
    let unit = matches!(o.field, Field::COLOR);
    let step = call.strength * scene.dt;
    for i in 0..parts.len() {
        if parts.scalars[store::LIFETIME][i] <= 0.0 {
            continue;
        }
        let seed = parts.seed[i];
        let life = normalized_age(scene, parts, i);
        let window = if o.proportional_window {
            life
        } else {
            scene.age - parts.scalars[store::CREATION][i]
        };
        if window < between(seed, call.salt + 11, o.start[0], o.start[1])
            || window >= between(seed, call.salt + 12, o.end[0], o.end[1])
        {
            continue;
        }
        let pick = |salt: u32, min: &[f32; 3], max: &[f32; 3], axis: usize| {
            between(seed, call.salt + salt, min[axis], max[axis])
        };
        let rate = Vec3::new(
            pick(3, &o.rate_min, &o.rate_max, 0),
            pick(7, &o.rate_min, &o.rate_max, 1),
            pick(9, &o.rate_min, &o.rate_max, 2),
        );
        let frequency = Vec3::new(
            pick(8, &o.frequency_min, &o.frequency_max, 0),
            pick(12, &o.frequency_min, &o.frequency_max, 1),
            pick(15, &o.frequency_min, &o.frequency_max, 2),
        );
        let multiplier = float(&o.multiplier, scene, parts, Some(i), call.salt + 20);
        let offset = float(&o.offset, scene, parts, Some(i), call.salt + 21);
        let scale = step * float(&o.rate_scale, scene, parts, Some(i), call.salt + 22);
        let phase = |f: f32| {
            let x = if o.proportional {
                life * f * multiplier + offset
            } else {
                f * (multiplier * scene.age + offset)
            };
            (x * std::f32::consts::PI).sin()
        };
        let value =
            rate * scale * Vec3::new(phase(frequency.x), phase(frequency.y), phase(frequency.z));
        let out = parts.vector(slot, i) + value;
        parts.set_vector(
            slot,
            i,
            if unit {
                out.clamp(Vec3::ZERO, Vec3::ONE)
            } else {
                out
            },
        );
        if o.offset_position {
            parts.vectors[store::PREVIOUS][i] += value;
        }
    }
}

/// Sets the output control point off from its origin by how far the input has come along the
/// way from start to end.
fn cp_offset(c: &CpOffset, scene: &Scene<'_>, call: &mut Call<'_>) {
    let point = |cp: heatel::particle::ControlPoint| scene.control_points[cp.index()].position;
    let (start, end, input) = (point(c.start), point(c.end), point(c.input));
    let length = start.distance(end);
    let offset = if c.scale_offset {
        Vec3::from(c.offset) * length
    } else {
        Vec3::from(c.offset)
    };
    let share = if c.radial {
        (start.distance(input) + f32::EPSILON) / length
    } else {
        let axis = end - start;
        if axis.length_squared() < 1e-5 {
            0.0
        } else {
            (input - start).dot(axis) / axis.length_squared()
        }
    };
    let [low, high] = c.input_range;
    let t = if low == high {
        f32::from(u8::from(share >= high))
    } else {
        ((share - low) / (high - low)).clamp(0.0, 1.0)
    };
    let t = bias(t.clamp(0.0, 1.0), c.bias.clamp(f32::MIN_POSITIVE, 1.0));
    let mut out = scene.control_points[c.output.index()];
    out.position = offset * t + point(c.origin);
    call.writes.push((Target::Own, c.output.index(), out));
}

/// A control point at particle `i`, facing where it moves when `orientation`.
fn particle_point(parts: &Particles, i: usize, orientation: bool) -> ControlPoint {
    let position = parts.vectors[store::POSITION][i];
    let mut point = ControlPoint {
        position,
        previous: position,
        ..ControlPoint::default()
    };
    if orientation {
        point.face(position - parts.vectors[store::PREVIOUS][i]);
    }
    point
}

fn from_particles(
    from: &ControlPointsFromParticles,
    target: Target,
    scene: &Scene<'_>,
    parts: &Particles,
    call: &mut Call<'_>,
) {
    let len = parts.len();
    let first = float(&from.first_source, scene, parts, None, call.salt).max(0.0) as usize;
    let count = usize::from(from.count)
        .min(len.saturating_sub(first))
        .min(64 - from.first_control_point.index());
    for k in 0..count {
        let index = if from.reverse {
            len.saturating_sub(first + k + 1)
        } else {
            first + k
        };
        if index >= len {
            break;
        }
        let point = particle_point(parts, index, from.orientation);
        call.writes
            .push((target, from.first_control_point.index() + k, point));
    }
}

fn vector_noise(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::VectorNoise {
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
    let slot = slot(*field);
    let half = (Vec3::from(*max) - Vec3::from(*min)) * 0.5;
    let base = half + Vec3::from(*min);
    for i in 0..parts.len() {
        let p = parts.vectors[store::POSITION][i] * *scale
            + Vec3::new(time_scale * scene.age, 0.0, 0.0);
        let mut value = noise3(p) * half + base;
        if *additive {
            value *= scene.dt * call.strength;
            let current = parts.vector(slot, i);
            parts.set_vector(slot, i, current + value);
            if *field == Field::POSITION {
                parts.vectors[store::PREVIOUS][i] += value;
            }
        } else {
            parts.set_vector(slot, i, value);
        }
    }
}

fn set_float(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::SetFloat {
        field,
        value,
        set_method,
        lerp,
    } = op
    else {
        return;
    };
    let slot = slot(*field);
    let (min, max) = field.range();
    for i in 0..parts.len() {
        let mut v = float(value, scene, parts, Some(i), call.salt);
        let t = float(lerp, scene, parts, Some(i), call.salt + 1).clamp(0.0, 1.0);
        if field.is_angle() && set_method.takes_degrees() {
            v = v.to_radians();
        }
        let current = parts.scalar(slot, i);
        let initial = parts.initial_scalar(slot, i);
        if *set_method == heatel::particle::SetMethod::RampCurrent {
            parts.set_scalar(
                slot,
                i,
                (t * call.strength * scene.dt * v + current).clamp(min, max),
            );
            continue;
        }
        let target = set_method.apply(v, initial, current, scene.dt);
        let blended = (current + (target - current) * t).clamp(min, max);
        let base = if set_method.reads_initial() {
            initial
        } else {
            current
        };
        parts.set_scalar(slot, i, blend(base, blended, call.strength));
    }
}

fn set_vec(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::SetVec {
        field,
        value,
        set_method,
        lerp,
        normalized,
    } = op
    else {
        return;
    };
    let slot = slot(*field);
    let unit = matches!(*field, Field::COLOR);
    for i in 0..parts.len() {
        let v = vector(value, scene, parts, Some(i), call.salt);
        let t = (float(lerp, scene, parts, Some(i), call.salt + 3) * call.strength).clamp(0.0, 1.0);
        let current = parts.vector(slot, i);
        let initial = parts.initial_vector(slot, i);
        let apply = |a: f32, init: f32, cur: f32| set_method.apply(a, init, cur, scene.dt);
        let mut target = Vec3::new(
            apply(v.x, initial.x, current.x),
            apply(v.y, initial.y, current.y),
            apply(v.z, initial.z, current.z),
        );
        if unit {
            target = target.clamp(Vec3::ZERO, Vec3::ONE);
        }
        let mut out = current.lerp(target, t);
        if *normalized && out != Vec3::ZERO {
            out = out.normalize_or_zero();
        }
        parts.set_vector(slot, i, out);
    }
}

fn orient_to_2d_direction(op: &Operator, parts: &mut Particles) {
    let Operator::OrientTo2dDirection {
        offset_degrees,
        strength,
        field,
    } = op
    else {
        return;
    };
    let slot = slot(*field);
    for i in 0..parts.len() {
        let d = parts.vectors[store::POSITION][i] - parts.vectors[store::PREVIOUS][i];
        if d.x == 0.0 && d.y == 0.0 {
            continue;
        }
        let target = d.y.atan2(d.x) + std::f32::consts::PI + offset_degrees.to_radians();
        let current = parts.scalar(slot, i);
        parts.set_scalar(slot, i, current + (target - current) * strength);
    }
}

fn set_per_child_control_point(
    op: &Operator,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Operator::SetPerChildControlPoint {
        group,
        control_point,
        count,
        first,
        increment,
        per_particle,
        orientation,
    } = op
    else {
        return;
    };
    let len = parts.len() as i32;
    let total = if *per_particle { len } else { *count };
    let mut source = float(first, scene, parts, None, call.salt) as i32;
    let step = float(increment, scene, parts, None, call.salt + 1) as i32 % len.max(1);
    for child in 0..total.clamp(0, 64) as usize {
        if !(0..len).contains(&source) {
            break;
        }
        let point = particle_point(parts, source as usize, *orientation);
        call.writes
            .push((Target::Child(*group, child), control_point.index(), point));
        source += step;
        if source < 0 {
            source += len;
        }
    }
}

fn fade_in_simple(op: &Operator, scene: &Scene<'_>, parts: &mut Particles) {
    let Operator::FadeInSimple { time, field } = op else {
        return;
    };
    let slot = slot(*field);
    for i in 0..parts.len() {
        let length = time * parts.scalars[store::LIFETIME][i];
        let age = scene.age - parts.scalars[store::CREATION][i];
        if (age - scene.dt).max(0.0) < length {
            let initial = parts.initial_scalar(slot, i);
            parts.set_scalar(slot, i, smoothstep(0.0, length, age) * initial);
        }
    }
}

fn fade_out_simple(op: &Operator, scene: &Scene<'_>, parts: &mut Particles) {
    let Operator::FadeOutSimple { time, field } = op else {
        return;
    };
    let slot = slot(*field);
    let start = 1.0 - time;
    for i in 0..parts.len() {
        let t = normalized_age(scene, parts, i);
        if start < t {
            let faded = (1.0 - smoothstep(start, 1.0, t)) * parts.initial_scalar(slot, i);
            let current = parts.scalar(slot, i);
            parts.set_scalar(slot, i, faded.min(current));
        }
    }
}

fn fade_out(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::FadeOut {
        min,
        max,
        exponent,
        proportional,
        ease,
    } = op
    else {
        return;
    };
    for i in 0..parts.len() {
        let time = if min == max {
            *min
        } else {
            between_exp(parts.seed[i], call.salt, *exponent, *min, *max)
        };
        let lifetime = parts.scalars[store::LIFETIME][i];
        let age = scene.age - parts.scalars[store::CREATION][i];
        let left = if *proportional {
            1.0 - normalized_age(scene, parts, i)
        } else {
            lifetime - age
        };
        if left < time {
            let mut t = (1.0 - left / time).clamp(0.0, 1.0);
            if *ease {
                t = smoothstep(0.0, 1.0, t);
            }
            parts.scalars[store::ALPHA][i] =
                ((1.0 - t) * parts.initial_scalars[store::ALPHA][i]).max(0.0);
        }
    }
}

fn fade_and_kill(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::FadeAndKill {
        fade_in,
        fade_out,
        start_alpha,
        end_alpha,
    } = op
    else {
        return;
    };
    for i in 0..parts.len() {
        let lifetime = parts.scalars[store::LIFETIME][i];
        let age = scene.age - parts.scalars[store::CREATION][i];
        if lifetime < age {
            parts.killed[i] = true;
            continue;
        }
        let t = age / lifetime.max(1e-4);
        let initial = parts.initial_scalars[store::ALPHA][i];
        let mut alpha = parts.scalars[store::ALPHA][i];
        if t >= fade_in[0] && t < fade_in[1] {
            alpha = initial
                * (start_alpha + (1.0 - start_alpha) * smoothstep(fade_in[0], fade_in[1], t));
        }
        if t >= fade_out[0] && t < fade_out[1] {
            alpha = initial * (1.0 + (end_alpha - 1.0) * smoothstep(fade_out[0], fade_out[1], t));
        }
        let current = parts.scalars[store::ALPHA][i];
        parts.scalars[store::ALPHA][i] = blend(current, alpha, call.strength);
    }
}

fn color_interpolate(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &mut Call<'_>) {
    let Operator::ColorInterpolate {
        fade,
        time,
        field,
        ease,
    } = op
    else {
        return;
    };
    if time[0] == time[1] {
        return;
    }
    let slot = slot(*field);
    for i in 0..parts.len() {
        let initial = parts.initial_vector(slot, i);
        let mut color = parts.vector(slot, i);
        if parts.scalars[store::LIFETIME][i] > 0.0 {
            let t = normalized_age(scene, parts, i);
            let f = if *ease {
                smoothstep(time[0], time[1], t)
            } else {
                ((t - time[0]) / (time[1] - time[0])).clamp(0.0, 1.0)
            };
            color = initial.lerp(Vec3::from(*fade), f);
        }
        parts.set_vector(slot, i, initial.lerp(color, call.strength.min(1.0)));
    }
}

fn ramp_scalar_linear_simple(
    op: &Operator,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Operator::RampScalarLinearSimple { rate, time, field } = op else {
        return;
    };
    let slot = slot(*field);
    let (min, max) = field.range();
    let step = rate * call.strength * scene.dt;
    for i in 0..parts.len() {
        if parts.scalars[store::LIFETIME][i] <= 0.0 {
            continue;
        }
        let t = normalized_age(scene, parts, i);
        if time[0] <= t && t < time[1] {
            let v = parts.scalar(slot, i) + step;
            parts.set_scalar(slot, i, v.clamp(min, max));
        }
    }
}

fn ramp_scalar_linear(
    op: &Operator,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Operator::RampScalarLinear {
        rate,
        start,
        end,
        proportional,
        field,
    } = op
    else {
        return;
    };
    let slot = slot(*field);
    let (min, max) = field.range();
    let windowed =
        start[0] != 0.0 || start[1] != 0.0 || end[0] != 1.0 || end[1] != 1.0 || !proportional;
    for i in 0..parts.len() {
        if parts.scalars[store::LIFETIME][i] <= 0.0 {
            continue;
        }
        let seed = parts.seed[i];
        if windowed {
            let age = scene.age - parts.scalars[store::CREATION][i];
            let t = if *proportional {
                normalized_age(scene, parts, i)
            } else {
                age
            };
            if t < between(seed, call.salt + 11, start[0], start[1])
                || t >= between(seed, call.salt + 12, end[0], end[1])
            {
                continue;
            }
        }
        let v = parts.scalar(slot, i)
            + between(seed, call.salt, rate[0], rate[1]) * scene.dt * call.strength;
        parts.set_scalar(slot, i, v.clamp(min, max));
    }
}
