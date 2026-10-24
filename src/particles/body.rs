//! A model's body as particles find it. The client has no model hitboxes: a place in a
//! body-sized capsule standing on the model's control point (where the unit is) stands in for a
//! place in one of its hitboxes, so what gathers on a unit gathers about its body.

use bevy::math::Vec3;
use heatel::particle::{Field, Initializer, Operator};

use super::eval::{Scene, draw, float, in_ball, vector};
use super::init::Batch;
use super::ops::Call;
use super::store::{self, Particles};

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
