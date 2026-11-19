//! The Glyph and the Scan as Dota's HUD stands them: two buttons of one size in the two wells of one
//! plate against the minimap's right edge, the Scan above the Glyph, each with its popup, and the
//! Glyph working by its click and its key with the wait the server reports.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_proto::{Cheat, HeroId, Team};

use super::harness::{Client, Patience, Server, quiet_seat};

const RADIANT_BADGE: &str = "hud glyph radiant charges";

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
    use bota_core::rules::{GLYPH_USE_COOLDOWN_TICKS, Schedule, TICKS_PER_SECOND};

    let server = Server::start(1, &["--immortal-structures"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    assert_eq!(
        glyph_charges(&client),
        Some(0),
        "charges at game tick {}",
        client.view().game_tick()
    );
    client.until("the empty Glyph count is drawn", |c| {
        glyph_badge(c).as_deref() == Some("0")
    });
    assert_eq!(badge_number(&mut client, RADIANT_BADGE), None);

    let schedule = Schedule::of(client.core().pregame_ticks());
    for (minute, charges, label) in [(3, 1, "1"), (9, 2, "2")] {
        let due = schedule.at_minute(minute);
        advance_to(&mut client, due - 1);
        assert_eq!(client.view().game_tick(), due - 1);
        assert_eq!(glyph_charges(&client), Some(charges - 1));
        client.until("the earned Glyph charge is drawn", |c| {
            glyph_charges(c) == Some(charges) && glyph_badge(c).as_deref() == Some(label)
        });
        assert!(client.view().game_tick() >= due);
    }
    super::hud_layout::on_screen(&mut client);
    let glyph = named_rect(&mut client, "hud glyph");
    let badge = named_rect(&mut client, "hud glyph charges");
    let unit = dota(&client);
    near("the badge's width", badge.width(), 20.0 * unit);
    near("the badge's right edge", badge.max.x, glyph.max.x);
    near(
        "the badge above the button's foot",
        glyph.max.y - badge.max.y,
        2.0 * unit,
    );

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
    client.until("the Glyph shows zero after the last use", |c| {
        glyph_charges(c) == Some(0) && glyph_badge(c).as_deref() == Some("0")
    });
}

#[test]
fn a_spectator_can_read_each_sides_glyph_count_and_players_only_receive_their_own() {
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
    }
    super::hud_layout::on_screen(&mut game.client);
    assert_eq!(
        badge_number(&mut game.client, RADIANT_BADGE).as_deref(),
        Some("0")
    );
    assert_eq!(glyph_badge(&mut game.client).as_deref(), Some("1"));
    let radiant = named_rect(&mut game.client, RADIANT_BADGE);
    let dire = named_rect(&mut game.client, "hud glyph charges");
    assert!(radiant.max.x < dire.min.x, "Radiant is left of Dire");
    for (side, expected) in [(Team::Radiant, "0"), (Team::Dire, "1")] {
        game.next_side(side);
        game.wait("the watched side's Glyph badge is drawn", |_, client| {
            glyph_badge(client).as_deref() == Some(expected)
                && badge_number(client, RADIANT_BADGE).is_none()
        });
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

/// A watcher of everything, and a replay seen from a seat, are told the Glyph of the side shown and
/// the button shows it, with its charges, as the Scan's does.
#[test]
fn a_replay_shows_the_glyph_of_the_side_it_is_seen_from_or_of_both_sides() {
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
                .is_some_and(|seen| seen.get()),
            "{pov:?}: the Glyph's button is drawn"
        );
        assert_eq!(
            glyph_badge(&mut client),
            held.map(|n| n.to_string()),
            "{pov:?}: its badge tells the charges"
        );
        assert_eq!(
            badge_number(&mut client, RADIANT_BADGE).as_deref(),
            pov.is_none().then_some("1"),
            "{pov:?}: a second badge is drawn only with the all-teams view"
        );
    }
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
fn a_late_watcher_sees_the_glyph_charge_without_a_recharge_sweep() {
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
    assert_eq!(
        badge_number(&mut watcher, RADIANT_BADGE).as_deref(),
        Some("1")
    );
    assert_eq!(
        glyph_cover(&mut watcher),
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
