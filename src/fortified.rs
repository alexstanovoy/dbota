//! What the Glyph of Fortification looks like: while it holds, a side's structures wear Dota's
//! `glyph.vpcf` (a tube of blue rings rising around them, 7 s) and its lane creeps
//! `glyph_creeps.vpcf` (3 s), its own particle effects, which play their end when the cover
//! does.
//!
//! A unit under a Glyph wears the `EffectId::FORTIFIED` buff with its ticks
//! left, whichever side's it is, when the viewer is told of it at all: the enemy's Glyph shows on what
//! one sees, as in Dota.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_core::{kind_def, rules};
use bota_proto::{EffectId, EntityId};

use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;

/// The effects of a structure's cover and a creep's, by their manifest names.
const STRUCTURE: &str = "glyph.structure";
const CREEP: &str = "glyph.creep";
/// The control points Dota gives them: the unit, and its radius (the first component).
const UNIT: usize = 0;
const RADIUS: usize = 1;

/// The effects playing, by the unit they are on.
#[derive(Resource, Default)]
struct Worn(HashMap<EntityId, Handle>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Worn>().add_systems(Update, wear);
}

/// The units wearing the Glyph's buff, whichever side's, and the seconds it
/// has left.
fn covered(view: &bota_proto::WorldView) -> Vec<(&bota_proto::UnitView, f32)> {
    let rate = rules::TICKS_PER_SECOND as f32;
    view.units
        .iter()
        .filter_map(|u| {
            let buff = u.effects.iter().find(|e| e.id == EffectId::FORTIFIED)?;
            Some((u, buff.ticks_left? as f32 / rate))
        })
        .collect()
}

/// Plays the Glyph's effect on the units under it, following them, and ends it on the rest.
fn wear(
    session: NonSend<Session>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut worn: ResMut<Worn>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let covered = covered(view);
    worn.0.retain(|id, handle| {
        let keep = covered.iter().any(|(unit, _)| unit.id == *id);
        if !keep && let Some(instance) = playing.get_mut(*handle) {
            instance.end();
        }
        keep
    });
    for (unit, _) in covered {
        let (x, y) = (unit.pos.x.to_f32(), unit.pos.y.to_f32());
        let at = Vec3::new(x, y, ground.at(x, y));
        let handle = match worn.0.get(&unit.id) {
            Some(handle) => *handle,
            None => {
                let name = if kind_def(unit.kind).structure {
                    STRUCTURE
                } else {
                    CREEP
                };
                let Some(effect) = library.as_deref().and_then(|l| l.effect(name)) else {
                    continue;
                };
                let Some(handle) = playing.start(effect, unit.id.idx) else {
                    continue;
                };
                if let Some(instance) = playing.get_mut(handle) {
                    let radius = Vec3::new(crate::pick::ring_radius(unit), 0.0, 0.0);
                    instance.set_control_point(RADIUS, radius, Vec3::ZERO, true);
                    instance.set_control_point(UNIT, at, Vec3::ZERO, true);
                }
                worn.0.insert(unit.id, handle);
                handle
            }
        };
        if let Some(instance) = playing.get_mut(handle) {
            instance.set_control_point(UNIT, at, Vec3::ZERO, false);
        }
    }
}
