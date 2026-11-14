//! Where the in-game HUD's parts stand in a window, against the reference game's layout.
//!
//! The reference game scales its HUD with the screen's height alone and anchors each part to a
//! corner or the middle of the screen. Every row of [`PARTS`] is one part's rectangle in a
//! 2877 by 1620 frame of that game (the numbers are measured layout rectangles, nothing of the
//! picture itself), with the anchors the part keeps when the window's size or shape changes: each
//! test lays the client out in a window of its own size and shape, scales the frame by the
//! window's height, and asks every edge to be within [`TOLERANCE`] of the screen's height.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_client_core::TOWN_PORTAL_SLOT;
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};

/// The reference frame, in the reference game's pixels.
const FRAME: Vec2 = Vec2::new(2877.0, 1620.0);
/// How far an edge may be from its place, in shares of the window's height.
const TOLERANCE: f32 = 0.01;

/// Where a part is measured from, per axis.
#[derive(Clone, Copy, Debug)]
enum Anchor {
    /// From the left or the top edge.
    Start,
    /// From the middle of the window.
    Middle,
    /// From the right or the bottom edge.
    End,
}

/// What finds a part's node.
#[derive(Clone, Copy, Debug)]
enum Find {
    Named(&'static str),
    Ability(u8),
    Item(u8),
    ShopButton,
    GrabAll,
}

/// A part of the HUD, in the reference frame: its place `(x, y, width, height)`.
struct Part {
    region: &'static str,
    name: &'static str,
    find: Find,
    anchors: (Anchor, Anchor),
    rect: (f32, f32, f32, f32),
}

const fn part(
    region: &'static str,
    name: &'static str,
    find: Find,
    anchors: (Anchor, Anchor),
    rect: (f32, f32, f32, f32),
) -> Part {
    Part {
        region,
        name,
        find,
        anchors,
        rect,
    }
}

use Anchor::{End, Middle, Start};
use Find::{Ability, GrabAll, Item, Named, ShopButton};

const PARTS: &[Part] = &[
    // The top bar: ten seats either side of the clock, centred.
    part(
        "top bar",
        "first radiant seat",
        Named("hud seat Radiant 0"),
        (Middle, Start),
        (816.0, 0.0, 92.5, 59.0),
    ),
    part(
        "top bar",
        "last dire seat",
        Named("hud seat Dire 4"),
        (Middle, Start),
        (1971.0, 0.0, 92.5, 59.0),
    ),
    part(
        "top bar",
        "clock",
        Named("hud clock"),
        (Middle, Start),
        (1281.0, 0.0, 318.0, 64.0),
    ),
    // The corner: the menu's buttons and the tally under them.
    part(
        "corner",
        "dashboard button",
        Named("hud menu dashboard"),
        (Start, Start),
        (22.0, 20.0, 40.0, 40.0),
    ),
    part(
        "corner",
        "scoreboard button",
        Named("hud menu scoreboard"),
        (Start, Start),
        (91.0, 20.0, 40.0, 40.0),
    ),
    part(
        "corner",
        "kills and last hits tally",
        Named("hud tally"),
        (Start, Start),
        (0.0, 90.0, 230.0, 60.0),
    ),
    // The minimap at the foot of the left edge, the glyph at its right.
    part(
        "minimap",
        "minimap",
        Named("hud minimap"),
        (Start, End),
        (0.0, 1200.0, 416.0, 420.0),
    ),
    part(
        "minimap",
        "glyph button",
        Named("hud glyph"),
        (Start, End),
        (420.0, 1538.0, 75.0, 75.0),
    ),
    // The hero panel at the foot of the middle.
    part(
        "hero panel",
        "panel",
        Named("hud panel"),
        (Middle, End),
        (690.0, 1402.0, 1465.0, 218.0),
    ),
    part(
        "hero panel",
        "portrait",
        Named("hud portrait"),
        (Middle, End),
        (742.0, 1402.0, 238.0, 218.0),
    ),
    part(
        "hero panel",
        "level badge",
        Named("hud level"),
        (Middle, End),
        (681.0, 1551.0, 62.0, 62.0),
    ),
    part(
        "hero panel",
        "innate mark",
        Named("hud innate"),
        (Middle, End),
        (1098.0, 1430.0, 55.0, 55.0),
    ),
    part(
        "hero panel",
        "first ability",
        Ability(0),
        (Middle, End),
        (1166.0, 1417.0, 77.0, 77.0),
    ),
    part(
        "hero panel",
        "sixth ability",
        Ability(5),
        (Middle, End),
        (1600.0, 1417.0, 77.0, 77.0),
    ),
    part(
        "hero panel",
        "first level-up plate",
        Named("hud level up 0"),
        (Middle, End),
        (1166.0, 1355.0, 77.0, 47.0),
    ),
    part(
        "hero panel",
        "health bar",
        Named("hud health"),
        (Middle, End),
        (1000.0, 1527.0, 749.0, 39.0),
    ),
    part(
        "hero panel",
        "mana bar",
        Named("hud mana"),
        (Middle, End),
        (1000.0, 1569.0, 749.0, 38.0),
    ),
    part(
        "hero panel",
        "first item",
        Item(0),
        (Middle, End),
        (1775.0, 1414.0, 89.0, 67.0),
    ),
    part(
        "hero panel",
        "sixth item",
        Item(5),
        (Middle, End),
        (1972.0, 1485.0, 89.0, 67.0),
    ),
    part(
        "hero panel",
        "first backpack item",
        Item(6),
        (Middle, End),
        (1775.0, 1559.0, 89.0, 48.0),
    ),
    part(
        "hero panel",
        "neutral item slot",
        Named("hud neutral"),
        (Middle, End),
        (2074.0, 1388.0, 64.0, 64.0),
    ),
    part(
        "hero panel",
        "town portal slot",
        Item(TOWN_PORTAL_SLOT),
        (Middle, End),
        (2072.0, 1531.0, 70.0, 70.0),
    ),
    // The shop's corner, at the foot of the right edge.
    part(
        "shop corner",
        "gold",
        ShopButton,
        (End, End),
        (2450.0, 1545.0, 151.0, 60.0),
    ),
    part(
        "shop corner",
        "grab all tab",
        GrabAll,
        (End, End),
        (2400.0, 1322.0, 130.0, 30.0),
    ),
];

/// A measured rectangle: its edges in the window's pixels.
fn measured(client: &mut Client, find: Find) -> Option<Rect> {
    let world = client.world_mut();
    let entity = match find {
        Find::Named(name) => world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, n)| n.as_str() == name)
            .map(|(e, _)| e),
        Find::Ability(slot) => world
            .query::<(Entity, &crate::hud::AbilityBox)>()
            .iter(world)
            .find(|(_, b)| b.0 == slot)
            .map(|(e, _)| e),
        Find::Item(slot) => world
            .query::<(Entity, &crate::hud::ItemBox)>()
            .iter(world)
            .find(|(_, b)| b.0 == slot)
            .map(|(e, _)| e),
        Find::ShopButton => world
            .query_filtered::<Entity, With<crate::shop::ShopButton>>()
            .iter(world)
            .next(),
        Find::GrabAll => world
            .query_filtered::<Entity, With<crate::shop::GrabAll>>()
            .iter(world)
            .next(),
    }?;
    let size = world.get::<ComputedNode>(entity)?.size();
    let centre = world.get::<UiGlobalTransform>(entity)?.translation;
    Some(Rect::from_center_size(centre, size))
}

/// Where a part should stand in a window, in the window's pixels.
fn expected(part: &Part, window: Vec2) -> Rect {
    let scale = window.y / FRAME.y;
    let (x, y, w, h) = part.rect;
    let place = |anchor: Anchor, at: f32, frame: f32, screen: f32| match anchor {
        Anchor::Start => at * scale,
        Anchor::Middle => screen / 2.0 + (at - frame / 2.0) * scale,
        Anchor::End => screen - (frame - at) * scale,
    };
    let left = place(part.anchors.0, x, FRAME.x, window.x);
    let top = place(part.anchors.1, y, FRAME.y, window.y);
    Rect::new(left, top, left + w * scale, top + h * scale)
}

/// Every part's complaint, by region: how far each edge is from its place, in shares of the height.
fn complaints(client: &mut Client, window: Vec2) -> Vec<String> {
    let mut all = Vec::new();
    for part in PARTS {
        let want = expected(part, window);
        let Some(got) = measured(client, part.find) else {
            all.push(format!("{}: {} is not laid out", part.region, part.name));
            continue;
        };
        let off = |a: f32, b: f32| (a - b).abs() / window.y;
        let edges = [
            ("left", got.min.x, want.min.x),
            ("top", got.min.y, want.min.y),
            ("right", got.max.x, want.max.x),
            ("bottom", got.max.y, want.max.y),
        ];
        for (edge, got, want) in edges {
            if off(got, want) > TOLERANCE {
                all.push(format!(
                    "{}: {} {edge} edge is at {got:.0} px, wanted {want:.0} px ({:.1} % of the height off)",
                    part.region,
                    part.name,
                    off(got, want) * 100.0
                ));
            }
        }
    }
    all
}

/// Waits until the game is on screen (the loading screen gone), where the game's interface scale
/// holds: before that the interface is laid out for the menu.
pub(super) fn on_screen(client: &mut Client) {
    client.within(
        std::time::Duration::from_secs(120),
        "the game on screen",
        |c| c.world().resource::<crate::ui::HudShown>().0,
    );
    client.frames(10);
}

/// The client in a game, laid out in a window of `width` by `height`.
fn in_window(width: u32, height: u32) -> (Client, Vec2) {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let size = format!("{width}x{height}");
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--window", &size]);
    // The fonts and the first layout settle over a few frames.
    on_screen(&mut client);
    client.frames(30);
    (client, Vec2::new(width as f32, height as f32))
}

fn layout_follows_reference_at(width: u32, height: u32) {
    let (mut client, window) = in_window(width, height);
    let complaints = complaints(&mut client, window);
    assert!(
        complaints.is_empty(),
        "at {width}x{height}:\n{}",
        complaints.join("\n")
    );
}

#[test]
fn hud_layout_follows_the_reference_at_1920x1080() {
    layout_follows_reference_at(1920, 1080);
}

#[test]
fn hud_layout_follows_the_reference_at_the_reference_size_2877x1620() {
    layout_follows_reference_at(2877, 1620);
}

#[test]
fn hud_layout_follows_the_reference_in_a_16_by_10_window() {
    layout_follows_reference_at(1920, 1200);
}

#[test]
fn hud_layout_follows_the_reference_in_an_ultrawide_window() {
    layout_follows_reference_at(3440, 1440);
}

/// The Demo Hero game's panel at the left: its width is the reference's (418 of 1620 lines of the
/// height), its top edge is at 175, and it stays clear of the minimap under it.
fn demo_panel_follows_reference_at(width: u32, height: u32) {
    use crate::main_menu::{Act, MenuCore};
    use bevy::ui_widgets::Button as NativeButton;

    let size = format!("{width}x{height}");
    let mut client = Client::run(&["--window", &size]);
    client.frames(30);
    crate::main_menu::tests::use_matching_local_server(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
    );
    super::menu::act(&mut client, Act::InspectHero(HeroId::SHADOW_FIEND));
    let world = client.world_mut();
    let demo = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "demo hero")
        .map(|(entity, _)| entity)
        .expect("the hero page's demo button");
    let at = client.centre_of(demo).expect("the demo button laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.within(
        std::time::Duration::from_secs(120),
        "the sandbox's hero",
        |c| c.core().my_hero().is_some(),
    );
    on_screen(&mut client);
    client.frames(30);
    let window = Vec2::new(width as f32, height as f32);
    let panel = measured(&mut client, Named("hud demo panel")).expect("the demo panel is drawn");
    let minimap = measured(&mut client, Named("hud minimap")).expect("the minimap");
    let scale = window.y / FRAME.y;
    let off = |got: f32, want: f32| (got - want).abs() / window.y;
    let mut complaints = Vec::new();
    if off(panel.min.x, 0.0) > TOLERANCE {
        complaints.push(format!("left edge at {}, wanted 0", panel.min.x));
    }
    if off(panel.min.y, 175.0 * scale) > TOLERANCE {
        complaints.push(format!(
            "top edge at {}, wanted {}",
            panel.min.y,
            175.0 * scale
        ));
    }
    if off(panel.max.x, 418.0 * scale) > TOLERANCE {
        complaints.push(format!(
            "right edge at {}, wanted {}",
            panel.max.x,
            418.0 * scale
        ));
    }
    if panel.max.y > minimap.min.y + 0.5 {
        complaints.push(format!(
            "bottom edge {} runs into the minimap at {}",
            panel.max.y, minimap.min.y
        ));
    }
    assert!(
        complaints.is_empty(),
        "at {size}: {}",
        complaints.join("; ")
    );
}

#[test]
fn demo_panel_follows_the_reference_at_1920x1080() {
    demo_panel_follows_reference_at(1920, 1080);
}

#[test]
fn demo_panel_follows_the_reference_in_a_16_by_10_window() {
    demo_panel_follows_reference_at(1920, 1200);
}

/// The seat's figures as the match counts them: kills, deaths, assists, last hits, denies.
fn figures(client: &Client) -> (u16, u16, u16, u16, u16) {
    let me = client.core().my_slot.expect("a seat");
    let player = client.view().player(me).expect("own row");
    let score = player.creep_score.expect("own creep score");
    (
        player.kills,
        player.deaths,
        player.assists,
        score.last_hits,
        score.denies,
    )
}

/// An enemy hero made where the hero stands, struck down by it.
fn kill_a_hero(client: &mut Client) {
    use bota_client_core::Intent;
    use bota_proto::{Cheat, UnitKind};
    let kills = figures(client).0;
    let at = bota_proto::Vec2::from_ints(super::harness::MID.0 + 250, super::harness::MID.1);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at,
    });
    let mine = client.me().team;
    client.until("the foe hero made", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Hero && u.team != mine)
    });
    let foe = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != mine)
        .map(|u| ((u.pos.x.to_f32(), u.pos.y.to_f32()), u.id))
        .expect("the foe");
    client.core_mut().act(Intent::AttackAt {
        at: foe.0,
        unit: Some(foe.1),
    });
    client.until("the foe struck down", |c| figures(c).0 > kills);
}

/// The corner tally tells each of the seat's figures in its own place: the kills, deaths and
/// assists on one line and the last hits and denies on the next, which differ from each other
/// here (two kills, a death to the foe's fountain, no assist, the creeps of a camp last hit, no deny) so
/// that a figure in the wrong place is a wrong word.
#[test]
fn the_corner_tally_tells_each_figure_of_the_seat_in_its_own_place() {
    use bota_client_core::Intent;
    use bota_proto::{Cheat, CreepSide};
    let server = Server::start(1, &["--map", "0", "--runes", "none"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    on_screen(&mut client);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::Levels { count: 24 });
    client.cheat(Cheat::Invulnerable { on: true });
    client.stand_at(super::harness::MID);
    kill_a_hero(&mut client);
    kill_a_hero(&mut client);
    let camp = (super::harness::MID.0 + 300, super::harness::MID.1);
    client.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: bota_proto::Vec2::from_ints(camp.0, camp.1),
    });
    client.core_mut().act(Intent::AttackAt {
        at: (camp.0 as f32, camp.1 as f32),
        unit: None,
    });
    client.until("the camp's creeps last hit", |c| figures(c).3 >= 2);
    // The foe's fountain takes the hero down, as it does a Demo Hero that wanders in.
    let foe_side = if client.me().team == bota_proto::Team::Radiant {
        bota_proto::Team::Dire
    } else {
        bota_proto::Team::Radiant
    };
    let fountain = bota_core::fountain_pos(client.core().session().expect("match").map(), foe_side);
    client.cheat(Cheat::Invulnerable { on: false });
    client.cheat(Cheat::Teleport { at: fountain });
    client.until("the hero fallen", |c| figures(c).1 >= 1);
    client.frames(3);
    let (kills, deaths, assists, last_hits, denies) = figures(&client);
    assert!(
        kills > deaths && deaths > assists && last_hits > denies,
        "the figures differ from each other: {kills} {deaths} {assists} {last_hits} {denies}"
    );
    let shown = client.shown_texts();
    for line in [
        "K / D / A".to_owned(),
        format!("{kills} / {deaths} / {assists}"),
        "LH / DN".to_owned(),
        format!("{last_hits} / {denies}"),
    ] {
        assert!(shown.contains(&line), "{line:?} in {shown:?}");
    }
}
