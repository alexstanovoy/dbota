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

/// The model's vertices as the stage's camera sees them, in window pixels: the top left and the
/// bottom right of the figure, and the stage picture's own rectangle.
fn model_bounds(client: &mut Client) -> (Vec2, Vec2) {
    let world = client.world_mut();
    let pivot = world
        .resource::<crate::main_menu::hero_stage::HeroStage>()
        .actor
        .as_ref()
        .expect("the stage's actor")
        .pivot;
    let (node, at) = world
        .query_filtered::<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<crate::main_menu::hero_stage::HeroImage>>()
        .single(world)
        .expect("the stage's picture");
    // In window pixels, as the layout computes them.
    let rect = Rect::from_center_size(at.translation, node.size());
    let (camera, projection) = world
        .query_filtered::<(&GlobalTransform, &Projection), With<crate::main_menu::hero_stage::HeroCamera>>()
        .single(world)
        .expect("the stage camera");
    let Projection::Perspective(lens) = projection else {
        panic!("a perspective lens");
    };
    let matrix = bevy::math::proj::perspective(lens.fov, lens.aspect_ratio, lens.near, lens.far)
        * Mat4::from(camera.affine().inverse());
    let mut geometry =
        bevy::ecs::system::SystemState::<crate::main_menu::hero_stage::ModelGeometry>::new(world);
    let points = geometry
        .get(world)
        .expect("model geometry queries")
        .points(pivot)
        .expect("the rendered vertices");
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for point in points {
        let projected = matrix.project_point3(point + crate::portrait::STAGE);
        let at = rect.min
            + Vec2::new((projected.x + 1.0) / 2.0, (1.0 - projected.y) / 2.0) * rect.size();
        lo = lo.min(at);
        hi = hi.max(at);
    }
    (lo, hi)
}

/// The hero stands on the strategy screen's stage as measured: its middle where the reference has
/// it across, its feet on the ground line low in the picture, 55 to 60 percent of the window tall
/// (the idle pose moves a little), and its contact shadow right under its feet.
#[test]
fn the_strategy_screen_stands_the_picked_hero_on_the_ground_where_the_reference_has_it() {
    let (_server, mut client) = ten_seats("1920x1080");
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    client.within(PATIENCE, "the model on its stage", |c| {
        let stage = c
            .world()
            .resource::<crate::main_menu::hero_stage::HeroStage>();
        stage.hero == Some(HeroId::SHADOW_FIEND) && stage.ready()
    });
    // The camera's transform reaches its global one a frame after the stage frames the model.
    client.frames(5);
    let window = window_of(&mut client);
    let (lo, hi) = model_bounds(&mut client);
    let (height, feet, middle) = (
        (hi.y - lo.y) / window.y,
        hi.y / window.y,
        (lo.x + hi.x) / 2.0,
    );
    assert!(
        (0.53..0.62).contains(&height),
        "the model's height {height} of the window"
    );
    assert!(
        (0.86..0.92).contains(&feet),
        "the model's feet at {feet} of the window"
    );
    let wanted = crate::pregame::reference::MODEL[2] * window.x;
    assert!(
        (middle - wanted).abs() < 0.03 * window.y,
        "the model's middle across: {middle} but measured {wanted}"
    );
    let world = client.world_mut();
    let (node, at) = world
        .query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "strategy hero shadow")
        .map(|(_, node, at)| (node.size(), at.translation))
        .expect("the contact shadow on the screen");
    assert!(node.x > 0.0, "the shadow is laid out");
    assert!(
        (at.x - middle).abs() < 0.02 * window.y,
        "the shadow {} is under the model's middle {middle}",
        at.x
    );
    assert!(
        (at.y - hi.y).abs() < 0.04 * window.y,
        "the shadow {} is under the model's feet {}",
        at.y,
        hi.y
    );
}

/// Every text of the pick and the strategy screens is set in the original game's faces
/// (letter-spaced capitals and all), none in the plain fallback fonts. The item slots and the
/// shop's cells are the shop's own components and are left out.
fn plain_texts_under(client: &mut Client, root: Entity) -> Vec<String> {
    use super::harness::subtree;
    let world = client.world_mut();
    let (plain, regular) = {
        let fonts = world.resource::<crate::ui::Fonts>();
        (fonts.bold.clone(), fonts.regular.clone())
    };
    let shop: std::collections::HashSet<Entity> = subtree(world, root)
        .into_iter()
        .filter(|e| {
            world.get::<crate::shop::ShopCell>(*e).is_some()
                || world.get::<crate::hud::ItemBox>(*e).is_some()
        })
        .flat_map(|e| subtree(world, e))
        .collect();
    subtree(world, root)
        .into_iter()
        .filter(|e| !shop.contains(e))
        .filter_map(|e| {
            let font = world.get::<TextFont>(e)?;
            let used = font.font == plain.clone().into() || font.font == regular.clone().into();
            used.then(|| {
                world
                    .get::<Text>(e)
                    .map_or_else(|| format!("{e:?}"), |t| t.0.clone())
            })
        })
        .collect()
}

#[test]
fn no_text_of_the_pick_or_the_strategy_screen_is_set_in_a_fallback_font() {
    let (_server, mut client) = ten_seats("1920x1080");
    client.within(PATIENCE, "the pick screen", |c| {
        named_root(c, "pick screen").is_some()
    });
    let root = named_root(&mut client, "pick screen").expect("the pick screen");
    let plain = plain_texts_under(&mut client, root);
    assert!(
        plain.is_empty(),
        "pick screen texts in a fallback font: {plain:?}"
    );
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    client.within(PATIENCE, "the strategy phase", |c| {
        c.core().in_tactical_phase()
    });
    client.within(PATIENCE, "the strategy screen", |c| {
        c.world_mut()
            .query_filtered::<Entity, With<crate::strategy::Screen>>()
            .iter(c.world())
            .next()
            .is_some()
    });
    let root = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<crate::strategy::Screen>>()
            .single(world)
            .expect("the strategy screen")
    };
    let plain = plain_texts_under(&mut client, root);
    assert!(
        plain.is_empty(),
        "strategy screen texts in a fallback font: {plain:?}"
    );
}

fn named_root(client: &mut Client, name: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.as_str() == name)
        .map(|(e, _)| e)
}
