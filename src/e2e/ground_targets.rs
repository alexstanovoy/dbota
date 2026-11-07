//! An item or a rune lying on the ground is lit with Dota's ring when a right click on it would
//! take it, and not otherwise: the ring and the click are gated by the same core functions.

use std::time::Duration;

use bota_client_core::Intent;
use bota_proto::{Cheat, EntityId, HeroId, ItemId, RuneKind, Vec2 as Spot};

use super::harness::{Client, MID, Server};
use crate::ground_targets::GroundTarget;

const SOON: Duration = Duration::from_secs(30);

/// What the ring is on now.
fn lit(client: &Client) -> Option<EntityId> {
    client
        .world()
        .resource::<GroundTarget>()
        .0
        .map(|lying| lying.id)
}

/// Puts the pointer over a point of the ground.
fn point_at(client: &mut Client, at: (f32, f32)) {
    let pixel = client.screen_of(at).expect("on screen");
    client.mouse(pixel);
}

/// A tango dropped on the ground beside one's hero is ringed under the pointer and not away from
/// it.
#[test]
fn an_item_lying_on_the_ground_is_ringed_under_the_pointer() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: spot,
        unit: None,
    });
    client.within(SOON, "the tango on the ground", |c| {
        c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
    });
    let tango = client.view().loot[0].id;
    point_at(&mut client, spot);
    assert_eq!(lit(&client), Some(tango), "ringed under the pointer");
    point_at(&mut client, (spot.0 + 400.0, spot.1 + 400.0));
    assert_eq!(lit(&client), None, "not ringed away from it");
}

/// A rune is ringed under the pointer, as an item is: the Haste Rune put at the spot, picked out
/// by its kind and where it lies.
#[test]
fn a_rune_lying_on_the_ground_is_ringed_under_the_pointer() {
    let server = Server::start(1, &["--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let top = bota_core::DOTA_RUNE_SPOTS[0].at;
    let spot = (top.x.to_int(), top.y.to_int());
    client.stand_at((spot.0 + 400, spot.1));
    client.cheat(Cheat::SpawnRune {
        kind: RuneKind::Haste,
        at: Spot::from_ints(spot.0, spot.1),
    });
    let haste_there = |c: &Client| {
        c.view()
            .runes
            .iter()
            .find(|r| {
                r.kind == RuneKind::Haste
                    && (r.pos.x.to_int(), r.pos.y.to_int()) == (spot.0, spot.1)
            })
            .map(|r| r.id)
    };
    client.within(SOON, "the Haste Rune on the ground", |c| {
        haste_there(c).is_some()
    });
    let rune = haste_there(&client).expect("there");
    let at = (spot.0 as f32, spot.1 as f32);
    point_at(&mut client, at);
    assert_eq!(lit(&client), Some(rune), "ringed under the pointer");
    point_at(&mut client, (at.0 + 400.0, at.1 - 300.0));
    assert_eq!(lit(&client), None, "not ringed away from it");
}

/// Aiming an item does not hide the ring: the right click cancels the aim and takes the item.
#[test]
fn an_item_is_ringed_while_another_item_is_being_aimed() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: spot,
        unit: None,
    });
    client.within(SOON, "the tango on the ground", |c| {
        c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
    });
    let lying = client.view().loot[0].id;
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let held = client.slot_of(ItemId::TANGO).expect("another tango");
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Item(held),
        ctrl: false,
    });
    assert!(client.core().aiming.is_some(), "the tango is being aimed");
    point_at(&mut client, spot);
    assert_eq!(lit(&client), Some(lying), "ringed while aiming");
}

/// With an enemy picked the click gives no order, so nothing is ringed.
#[test]
fn nothing_is_ringed_while_an_enemy_is_picked() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: Spot::from_ints(MID.0 + 900, MID.1 + 900),
    });
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: spot,
        unit: None,
    });
    client.within(SOON, "the tango on the ground", |c| {
        c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
    });
    let mine = client.me().team;
    client.within(SOON, "the enemy hero in view", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
        .map(|u| u.id)
        .expect("there");
    point_at(&mut client, spot);
    assert!(lit(&client).is_some(), "ringed before the enemy is picked");
    client.core_mut().act(Intent::SelectUnit(Some(enemy)));
    client.frames(3);
    assert_eq!(lit(&client), None, "nothing is ringed with an enemy picked");
}

/// In a replay the click gives no order, so an item in plain view under the pointer is not
/// ringed; the ring and the click answer to the same gate.
#[test]
fn nothing_is_ringed_in_a_replay_even_in_plain_view() {
    let replay =
        crate::test_support::scratch_path(&format!("ground-targets-{}.brp", std::process::id()));
    let (spot, tick) = {
        let mut server = Server::start(
            1,
            &[
                "--replay",
                replay.to_str().expect("UTF-8"),
                "--empty-after-secs",
                "1",
            ],
        );
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        client.stand_at(MID);
        client.cheat(Cheat::Item {
            item: ItemId::TANGO,
        });
        let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
        let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
        client.core_mut().act(Intent::PutAt {
            slot,
            at: spot,
            unit: None,
        });
        client.within(SOON, "the tango on the ground", |c| {
            c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
        });
        client.frames(30);
        let tick = client.tick();
        drop(client);
        server.wait();
        (spot, tick)
    };
    let mut client = Client::run(&["--replay", replay.to_str().expect("UTF-8"), "--pov", "0"]);
    client.until("the replay at the dropped item", |c| {
        let now = c.tick();
        if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
            if now < tick {
                player.advance_ticks(f64::from(tick - now));
            }
            player.paused = now >= tick;
        }
        now >= tick && !c.view().loot.is_empty()
    });
    client.follow_me();
    point_at(&mut client, spot);
    assert_eq!(lit(&client), None, "no ring where the click is inert");
}
