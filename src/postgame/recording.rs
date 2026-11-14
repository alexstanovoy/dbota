//! Optional recording facts: absent in older files, bounded independently of their contents.
use bota_proto::{Team, UnitKind};
use bota_replay::meta::{Meta, MetaRecord};

use super::summary::Row;

/// A building fall in metadata order. Its location is optional for old recordings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Exposed to the match-results consumer.
pub struct StructureFall {
    pub tick: u32,
    pub team: Team,
    pub kind: UnitKind,
    pub lane: Option<u8>,
    pub tier: Option<u8>,
    pub ranged: Option<bool>,
}

pub(super) fn enrich_rows(meta: &Meta, rows: &mut [Row]) {
    // Identities can precede Seat records. Rows are resolved first, so unknown slots never
    // allocate and optional facts attach correctly regardless of their record ordering.
    for record in &meta.records {
        match record {
            // The old name record only serves a recording that has no identity for the seat.
            MetaRecord::SeatName { slot, name, bot }
                if name.len() <= bota_proto::MAX_SHOWN_NAME_BYTES
                    && !meta.records.iter().any(|other| {
                        matches!(other, MetaRecord::SeatIdentity(identity) if identity.slot.0 == *slot)
                    }) =>
            {
                if let Some(row) = rows.iter_mut().find(|row| row.slot.0 == *slot) {
                    row.name = (!name.is_empty()).then(|| bota_core::clean_name(name));
                    row.bot = Some(*bot);
                    row.version = None;
                }
            }
            MetaRecord::Leave { tick, slot } => {
                if let Some(row) = rows.iter_mut().find(|row| row.slot.0 == *slot) {
                    row.left_at = Some(row.left_at.map_or(*tick, |first| first.min(*tick)));
                }
            }
            _ => {}
        }
    }
    for record in &meta.records {
        if let MetaRecord::SeatIdentity(identity) = record
            && let Some(row) = rows.iter_mut().find(|row| row.slot == identity.slot)
        {
            row.identity(identity);
        }
    }
}

pub(super) fn structures(meta: &Meta) -> Vec<StructureFall> {
    let mut structures = Vec::new();
    for record in &meta.records {
        match record {
            MetaRecord::Structure { tick, kind, team }
                if structures.len() < bota_replay::limits::MAX_TRACKED =>
            {
                // Keep unknown buildings as holes until companions are resolved: skipping one
                // here would make every following ordinal refer to the wrong building.
                structures.push(building(*tick, *kind, *team));
            }
            MetaRecord::StructureLocation {
                index,
                lane,
                tier,
                ranged,
            } => {
                if let Some(Some(building)) = structures.get_mut(*index as usize) {
                    building.lane = lane.filter(|lane| *lane <= bota_core::rules::LANE_BOT);
                    building.tier = tier
                        .filter(|tier| (1..=bota_core::TOWERS.len()).contains(&usize::from(*tier)));
                    building.ranged = *ranged;
                }
            }
            _ => {}
        }
    }
    structures.into_iter().flatten().collect()
}

fn building(tick: u32, kind: u8, team: u8) -> Option<StructureFall> {
    let kind = bota_core::UNIT_KINDS
        .iter()
        .find(|definition| definition.structure && definition.kind as u8 == kind)?
        .kind;
    let team = match team {
        0 => Team::Radiant,
        1 => Team::Dire,
        _ => return None,
    };
    Some(StructureFall {
        tick,
        kind,
        team,
        lane: None,
        tier: None,
        ranged: None,
    })
}
