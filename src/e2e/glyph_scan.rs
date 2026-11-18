//! The Glyph and the Scan as Dota's HUD stands them: two buttons of one size in the two wells of one
//! plate against the minimap's right edge, the Scan above the Glyph, each with its popup, and the
//! Glyph working by its click and its key with the wait the server reports.

use std::time::Duration;

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
