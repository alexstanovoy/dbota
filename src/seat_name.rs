//! What a seat or a speaker is called in the chat log and the kill feed.

use bota_client_core::App as Core;
use bota_proto::{SlotId, Speaker};

/// The display name of a seat.
///
/// A seat that sent no name, as in a replay, is called by its hero and its
/// side, and by its number before the match tells either.
pub fn seat_name(core: &Core, slot: SlotId) -> String {
    if let Some(name) = core.player_name(slot) {
        return name.to_string();
    }
    let known = core
        .view()
        .and_then(|view| view.player(slot))
        .and_then(|p| {
            let hero = bota_core::hero_def(p.hero)?;
            Some((hero, p.team))
        });
    match known {
        Some((hero, team)) => format!("{} ({team:?})", hero.name),
        None => format!("seat {}", slot.0),
    }
}

/// What a speaker is called: a seat by its name, a spectator by its own.
pub fn speaker_name(core: &Core, from: &Speaker) -> String {
    match from {
        Speaker::Seat(slot) => seat_name(core, *slot),
        Speaker::Spectator(name) => name.clone(),
    }
}
