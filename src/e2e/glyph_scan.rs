//! The Glyph and the Scan as Dota's HUD stands them: two buttons of one size in the two wells of one
//! plate against the minimap's right edge, the Scan above the Glyph, each with its popup, and the
//! Glyph working by its click and its key with the wait the server reports.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_proto::{Cheat, HeroId, Team};

use super::harness::{Client, Patience, Server, quiet_seat};

fn joined_scene() -> (Server, Client) {
    let server = Server::for_hud(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    (server, client)
}

fn scene() -> (Server, Client) {
    let (server, mut client) = joined_scene();
    client.ready_with_match_held(&server);
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
    let glyph = named(&mut client, "hud glyph");
    let scan = named(&mut client, "hud scan");
    let glyph_bounds = rect(&client, glyph);
    let scan_bounds = rect(&client, scan);
    let world = client.world();
    let interactive = [
        world.get::<crate::glyph::GlyphButton>(glyph).is_some(),
        world.get::<crate::scan::ScanButton>(scan).is_some(),
    ];
    assert_eq!(
        interactive,
        [true, true],
        "layout measures the interactive buttons, at {glyph_bounds:?} and {scan_bounds:?}"
    );
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
    for (name, button, foot) in [("glyph", glyph_bounds, 6.0), ("scan", scan_bounds, 50.0)] {
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

/// Hovered, the Glyph uses the install's description and countdown label.
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
    // The popup says the install's words, not the client's own: its description is the install's
    // line, which the client's fallback is not.
    let install = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("DOTA_Glossary_Map_Structures_Glyph_Desc", &[])
        .expect("the install's description of the Glyph");
    assert!(
        said(&mut client, &install),
        "the popup carries the install's description: {:?}",
        super::hero_page::tooltip_words(&mut client)
    );
    client.frames(10);
    let template = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(
            "DOTA_HUD_GlyphCooldown",
            &[("cooldown_time", "@time@".into())],
        )
        .expect("the install's Glyph countdown label");
    let (before, after) = template.split_once("@time@").expect("the time placeholder");
    let words = super::hero_page::tooltip_words(&mut client);
    assert!(
        words
            .iter()
            .flat_map(|text| text.lines())
            .filter_map(|line| line.trim().strip_prefix(before)?.strip_suffix(after))
            .any(|time| time.parse::<u32>().is_ok_and(|seconds| seconds > 0)),
        "the wait in the popup uses {template:?}: {words:?}"
    );
}

/// The button counts the wait the server reports, in seconds.
#[test]
fn the_glyph_button_counts_the_wait_the_server_reports() {
    let (_server, mut client) = scene();
    assert_countdown(&mut client);
}

/// Bring the live match to the first-charge deadline with the HUD still covered.
#[test]
fn loading_through_the_first_glyph_deadline_preserves_the_countdown() {
    const PART: &str = "delayed HUD fixture";
    let (server, mut client) = joined_scene();
    let first_charge =
        bota_core::rules::Schedule::of(client.core().pregame_ticks()).glyph_first_use();
    let before_charge = first_charge - 1;
    client
        .world_mut()
        .resource_mut::<crate::loading::Parts>()
        .set(PART, 0, 1);
    assert!(!client.world().resource::<crate::loading::Loading>().done());

    // Take each lockstep snapshot without scheduling thousands of full UI frames under load.
    let mut patience = super::harness::Patience::of(Duration::from_secs(30));
    while client.tick() < before_charge {
        patience.pass("the snapshot before the first Glyph charge");
        let tick = client.tick();
        client.core_mut().pump(0.0);
        if client.tick() == tick {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(client.tick(), before_charge);

    let mut frames = 3u8;
    client
        .app
        .add_systems(Update, move |mut parts: ResMut<crate::loading::Parts>| {
            frames = frames.saturating_sub(1);
            if frames == 0 {
                parts.set(PART, 1, 1);
            }
        });

    client.ready_with_match_held(&server);
    assert_eq!(
        client
            .world()
            .resource::<crate::loading::Parts>()
            .0
            .get(PART),
        Some(&(1, 1)),
        "the HUD stayed covered until the delayed part completed"
    );
    assert_eq!(client.tick(), before_charge);
    assert_countdown(&mut client);
    client.until("the first charge arrives after readiness", |c| {
        c.tick() >= first_charge && glyph_charges(c) == Some(1)
    });
}

fn assert_countdown(client: &mut Client) {
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
        seconds == told.div_ceil(rate),
        "the button says {seconds} s, the server reports {told} ticks"
    );
}

/// What the Glyph's badge shows: its number while it is drawn, None while it is not.
fn glyph_badge(client: &mut Client) -> Option<String> {
    badge_number(client, "hud glyph charges")
}

fn badge_number(client: &mut Client, name: &str) -> Option<String> {
    let badge = named(client, name);
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

#[test]
fn the_empty_glyph_shows_its_wait_without_a_charge_badge() {
    let server = Server::for_hud(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.ready_with_match_held(&server);
    client.cheat(Cheat::StartGame);
    client.until("the Glyph button is drawn", |client| {
        let button = named(client, "hud glyph");
        client
            .world()
            .get::<InheritedVisibility>(button)
            .is_some_and(|seen| seen.get())
    });
    assert_eq!(glyph_charges(&client), Some(0));
    assert_countdown(&mut client);
    assert_eq!(glyph_badge(&mut client), None, "zero hides the charge disc");
}

#[test]
fn both_charge_badges_use_the_reference_label_geometry_and_font() {
    let (_server, mut client) = scene();
    client.cheat(Cheat::Refresh);
    client.frames(4);
    let unit = dota(&client);
    for (button, badge) in [
        ("hud glyph", "hud glyph charges"),
        ("hud scan", "hud scan charges"),
    ] {
        let entity = named(&mut client, badge);
        let face = named_rect(&mut client, button);
        let bounds = rect(&client, entity);
        near("reference disc diameter", bounds.width(), 20.0 * unit);
        near("reference right alignment", bounds.max.x, face.max.x);
        near(
            "reference bottom inset",
            face.max.y - bounds.max.y,
            2.0 * unit,
        );
        let world = client.world();
        assert!(
            world
                .get::<InheritedVisibility>(entity)
                .is_some_and(|seen| seen.get()),
            "{badge} is drawn"
        );
        let label = world
            .get::<Children>(entity)
            .expect("badge children")
            .iter()
            .find(|child| world.get::<Text>(*child).is_some())
            .expect("charge label");
        let font = world.get::<TextFont>(label).expect("charge font");
        let ink = world.get::<TextColor>(label).expect("charge label colour");
        assert_eq!(ink.0, Color::srgb_u8(176, 188, 194));
        assert_eq!(
            font.font_size,
            bevy::text::FontSize::Px(18.0 * crate::ui::DOTA)
        );
        assert_eq!(
            font.font,
            world.resource::<crate::ui::Fonts>().regular.clone().into()
        );
    }
}

#[test]
fn the_glyph_omits_the_popup_without_installed_text() {
    let (_server, client) = scene();
    assert!(crate::glyph::tip(&crate::dota_text::Texts::default(), client.core()).is_none());
}

pub(super) fn recorded_team_buttons() -> super::fog_pov::ReplayFile {
    let mut game = super::fog_pov::Spectated::pair("2");
    game.cheat(Cheat::Refresh);
    game.wait("a charge on each side", |driver, _| {
        driver.glyph().and_then(|glyph| glyph.charges) == Some(1)
    });
    game.order(bota_proto::Order::Glyph);
    game.wait("one Glyph waiting and the other ready", |_, client| {
        let waits = |team| {
            client
                .view()
                .glyphs
                .iter()
                .find(|glyph| glyph.team == team)
                .and_then(|glyph| glyph.ready_after)
        };
        waits(Team::Radiant).is_some_and(|ticks| ticks >= 100 * bota_core::rules::TICKS_PER_SECOND)
            && waits(Team::Dire) == Some(0)
    });
    game.save()
}

/// The lockstep server advances once the client acknowledges a view. Pump the connection
/// between HUD checks so the intervening ticks need neither UI frames nor wall-clock pacing.
fn advance_to(client: &mut Client, tick: u32) {
    let mut patience = Patience::of(Duration::from_secs(30));
    while client.view().game_tick() < tick {
        patience.pass("the Glyph's next charge boundary");
        client.core_mut().pump(0.0);
        std::thread::yield_now();
    }
}

#[test]
fn the_glyph_button_counts_from_empty_to_full_and_back_after_both_uses() {
    use bota_core::rules::{GLYPH_USE_COOLDOWN_TICKS, TICKS_PER_SECOND};

    let server = Server::for_hud(1, &["--immortal-structures"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.ready_with_match_held(&server);
    client.cheat(Cheat::StartGame);
    assert_eq!(
        glyph_charges(&client),
        Some(0),
        "charges at game tick {}",
        client.view().game_tick()
    );
    for (charges, label) in [(1, "1"), (2, "2")] {
        client.cheat(Cheat::Refresh);
        client.until("the granted Glyph charge is drawn", |c| {
            glyph_charges(c) == Some(charges) && glyph_badge(c).as_deref() == Some(label)
        });
    }

    press(&mut client, KeyCode::KeyJ);
    client.until("the Glyph shows one charge after the first use", |c| {
        glyph_charges(c) == Some(1) && glyph_badge(c).as_deref() == Some("1")
    });
    let glyph = client.core().glyph().expect("the player's Glyph");
    let wait = glyph.ready_after.expect("the player's wait");
    assert!(
        wait > GLYPH_USE_COOLDOWN_TICKS - TICKS_PER_SECOND && wait <= GLYPH_USE_COOLDOWN_TICKS,
        "the use starts the seven-second lock: {wait} ticks left"
    );
    let cover = glyph_cover(&mut client);
    assert!(cover > 0.6, "the lock darkens the Glyph: {cover}");
    let unlock = client.view().game_tick() + wait;
    advance_to(&mut client, unlock - GLYPH_USE_COOLDOWN_TICKS / 2);
    client.step_frames(2);
    let angle = glyph_sweep(&mut client).stops[1]
        .angle
        .expect("the edge of the sweep");
    assert!(
        (angle - std::f32::consts::PI).abs() < 0.2,
        "the seven-second lock is about halfway swept after 3.5 seconds: {angle} radians"
    );
    advance_to(&mut client, unlock - 1);
    client.until("the use lock clears and its sweep lightens", |c| {
        c.core().glyph().and_then(|glyph| glyph.ready_after) == Some(0) && glyph_cover(c) < cover
    });
    press(&mut client, KeyCode::KeyJ);
    client.until("the empty Glyph hides the badge after the last use", |c| {
        glyph_charges(c) == Some(0) && glyph_badge(c).is_none()
    });
}

#[test]
fn the_all_teams_halves_show_their_sweeps_and_popups_without_count_labels() {
    use bota_proto::Order;

    let mut game = super::fog_pov::Spectated::pair("2");
    game.cheat(Cheat::Refresh);
    game.wait("one charge before spending", |driver, _| {
        driver.glyph().and_then(|glyph| glyph.charges) == Some(1)
    });
    game.order(Order::Glyph);
    game.wait("different charges for the two sides", |driver, client| {
        driver.glyph().and_then(|glyph| glyph.charges) == Some(0)
            && client
                .view()
                .glyphs
                .iter()
                .any(|glyph| glyph.team == Team::Radiant && glyph.charges == Some(0))
            && client
                .view()
                .glyphs
                .iter()
                .any(|glyph| glyph.team == Team::Dire && glyph.charges == Some(1))
    });
    let fountain = bota_core::map_of(bota_core::MAP2_ID).fountains[1];
    game.order(Order::Scan { at: fountain });
    game.wait("Radiant has spent its Scan charge", |_, client| {
        client
            .core()
            .scan_of(Team::Radiant)
            .is_some_and(|side| side.ledger.charges == 0)
    });

    for player in [&game.driver, game.other.as_ref().expect("the Dire player")] {
        let side = player.my_team().expect("the player's own side");
        let view = player.view().expect("the player's view");
        let opposing = view
            .glyphs
            .iter()
            .find(|glyph| glyph.team != side)
            .expect("the enemy Glyph");
        assert_eq!(
            opposing.charges, None,
            "{side:?} only receives its own count"
        );
        assert_eq!(
            opposing.ready_after, None,
            "{side:?} only receives its own wait"
        );
    }
    super::hud_layout::on_screen(&mut game.client);
    assert_team_tooltips_without_face_labels(&mut game.client);
    assert!(team_cover(&mut game.client, "hud glyph radiant") > 0.0);
    assert_eq!(team_cover(&mut game.client, "hud glyph dire"), 0.0);
    assert!(team_cover(&mut game.client, "hud scan radiant") > 0.0);
    assert_eq!(team_cover(&mut game.client, "hud scan dire"), 0.0);
    assert_eq!(glyph_badge(&mut game.client), None);
    assert_eq!(badge_number(&mut game.client, "hud scan charges"), None);
    for (side, expected) in [(Team::Radiant, None), (Team::Dire, Some("1"))] {
        game.next_side(side);
        game.wait("the watched side's Glyph badge", |_, client| {
            glyph_badge(client).as_deref() == expected
        });
    }
}

fn assert_team_tooltips_without_face_labels(client: &mut Client) {
    for ability in ["glyph", "scan"] {
        for (side, team_key) in [
            ("radiant", "DOTA_Tooltip_Glyph_Radiant"),
            ("dire", "DOTA_Tooltip_Glyph_Dire"),
        ] {
            let face = named(client, &format!("hud {ability} {side}"));
            let world = client.world_mut();
            let mut descendants = world.query::<&Children>();
            assert!(
                descendants
                    .query(world)
                    .iter_descendants(face)
                    .all(|part| world.get::<Text>(part).is_none()),
                "the {side} {ability} half has no count or cooldown label"
            );
            let team = client
                .world()
                .resource::<crate::dota_text::Texts>()
                .label(team_key, &[])
                .expect("the installed team label");
            client.hover_ui(face, Duration::from_secs(10));
            client.within(
                Duration::from_secs(10),
                "the hovered team's tooltip",
                |client| {
                    super::hero_page::tooltip_words(client)
                        .iter()
                        .any(|text| text.contains(&team))
                },
            );
        }
    }
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
        let server = Server::for_hud(2, &[]);
        quiet_seat(&server, HeroId::SHADOW_FIEND);
        let setting = format!("ui_scale={scale}");
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--set", &setting]);
        client.ready_with_match_held(&server);
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

/// A recorded match in which Radiant's Glyph holds its charges.
fn recorded_glyph() -> super::fog_pov::ReplayFile {
    let mut game = super::fog_pov::Spectated::start("2");
    game.cheat(bota_proto::Cheat::Refresh);
    game.wait("a charge told", |driver, _| {
        driver
            .glyph()
            .and_then(|glyph| glyph.charges)
            .is_some_and(|n| n > 0)
    });
    for _ in 0..10 {
        game.cheat(bota_proto::Cheat::Wtf { on: false });
    }
    game.save()
}

/// A replay has charge data in either view. The team view has a badge and the all-teams view
/// draws the two halves.
#[test]
fn a_replay_draws_the_glyph_charge_badge_only_from_a_team_perspective() {
    let file = recorded_glyph();
    for pov in [None, Some(0)] {
        let mut client = super::fog_pov::replay_observed(&file, pov, &[], |_| {});
        client.frames(10);
        let held = glyph_charges(&client);
        assert!(
            held.is_some_and(|n| n > 0),
            "{pov:?}: the stream tells charges: {held:?}"
        );
        let glyph = named(&mut client, "hud glyph");
        let world = client.world();
        assert!(
            world
                .get::<InheritedVisibility>(glyph)
                .is_some_and(|seen| seen.get())
                == pov.is_some(),
            "{pov:?}: the normal button is drawn for a team view"
        );
        assert_eq!(
            glyph_badge(&mut client),
            pov.and(held).map(|n| n.to_string())
        );
    }
}

fn team_cover(client: &mut Client, name: &str) -> f32 {
    let face = named(client, name);
    let world = client.world();
    let paint = world.get::<Children>(face).expect("a clipped disc")[0];
    let gradient = world
        .get::<Children>(paint)
        .expect("the face's art and sweep")
        .iter()
        .find_map(|child| world.get::<BackgroundGradient>(child))
        .expect("the team's sweep");
    gradient
        .0
        .iter()
        .filter_map(|gradient| match gradient {
            bevy::ui::Gradient::Conic(conic) => Some(conic),
            _ => None,
        })
        .flat_map(|conic| &conic.stops)
        .map(|stop| stop.color.to_srgba().alpha)
        .fold(0.0, f32::max)
}

/// The darkest the Glyph's sweep is over the button, 0 when it is clear.
fn glyph_sweep(client: &mut Client) -> &bevy::ui::ConicGradient {
    let world = client.world_mut();
    let gradient = world
        .query_filtered::<&BackgroundGradient, With<crate::glyph::GlyphSweep>>()
        .single(world)
        .expect("the sweep");
    gradient
        .0
        .iter()
        .find_map(|gradient| match gradient {
            bevy::ui::Gradient::Conic(conic) => Some(conic),
            _ => None,
        })
        .expect("the Glyph's conic sweep")
}

fn glyph_cover(client: &mut Client) -> f32 {
    glyph_sweep(client)
        .stops
        .iter()
        .map(|stop| stop.color.to_srgba().alpha)
        .fold(0.0, f32::max)
}

/// With one of two charges spent, the button is usable once the seven seconds between uses are out
/// but still shows the cooldown face while the other charge is to come, as the game's does.
#[test]
fn the_glyph_shows_the_recharge_of_the_charge_still_to_come() {
    let (_server, mut client) = scene();
    client.cheat(bota_proto::Cheat::Refresh);
    client.cheat(bota_proto::Cheat::Refresh);
    assert_eq!(
        glyph_charges(&client),
        Some(2),
        "two charges are held before spending"
    );
    press(&mut client, KeyCode::KeyJ);
    client.until("a charge spent", |c| glyph_charges(c) == Some(1));
    client.until("the seven seconds out", |c| {
        c.core().glyph().and_then(|glyph| glyph.ready_after) == Some(0)
    });
    client.frames(5);
    assert_eq!(glyph_charges(&client), Some(1), "one charge is held");
    assert!(
        glyph_cover(&mut client) > 0.2,
        "the cooldown face is drawn while a charge is missing"
    );
}

#[test]
fn a_late_all_teams_watcher_sees_the_glyph_half_without_a_badge_or_recharge_sweep() {
    let server = Server::for_hud(1, &["--spectators", "open"]);
    let mut player = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    player.ready_with_match_held(&server);
    player.cheat(Cheat::Refresh);
    player.cheat(Cheat::Refresh);
    assert_eq!(
        glyph_charges(&player),
        Some(2),
        "two charges before the watcher arrives"
    );
    player.core_mut().activate_glyph();
    player.until("one usable charge", |c| {
        c.core()
            .glyph()
            .is_some_and(|glyph| glyph.charges == Some(1) && glyph.ready_after == Some(0))
    });
    let mut watcher = Client::run_on(&server, &["--spectate"], &[]);
    watcher.within(
        Duration::from_secs(25),
        "the late watcher receives one charge",
        |c| {
            player.step_frames(1);
            glyph_charges(c) == Some(1)
        },
    );
    watcher.ready_with_match_held(&server);
    assert_eq!(glyph_badge(&mut watcher), None);
    assert_eq!(
        team_cover(&mut watcher, "hud glyph radiant"),
        0.0,
        "the watcher never saw the recharge begin"
    );
}

#[test]
fn switching_the_replay_side_clears_the_glyph_recharge_sweep() {
    use bota_client_core::Intent;
    use bota_proto::{Cheat, Order, SlotId};

    let mut game = super::fog_pov::Spectated::pair("2");
    game.cheat(Cheat::Refresh);
    game.cheat(Cheat::Refresh);
    game.wait("two charges before spending", |driver, _| {
        driver.glyph().is_some_and(|glyph| glyph.charges == Some(2))
    });
    // Leave full charges visible for several replay frames before either use.
    for _ in 0..4 {
        game.cheat(Cheat::Wtf { on: false });
    }
    game.order(Order::Glyph);
    let other = game.other.as_mut().expect("the Dire player");
    other.activate_glyph();
    game.wait(
        "both Glyphs have spent one charge and their use lock has ended",
        |_, c| {
            c.view()
                .glyphs
                .iter()
                .all(|glyph| glyph.charges == Some(1) && glyph.ready_after == Some(0))
        },
    );
    let file = game.save();
    let mut client = super::fog_pov::replay_from_side(&file, 0);
    client.frames(2);
    assert!(
        glyph_cover(&mut client) > 0.2,
        "Radiant's recharge was observed"
    );
    client.core_mut().act(Intent::NextEyes);
    client.frames(2);
    assert_eq!(client.core().my_slot, Some(SlotId(1)));
    assert_eq!(glyph_badge(&mut client).as_deref(), Some("1"));
    assert_eq!(
        glyph_cover(&mut client),
        0.0,
        "Dire must not inherit Radiant's observed timer"
    );
}
