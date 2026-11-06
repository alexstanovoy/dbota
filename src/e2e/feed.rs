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

/// The one row a line of the feed draws at a given age, as it is dressed.
fn dressed(client: &mut Client, kind: FeedKind, age: f32) -> crate::feed::Look {
    client.core_mut().feed.push(FeedLine {
        kind,
        age,
        told: None,
    });
    client.frames(3);
    let mut looks = crate::feed::looks(client.world_mut());
    assert_eq!(looks.len(), 1, "one line draws exactly one row");
    looks.pop().expect("the row")
}

/// A hero kill of one's own side is a row tinted green, with the slanted end at the right in
/// the same green, in Dota's size of text; the same kill by the other side is tinted brown.
#[test]
fn a_hero_kill_row_is_tinted_by_whose_side_it_is_good_for() {
    let (_server, mut client) = four_seats();
    client.within(SOON, "four seats on the scoreboard", |c| {
        c.view().players.len() == 4
    });
    let mine = client.me().team;
    let ours = client.core().my_slot.expect("a seat");
    let foe = SlotId(
        (0..4u8)
            .find(|s| client.view().player(SlotId(*s)).expect("a seat").team != mine)
            .expect("a foe"),
    );
    let kill = |killer, victim| FeedKind::HeroDied {
        victim: Victim::Seat(victim),
        killer: Some(Killer::Seat(killer)),
        gold: 250,
        assists: 0,
    };
    use crate::feed::{ALLY_TINT, ENEMY_TINT, SHADE};
    let ally = dressed(&mut client, kill(ours, foe), 1.0);
    assert!(!ally.left);
    assert_eq!(
        ally.gradient,
        Some([SHADE, ALLY_TINT]),
        "the shade into the green"
    );
    assert_eq!(ally.slash, Some(ALLY_TINT), "the slanted end in the green");
    assert_eq!(ally.bar, None, "the bar belongs to the toasts at the left");
    assert!(
        (ally.text_size - 18.0 * crate::ui::DOTA).abs() < 0.01,
        "Dota's 18px text"
    );
    client.core_mut().feed.clear();
    client.frames(2);
    let enemy = dressed(&mut client, kill(foe, ours), 1.0);
    assert_eq!(enemy.gradient, Some([SHADE, ENEMY_TINT]));
    assert_eq!(enemy.slash, Some(ENEMY_TINT));
}

/// A toast at the left about a building has the slanted end in the shade at its left, the tint
/// running out into the shade, and the slanted bar of its side's colours turned by 25 degrees; a
/// building of the other side falling is the good news, in green.
#[test]
fn a_building_toast_has_the_tint_the_slanted_end_and_the_bar_of_its_side() {
    let (_server, mut client) = four_seats();
    let mine = client.me().team;
    let theirs = if mine == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let fell = |team| FeedKind::StructureFell {
        team,
        kind: UnitKind::Tower,
        lane: Some(1),
        tier: Some(2),
        ranged: None,
        killer: None,
    };
    use crate::feed::{ALLY_BAR, ALLY_TINT, ENEMY_BAR, ENEMY_TINT, SHADE};
    let good = dressed(&mut client, fell(theirs), 1.0);
    assert!(good.left);
    assert_eq!(
        good.gradient,
        Some([ALLY_TINT, SHADE]),
        "the green into the shade"
    );
    assert_eq!(good.slash, Some(SHADE));
    assert_eq!(good.bar, Some(ALLY_BAR));
    assert!(
        good.bar_turn_degrees
            .is_some_and(|turn| (turn - 25.0).abs() < 0.01),
        "turned by 25 degrees: {:?}",
        good.bar_turn_degrees
    );
    client.core_mut().feed.clear();
    client.frames(2);
    let bad = dressed(&mut client, fell(mine), 1.0);
    assert_eq!(bad.gradient, Some([ENEMY_TINT, SHADE]));
    assert_eq!(bad.bar, Some(ENEMY_BAR));
}

/// A rune toast is about neither side: no tint, no slanted end, no bar, as Dota's row is left.
#[test]
fn a_rune_toast_wears_no_side() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let look = dressed(
        &mut client,
        FeedKind::Rune {
            by: Some(me),
            team,
            kind: bota_proto::RuneKind::Haste,
            act: bota_client_core::RuneAct::Activated { gold: None },
        },
        1.0,
    );
    assert!(look.left);
    assert!(look.gradient.is_none() && look.slash.is_none() && look.bar.is_none());
}

/// A row comes in as Dota's keyframes have it, growing from a fifth of its size to one and a
/// half and settling to one, and fades out in the last moments before it goes.
#[test]
fn a_row_grows_in_and_fades_out_as_dotas_keyframes_have_it() {
    let (_server, mut client) = four_seats();
    // The clock stands still, so that the ages here are the ages asked for.
    client
        .world_mut()
        .resource_mut::<Time<bevy::time::Virtual>>()
        .pause();
    let mine = client.me().team;
    let theirs = if mine == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let fell = FeedKind::StructureFell {
        team: theirs,
        kind: UnitKind::Tower,
        lane: Some(1),
        tier: Some(2),
        ranged: None,
        killer: None,
    };
    let mut scale_at = |age| {
        client.core_mut().feed.clear();
        client.frames(2);
        dressed(&mut client, fell.clone(), age)
    };
    let arriving = scale_at(0.0);
    assert!(arriving.scale < 0.8, "starts small: {}", arriving.scale);
    let swelling = scale_at(0.15);
    assert!(
        swelling.scale > 1.3,
        "swells past its size: {}",
        swelling.scale
    );
    let settled = scale_at(1.0);
    assert!(
        (settled.scale - 1.0).abs() < 1e-3,
        "settles to one: {}",
        settled.scale
    );
    let leaving = scale_at(7.9);
    let alpha = |look: &crate::feed::Look| look.gradient.expect("tinted")[0].alpha();
    assert!(
        alpha(&leaving) < alpha(&settled),
        "fades out before it goes"
    );
}
