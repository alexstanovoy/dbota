//! A model's body as particles find it. The client has no model hitboxes: a place in a
//! body-sized capsule standing on the model's control point (where the unit is) stands in for a
//! place in one of its hitboxes, so what gathers on a unit gathers about its body.

use bevy::math::Vec3;
use heatel::particle::{Field, Initializer, Operator};

use super::eval::{Scene, draw, float, in_ball, vector};
use super::init::Batch;
use super::ops::Call;
use super::store::{self, Particles, slot};

/// The capsule's radius and height, in Dota's units: about a hero's.
const BODY_RADIUS: f32 = 30.0;
const BODY_HEIGHT: f32 = 120.0;

/// `C_INIT_SetHitboxToModel`: each new particle's place on the body, as an offset from the
/// model's control point (the hitbox offset attribute), unless an earlier initializer gave one.
pub fn set_hitbox(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::SetHitboxToModel(hitbox) = init else {
        return;
    };
    if batch.written & (1 << Field::HITBOX_OFFSET_POSITION.id()) != 0 {
        return;
    }
    let scale = vector(&hitbox.scale, scene, batch.parts, None, batch.salt);
    for i in batch.first..batch.parts.len() {
        let seed = batch.parts.seed[i];
        let (across, _) = in_ball(seed, batch.salt + 3);
        let up = draw(seed, batch.salt + 6) * BODY_HEIGHT;
        let offset = Vec3::new(across.x * BODY_RADIUS, across.y * BODY_RADIUS, up) * scale;
        batch.parts.vectors[store::HITBOX_OFFSET][i] = offset;
    }
}

/// `C_OP_MoveToHitbox`: particles move to their place on the body at the model's control
/// point, by a share growing over their age's window or by a constant share each step.
pub fn move_to_hitbox(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let Operator::MoveToHitbox {
        control_point,
        lifetime: [start, end],
        previous_scale,
        constant,
        interpolation,
    } = op
    else {
        return;
    };
    let origin = scene.control_points[control_point.index()].position;
    let inverse = if end > start {
        1.0 / (end - start)
    } else {
        0.0
    };
    // An end of 0 never ends the window.
    let end = if *end > 0.0 { *end } else { f32::MAX };
    for i in 0..parts.len() {
        let fraction = if *constant {
            call.strength * float(interpolation, scene, parts, Some(i), call.salt)
        } else {
            let age = scene.age - parts.scalars[store::CREATION][i];
            if !(end > age && age > *start) {
                continue;
            }
            (age - start) * inverse * call.strength
        };
        let target = origin + parts.vectors[store::HITBOX_OFFSET][i];
        let position = &mut parts.vectors[store::POSITION][i];
        *position = position.lerp(target, fraction);
        let previous = &mut parts.vectors[store::PREVIOUS][i];
        *previous = previous.lerp(target, fraction * previous_scale);
    }
}

/// A place in the body: an offset from the model's control point, scaled per axis.
fn place(seed: u32, salt: u32, scale: Vec3) -> Vec3 {
    let (across, _) = in_ball(seed, salt);
    let up = draw(seed, salt + 3) * BODY_HEIGHT;
    Vec3::new(across.x * BODY_RADIUS, across.y * BODY_RADIUS, up) * scale
}

/// `C_INIT_CreateOnModel`: each new particle at a place in the body at the model's control
/// point where it was at the particle's moment, the place kept as its hitbox offset; it starts
/// with the share of the body's velocity the bone velocity asks for.
pub fn create_on_model(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::CreateOnModel(on_model) = init else {
        return;
    };
    let unwritten = |field: Field| batch.written & (1 << field.id()) == 0;
    let (keep_offset, keep_previous) = (
        unwritten(Field::HITBOX_OFFSET_POSITION),
        unwritten(Field::POSITION_PREVIOUS),
    );
    let scale = vector(&on_model.scale, scene, batch.parts, None, batch.salt);
    let cp = on_model.model.index();
    let mut velocity = Vec3::ZERO;
    if on_model.bone_velocity != 0.0 && scene.frame_len > 0.0 {
        let point = scene.control_points[cp];
        velocity = (point.position - point.previous) / scene.frame_len * on_model.bone_velocity;
        if on_model.max_bone_velocity > 0.0 {
            velocity = velocity.clamp_length_max(on_model.max_bone_velocity);
        }
    }
    for i in batch.first..batch.parts.len() {
        let offset = place(batch.parts.seed[i], batch.salt + 4, scale);
        let creation = batch.parts.scalars[store::CREATION][i];
        let position = scene.control_point_at(cp, creation).position + offset;
        batch.parts.vectors[store::POSITION][i] = position;
        if keep_offset {
            batch.parts.vectors[store::HITBOX_OFFSET][i] = offset;
        }
        if keep_previous {
            batch.parts.vectors[store::PREVIOUS][i] = position - velocity * scene.dt;
        }
    }
}

/// `C_OP_LockToBone`: particles move as their place in the body moves (the model's control
/// point's move this step, as much of it as they have lived), fading out over their age's
/// window; a jump of the transform is followed whole, and a rigid lock puts them at the place.
pub fn lock_to_bone(op: &Operator, scene: &Scene<'_>, parts: &mut Particles, call: &Call<'_>) {
    let Operator::LockToBone(lock) = op else {
        return;
    };
    if scene.dt <= 0.0 {
        return;
    }
    let cp = lock.model.index();
    let start = scene.age - scene.dt;
    let (now, before) = (
        scene.control_point_at(cp, scene.age).position,
        scene.control_point_at(cp, start).position,
    );
    let moved = scene.transform_at(&lock.transform, scene.age).position
        - scene.transform_at(&lock.transform, start).position;
    let jumped = moved.length_squared() > lock.jump_threshold * lock.jump_threshold;
    let [fade_start, fade_end] = lock.fade;
    let end = if fade_end > 0.0 { fade_end } else { 1e20 };
    let inverse = if fade_end > fade_start {
        1.0 / (fade_end - fade_start)
    } else {
        0.0
    };
    let previous_scale = if jumped { 1.0 } else { lock.previous_scale };
    let (output, output_previous) = (slot(lock.output), slot(lock.output_previous));
    let write_previous = previous_scale != 0.0 && lock.output_previous != Field::DISABLED;
    for i in 0..parts.len() {
        let age = scene.age - parts.scalars[store::CREATION][i];
        if age >= end && !jumped {
            continue;
        }
        let offset = parts.vectors[store::HITBOX_OFFSET][i];
        if lock.rigid {
            parts.set_vector(output, i, now + offset);
            if write_previous {
                parts.set_vector(output_previous, i, now + offset);
            }
            continue;
        }
        let mut delta = (now - before) * (age.min(scene.dt) / scene.dt);
        if fade_start < age && !jumped {
            delta *= (1.0 - (age - fade_start) * inverse) * call.strength;
        }
        let current = parts.vector(output, i);
        parts.set_vector(output, i, current + delta);
        if write_previous {
            let previous = parts.vector(output_previous, i);
            parts.set_vector(output_previous, i, previous + delta * previous_scale);
        }
    }
}
