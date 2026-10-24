//! The initializers a new particle runs, as Source 2 runs them.

use std::f32::consts::{PI, TAU};

use bevy::math::{Quat, Vec3};
use heatel::particle::{DistanceToCp, Field, Initializer, Ring, Sphere, Transform, Warp};

use super::eval::{
    Scene, between, between_exp, draw, float, in_ball, noise, noise3, remap_clamped, vector,
};
use super::store::{self, Particles, Slot, slot};
use super::{body, ground, parent};

/// A bit per attribute an initializer declares it writes: under behaviour version 6 the first
/// writer of an attribute wins.
pub fn writes(init: &Initializer) -> u64 {
    let bit = |field: Field| 1u64 << field.id();
    match init {
        Initializer::InitFloat { field, .. }
        | Initializer::RandomColor { field, .. }
        | Initializer::RemapParticleCountToScalar { field, .. } => bit(*field),
        Initializer::RandomSequence { .. } => bit(Field::SEQUENCE_NUMBER),
        Initializer::RandomYawFlip { .. } => bit(Field::YAW),
        Initializer::CreateWithinSphere(s) => {
            let position = if s.output == Field::POSITION {
                bit(Field::POSITION) | bit(Field::POSITION_PREVIOUS)
            } else {
                bit(s.output)
            };
            position | bit(s.velocity)
        }
        Initializer::PositionOffset { .. } | Initializer::RingWave(_) => {
            bit(Field::POSITION) | bit(Field::POSITION_PREVIOUS)
        }
        Initializer::InitialVelocityNoise { .. }
        | Initializer::VelocityRandom { .. }
        | Initializer::InheritVelocity { .. } => bit(Field::POSITION_PREVIOUS),
        Initializer::AgeNoise { .. } => bit(Field::CREATION_TIME),
        _ => more_writes(init),
    }
}

/// What the initializers of shadows, souls and the ground write.
fn more_writes(init: &Initializer) -> u64 {
    let bit = |field: Field| 1u64 << field.id();
    let position = bit(Field::POSITION) | bit(Field::POSITION_PREVIOUS);
    match init {
        Initializer::InitVec {
            field,
            write_previous,
            ..
        } => {
            if *write_previous && *field == Field::POSITION {
                position
            } else {
                bit(*field)
            }
        }
        Initializer::PositionPlaceOnGround(p) => {
            let moved = if p.field == Field::POSITION {
                position
            } else {
                bit(p.field)
            };
            moved | bit(Field::LIFE_DURATION) | if p.set_normal { bit(p.normal) } else { 0 }
        }
        Initializer::PositionWarp(_) => position | bit(Field::RADIUS),
        Initializer::VelocityFromCp { .. } => bit(Field::POSITION_PREVIOUS),
        Initializer::DistanceToCp(d) => bit(d.field),
        Initializer::Orient2dRelToCp { field, .. } => bit(*field),
        Initializer::CreateFromParentParticles { .. } => {
            position
                | bit(Field::LIFE_DURATION)
                | bit(Field::PARENT_PARTICLE_INDEX)
                | bit(Field::PARENT_PARTICLE_ID)
        }
        Initializer::SetHitboxToModel(_) => {
            bit(Field::HITBOX_INDEX) | bit(Field::HITBOX_OFFSET_POSITION)
        }
        _ => 0,
    }
}

/// What one initializer call has: the particles made this step from `first` on, the
/// attributes written before it, its keys for draws, its counter and where it has walked to
/// (both kept from call to call).
pub struct Batch<'a> {
    pub parts: &'a mut Particles,
    pub first: usize,
    pub written: u64,
    pub salt: u32,
    pub counter: &'a mut u32,
    pub walk: &'a mut f32,
}

/// Runs `init` on the batch.
pub fn initialize(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let (first, len) = (batch.first, batch.parts.len());
    match init {
        Initializer::InitFloat {
            field,
            value,
            set_method,
            strength,
        } => {
            let slot = slot(*field);
            for i in first..len {
                let mut v = float(value, scene, batch.parts, Some(i), batch.salt);
                if field.is_angle() && set_method.takes_degrees() {
                    v = v.to_radians();
                }
                let current = batch.parts.scalar(slot, i);
                let target = set_method.apply(v, current, current, scene.dt);
                let s = float(strength, scene, batch.parts, Some(i), batch.salt + 7);
                batch
                    .parts
                    .set_scalar(slot, i, current + (target - current) * s);
            }
        }
        Initializer::RandomColor { min, max, field } => {
            let slot = slot(*field);
            for i in first..len {
                let t = draw(batch.parts.seed[i], batch.salt);
                batch
                    .parts
                    .set_vector(slot, i, Vec3::from(*min).lerp(Vec3::from(*max), t));
            }
        }
        Initializer::RandomSequence {
            min,
            max,
            shuffle,
            linear,
        } => sequence(batch, *min, *max, *shuffle || *linear),
        Initializer::RandomYawFlip { percent } => {
            for i in first..len {
                if draw(batch.parts.seed[i], batch.salt) < *percent {
                    let yaw = &mut batch.parts.scalars[store::YAW][i];
                    *yaw = *yaw % PI + PI;
                }
            }
        }
        Initializer::CreateWithinSphere(sphere) => within_sphere(sphere, scene, batch),
        Initializer::PositionOffset { .. } => position_offset(init, scene, batch),
        Initializer::InitialVelocityNoise { .. } => velocity_noise(init, scene, batch),
        Initializer::VelocityRandom { .. } => velocity_random(init, scene, batch),
        Initializer::RingWave(ring) => ring_wave(ring, scene, batch),
        Initializer::RemapParticleCountToScalar { .. } => count_to_scalar(init, scene, batch),
        Initializer::AgeNoise { .. } => age_noise(init, batch),
        Initializer::InheritVelocity {
            control_point,
            scale,
        } => {
            let cp = &scene.control_points[control_point.index()];
            let step = cp.position - cp.previous;
            if step.length() < 100.0 {
                for i in first..len {
                    batch.parts.vectors[store::PREVIOUS][i] -= step * *scale;
                }
            }
        }
        _ => initialize_more(init, scene, batch),
    }
}

/// Runs one of the initializers of shadows, souls and the ground.
fn initialize_more(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    match init {
        Initializer::InitVec { .. } => init_vec(init, scene, batch),
        Initializer::PositionPlaceOnGround(_) => ground::place_on_ground(init, scene, batch),
        Initializer::PositionWarp(warp) => position_warp(warp, scene, batch),
        Initializer::VelocityFromCp { .. } => velocity_from_cp(init, scene, batch),
        Initializer::DistanceToCp(d) => distance_to_cp(d, scene, batch),
        Initializer::Orient2dRelToCp {
            control_point,
            field,
            offset_degrees,
        } => {
            let center = scene.control_points[control_point.index()].position;
            let slot = slot(*field);
            for i in batch.first..batch.parts.len() {
                let position = batch.parts.vectors[store::POSITION][i];
                if let Some(angle) = angle_from(position, center) {
                    batch
                        .parts
                        .set_scalar(slot, i, angle + offset_degrees.to_radians());
                }
            }
        }
        Initializer::CreateFromParentParticles { .. } => {
            parent::create_from_parent(init, scene, batch)
        }
        Initializer::SetHitboxToModel(_) => body::set_hitbox(init, scene, batch),
        _ => {}
    }
}

/// The angle on the ground plane of `position` seen from `center`, turned half around; none
/// when it is right above or below.
pub fn angle_from(position: Vec3, center: Vec3) -> Option<f32> {
    let (dx, dy) = (position.x - center.x, position.y - center.y);
    (dx != 0.0 || dy != 0.0).then(|| dy.atan2(dx) + PI)
}

fn init_vec(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::InitVec {
        field,
        value,
        set_method,
        normalized,
        write_previous,
    } = init
    else {
        return;
    };
    let slot = slot(*field);
    for i in batch.first..batch.parts.len() {
        let v = vector(value, scene, batch.parts, Some(i), batch.salt);
        let current = batch.parts.vector(slot, i);
        let apply = |v: f32, c: f32| set_method.apply(v, c, c, scene.dt);
        let mut out = Vec3::new(
            apply(v.x, current.x),
            apply(v.y, current.y),
            apply(v.z, current.z),
        );
        if *normalized {
            out = out.normalize_or_zero();
        }
        batch.parts.set_vector(slot, i, out);
        if *write_previous && *field == Field::POSITION {
            batch.parts.vectors[store::PREVIOUS][i] = out;
        }
    }
}

fn position_warp(warp: &Warp, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let mut low = vector(&warp.min, scene, parts, None, batch.salt);
        let mut high = vector(&warp.max, scene, parts, None, batch.salt + 3);
        if warp.invert {
            (low, high) = (high, low);
        }
        if let Some(cp) = warp.scale_control_point {
            let scale = scene.control_points[cp.index()].position;
            (low, high) = (low * scale, high * scale);
        }
        let seed = parts.seed[i];
        let creation = parts.scalars[store::CREATION][i];
        let factor = if warp.time == 0.0 {
            Vec3::new(
                between(seed, batch.salt + 6, low.x, high.x),
                between(seed, batch.salt + 7, low.y, high.y),
                between(seed, batch.salt + 8, low.z, high.z),
            )
        } else {
            let input = if warp.use_count {
                let count = *batch.counter as f32;
                *batch.counter = batch.counter.wrapping_add(1);
                count
            } else {
                creation
            };
            let start = warp.start_time;
            low.lerp(
                high,
                remap_clamped(input, start, start + warp.time, 0.0, 1.0),
            )
        };
        let frame = scene.control_point_at(warp.control_point.index(), creation);
        let parts = &mut *batch.parts;
        let local = frame.unrotate(parts.vectors[store::POSITION][i] - frame.position);
        parts.vectors[store::POSITION][i] = frame.position + frame.rotate(local * factor);
        let local = frame.unrotate(parts.vectors[store::PREVIOUS][i] - frame.position);
        parts.vectors[store::PREVIOUS][i] =
            frame.position + frame.rotate(warp.previous_scale * factor * local);
        if let Some(component) = warp.radius_component {
            parts.scalars[store::RADIUS][i] *= factor[usize::from(component)];
        }
    }
}

fn velocity_from_cp(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::VelocityFromCp {
        velocity,
        transform,
        scale,
        direction_only,
    } = init
    else {
        return;
    };
    let mut v = vector(velocity, scene, batch.parts, None, batch.salt);
    if !matches!(transform, Transform::Identity) {
        v = scene.transform_at(transform, scene.age).rotate(v);
    }
    if *direction_only {
        v = v.normalize_or_zero();
    }
    let displacement = v * scene.dt * *scale;
    for i in batch.first..batch.parts.len() {
        batch.parts.vectors[store::PREVIOUS][i] -= displacement;
    }
}

fn distance_to_cp(d: &DistanceToCp, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let slot = slot(d.field);
    let center = scene.control_points[d.control_point.index()].position;
    let alpha = matches!(d.field, Field::ALPHA | Field::ALPHA_ALTERNATE);
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let value = |k: usize, input: &heatel::particle::FloatInput| {
            float(input, scene, parts, Some(i), batch.salt + k as u32)
        };
        let (in_min, in_max) = (value(0, &d.input[0]), value(1, &d.input[1]));
        let (mut out_min, mut out_max) = (value(2, &d.output[0]), value(3, &d.output[1]));
        if alpha {
            (out_min, out_max) = (out_min.clamp(0.0, 1.0), out_max.clamp(0.0, 1.0));
        }
        let position = parts.vectors[store::POSITION][i];
        let distance = ((center - position) * Vec3::from(d.distance_scale)).length();
        if d.active_range && (distance < in_min || distance > in_max) {
            continue;
        }
        let mut v = remap_clamped(distance, in_min, in_max, out_min, out_max);
        if d.bias != 0.5 {
            v = super::eval::bias(v.clamp(0.0, 1.0), d.bias);
        }
        let current = parts.scalar(slot, i);
        let v = d.set_method.apply(v, current, current, scene.dt);
        batch.parts.set_scalar(slot, i, v);
    }
}

fn position_offset(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::PositionOffset {
        min,
        max,
        transform,
        local,
        proportional,
    } = init
    else {
        return;
    };
    let first = batch.first;
    let len = batch.parts.len();
    for i in first..len {
        let (lo, hi) = (
            vector(min, scene, batch.parts, Some(i), batch.salt),
            vector(max, scene, batch.parts, Some(i), batch.salt + 3),
        );
        let seed = batch.parts.seed[i];
        let mut offset = Vec3::new(
            between(seed, batch.salt + 6, lo.x, hi.x),
            between(seed, batch.salt + 7, lo.y, hi.y),
            between(seed, batch.salt + 8, lo.z, hi.z),
        );
        if *proportional {
            offset *= batch.parts.scalars[store::RADIUS][i];
        }
        if *local {
            offset = scene.transform_at(transform, scene.age).rotate(offset);
        }
        batch.parts.vectors[store::POSITION][i] += offset;
        batch.parts.vectors[store::PREVIOUS][i] += offset;
    }
}

fn velocity_random(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::VelocityRandom {
        local_min,
        local_max,
        speed_min,
        speed_max,
        control_point,
        ignore_dt,
    } = init
    else {
        return;
    };
    let first = batch.first;
    let len = batch.parts.len();
    for i in first..len {
        let (lo, hi) = (
            vector(local_min, scene, batch.parts, Some(i), batch.salt),
            vector(local_max, scene, batch.parts, Some(i), batch.salt + 3),
        );
        let seed = batch.parts.seed[i];
        let mut velocity = Vec3::ZERO;
        if lo != Vec3::ZERO || hi != Vec3::ZERO {
            let local = Vec3::new(
                between(seed, batch.salt + 6, lo.x, hi.x),
                between(seed, batch.salt + 7, lo.y, hi.y),
                between(seed, batch.salt + 8, lo.z, hi.z),
            );
            let creation = batch.parts.scalars[store::CREATION][i];
            velocity = scene
                .control_point_at(control_point.index(), creation)
                .rotate(local);
        }
        let (smin, smax) = (
            float(speed_min, scene, batch.parts, Some(i), batch.salt + 9),
            float(speed_max, scene, batch.parts, Some(i), batch.salt + 10),
        );
        if smin != 0.0 || smax != 0.0 {
            let (direction, _) = in_ball(seed, batch.salt + 11);
            velocity += direction.normalize_or_zero() * between(seed, batch.salt + 14, smin, smax);
        }
        add_velocity(batch.parts, i, velocity, *ignore_dt, scene.dt);
    }
}

fn age_noise(init: &Initializer, batch: &mut Batch<'_>) {
    let Initializer::AgeNoise {
        absolute,
        absolute_inverted,
        offset,
        age_min,
        age_max,
        scale,
        scale_location,
        offset_location,
    } = init
    else {
        return;
    };
    let first = batch.first;
    let len = batch.parts.len();
    for i in first..len {
        let creation = batch.parts.scalars[store::CREATION][i];
        let p = (batch.parts.vectors[store::POSITION][i] + Vec3::from(*offset_location))
            * *scale_location;
        let n = noise(p + Vec3::splat((creation + offset) * scale));
        let t = map_noise(n, *absolute, *absolute_inverted, *age_min, *age_max).clamp(0.0, 1.0);
        batch.parts.scalars[store::CREATION][i] -= t * batch.parts.scalars[store::LIFETIME][i];
    }
}

/// The velocity a sphere gives particle `i`: out of its centre along `direction`, plus a
/// velocity in the frame's axes.
fn sphere_velocity(
    sphere: &Sphere,
    scene: &Scene<'_>,
    batch: &Batch<'_>,
    i: usize,
    direction: Vec3,
    frame: &super::eval::ControlPoint,
) -> Vec3 {
    let seed = batch.parts.seed[i];
    let smin = float(
        &sphere.speed_min,
        scene,
        batch.parts,
        Some(i),
        batch.salt + 8,
    );
    let smax = float(
        &sphere.speed_max,
        scene,
        batch.parts,
        Some(i),
        batch.salt + 9,
    );
    let speed = between_exp(seed, batch.salt + 10, sphere.speed_exponent, smin, smax);
    let lmin = vector(
        &sphere.local_speed_min,
        scene,
        batch.parts,
        Some(i),
        batch.salt + 11,
    );
    let lmax = vector(
        &sphere.local_speed_max,
        scene,
        batch.parts,
        Some(i),
        batch.salt + 14,
    );
    let local = Vec3::new(
        between(seed, batch.salt + 17, lmin.x, lmax.x),
        between(seed, batch.salt + 18, lmin.y, lmax.y),
        between(seed, batch.salt + 19, lmin.z, lmax.z),
    );
    direction * speed + frame.rotate(local)
}

/// Noise in -1..1 mapped onto `min..max`, folded when absolute.
fn map_noise(n: f32, absolute: bool, inverted: bool, min: f32, max: f32) -> f32 {
    let v = if absolute { n.abs() } else { n * 0.5 + 0.5 };
    let v = if inverted { 1.0 - n.abs() } else { v };
    min + (max - min) * v
}

fn add_velocity(parts: &mut Particles, i: usize, velocity: Vec3, per_step: bool, dt: f32) {
    parts.vectors[store::PREVIOUS][i] -= if per_step { velocity } else { velocity * dt };
}

fn sequence(batch: &mut Batch<'_>, min: i32, max: i32, ordered: bool) {
    for i in batch.first..batch.parts.len() {
        let value = if ordered {
            let span = (max - min + 1).max(1) as u32;
            let v = min + (*batch.counter % span) as i32;
            *batch.counter = batch.counter.wrapping_add(1);
            v
        } else {
            let span = (max - min + 1).max(1);
            min + ((draw(batch.parts.seed[i], batch.salt) * span as f32) as i32).min(span - 1)
        };
        batch.parts.scalars[store::SEQUENCE][i] = value as f32;
    }
}

fn within_sphere(sphere: &Sphere, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let output = slot(sphere.output);
    let velocity_slot = slot(sphere.velocity);
    let velocity_unwritten = batch.written & (1 << sphere.velocity.id()) == 0;
    for i in batch.first..batch.parts.len() {
        let seed = batch.parts.seed[i];
        let (mut v, fraction) = in_ball(seed, batch.salt);
        for (axis, absolute) in sphere.distance_bias_absolute.iter().enumerate() {
            if *absolute {
                v[axis] = v[axis].abs();
            }
        }
        let bias = vector(
            &sphere.distance_bias,
            scene,
            batch.parts,
            Some(i),
            batch.salt + 3,
        );
        let mut direction = (v * bias).normalize_or_zero();
        let creation = batch.parts.scalars[store::CREATION][i];
        let frame = scene.transform_at(&sphere.transform, creation);
        if sphere.local && (bias != Vec3::ONE || sphere.distance_bias_absolute.contains(&true)) {
            direction = frame.rotate(direction);
        }
        let rmin = float(
            &sphere.radius_min,
            scene,
            batch.parts,
            Some(i),
            batch.salt + 6,
        );
        let rmax = float(
            &sphere.radius_max,
            scene,
            batch.parts,
            Some(i),
            batch.salt + 7,
        );
        let at = frame.position + direction * (rmin + (rmax - rmin) * fraction);
        batch.parts.set_vector(output, i, at);
        if velocity_slot == Slot::None || !velocity_unwritten {
            continue;
        }
        let velocity = sphere_velocity(sphere, scene, batch, i, direction, &frame);
        if velocity_slot == Slot::Vector(store::PREVIOUS) {
            batch.parts.vectors[store::PREVIOUS][i] = at - velocity * scene.dt;
        } else {
            if output == Slot::Vector(store::POSITION) {
                batch.parts.vectors[store::PREVIOUS][i] = at;
            }
            batch.parts.set_vector(velocity_slot, i, at - velocity);
        }
    }
}

fn velocity_noise(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::InitialVelocityNoise {
        min,
        max,
        scale,
        scale_location,
        offset,
        offset_location,
        absolute,
        absolute_inverted,
        transform,
        ignore_dt,
    } = init
    else {
        return;
    };
    let any_inverted = absolute_inverted.contains(&true);
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let creation = parts.scalars[store::CREATION][i];
        let sample = (parts.vectors[store::POSITION][i]
            + vector(offset_location, scene, parts, Some(i), batch.salt))
            * float(scale_location, scene, parts, Some(i), batch.salt + 3)
            + Vec3::splat(
                (creation + float(offset, scene, parts, Some(i), batch.salt + 4))
                    * float(scale, scene, parts, Some(i), batch.salt + 5),
            );
        let n = noise3(sample);
        let (lo, hi) = (
            vector(min, scene, parts, Some(i), batch.salt + 6),
            vector(max, scene, parts, Some(i), batch.salt + 9),
        );
        let mut velocity = Vec3::ZERO;
        for axis in 0..3 {
            let value = if absolute[axis] {
                n[axis].abs()
            } else {
                n[axis]
            };
            let value = if any_inverted {
                if absolute_inverted[axis] {
                    1.0 + value
                } else {
                    -value
                }
            } else {
                value
            };
            let half = if absolute[axis] { 1.0 } else { 0.5 };
            let span = hi[axis] - lo[axis];
            velocity[axis] = value * span * half + lo[axis] + span * (1.0 - half);
        }
        let velocity = match transform {
            Transform::Identity => velocity,
            _ => scene.transform_at(transform, creation).rotate(velocity),
        };
        add_velocity(batch.parts, i, velocity, *ignore_dt, scene.dt);
    }
}

fn ring_wave(ring: &Ring, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let count = batch.parts.len();
    for i in batch.first..count {
        let parts = &*batch.parts;
        let seed = parts.seed[i];
        let thickness = float(&ring.thickness, scene, parts, Some(i), batch.salt);
        let per_orbit = float(&ring.per_orbit, scene, parts, None, batch.salt + 1);
        let radius = float(&ring.radius, scene, parts, Some(i), batch.salt + 2);
        let creation = parts.scalars[store::CREATION][i];
        let frame = scene.transform_at(&ring.transform, creation);
        *batch.counter = batch.counter.wrapping_add(1);
        let orbit = *batch.counter as f32;
        let angle = if ring.even {
            let per = if per_orbit == -1.0 {
                count as f32
            } else {
                per_orbit
            };
            orbit * (TAU / per.max(1.0))
        } else {
            orbit * draw(seed, batch.salt + 3) * TAU
        };
        let angles: Vec<f32> = ring
            .angles
            .iter()
            .enumerate()
            .map(|(k, a)| float(a, scene, parts, Some(i), batch.salt + 4 + k as u32))
            .collect();
        // Pitch, yaw and roll in degrees, as Source turns a QAngle.
        let turn = Quat::from_rotation_z(angles[1].to_radians())
            * Quat::from_rotation_y(angles[0].to_radians())
            * Quat::from_rotation_x(angles[2].to_radians());
        let local = turn * Vec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
            + in_ball(seed, batch.salt + 8).0 * thickness;
        let position = frame.position + frame.rotate(local);
        let outward = (position - frame.position).normalize_or_zero();
        let smin = float(&ring.speed_min, scene, parts, Some(i), batch.salt + 11);
        let smax = float(&ring.speed_max, scene, parts, Some(i), batch.salt + 12);
        let mut previous =
            position - outward * (between(seed, batch.salt + 13, smin, smax) * scene.dt);
        if ring.xy_velocity_only {
            previous.z = position.z;
        }
        batch.parts.vectors[store::POSITION][i] = position;
        if batch.written & (1 << Field::POSITION_PREVIOUS.id()) == 0 {
            batch.parts.vectors[store::PREVIOUS][i] = previous;
        }
    }
}

fn count_to_scalar(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::RemapParticleCountToScalar {
        field,
        input,
        output,
        active_range,
        invert,
        wrap,
        bias,
        set_method,
    } = init
    else {
        return;
    };
    let slot = slot(*field);
    let (out0, out1) = if matches!(*field, Field::ALPHA | Field::ALPHA_ALTERNATE) {
        (output[0].clamp(0.0, 1.0), output[1].clamp(0.0, 1.0))
    } else {
        (output[0], output[1])
    };
    let count = batch.parts.len() as i64;
    for i in batch.first..batch.parts.len() {
        let (min, max) = if *invert {
            (count - input[1] - 1, count - input[0] - 1)
        } else {
            (input[0], input[1])
        };
        let n = i64::from(*batch.counter);
        if *active_range && n > max {
            continue;
        }
        if !*active_range || n >= min {
            let t = if min == max {
                f32::from(u8::from(n >= max))
            } else {
                ((n - min) as f32 / (max - min) as f32).clamp(0.0, 1.0)
            };
            let mut v = out0 + (out1 - out0) * t;
            if *bias != 0.5 {
                v = super::eval::bias(v.clamp(0.0, 1.0), *bias);
            }
            let current = batch.parts.scalar(slot, i);
            batch
                .parts
                .set_scalar(slot, i, set_method.apply(v, current, current, scene.dt));
        }
        *batch.counter = batch.counter.wrapping_add(1);
        if *wrap && i64::from(*batch.counter) > max {
            *batch.counter = 0;
        }
    }
}
