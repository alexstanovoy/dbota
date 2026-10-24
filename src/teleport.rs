//! Town portal scrolls on the map as Dota draws them, its own particle effects: the landing
//! (`teleport_end`), shown to both sides through the fog, and the start at the feet of the
//! hero that channels it while it is in sight (`teleport_start`), each in the seat's colour;
//! when the channel ends, each plays its end.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_proto::{EntityId, SlotId, TeleportView, WorldView};

use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;

/// The effects of a teleport's start and landing, by their manifest names.
const START: &str = "item.teleport_start";
const LANDING: &str = "item.teleport_end";
/// The effects playing, by seat and end (true: the landing).
#[derive(Resource, Default)]
struct Portals(HashMap<(SlotId, bool), Handle>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Portals>().add_systems(Update, portals);
}

/// How much of a hero's channel has run, 0 to 1, when it channels a teleport.
pub fn channel_share(view: &WorldView, unit: EntityId) -> Option<f32> {
    let player = view.players.iter().find(|p| p.unit == Some(unit))?;
    let teleport = view.teleports.iter().find(|t| t.slot == player.slot)?;
    Some(progress(teleport))
}

/// How much of a teleport's channel has run, 0 to 1.
pub fn progress(teleport: &TeleportView) -> f32 {
    1.0 - teleport.ticks_left as f32 / teleport.ticks_total.max(1) as f32
}

/// Where the channelling hero of a teleport stands, when it is in sight.
pub fn origin(view: &WorldView, teleport: &TeleportView) -> Option<(f32, f32)> {
    let unit = view.player(teleport.slot)?.unit?;
    let unit = view.unit(unit)?;
    Some((unit.pos.x.to_f32(), unit.pos.y.to_f32()))
}

/// Keeps Dota's effect at the landing of every teleport and at the feet of every hero in sight
/// that channels one; one whose channel ended plays its end.
fn portals(
    session: NonSend<Session>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    colours: Res<crate::player_colour::PlayerColours>,
    mut playing: ResMut<Playing>,
    mut known: ResMut<Portals>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let mut wanted = Vec::with_capacity(2 * view.teleports.len());
    for teleport in &view.teleports {
        let landing = (
            teleport.destination.x.to_f32(),
            teleport.destination.y.to_f32(),
        );
        wanted.push((teleport, true, landing));
        if let Some(at) = origin(view, teleport) {
            wanted.push((teleport, false, at));
        }
    }
    known.0.retain(|key, handle| {
        let keep = wanted
            .iter()
            .any(|(t, landing, _)| (t.slot, *landing) == *key);
        if !keep && let Some(instance) = playing.get_mut(*handle) {
            instance.end();
        }
        keep
    });
    for (teleport, landing, (x, y)) in wanted {
        let at = Vec3::new(x, y, ground.at(x, y));
        let key = (teleport.slot, landing);
        let handle = match known.0.get(&key) {
            Some(handle) => *handle,
            None => {
                let name = if landing { LANDING } else { START };
                let Some(effect) = library.as_deref().and_then(|l| l.effect(name)) else {
                    continue;
                };
                let seed = u32::from(teleport.slot.0) << 1 | u32::from(landing);
                let Some(handle) = playing.start(effect, seed) else {
                    continue;
                };
                let color = colours.of(view, teleport.slot).to_srgba();
                if let Some(instance) = playing.get_mut(handle) {
                    let color = Vec3::new(color.red, color.green, color.blue);
                    instance.set_control_point(COLOR, color, Vec3::ZERO, true);
                    instance.set_control_point(HERO, at, Vec3::ZERO, true);
                    instance.set_control_point(DESTINATION, at, Vec3::ZERO, true);
                }
                known.0.insert(key, handle);
                handle
            }
        };
        if let Some(instance) = playing.get_mut(handle) {
            // The start's place is the hero's; the landing's, Dota's CP1, is where it lands.
            instance.set_control_point(HERO, at, Vec3::ZERO, false);
            if landing {
                instance.set_control_point(DESTINATION, at, Vec3::ZERO, false);
            }
        }
    }
}

/// The control points Dota gives a teleport's effects: the hero, where it lands, and the
/// seat's colour (red, green and blue, 0 to 1).
const HERO: usize = 0;
const DESTINATION: usize = 1;
const COLOR: usize = 2;
