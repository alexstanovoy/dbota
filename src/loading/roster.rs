//! Match identity from the handoff, connection facts from the game; no per-player load percent.
use bota_client_core::App as Core;
use bota_proto::{Role, SlotId, Team};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub match_id: Option<u64>,
    pub mode: String,
    pub seats: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub slot: u8,
    pub team: Team,
    pub name: String,
    pub bot: bool,
    pub connected: bool,
    pub me: bool,
}

impl Details {
    pub fn with_connections(mut self, core: &Core, texts: &crate::dota_text::Texts) -> Self {
        if self.seats.is_empty() {
            self.seats = rows(core, texts);
        } else {
            for row in &mut self.seats {
                row.connected = core
                    .lobby
                    .slots
                    .iter()
                    .any(|seat| seat.slot.0 == row.slot && seat.role.is_some());
            }
        }
        self
    }
}

pub fn rows(core: &Core, texts: &crate::dota_text::Texts) -> Vec<Row> {
    let row = |slot: SlotId, team: Team, given: &str, role: Option<Role>| {
        let name = if given.is_empty() {
            crate::seat_name::seat_name(core, slot, Some(texts))
        } else {
            given.to_owned()
        };
        Row {
            slot: slot.0,
            team,
            name: crate::main_menu::line(&name, crate::main_menu::text::NAME),
            bot: role == Some(Role::Bot),
            connected: role.is_some(),
            me: core.my_slot == Some(slot),
        }
    };
    if let Some(statics) = core.statics() {
        return statics
            .roster()
            .iter()
            .map(|pick| {
                let seat = core.lobby.slots.iter().find(|s| s.slot == pick.slot);
                row(pick.slot, pick.team, "", seat.and_then(|s| s.role))
            })
            .collect();
    }
    core.lobby
        .slots
        .iter()
        .filter(|seat| seat.role.is_some())
        .map(|seat| row(seat.slot, seat.team, &seat.name, seat.role))
        .collect()
}
