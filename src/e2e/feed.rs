//! The kill feed in Dota's look: a hero's fall with the killer's, the kill's and the victim's
//! icons and the assisting heroes' after them, and a building's fall in Dota's words.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{FeedKind, FeedLine, Killer, Victim};
use bota_proto::{HeroId, SlotId, Team, UnitKind};

use super::harness::{Client, Server, quiet_seat};

const SOON: Duration = Duration::from_secs(30);

/// Four seats in a match, the client the first-joined (seat 0), the others quiet.
fn four_seats() -> (Server, Client) {
    let server = Server::start(4, &[]);
    for _ in 0..3 {
        quiet_seat(&server, HeroId::PUDGE);
    }
    let client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    (server, client)
}

/// Tells a line to the feed as the core would, and waits for it to be drawn.
fn feed(client: &mut Client, kind: FeedKind) -> (bool, Vec<String>, String) {
    let gold = crate::bars::drawn_gold_popups(client.world_mut());
    client.core_mut().feed.push(FeedLine {
        kind,
        age: 0.0,
        told: None,
    });
    client.frames(3);
    let mut rows = crate::feed::shown(client.world_mut());
    assert_eq!(rows.len(), 1, "one feed event draws exactly one row");
    assert_eq!(
        crate::bars::drawn_gold_popups(client.world_mut()),
        gold,
        "drawing the feed never emits another gold popup"
    );
    rows.pop().expect("the line is drawn")
}

fn portrait(client: &Client, slot: u8) -> String {
    let hero = client.view().player(SlotId(slot)).expect("a seat").hero.0;
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .hero_icon(hero)
        .expect("the hero's icon in the cache")
}

fn file(client: &Client, logical: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("{logical} in the cache"))
}

/// A hero's fall is the killer's icon, the kill icon, the victim's icon, then the assists' icons.
#[test]
fn a_hero_kill_is_icons_for_killer_kill_victim_and_assists() {
    let (_server, mut client) = four_seats();
    client.within(SOON, "four seats on the scoreboard", |c| {
        c.view().players.len() == 4
    });
    let mine = client.me().team;
    let team_of = |c: &Client, slot: u8| c.view().player(SlotId(slot)).expect("a seat").team;
    // A seat of the other side falls to one of ours, the third seat helping.
    let killer = client.core().my_slot.expect("a seat").0;
    let victim = (0..4u8)
        .find(|s| team_of(&client, *s) != mine)
        .expect("a foe");
    let helper = (0..4u8)
        .find(|s| *s != victim && *s != killer && team_of(&client, *s) == mine)
        .expect("an ally besides the killer");
    let (left, icons, text) = feed(
        &mut client,
        FeedKind::HeroDied {
            victim: Victim::Seat(SlotId(victim)),
            killer: Some(Killer::Seat(SlotId(killer))),
            gold: 250,
            assists: 1 << helper,
        },
    );
    let expected = vec![
        portrait(&client, killer),
        file(&client, "hud.kill_ally"),
        portrait(&client, victim),
        portrait(&client, helper),
    ];
    assert!(
        !left,
        "a hero's fall stays in the kill feed at the top right"
    );
    assert_eq!(
        icons, expected,
        "killer, kill, victim, assist; text {text:?}"
    );
}

/// A tower's fall is Dota's line of it: the hero that destroyed it, the kill icon of its side
/// and the tower's name. The destroyer is a seat of the other side, so that its icon cannot be
/// taken for the viewer's own.
#[test]
fn a_fallen_tower_is_worded_as_dota_words_it() {
    let (_server, mut client) = four_seats();
    client.within(SOON, "four seats on the scoreboard", |c| {
        c.view().players.len() == 4
    });
    let mine = client.me().team;
    let foe = (0..4u8)
        .find(|s| client.view().player(SlotId(*s)).expect("a seat").team != mine)
        .expect("a foe");
    let (left, icons, text) = feed(
        &mut client,
        FeedKind::StructureFell {
            team: mine,
            kind: UnitKind::Tower,
            lane: Some(1),
            tier: Some(2),
            ranged: None,
            killer: Some(Killer::Seat(SlotId(foe))),
        },
    );
    assert!(
        left,
        "a building's fall is a toast at the left above the HUD"
    );
    let side = if mine == Team::Radiant {
        "Radiant"
    } else {
        "Dire"
    };
    assert_eq!(text.trim(), format!("{side} Top Tier 2 Tower"));
    assert_ne!(
        icons[0],
        portrait(&client, client.core().my_slot.expect("a seat").0),
        "the destroyer is not the viewer"
    );
    assert_eq!(
        icons,
        vec![portrait(&client, foe), file(&client, "hud.kill_enemy")],
        "the hero that destroyed it, then the sword of the other side"
    );
}

/// A barracks' fall names its side, lane and kind.
#[test]
fn a_fallen_barracks_is_worded_as_dota_words_it() {
    let (_server, mut client) = four_seats();
    let (left, icons, text) = feed(
        &mut client,
        FeedKind::StructureFell {
            team: Team::Dire,
            kind: UnitKind::Barracks,
            lane: Some(2),
            tier: None,
            ranged: Some(true),
            killer: None,
        },
    );
    assert!(left, "at the left");
    assert_eq!(text.trim(), "Dire Bottom Ranged Barracks");
    assert_eq!(icons.len(), 1, "one kill icon");
}

/// A rune taken, a Shrine gathered and an Outpost captured are toasts at the left too, with the
/// rune's own icon.
#[test]
fn a_rune_taken_is_a_toast_at_the_left_with_its_icon() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let (left, icons, text) = feed(
        &mut client,
        FeedKind::Rune {
            by: Some(me),
            team,
            kind: bota_proto::RuneKind::Haste,
            act: bota_client_core::RuneAct::Activated { gold: None },
        },
    );
    assert!(left, "at the left above the HUD: {text:?}");
    assert_eq!(
        icons,
        vec![file(&client, "icon.rune.haste")],
        "the rune's own icon"
    );
}
