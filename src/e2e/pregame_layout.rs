//! The pick and the strategy screens laid out as reference screenshots of the original game measure them:
//! the rectangles the layout really computes for the measured elements
//! ([`crate::pregame::reference::ELEMENTS`]) against the measurements, at the reference's own
//! size and at 1920 by 1080, within one percent of the window's height.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};
use crate::pregame::Spot;
use crate::pregame::reference::{Anchor, ELEMENTS, Screen};

const PATIENCE: Duration = Duration::from_secs(30);
/// How far a rectangle may be from its measurement, as a share of the window's height.
const TOLERANCE: f32 = 0.01;

/// A lobby of ten seats, nine held and locked in by bots (Pudge and Shadow Fiend alternately),
/// and the client on the last one in a window of `window`.
fn ten_seats(window: &str) -> (Server, Client) {
    let server = Server::start(
        10,
        &[
            "--pick-secs",
            "600",
            "--tactical-secs",
            "600",
            "--same-heroes-allowed",
            "true",
        ],
    );
    for seat in 0..9u16 {
        quiet_seat(&server, HeroId(seat % 2));
    }
    let mut client = Client::run(&["--addr", &server.address, "--window", window]);
    client.until("the selection clock running", |c| {
        c.core().lobby.pick != bota_proto::PickPhase::Waiting
    });
    client.within(PATIENCE, "the loading screen lifted", |client| {
        let world = client.world();
        world.resource::<crate::loading::Loading>().done()
            && world.resource::<crate::trees::SceneryReady>().0
            && world.resource::<crate::minimap::PhotoTaken>().0
            && world
                .resource::<crate::loading::Parts>()
                .0
                .values()
                .all(|(done, total)| done == total)
    });
    (server, client)
}

/// The window's size in physical pixels.
fn window_of(client: &mut Client) -> Vec2 {
    let world = client.world_mut();
    world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("the window")
        .physical_size()
        .as_vec2()
}

/// The left, top, width and height of the nodes marked as a measured element, shown and laid out.
fn laid_out(client: &mut Client) -> Vec<(&'static str, [f32; 4])> {
    let world = client.world_mut();
    world
        .query::<(
            &Spot,
            &ComputedNode,
            &bevy::ui::UiGlobalTransform,
            &InheritedVisibility,
        )>()
        .iter(world)
        .filter(|(spot, node, _, seen)| !spot.0.is_empty() && seen.get() && node.size().x > 0.0)
        .map(|(spot, node, at, _)| {
            let (size, centre) = (node.size(), at.translation);
            (
                spot.0,
                [
                    centre.x - size.x / 2.0,
                    centre.y - size.y / 2.0,
                    size.x,
                    size.y,
                ],
            )
        })
        .collect()
}

/// Every element of the screen against its measurement: the faults, one line each.
fn faults(client: &mut Client, screen: Screen, report: bool) -> Vec<String> {
    let window = window_of(client);
    let window_y = window.y;
    let [reference_width, reference_height] = screen.size();
    let scale = window.y / reference_height;
    let laid = laid_out(client);
    let mut faults = Vec::new();
    for element in ELEMENTS.iter().filter(|e| e.screen == screen) {
        let Some((_, have)) = laid.iter().find(|(name, _)| *name == element.name) else {
            faults.push(format!("{}: not laid out", element.name));
            continue;
        };
        let [left, top, width, height] = element.rect;
        let left = match element.anchor {
            Anchor::Left => left * scale,
            Anchor::Centre => window.x / 2.0 + left * scale,
            Anchor::Right => window.x - (reference_width - left) * scale,
        };
        let want = [left, top * scale, width * scale, height * scale];
        let tolerance = TOLERANCE * window.y + 1.0;
        for (index, what) in ["left", "top", "width", "height"].into_iter().enumerate() {
            if index == 3 && element.rect[3] == 0.0 {
                continue;
            }
            let delta = have[index] - want[index];
            // The table the comparison page is made from: run with --nocapture.
            if report {
                println!(
                    "pregame-delta | {window_y:.0} | {} | {what} | {:.1} | {:.1} | {:.2}%",
                    element.name,
                    have[index],
                    want[index],
                    100.0 * delta / window.y
                );
            }
            if delta.abs() > tolerance {
                faults.push(format!(
                    "{}: {what} {:.1} but measured {:.1} ({delta:+.1} px of {:.0})",
                    element.name, have[index], want[index], window.y
                ));
            }
        }
    }
    faults
}

fn pick_screen_matches_at(window: &str) {
    let (_server, mut client) = ten_seats(window);
    client.within(PATIENCE, "the pick screen laid out", |c| {
        faults(c, Screen::Pick, false)
            .iter()
            .all(|fault| !fault.ends_with("not laid out"))
    });
    let faults = faults(&mut client, Screen::Pick, true);
    assert!(faults.is_empty(), "at {window}:\n{}", faults.join("\n"));
}

fn strategy_screen_matches_at(window: &str) {
    let (_server, mut client) = ten_seats(window);
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    client.within(PATIENCE, "the strategy phase", |c| {
        c.core().in_tactical_phase()
    });
    client.within(PATIENCE, "the strategy screen laid out", |c| {
        faults(c, Screen::Strategy, false)
            .iter()
            .all(|fault| !fault.ends_with("not laid out"))
    });
    let faults = faults(&mut client, Screen::Strategy, true);
    assert!(faults.is_empty(), "at {window}:\n{}", faults.join("\n"));
}

#[test]
fn the_pick_screens_measured_rectangles_hold_at_the_references_own_size() {
    pick_screen_matches_at("2868x1618");
}

#[test]
fn the_pick_screens_measured_rectangles_hold_at_full_hd() {
    pick_screen_matches_at("1920x1080");
}

#[test]
fn the_strategy_screens_measured_rectangles_hold_at_the_references_own_size() {
    strategy_screen_matches_at("2872x1611");
}

#[test]
fn the_strategy_screens_measured_rectangles_hold_at_full_hd() {
    strategy_screen_matches_at("1920x1080");
}

/// The hero's model stands on the strategy screen's stage with its middle where the reference has
/// it across the screen.
#[test]
fn the_strategy_screen_stands_the_picked_heros_model_where_the_reference_has_it() {
    let (_server, mut client) = ten_seats("1920x1080");
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    client.within(PATIENCE, "the model on its stage", |c| {
        let stage = c
            .world()
            .resource::<crate::main_menu::hero_stage::HeroStage>();
        stage.hero == Some(HeroId::SHADOW_FIEND) && stage.ready()
    });
    let window = window_of(&mut client);
    let world = client.world_mut();
    let (node, at) = world
        .query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "strategy hero")
        .map(|(_, node, at)| (node.size(), at.translation))
        .expect("the stage's picture on the screen");
    let middle = crate::pregame::reference::MODEL[2] * window.x;
    assert!(
        (at.x - middle).abs() < TOLERANCE * window.y,
        "the model's middle across: {} but measured {middle}",
        at.x
    );
    assert!(node.y > 0.4 * window.y, "the model is tall: {node}");
}
