//! Roshan's last actual sighting in the selected player's vision.
use bevy::prelude::*;
use bota_proto::{EventKind, ObjectiveKind, Team, UnitKind, UnitView};

/// A remembered location is neither a public respawn prediction nor another viewer's memory.
#[derive(Resource, Default)]
pub struct Sighting {
    pub unit: Option<UnitView>,
    tick: u32,
    vision: Option<(bool, Option<Team>)>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Sighting>()
        .add_systems(
            First,
            crate::loading::reset::<Sighting>.in_set(crate::loading::Begin),
        )
        .add_systems(Update, remember);
}

pub fn remember(sight: crate::fog::EventSight, mut memory: ResMut<Sighting>) {
    let core = sight.core();
    let fog = sight.fog();
    let Some(view) = core.view() else {
        return;
    };
    let vision = Some((fog.on, fog.team));
    if memory.vision != vision {
        memory.vision = vision;
        memory.unit = None;
    }
    let death = core.session().and_then(|session| {
        session.recent_events().iter().rev().find_map(|event| {
            matches!(
                event.kind,
                EventKind::BossFell {
                    objective: ObjectiveKind::Roshan,
                    ..
                }
            )
            .then_some(event.tick)
        })
    });
    // Established from installed game build 6975: a Roshan death is announced to both teams,
    // including players without vision of the pit. Receipt retires the alive sighting.
    if death.is_some_and(|tick| tick >= memory.tick) {
        memory.unit = None;
    }
    let visible = view
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Roshan && unit.hp > 0 && sight.allows_unit(unit));
    if let Some(unit) = visible.filter(|_| death.is_none_or(|tick| tick < view.tick)) {
        memory.unit = Some(unit.clone());
        memory.tick = view.tick;
    } else if let Some(unit) = &memory.unit {
        // As with rune memory, a new lighting result must confirm that the last spot is empty.
        let empty = !fog.on
            || fog.worked_out_at().is_some_and(|tick| tick > memory.tick)
                && fog.lit(unit.pos.x.to_f32(), unit.pos.y.to_f32());
        if empty {
            memory.unit = None;
        }
    }
}
