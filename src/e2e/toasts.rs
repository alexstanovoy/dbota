//! The event messages above the minimap: each kind of event is one banner of segments (who, the
//! kind of event on a plate, what happened with its figures lit), standing where the reference game's
//! standing, at its height.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{FeedKind, FeedLine, Killer, RuneAct};
use bota_proto::{HeroId, RuneKind, SlotId, Team, UnitKind, Vec2 as WorldVec};

use super::harness::{Client, Server, quiet_seat};
use crate::feed::{NUMBER_INK, Segment};

const SOON: Duration = Duration::from_secs(30);
/// How far an edge of the banner may be from its place, in shares of the window's height.
const TOLERANCE: f32 = 0.01;

/// Four seats in a match, the client the first-joined (seat 0), the others quiet.
fn four_seats() -> (Server, Client) {
    let server = Server::start(4, &[]);
    for _ in 0..3 {
        quiet_seat(&server, HeroId::PUDGE);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.within(SOON, "four seats on the scoreboard", |c| {
        c.view().players.len() == 4
    });
    super::hud_layout::on_screen(&mut client);
    (server, client)
}

/// A seat of the side that is not the viewer's.
fn foe(client: &Client) -> SlotId {
    let mine = client.me().team;
    client
        .view()
        .players
        .iter()
        .find(|p| p.team != mine)
        .expect("a foe")
        .slot
}

/// Tells a line to the feed as the core would, once it has settled in (a second old), and returns
/// its one banner: where it stands and its segments.
fn banner(client: &mut Client, kind: FeedKind) -> (Rect, Vec<Segment>) {
    client.core_mut().feed.clear();
    client.core_mut().feed.push(FeedLine {
        kind,
        age: 1.0,
        told: None,
    });
    client.frames(4);
    let mut toasts = crate::feed::toasts(client.world_mut());
    assert_eq!(toasts.len(), 1, "one line draws exactly one banner");
    toasts.pop().expect("the banner")
}

fn file(client: &Client, logical: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("{logical} in the cache"))
}

fn portrait(client: &Client, slot: SlotId) -> String {
    let hero = client.view().player(slot).expect("a seat").hero.0;
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .hero_icon(hero)
        .expect("the hero's icon in the cache")
}

fn name_of(client: &Client, slot: SlotId) -> String {
    crate::seat_name::seat_name(
        client.core(),
        slot,
        client.world().get_resource::<crate::dota_text::Texts>(),
    )
}

fn colour_of(client: &Client, slot: SlotId) -> Color {
    client
        .world()
        .resource::<crate::player_colour::PlayerColours>()
        .of(client.view(), slot)
}

fn window_height(client: &mut Client) -> f32 {
    let world = client.world_mut();
    world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("the window")
        .height()
}

fn minimap_top(client: &mut Client) -> f32 {
    let world = client.world_mut();
    let (node, at) = world
        .query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "hud minimap")
        .map(|(_, node, at)| (*node, *at))
        .expect("the minimap");
    at.translation.y - node.size().y / 2.0
}

/// The words and figures of a banner's pairs, for a plain comparison: every text of it in order.
fn texts(segments: &[Segment]) -> Vec<&str> {
    segments.iter().map(|s| s.text.as_str()).collect()
}

fn tower_fell(client: &Client, killer: Option<Killer>) -> FeedKind {
    FeedKind::StructureFell {
        team: client.me().team,
        kind: UnitKind::Tower,
        lane: Some(1),
        tier: Some(1),
        ranged: None,
        killer,
    }
}

/// The banner of a tower's fall is the reference game's: its height, its place at the screen's left
/// edge over the minimap, with the destroyer first, then the plate, the words and the gold.
#[test]
fn a_tower_banner_stands_where_and_as_high_as_the_reference_games() {
    let (_server, mut client) = four_seats();
    let striker = foe(&client);
    let kind = tower_fell(&client, Some(Killer::Seat(striker)));
    let (rect, _) = banner(&mut client, kind);
    let height = window_height(&mut client);
    // Fractions of the height in the reference frame (1620 lines): a banner is 45 lines high, its
    // foot stands 78 over the minimap's top, and it begins at the screen's edge.
    let (want_height, want_gap) = (45.0 / 1620.0 * height, 78.0 / 1620.0 * height);
    let off = |got: f32, want: f32| (got - want).abs() / height;
    assert!(
        off(rect.height(), want_height) < TOLERANCE,
        "banner height {} wanted {want_height}",
        rect.height()
    );
    let top = minimap_top(&mut client);
    assert!(
        off(top - rect.max.y, want_gap) < TOLERANCE,
        "banner foot {} over the minimap's top {top}, wanted {want_gap} between",
        rect.max.y
    );
    assert!(
        rect.min.x.abs() / height < TOLERANCE,
        "banner begins at the screen's edge, not {}",
        rect.min.x
    );
}

#[test]
fn a_tower_banner_tells_who_destroyed_it_the_sword_of_the_side_the_tower_and_its_gold() {
    let (_server, mut client) = four_seats();
    let striker = foe(&client);
    let kind = tower_fell(&client, Some(Killer::Seat(striker)));
    let (_, segments) = banner(&mut client, kind);
    let side = if client.me().team == Team::Radiant {
        "Radiant"
    } else {
        "Dire"
    };
    assert_eq!(
        segments[0],
        Segment {
            icon: Some(portrait(&client, striker)),
            text: name_of(&client, striker),
            ink: Some(colour_of(&client, striker)),
            plate: None,
        },
        "the destroyer: its hero's icon and its name in its colour"
    );
    assert_eq!(
        segments[1].icon,
        Some(file(&client, "hud.kill_enemy")),
        "the kind of event: the sword of the side that struck"
    );
    assert!(segments[1].plate.is_some(), "on a plate");
    assert_eq!(
        texts(&segments[2..]),
        [&format!("{side} Top Tier 1 Tower ")[..], "90", "each"],
        "the tower, the gold it pays each seat of the side, the coin before `each`"
    );
    assert_eq!(segments[3].ink, Some(NUMBER_INK), "the sum is lit");
    assert_eq!(
        segments[4].icon,
        Some(file(&client, "hud.gold")),
        "the coin"
    );
}

#[test]
fn a_tower_taken_by_no_seat_has_no_destroyer_segment() {
    let (_server, mut client) = four_seats();
    let kind = tower_fell(&client, None);
    let (_, segments) = banner(&mut client, kind);
    assert!(
        segments[0].plate.is_some(),
        "the plate leads: no hero to name"
    );
}

#[test]
fn a_barracks_banner_names_its_side_lane_and_kind_and_pays_no_sum() {
    let (_server, mut client) = four_seats();
    let (_, segments) = banner(
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
    assert!(
        segments[0].plate.is_some(),
        "the plate leads: no hero to name"
    );
    assert_eq!(texts(&segments[1..]), ["Dire Bottom Ranged Barracks "]);
    assert_eq!(segments.len(), 2, "no sum for a barracks");
}

#[test]
fn a_rune_banner_tells_who_took_it_the_runes_own_icon_and_its_name() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let (_, segments) = banner(
        &mut client,
        FeedKind::Rune {
            by: Some(me),
            team,
            kind: RuneKind::Haste,
            act: RuneAct::Activated { gold: None },
        },
    );
    assert_eq!(segments[0].icon, Some(portrait(&client, me)));
    assert_eq!(segments[0].text, name_of(&client, me));
    assert_eq!(segments[1].icon, Some(file(&client, "icon.rune.haste")));
    assert!(segments[1].plate.is_some());
    assert_eq!(texts(&segments[2..]), ["Haste rune"]);
}

#[test]
fn a_bounty_rune_banner_lights_its_gold_with_the_coin() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let (_, segments) = banner(
        &mut client,
        FeedKind::Rune {
            by: Some(me),
            team,
            kind: RuneKind::Bounty,
            act: RuneAct::Activated { gold: Some(104) },
        },
    );
    assert_eq!(texts(&segments[2..]), ["Bounty rune", "104", ""]);
    assert_eq!(segments[3].ink, Some(NUMBER_INK));
    assert_eq!(segments[4].icon, Some(file(&client, "hud.gold")));
}

#[test]
fn a_bottled_and_a_denied_rune_say_so() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let rune = |act| FeedKind::Rune {
        by: Some(me),
        team,
        kind: RuneKind::Haste,
        act,
    };
    let (_, bottled) = banner(&mut client, rune(RuneAct::Bottled));
    assert_eq!(texts(&bottled[2..]), ["bottled Haste rune"]);
    let (_, denied) = banner(&mut client, rune(RuneAct::Denied));
    assert_eq!(texts(&denied[2..]), ["denied Haste rune"]);
}

#[test]
fn a_rune_taken_by_no_known_seat_is_told_by_its_side() {
    let (_server, mut client) = four_seats();
    let team = client.me().team;
    let (_, segments) = banner(
        &mut client,
        FeedKind::Rune {
            by: None,
            team,
            kind: RuneKind::Haste,
            act: RuneAct::Activated { gold: None },
        },
    );
    assert_eq!(segments[0].icon, None);
    assert_eq!(
        segments[0].text,
        crate::feed::side_name(team),
        "the side's name stands for the hero"
    );
    assert_eq!(segments[0].ink, Some(crate::overlay::team_color(team)));
}

#[test]
fn a_shrine_banner_lights_the_experience_it_gave() {
    let (_server, mut client) = four_seats();
    let me = client.core().my_slot.expect("a seat");
    let team = client.me().team;
    let (_, segments) = banner(
        &mut client,
        FeedKind::ShrineActivated {
            by: Some(me),
            team,
            xp: 200,
        },
    );
    assert_eq!(segments[0].text, name_of(&client, me));
    assert_eq!(
        segments[1].icon,
        Some(file(&client, "hud.rune_timer.wisdom"))
    );
    assert_eq!(texts(&segments[2..]), ["Shrine of Wisdom (", "200", " XP)"]);
    assert_eq!(segments[3].ink, Some(NUMBER_INK));
}

#[test]
fn an_outpost_banner_leads_with_its_plate_and_names_the_outpost() {
    let (_server, mut client) = four_seats();
    let team = client.me().team;
    let (_, segments) = banner(
        &mut client,
        FeedKind::Captured {
            building: Some((UnitKind::Outpost, team, WorldVec::from_ints(0, 0))),
            by: team,
        },
    );
    assert_eq!(
        segments[0].icon,
        Some(file(&client, "hud.outpost_captured"))
    );
    assert!(segments[0].plate.is_some(), "the kind of event on a plate");
    assert!(
        texts(&segments).concat().contains("Your team captured"),
        "{:?}",
        texts(&segments)
    );
}

/// The kinds of line that go to the kill feed at the top right never appear in the toast column.
#[test]
fn a_hero_kill_stays_out_of_the_toasts() {
    let (_server, mut client) = four_seats();
    client.core_mut().feed.clear();
    client.core_mut().feed.push(FeedLine {
        kind: FeedKind::Disconnected { seat: SlotId(1) },
        age: 1.0,
        told: None,
    });
    client.frames(3);
    assert!(crate::feed::toasts(client.world_mut()).is_empty());
}
