//! The Lotus Pools as Dota's model draws them: the pool of the Dire's corner
//! in its own material group, each pool in the group of the lotus it grows,
//! and as many lotuses floating in it as it holds (the model's body groups
//! `mango1` to `mango6`).

use bevy::prelude::*;
use bota_proto::{LotusTier, UnitKind, UnitView, WorldView};

use crate::session::Session;
use crate::units::{BuildingYaws, ModelOf, Posable, Unit};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, show_lotuses);
}

/// The material group of `fountain_of_life_update.vmdl` a unit is drawn in:
/// 0 for anything but a Lotus Pool. The map draws the pool of the Dire's
/// corner in group 1 and the other in 0; the model's groups 2 and 3
/// (`fountain_of_life_update_lvl2`, `_lvl3`) are the Radiant's pool and 4
/// and 5 (`_lvl2_dire`, `_lvl3_dire`) the Dire's, growing Great and Greater
/// Healing Lotuses.
pub fn skin(u: &UnitView, view: &WorldView, yaws: &BuildingYaws, at: Vec2) -> u8 {
    if u.kind != UnitKind::LotusPool {
        return 0;
    }
    let dire = yaws.placed(u, at).is_some_and(|b| b.skin == 1);
    let tier = view
        .pools
        .iter()
        .find(|pool| pool.id == u.id)
        .map_or(LotusTier::Healing, |pool| pool.tier);
    match (dire, tier) {
        (false, LotusTier::Healing) => 0,
        (true, LotusTier::Healing) => 1,
        (false, LotusTier::Great) => 2,
        (false, LotusTier::Greater) => 3,
        (true, LotusTier::Great) => 4,
        (true, LotusTier::Greater) => 5,
    }
}

/// Body groups of the pool's model, one per lotus it can hold.
const LOTUS_PARTS: [&str; 6] = ["mango1", "mango2", "mango3", "mango4", "mango5", "mango6"];

/// The lotuses of a pool's model, by place, and how many show.
#[derive(Component)]
pub struct PoolLotuses {
    parts: Vec<(usize, Entity)>,
    shown: Option<u8>,
}

/// Shows as many of a pool's lotuses as it holds, the first ones first.
fn show_lotuses(
    mut commands: Commands,
    session: NonSend<Session>,
    mut pools: Query<(Entity, &Unit, &ModelOf, Option<&mut PoolLotuses>)>,
    children: Query<&Children>,
    scenes: Query<(), With<Posable>>,
    names: Query<&Name>,
    mut shown: Query<&mut Visibility>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    for (entity, unit, pivot, lotuses) in &mut pools {
        if unit.kind != UnitKind::LotusPool {
            continue;
        }
        let held = view
            .pools
            .iter()
            .find(|pool| pool.id == unit.id)
            .map_or(0, |pool| pool.lotuses);
        match lotuses {
            Some(mut lotuses) => lotuses.show(held, &mut shown),
            // Once its model's instance is there.
            None => {
                let Some(scene) = children
                    .get(pivot.0)
                    .ok()
                    .and_then(|c| c.iter().find(|e| scenes.contains(*e)))
                else {
                    continue;
                };
                let parts = children
                    .iter_descendants(scene)
                    .filter_map(|part| {
                        let name = names.get(part).ok()?.as_str();
                        // A mesh's node is named `<model path>.<mesh>`.
                        let mesh = name.rsplit_once('.').map_or(name, |(_, mesh)| mesh);
                        Some((LOTUS_PARTS.iter().position(|p| *p == mesh)?, part))
                    })
                    .collect();
                let mut lotuses = PoolLotuses { parts, shown: None };
                lotuses.show(held, &mut shown);
                commands.entity(entity).insert(lotuses);
            }
        }
    }
}

impl PoolLotuses {
    /// Shows the first `held` lotuses and hides the others.
    fn show(&mut self, held: u8, shown: &mut Query<&mut Visibility>) {
        if self.shown == Some(held) {
            return;
        }
        self.shown = Some(held);
        for &(place, part) in &self.parts {
            if let Ok(mut visibility) = shown.get_mut(part) {
                *visibility = if place < usize::from(held) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}
