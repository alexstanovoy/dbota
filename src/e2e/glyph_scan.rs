//! The Glyph and the Scan as Dota's HUD stands them: two buttons of one size in the two wells of one
//! plate against the minimap's right edge, the Scan above the Glyph, each with its popup, and the
//! Glyph working by its click and its key with the wait the server reports.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};

fn scene() -> (Server, Client) {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    super::hud_layout::on_screen(&mut client);
    (server, client)
}

fn named(client: &mut Client, name: &str) -> Entity {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} is in the HUD"))
}

fn rect(client: &Client, entity: Entity) -> Rect {
    let world = client.world();
    let node = world.get::<ComputedNode>(entity).expect("laid out");
    let at = world.get::<UiGlobalTransform>(entity).expect("placed");
    Rect::from_center_size(at.translation, node.size())
}

fn named_rect(client: &mut Client, name: &str) -> Rect {
    let entity = named(client, name);
    rect(client, entity)
}

/// One of Dota's pixels, in the window's.
fn dota(client: &Client) -> f32 {
    crate::ui::DOTA * client.world().resource::<UiScale>().0
}

fn near(what: &str, got: f32, want: f32) {
    assert!(
        (got - want).abs() <= 2.0,
        "{what}: {got:.1} px, wanted {want:.1} px"
    );
}

/// Two buttons of 44 of Dota's pixels in the wells of the plate (6 from its left edge, 6 and 50 from
/// its foot), the plate against the minimap's right edge and as high as the minimap.
#[test]
fn the_glyph_and_the_scan_stand_in_the_two_wells_of_the_plate_against_the_minimaps_right_edge() {
    let (_server, mut client) = scene();
    client.frames(20);
    let unit = dota(&client);
    let minimap = named_rect(&mut client, "hud minimap");
    let plate = named_rect(&mut client, "hud glyph plate");
    let glyph = named_rect(&mut client, "hud glyph");
    let scan = named_rect(&mut client, "hud scan");
    near(
        "the plate's left edge at the minimap's right",
        plate.min.x,
        minimap.max.x,
    );
    near("the plate's width", plate.width(), 74.0 * unit);
    near(
        "the plate as high as the minimap",
        plate.height(),
        minimap.height(),
    );
    near(
        "the plate's foot at the minimap's",
        plate.max.y,
        minimap.max.y,
    );
    for (name, button, foot) in [("glyph", glyph, 6.0), ("scan", scan, 50.0)] {
        near(
            &format!("the {name} button's width"),
            button.width(),
            44.0 * unit,
        );
        near(
            &format!("the {name} button's height"),
            button.height(),
            44.0 * unit,
        );
        near(
            &format!("the {name} button from the plate's left"),
            button.min.x - plate.min.x,
            6.0 * unit,
        );
        near(
            &format!("the {name} button from the plate's foot"),
            plate.max.y - button.max.y,
            foot * unit,
        );
    }
}

/// Hovered, the Glyph says what it is in the game's words, and the wait it has to run (the Glyph is
/// not ready before the game's fifth minute).
#[test]
fn the_glyph_has_the_games_popup_with_its_wait() {
    let (_server, mut client) = scene();
    let glyph = named(&mut client, "hud glyph");
    client.hover_ui(glyph, Duration::from_secs(30));
    // The popup's lines are spans of its body, which the texts shown do not list.
    let said = |c: &mut Client, word: &str| {
        super::hero_page::tooltip_words(c)
            .iter()
            .any(|text| text.to_lowercase().contains(&word.to_lowercase()))
    };
    client.within(Duration::from_secs(20), "the Glyph's popup", |c| {
        said(c, "Glyph of Fortification")
    });
    client.frames(10);
    assert!(
        said(&mut client, "Cooldown:"),
        "the wait in the popup: {:?}",
        super::hero_page::tooltip_words(&mut client)
    );
}

/// The button counts the wait the server reports, in seconds.
#[test]
fn the_glyph_button_counts_the_wait_the_server_reports() {
    let (_server, mut client) = scene();
    client.frames(5);
    let told = client
        .core()
        .glyph()
        .and_then(|glyph| glyph.ready_after)
        .expect("told");
    assert!(told > 0, "the Glyph is still to wait for at the start");
    let rate = u32::from(client.core().tick_rate());
    let world = client.world_mut();
    let shown = world
        .query_filtered::<(&Text, &InheritedVisibility), With<crate::glyph::GlyphSeconds>>()
        .iter(world)
        .map(|(text, seen)| (text.0.clone(), seen.get()))
        .next()
        .expect("the seconds");
    assert!(shown.1, "the seconds are drawn on the button");
    let seconds: u32 = shown.0.parse().expect("a number of seconds");
    assert!(
        seconds.abs_diff(told.div_ceil(rate)) <= 2,
        "the button says {seconds} s, the server reports {told} ticks"
    );
}

/// What the Glyph's badge shows: its number while it is drawn, None while it is not.
fn glyph_badge(client: &mut Client) -> Option<String> {
    let badge = named(client, "hud glyph charges");
    let world = client.world();
    let shown = world.get::<InheritedVisibility>(badge)?.get();
    let text = world
        .get::<Children>(badge)?
        .iter()
        .find_map(|child| world.get::<Text>(child))?;
    shown.then(|| text.0.clone())
}

fn glyph_charges(client: &Client) -> Option<u8> {
    client.core().glyph().and_then(|glyph| glyph.charges)
}

fn press(client: &mut Client, key: KeyCode) {
    client.key(key, ButtonState::Pressed);
    client.key(key, ButtonState::Released);
}

/// Since the game's 7.42 the Glyph holds charges, and its button says how many, as the Scan's does:
/// nothing while none is held, the number while one or two are, one fewer once it is used.
#[test]
fn the_glyph_button_shows_the_charges_it_holds_and_one_fewer_once_it_is_used() {
    let (_server, mut client) = scene();
    client.frames(5);
    assert_eq!(
        glyph_charges(&client),
        Some(0),
        "none before the first wait is out"
    );
    assert_eq!(glyph_badge(&mut client), None, "no badge without a charge");
    client.cheat(bota_proto::Cheat::Refresh);
    client.until("a charge held", |c| glyph_charges(c).is_some_and(|n| n > 0));
    client.frames(5);
    let held = glyph_charges(&client).expect("told");
    assert_eq!(
        glyph_badge(&mut client),
        Some(held.to_string()),
        "the badge tells the {held} charges held"
    );
    press(&mut client, KeyCode::KeyJ);
    client.until("a charge spent", |c| glyph_charges(c) == Some(held - 1));
    client.frames(5);
    assert!(
        client
            .core()
            .glyph()
            .and_then(|glyph| glyph.ready_after)
            .is_some_and(|wait| wait > 0),
        "the button waits between two uses"
    );
    let left = held - 1;
    assert_eq!(
        glyph_badge(&mut client),
        (left > 0).then(|| left.to_string()),
        "the badge tells what is left"
    );
}

/// A click on the Glyph's button uses a charge, as its key does.
#[test]
fn a_click_on_the_glyph_button_uses_a_charge() {
    let (_server, mut client) = scene();
    client.cheat(bota_proto::Cheat::Refresh);
    client.until("a charge held", |c| glyph_charges(c).is_some_and(|n| n > 0));
    let held = glyph_charges(&client).expect("told");
    let glyph = named(&mut client, "hud glyph");
    client.hover_ui(glyph, Duration::from_secs(30));
    client.click(MouseButton::Left);
    client.until("a charge spent", |c| glyph_charges(c) == Some(held - 1));
}

/// The plate's art is clear above its wells, and a click there is a click on the world: a right
/// click on the terrain beside the minimap, over the plate's clear upper part, orders a move, at
/// every interface scale.
#[test]
fn a_right_click_on_the_terrain_over_the_plates_clear_part_orders_a_move() {
    for scale in ["0.5", "1", "2"] {
        let server = Server::start(2, &[]);
        quiet_seat(&server, HeroId::SHADOW_FIEND);
        let setting = format!("ui_scale={scale}");
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--set", &setting]);
        super::hud_layout::on_screen(&mut client);
        client.frames(20);
        let plate = named_rect(&mut client, "hud glyph plate");
        // A fifth of the plate's way down is far above its wells and its art.
        let spot = Vec2::new(
            plate.min.x + 0.3 * plate.width(),
            plate.min.y + 0.2 * plate.height(),
        );
        let sent = client.record_sent_orders();
        client.point(spot);
        client.frames(3);
        let before = sent.get();
        client.click(MouseButton::Right);
        client.frames(5);
        assert!(
            sent.get() > before,
            "scale {scale}: a right click at {spot} beside the minimap orders no move"
        );
    }
}
