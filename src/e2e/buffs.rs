//! Buffs named as Dota names them (reported: buff names from the install).

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId};

use super::harness::{Client, Server};

/// The flagbearer's aura on the creeps beside it (bota's effect 14, Dota's
/// modifier_flagbearer_creep_aura_effect).
const FLAGBEARER_AURA: u16 = 14;

/// A creep of one's side under its flagbearer's aura, selected: hovered, the aura's chip shows
/// a tooltip titled with the install's name of Dota's modifier, "Inspiration Aura"
/// (DOTA_Tooltip_modifier_flagbearer_creep_aura_effect), not dbota's own "Inspiration".
#[test]
fn a_buffs_tooltip_is_titled_with_dotas_modifier_name() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let name = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("DOTA_Tooltip_modifier_flagbearer_creep_aura_effect", &[])
        .expect("the install's name of the modifier");
    // The horn now: the first waves, each with its flagbearer, come out at once.
    client.cheat(Cheat::StartGame);
    let inspired = |c: &Client| {
        let mine = c.me().team;
        c.view()
            .units
            .iter()
            .find(|u| u.team == mine && u.effects.iter().any(|e| e.id.0 == FLAGBEARER_AURA))
            .map(|u| u.id)
    };
    client.until("a creep under the flagbearer's aura", |c| {
        inspired(c).is_some()
    });
    let creep = inspired(&client).expect("there");
    client.core_mut().act(Intent::SelectUnit(Some(creep)));
    let chip = |c: &mut Client| {
        let world = c.world_mut();
        world
            .query::<(Entity, &crate::buffs::BuffChip, &InheritedVisibility)>()
            .iter(world)
            .find(|(_, chip, seen)| {
                seen.get() && chip.shown.is_some_and(|s| s.id == FLAGBEARER_AURA)
            })
            .map(|(e, ..)| e)
    };
    let patience = Duration::from_secs(20);
    client.within(patience, "the aura's chip shown", |c| {
        chip(c)
            .and_then(|e| c.centre_of(e))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    let on = chip(&mut client)
        .and_then(|e| client.centre_of(e))
        .expect("laid out");
    client.mouse(on);
    client.within(patience, "its tooltip titled with Dota's name", |c| {
        c.shown_texts()
            .iter()
            .any(|t| t.lines().next() == Some(name.as_str()))
    });
}
