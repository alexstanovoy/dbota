//! A child system's particles made from and following its parent system's particles.

use bevy::math::Vec3;
use heatel::particle::{Field, Initializer, Operator};

use super::eval::{Scene, draw};
use super::init::Batch;
use super::ops::Call;
use super::store::{self, Particles, Slot, slot};

/// `C_INIT_CreateFromParentParticles`: each new particle where one of the parent's is, at the
/// particle's moment within the step, the parent's motion kept by the velocity scale.
pub fn create_from_parent(init: &Initializer, scene: &Scene<'_>, batch: &mut Batch<'_>) {
    let Initializer::CreateFromParentParticles {
        velocity_scale,
        increment,
        random,
        random_seed,
        sub_frame,
    } = init
    else {
        return;
    };
    let fractional = increment.fract() != 0.0;
    for i in batch.first..batch.parts.len() {
        let parent = match scene.parent {
            Some(parent) if !parent.is_empty() => parent,
            missing => {
                // No parent particle to be made at: the particle is not made (Source's).
                if missing.is_some() {
                    batch.parts.scalars[store::LIFETIME][i] = 0.0;
                }
                batch.parts.vectors[store::POSITION][i] = Vec3::ZERO;
                batch.parts.vectors[store::PREVIOUS][i] = Vec3::ZERO;
                continue;
            }
        };
        let last = parent.len() - 1;
        let walk = if *random {
            let key = if *random_seed != 0 {
                *random_seed as u32
            } else {
                batch.parts.seed[i]
            };
            let sample = draw(key, batch.salt.wrapping_add(*batch.counter));
            *batch.counter = batch.counter.wrapping_add(1);
            if fractional {
                sample * last as f32
            } else {
                (sample * (last + 1) as f32).floor()
            }
        } else {
            if *batch.walk > last as f32 {
                *batch.walk = 0.0;
            }
            let walk = *batch.walk;
            *batch.walk += increment;
            walk
        };
        let at = |k: f32| (k as usize).min(last);
        let (low, high) = (at(walk.floor().max(0.0)), at(walk.ceil().max(0.0)));
        let blend = if fractional { walk.fract() } else { 0.0 };
        let position =
            parent.vectors[store::POSITION][low].lerp(parent.vectors[store::POSITION][high], blend);
        let previous =
            parent.vectors[store::PREVIOUS][low].lerp(parent.vectors[store::PREVIOUS][high], blend);
        let birth = if *sub_frame && scene.dt > 0.0 {
            let creation = batch.parts.scalars[store::CREATION][i];
            ((creation - (scene.age - scene.dt)) / scene.dt).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let born = previous.lerp(position, birth);
        batch.parts.vectors[store::POSITION][i] = born;
        batch.parts.vectors[store::PREVIOUS][i] = born.lerp(previous, *velocity_scale);
    }
}

/// `C_OP_InheritFromParentParticles`: an attribute copied each step from the parent's
/// particles, one after another (the operator's memory keeps where it is).
pub fn inherit_from_parent(
    op: &Operator,
    scene: &Scene<'_>,
    parts: &mut Particles,
    call: &mut Call<'_>,
) {
    let Operator::InheritFromParentParticles {
        scale,
        field,
        increment,
        random,
    } = op
    else {
        return;
    };
    let Some(parent) = scene.parent.filter(|p| !p.is_empty()) else {
        return;
    };
    let target = slot(*field);
    let count = parent.len() as i32;
    let index = &mut call.memory.parent_index;
    for i in 0..parts.len() {
        if *random {
            let key = parts.seed[i] ^ scene.step_seed.wrapping_mul(0x2545_F491);
            *index = ((draw(key, call.salt) * count as f32) as i32).min(count - 1);
        } else if *index > count - 1 {
            *index = 0;
        }
        if let Ok(from) = usize::try_from(*index)
            && from < parent.len()
        {
            copy(parent, from, parts, i, target, *field, *scale);
        }
        *index = index.wrapping_add(*increment);
    }
}

/// Copies attribute `field` of the parent's particle `from` to particle `i`, scaled, a colour
/// or an alpha kept within 0..1.
fn copy(
    parent: &Particles,
    from: usize,
    parts: &mut Particles,
    i: usize,
    target: Slot,
    field: Field,
    scale: f32,
) {
    match target {
        Slot::Vector(_) => {
            let mut value = parent.vector(target, from) * scale;
            if field == Field::COLOR {
                value = value.clamp(Vec3::ZERO, Vec3::ONE);
            }
            parts.set_vector(target, i, value);
        }
        // A sequence is a number, not a quantity: it is copied unscaled.
        Slot::Scalar(_)
            if matches!(
                field,
                Field::SEQUENCE_NUMBER | Field::SECOND_SEQUENCE_NUMBER
            ) =>
        {
            parts.set_scalar(target, i, parent.scalar(target, from));
        }
        Slot::Scalar(_) => {
            let mut value = parent.scalar(target, from) * scale;
            if matches!(field, Field::ALPHA | Field::ALPHA_ALTERNATE) {
                value = value.clamp(0.0, 1.0);
            }
            parts.set_scalar(target, i, value);
        }
        Slot::Id | Slot::None => {}
    }
}
