//! An Arcane Rune on the hero shortens the cooldowns and mana costs a tooltip tells, as it
//! shortens what a cast takes: the tooltip is held to what the simulator did with a real cast.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, EffectId, HeroId, RuneKind, Vec2 as Spot};

use super::harness::{Client, Server};
use super::runes::shown_spans;
use crate::dota_text::plain;
use crate::tip_numbers::Rates;

const SOON: Duration = Duration::from_secs(30);

/// The ability box of a slot of the bottom panel, once laid out.
fn ability_box(client: &mut Client, slot: u8) -> Option<Vec2> {
    let world = client.world_mut();
    let node = world
        .query::<(Entity, &crate::hud::AbilityBox)>()
        .iter(world)
        .find(|(_, b)| b.0 == slot)
        .map(|(e, _)| e)?;
    client.centre_of(node).filter(|at| *at != Vec2::ZERO)
}

/// Whether a tooltip over the first ability tells `figure` somewhere.
fn tooltip_tells(client: &mut Client, figure: &str) -> bool {
    let Some(at) = ability_box(client, 0) else {
        return false;
    };
    client.mouse(at);
    client.frames(3);
    shown_spans(client).iter().any(|span| span.trim() == figure)
}

/// Takes an Arcane Rune with the hero.
fn take_an_arcane_rune(client: &mut Client) {
    let top = bota_core::DOTA_RUNE_SPOTS[0].at;
    let spot = (top.x.to_int(), top.y.to_int());
    client.stand_at((spot.0 + 400, spot.1));
    client.cheat(Cheat::SpawnRune {
        kind: RuneKind::Arcane,
        at: Spot::from_ints(spot.0, spot.1),
    });
    client.stand_at(spot);
    client.within(SOON, "the Arcane Rune on the hero", |c| {
        c.me().effects.iter().any(|e| e.id == EffectId::RUNE_ARCANE)
    });
}

/// With the rune on, Shadowraze is cast: its cooldown and its mana cost, as the simulator set
/// them, are what the tooltip tells.
#[test]
fn a_tooltip_tells_the_cooldown_and_cost_the_simulator_gave_a_cast_under_an_arcane_rune() {
    let server = Server::start(1, &["--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.within(SOON, "the first ability box laid out", |c| {
        ability_box(c, 0).is_some()
    });
    take_an_arcane_rune(&mut client);
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: true,
    });
    client.within(SOON, "Shadowraze learned", |c| {
        c.me().abilities[0].level >= 1
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: false,
    });
    // The first tick a cooldown shows: what the simulator set, at most a tick or two spent.
    let mut set = 0;
    client.within(SOON, "Shadowraze on cooldown", |c| {
        set = c.me().abilities[0].cooldown_left;
        set > 0
    });
    let me = client.me().clone();
    let (cooldown_bp, mana_bp) = me.cast_rates.expect("the rune sets the rates");
    assert!(
        cooldown_bp < 10_000 && mana_bp < 10_000,
        "the rune shortens both"
    );
    let def = bota_core::ability_def(AbilityId::RAZE_NEAR).expect("an ability");
    let level = usize::from(me.abilities[0].level) - 1;
    let table = def.cooldown[level.min(def.cooldown.len() - 1)];
    assert_eq!(
        set,
        bota_core::cooldown_after(table, cooldown_bp),
        "the simulator cut {table} ticks by the rate its view tells"
    );
    let shown_cooldown = plain(
        f64::from(bota_core::cooldown_after(table, cooldown_bp))
            / f64::from(bota_core::rules::TICKS_PER_SECOND),
    );
    let shown_mana = me.abilities[0].mana_cost.to_string();
    assert!(
        Rates::of_view(&me).cooldown_bp == cooldown_bp,
        "the tooltip reads the view's rate"
    );
    client.within(SOON, "the tooltip tells the cooldown the cast left", |c| {
        tooltip_tells(c, &shown_cooldown)
    });
    assert!(
        tooltip_tells(&mut client, &shown_mana),
        "and the mana cost the view tells: {shown_mana}"
    );
}
