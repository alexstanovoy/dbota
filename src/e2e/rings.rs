//! The rings at a unit's feet and the map's lines: hovering shows Dota's
//! yellow ring only, round the selection ring's own spot; every line one width, scaled with the
//! window.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, MID, Server, ring_of};

/// The rings drawn this frame in a colour: each one's middle and radius.
fn rings(client: &Client, colour: Color) -> Vec<(Vec2, f32)> {
    let colour = colour.to_linear();
    client
        .line_strips()
        .into_iter()
        .filter(|(points, c)| *c == colour && points.len() > 8)
        .map(|(points, _)| ring_of(&points))
        .collect()
}

/// One's own hero hovered while it walks: on every frame the hover ring is Dota's yellow, round
/// the very spot its selection ring is drawn round, a little wider, and it stays as long as the
/// cursor does (it was a ring in the side's colour closing in from three times the size and
/// gone in a quarter of a second, round the spot the cursor's pick found, which a walking hero
/// had left: two rings off each other).
#[test]
fn the_hover_ring_is_yellow_and_round_the_selection_rings_spot() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let me = client.me().clone();
    // The hero is picked only once the player picks it: the selection ring is under what is.
    client.core_mut().act(Intent::SelectUnit(Some(me.id)));
    let selection = crate::overlay::team_color(me.team).with_alpha(0.9);
    client
        .core_mut()
        .act(Intent::MoveTo((MID.0 as f32 + 2000.0, MID.1 as f32)));
    let mut seen = 0;
    for _ in 0..90 {
        // The camera rides with the hero: the pointer stays on it at its drawn place.
        let body = client.body(me.id).expect("drawn");
        let at = client
            .world()
            .get::<crate::units::Unit>(body)
            .expect("a unit")
            .at;
        let on = client.screen_of((at.x, at.y)).expect("on screen");
        client.mouse(on);
        let hover = rings(&client, crate::pick::HOVER_RING);
        let picked = rings(&client, selection);
        if hover.is_empty() {
            continue;
        }
        assert_eq!(hover.len(), 1, "one hover ring");
        assert_eq!(picked.len(), 1, "one selection ring");
        let ((h, hr), (p, pr)) = (hover[0], picked[0]);
        assert!(h.distance(p) < 0.5, "concentric: hover {h}, selection {p}");
        assert!(
            (hr - pr - 9.0).abs() < 1.0,
            "a little wider: {hr} against {pr}"
        );
        seen += 1;
    }
    assert!(seen > 60, "the hover ring stayed: {seen} frames of 90");
    let walked = client.view().unit(me.id).expect("seen").pos;
    assert!(walked.x.to_f32() > MID.0 as f32 + 300.0, "the hero walked");
}

/// The map's lines (every one drawn with the one gizmo setting) are 4 pixels wide at 900 lines, scaled with the window
/// (it was 2.5 at every size).
#[test]
fn the_lines_on_the_map_are_one_width_scaled_with_the_window() {
    for (window, width) in [("1920x1080", 4.8), ("3840x2160", 9.6)] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--window", window]);
        client.frames(3);
        let store = client
            .world()
            .resource::<bevy::gizmos::config::GizmoConfigStore>();
        let (config, _) = store.config::<DefaultGizmoConfigGroup>();
        assert!(
            (config.line.width - width).abs() < 1e-3,
            "{window}: {}",
            config.line.width
        );
    }
}

/// The middles of the rings drawn this frame at a radius, in a colour.
pub(super) fn rings_of(client: &Client, colour: Color, radius: f32) -> Vec<Vec2> {
    rings(client, colour)
        .into_iter()
        .filter(|(_, r)| (r - radius).abs() < 2.0)
        .map(|(at, _)| at)
        .collect()
}

/// Shadow Fiend walking with the show-ranges key held, at the client's frame rate between the
/// game's ticks: on every frame his attack range ring is round where his model is drawn that
/// frame, within a pixel, and each raze spot moves as far as his model does, frame by frame (they
/// were placed from where the game's tick put him, a step behind at times, and jumped at each
/// tick: Alt's circles jittered as he walked).
#[test]
fn the_show_ranges_circles_move_with_the_walking_hero() {
    use bevy::time::TimeUpdateStrategy;
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--show-ranges"]);
    client.stand_at(MID);
    let raze = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == bota_proto::AbilityId::RAZE_NEAR)
        .expect("Shadowraze");
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Ability(raze as u8),
        ctrl: true,
    });
    client.until("Shadowraze learned", |c| c.me().abilities[raze].level > 0);
    client.follow_me();
    let me = client.me().clone();
    let reach = me.attack_range.to_f32() + me.bound.to_f32();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1.0 / 75.0),
        ));
    client
        .core_mut()
        .act(Intent::MoveTo((MID.0 as f32 + 2000.0, MID.1 as f32)));
    client.until("walking", |c| {
        c.view()
            .unit(me.id)
            .is_some_and(|u| u.pos.x.to_f32() > MID.0 as f32 + 50.0)
    });
    let raze_red = crate::overlay::SOUL_RED.with_alpha(0.7);
    let raze_radius = bota_core::rules::RAZE_RADIUS as f32;
    let mut last: Option<(Vec2, Vec<Vec2>)> = None;
    for _ in 0..60 {
        client.frames(1);
        let body = client.body(me.id).expect("drawn");
        let at = client
            .world()
            .get::<crate::units::Unit>(body)
            .expect("a unit")
            .at;
        let attack = rings_of(&client, Color::WHITE.with_alpha(0.6), reach);
        assert_eq!(attack.len(), 1, "one attack range ring");
        let (ring, model) = (
            client
                .screen_of((attack[0].x, attack[0].y))
                .expect("on screen"),
            client.screen_of((at.x, at.y)).expect("on screen"),
        );
        assert!(
            ring.distance(model) < 1.0,
            "ring {ring} round the model {model}"
        );
        let mut razes = rings_of(&client, raze_red, raze_radius);
        assert_eq!(razes.len(), 3, "three raze spots");
        razes.sort_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
        if let Some((was, spots)) = &last {
            let moved = at - *was;
            for (now, then) in razes.iter().zip(spots) {
                let step = *now - *then;
                assert!(
                    (step - moved).length() < 0.5,
                    "a raze spot stepped {step}, the model {moved}"
                );
            }
        }
        last = Some((at, razes));
    }
}

/// Blood Grenade taken up to throw, the pointer moving over the ground near the hero: on every
/// frame the circle of the ground it would splash (its area, from the game's data) is round the
/// very point under the pointer, within a pixel, as Dota draws it (it was a small mark of a
/// fixed size).
#[test]
fn aiming_blood_grenade_draws_its_area_under_the_cursor() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(bota_proto::Cheat::Item {
        item: bota_proto::ItemId::BLOOD_GRENADE,
    });
    let slot = client
        .slot_of(bota_proto::ItemId::BLOOD_GRENADE)
        .expect("the grenade in the bag");
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Item(slot),
        ctrl: false,
    });
    let area = bota_core::item_def(bota_proto::ItemId::BLOOD_GRENADE)
        .expect("the grenade's data")
        .radius()
        .expect("the grenade takes in an area") as f32;
    let me = client.me().pos;
    let (x, y) = (me.x.to_f32(), me.y.to_f32());
    for step in 0..30 {
        let spot = (x + 200.0 + 10.0 * step as f32, y + 100.0);
        let pointer = client.screen_of(spot).expect("the spot on screen");
        client.mouse(pointer);
        client.frames(1);
        let circles = rings_of(&client, crate::overlay::GOOD, area);
        assert_eq!(circles.len(), 1, "one circle of the grenade's area");
        let middle = client
            .screen_of((circles[0].x, circles[0].y))
            .expect("on screen");
        assert!(
            middle.distance(pointer) < 1.0,
            "the circle round {middle}, the pointer at {pointer}"
        );
    }
}

/// What a ward gives, from its item's data row: its true sight where it gives that, else its
/// vision.
fn what_a_ward_gives(item: bota_proto::ItemId) -> f32 {
    let def = bota_core::item_def(item).expect("the ward's data");
    let bota_core::UseSpec::Ward { units, .. } = def.effect else {
        panic!("{item:?} is no ward");
    };
    let unit = units[0].unit();
    (if unit.true_sight > 0 {
        unit.true_sight
    } else {
        unit.day_vision
    }) as f32
}

/// Aiming a ward draws the circle of what it will give, as Dota does: an Observer Ward's vision
/// and a Sentry Ward's true sight, from the data of each. Round the very point under the pointer
/// as it moves, within a pixel, like the grenade's (only a small mark marked the spot before).
#[test]
fn aiming_a_ward_draws_its_vision_or_true_sight_circle_under_the_cursor() {
    use bota_client_core::{Intent, Slot};
    for item in [
        bota_proto::ItemId::OBSERVER_WARD,
        bota_proto::ItemId::SENTRY_WARD,
    ] {
        let radius = what_a_ward_gives(item);
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        client.stand_at(MID);
        client.cheat(bota_proto::Cheat::Item { item });
        let slot = client.slot_of(item).expect("the ward in the bag");
        client.core_mut().act(Intent::Press {
            slot: Slot::Item(slot),
            ctrl: false,
        });
        let me = client.me().pos;
        let (x, y) = (me.x.to_f32(), me.y.to_f32());
        // Within the ward's reach (500), where the circle is drawn in the colour of a use that
        // would be made.
        for step in 0..25 {
            let spot = (x + 200.0 + 10.0 * step as f32, y + 100.0);
            let pointer = client.screen_of(spot).expect("the spot on screen");
            client.mouse(pointer);
            client.frames(1);
            let circles = rings_of(&client, crate::overlay::GOOD, radius);
            assert_eq!(circles.len(), 1, "{item:?}: one circle of {radius}");
            let middle = client
                .screen_of((circles[0].x, circles[0].y))
                .expect("on screen");
            assert!(
                middle.distance(pointer) < 1.0,
                "{item:?}: the circle round {middle}, the pointer at {pointer}"
            );
        }
    }
}

/// A Chasm Stone's AoE bonus widens what is drawn as the server widens it: with one carried, the
/// three raze spots of `--show-ranges` and the circle under the cursor while Blood Grenade is
/// aimed grow by its 40 (`shadowraze_radius` and the grenade's `radius` are
/// `affected_by_aoe_increase`).
#[test]
fn a_chasm_stone_widens_the_drawn_raze_spots_and_grenade_circle() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--show-ranges"]);
    client.stand_at(MID);
    for item in [
        bota_proto::ItemId::CHASM_STONE,
        bota_proto::ItemId::BLOOD_GRENADE,
    ] {
        client.cheat(bota_proto::Cheat::Item { item });
    }
    let bonus = bota_core::item_def(bota_proto::ItemId::CHASM_STONE)
        .expect("the stone's data")
        .carried
        .aoe_bonus;
    client.until("the stone shown carried", |c| {
        bota_client_core::carried_of(c.me()).aoe_bonus == bonus
    });
    let raze = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == bota_proto::AbilityId::RAZE_NEAR)
        .expect("Shadowraze");
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Ability(raze as u8),
        ctrl: true,
    });
    client.until("Shadowraze learned", |c| c.me().abilities[raze].level > 0);
    client.follow_me();
    client.frames(2);
    let raze_red = crate::overlay::SOUL_RED.with_alpha(0.7);
    let widened = (bota_core::rules::RAZE_RADIUS + bonus) as f32;
    assert_eq!(
        rings_of(&client, raze_red, widened).len(),
        3,
        "three wider raze spots"
    );
    let slot = client
        .slot_of(bota_proto::ItemId::BLOOD_GRENADE)
        .expect("the grenade in the bag");
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Item(slot),
        ctrl: false,
    });
    let area = bota_core::item_def(bota_proto::ItemId::BLOOD_GRENADE)
        .and_then(bota_core::ItemDef::radius)
        .expect("the grenade takes in an area");
    let me = client.me().pos;
    let spot = (me.x.to_f32() + 300.0, me.y.to_f32() + 100.0);
    let pointer = client.screen_of(spot).expect("the spot on screen");
    client.mouse(pointer);
    client.frames(1);
    assert_eq!(
        rings_of(&client, crate::overlay::GOOD, (area + bonus) as f32).len(),
        1,
        "one wider circle of the grenade's area"
    );
}

/// An Aether Lens lengthens the drawn reach of an aimed item as the server lengthens the cast:
/// with one carried, the ring round a Healing Salve being aimed grows by its `cast_range_bonus`.
#[test]
fn an_aether_lens_lengthens_the_drawn_reach_of_an_aimed_salve() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    for item in [
        bota_proto::ItemId::AETHER_LENS,
        bota_proto::ItemId::HEALING_SALVE,
    ] {
        client.cheat(bota_proto::Cheat::Item { item });
    }
    let bonus = bota_core::item_def(bota_proto::ItemId::AETHER_LENS)
        .expect("the lens's data")
        .carried
        .cast_range_bonus;
    client.until("the lens shown carried", |c| {
        bota_client_core::carried_of(c.me()).cast_range_bonus == bonus
    });
    let reach = bota_core::item_def(bota_proto::ItemId::HEALING_SALVE)
        .expect("the salve's data")
        .cast_range;
    let slot = client
        .slot_of(bota_proto::ItemId::HEALING_SALVE)
        .expect("the salve in the bag");
    client.follow_me();
    client.core_mut().act(Intent::Press {
        slot: bota_client_core::Slot::Item(slot),
        ctrl: false,
    });
    client.frames(2);
    assert_eq!(
        rings_of(&client, crate::overlay::ARCANE, (reach + bonus) as f32).len(),
        1,
        "one ring at the lengthened reach"
    );
    assert!(
        rings_of(&client, crate::overlay::ARCANE, reach as f32).is_empty(),
        "none at the salve's own"
    );
}
