//! The initializers of boxes, curves, noise and remaps of a transform, as Source 2 runs them.

use bevy::math::Vec3;
use heatel::particle::{Epitrochoid, Field, FloatInput, Initializer, TransformToVector, WithinBox};

use super::eval::{Scene, draw, float, noise, radical_inverse, remap_clamped, vector};
use super::init::Batch;
use super::store::{self, Slot, slot};

/// Runs one of the initializers of boxes, curves, noise and remaps.
pub fn initialize(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    match init {
        Initializer::CreateWithinBox(b) => within_box(b, scene, batch),
        Initializer::CreateInEpitrochoid(e) => epitrochoid(e, scene, batch),
        Initializer::CreationNoise(_) => creation_noise(init, scene, batch),
        Initializer::RandomSecondSequence { min, max } => {
            let span = (max - min + 1).max(1);
            for i in batch.first..batch.parts.len() {
                let pick = (draw(batch.parts.seed[i], batch.salt) * span as f32) as i32;
                batch.parts.set_scalar(
                    slot(Field::SECOND_SEQUENCE_NUMBER),
                    i,
                    (min + pick.min(span - 1)) as f32,
                );
            }
        }
        Initializer::RemapTransformToVector(r) => transform_to_vector(r, scene, batch),
        Initializer::NormalAlignToCp { transform, axis } => {
            let local = [Vec3::X, Vec3::Y, Vec3::Z][usize::from(*axis % 3)];
            let local = if *axis >= 3 { -local } else { local };
            for i in batch.first..batch.parts.len() {
                let creation = batch.parts.scalars[store::CREATION][i];
                let frame = scene.transform_at(transform, creation);
                batch.parts.vectors[store::NORMAL][i] = frame.rotate(local);
            }
        }
        Initializer::RemapInitialDirectionToTransformToVector { .. } => {
            initial_direction(init, scene, batch);
        }
        _ => {}
    }
}

/// `C_INIT_CreateWithinBox`: a position between the corners, at random or spread evenly (a
/// Halton sequence keyed by the particle's number), about the control point where it was at
/// the particle's moment, or in its frame.
fn within_box(b: &WithinBox, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let low = vector(&b.min, scene, parts, Some(i), batch.salt);
        let high = vector(&b.max, scene, parts, Some(i), batch.salt + 3);
        let seed = parts.seed[i];
        let fraction = match b.even {
            Some(start) => {
                let start = if start < 0 {
                    start.wrapping_add(scene.seed as i32)
                } else {
                    start
                };
                let index = (start as u32).wrapping_add(parts.number[i]) & 0x7FFF_FFFF;
                Vec3::new(
                    radical_inverse(index, 2),
                    radical_inverse(index, 3),
                    radical_inverse(index, 5),
                )
            }
            None => Vec3::new(
                draw(seed, batch.salt + 6),
                draw(seed, batch.salt + 7),
                draw(seed, batch.salt + 8),
            ),
        };
        let local = low + (high - low) * fraction;
        let creation = parts.scalars[store::CREATION][i];
        let point = scene.control_point_at(b.control_point.index(), creation);
        let position = if b.local {
            point.position + point.rotate(local)
        } else {
            point.position + local
        };
        batch.parts.vectors[store::POSITION][i] = position;
        batch.parts.vectors[store::PREVIOUS][i] = position;
    }
}

/// `C_INIT_CreateInEpitrochoid`: a position on the curve a point at the offset from a circle's
/// centre draws as the circle rolls around another, at the angle the particle's creation time
/// (or its number) gives.
fn epitrochoid(e: &Epitrochoid, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let frame = scene.transform_at(&e.transform, scene.age);
    for i in batch.first..batch.parts.len() {
        let parts = &*batch.parts;
        let value =
            |input: &FloatInput, k: u32| float(input, scene, parts, Some(i), batch.salt + k);
        let rolling = value(&e.radius2, 0) + f32::EPSILON;
        let outer = value(&e.radius1, 1) + rolling;
        let offset = value(&e.offset, 2);
        let density = value(&e.density, 3);
        let t = if e.use_count {
            let count = batch.memory.counter as f32;
            batch.memory.counter = batch.memory.counter.wrapping_add(1);
            count * density
        } else {
            density * parts.scalars[store::CREATION][i]
        };
        let roll = t / rolling * outer;
        let mut curve = Vec3::ZERO;
        if let Some(axis) = e.components[0] {
            curve[usize::from(axis)] = t.cos() * outer + roll.cos() * offset;
        }
        if let Some(axis) = e.components[1] {
            curve[usize::from(axis)] = t.sin() * outer + roll.sin() * offset;
        }
        if e.local {
            curve = frame.rotate(curve);
        }
        let parts = &mut *batch.parts;
        if e.offset_existing {
            parts.vectors[store::POSITION][i] += curve;
            parts.vectors[store::PREVIOUS][i] += curve;
        } else {
            parts.vectors[store::POSITION][i] = frame.position + curve;
            parts.vectors[store::PREVIOUS][i] = frame.position + curve;
        }
    }
}

/// `C_INIT_CreationNoise`: a float attribute from noise at the particle's place, at the time
/// the batch's first particle was made (and the world's time), mapped onto the output; angles
/// in degrees, an alpha within 0..1.
fn creation_noise(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::CreationNoise(n) = init else {
        return;
    };
    let target = slot(n.field);
    if !matches!(target, Slot::Scalar(_)) || batch.first >= batch.parts.len() {
        return;
    }
    let first = batch.parts.scalars[store::CREATION][batch.first];
    let time = (first + n.offset) * n.scale + scene.world_time * 1000.0 * n.world_time_scale;
    let (mut low, mut high) = (n.output_min, n.output_max);
    if n.field.is_angle() {
        (low, high) = (low.to_radians(), high.to_radians());
    }
    let alpha = matches!(n.field, Field::ALPHA | Field::ALPHA_ALTERNATE);
    for i in batch.first..batch.parts.len() {
        let at = (batch.parts.vectors[store::POSITION][i] + Vec3::from(n.offset_location))
            * n.scale_location;
        let mut value = noise(at + Vec3::splat(time));
        let half = if n.absolute { 1.0 } else { 0.5 };
        if n.absolute {
            value = value.abs();
        }
        if n.absolute_inverted {
            value = 1.0 - value;
        }
        let span = high - low;
        let mut out = low + (1.0 - half) * span + half * span * value;
        if alpha {
            out = out.clamp(0.0, 1.0);
        }
        batch.parts.set_scalar(target, i, out);
    }
}

/// `C_INIT_RemapTransformToVector`: a vector attribute from the transform's position remapped
/// per axis (the output turned by the local frame), for the particles made within the window.
fn transform_to_vector(r: &TransformToVector, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let target = slot(r.field);
    if !matches!(target, Slot::Vector(_)) {
        return;
    }
    let position = scene.transform_at(&r.transform, scene.age).position;
    let (mut low, mut high) = (Vec3::from(r.output[0]), Vec3::from(r.output[1]));
    if let Some(local) = &r.local {
        let frame = scene.transform_at(local, scene.age);
        (low, high) = (frame.rotate(low), frame.rotate(high));
    }
    let (from, to) = (Vec3::from(r.input[0]), Vec3::from(r.input[1]));
    let mut value = Vec3::ZERO;
    for axis in 0..3 {
        value[axis] = remap_clamped(position[axis], from[axis], to[axis], low[axis], high[axis]);
        if r.bias != 0.5 {
            value[axis] = super::eval::bias(value[axis].clamp(0.0, 1.0), r.bias);
        }
    }
    let unit = matches!(r.field, Field::COLOR | Field::ALPHA);
    for i in batch.first..batch.parts.len() {
        if let Some([start, end]) = r.window {
            let creation = batch.parts.scalars[store::CREATION][i];
            if creation < start || creation >= end {
                continue;
            }
        }
        let parts = &mut *batch.parts;
        let current = parts.vector(target, i);
        let apply = |v: f32, c: f32| r.set_method.apply(v, c, c, scene.dt);
        let mut v = Vec3::new(
            apply(value.x, current.x),
            apply(value.y, current.y),
            apply(value.z, current.z),
        );
        if unit {
            v = v.clamp(Vec3::ZERO, Vec3::ONE);
        } else if r.accelerate && !r.offset {
            v *= scene.dt;
        } else if r.offset {
            v += current;
            parts.vectors[store::PREVIOUS][i] += v;
            if r.accelerate {
                v *= 1.0 + scene.dt;
            }
        }
        parts.set_vector(target, i, v);
    }
}

/// `C_INIT_RemapInitialDirectionToTransformToVector`: a vector attribute from the way from the
/// transform to the particle, turned by the offset rotation about its axis, scaled.
fn initial_direction(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::RemapInitialDirectionToTransformToVector {
        transform,
        field,
        scale,
        offset_rotation_degrees,
        offset_axis,
        normalize,
    } = init
    else {
        return;
    };
    let origin = scene.transform_at(transform, scene.age).position;
    let (sin, cos) = offset_rotation_degrees.to_radians().sin_cos();
    let a = Vec3::from(*offset_axis);
    let reverse = 1.0 - cos;
    // Source's rotation about an axis, row by row (the axis taken as it is, not normalized).
    let rows = [
        Vec3::new(
            (1.0 - a.x * a.x) * cos + a.x * a.x,
            a.x * a.y * reverse - a.z * sin,
            a.x * a.z * reverse + a.y * sin,
        ),
        Vec3::new(
            a.y * a.x * reverse + a.z * sin,
            (1.0 - a.y * a.y) * cos + a.y * a.y,
            a.y * a.z * reverse - a.x * sin,
        ),
        Vec3::new(
            a.x * a.z * reverse - a.y * sin,
            a.y * a.z * reverse + a.x * sin,
            (1.0 - a.z * a.z) * cos + a.z * a.z,
        ),
    ];
    let target = slot(*field);
    for i in batch.first..batch.parts.len() {
        let delta = batch.parts.vectors[store::POSITION][i] - origin;
        let mut direction = Vec3::new(delta.dot(rows[0]), delta.dot(rows[1]), delta.dot(rows[2]));
        if *normalize {
            direction.z += f32::EPSILON;
            direction = direction.normalize_or_zero();
        }
        batch.parts.set_vector(target, i, direction * *scale);
    }
}
