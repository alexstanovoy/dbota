//! The pick and the strategy screens laid out as reference screenshots of the original game measure
//! them: the rectangles the layout really computes for the measured elements against
//! [`MEASURED`], at the reference's own size and at 1920 by 1080, within one percent of the
//! window's height.
//!
//! `MEASURED` is this test's own copy of the measurements, in the screenshots' pixels, read by
//! hand off the two reference screenshots (the pick one 2868 by 1618, the strategy one 2872 by
//! 1611: the elements' edges and the sizes of their boxes). It is deliberately not the table
//! the screens are drawn from (`pregame::reference`): a measurement changed there must be caught
//! here, not followed.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};
use crate::pregame::Spot;

/// Which side of the screen a measured left edge is counted from.
#[derive(Clone, Copy)]
enum Anchor {
    Left,
    Centre,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Pick,
    Strategy,
}

impl Screen {
    /// The reference screenshot's width and height.
    fn size(self) -> [f32; 2] {
        match self {
            Screen::Pick => [2868.0, 1618.0],
            Screen::Strategy => [2872.0, 1611.0],
        }
    }
}

/// One measured element: its name, screen, anchor and `[left, top, width, height]` (a zero height
/// is not compared). The strategy panel's top is the first block of the right column, where the
/// reference has the strategy map (a deliberate difference); the pick panel's lower part (the
/// description and similar heroes) is left empty, so its height is the reference's.
const MEASURED: &[(&str, Screen, Anchor, [f32; 4])] = &[
    (
        "pick seat radiant 0",
        Screen::Pick,
        Anchor::Centre,
        [-1122.0, 0.0, 176.0, 104.0],
    ),
    (
        "pick seat dire 0",
        Screen::Pick,
        Anchor::Centre,
        [206.0, 0.0, 176.0, 104.0],
    ),
    (
        "pick card 0",
        Screen::Pick,
        Anchor::Left,
        [197.0, 310.0, 64.0, 112.0],
    ),
    (
        "pick panel",
        Screen::Pick,
        Anchor::Left,
        [2012.0, 268.0, 564.0, 907.0],
    ),
    (
        "pick panel portrait",
        Screen::Pick,
        Anchor::Left,
        [2038.0, 335.0, 238.0, 366.0],
    ),
    (
        "pick panel attributes",
        Screen::Pick,
        Anchor::Left,
        [2276.0, 335.0, 274.0, 190.0],
    ),
    (
        "pick panel skill 0",
        Screen::Pick,
        Anchor::Left,
        [2152.0, 715.0, 54.0, 54.0],
    ),
    (
        "pick panel health bar",
        Screen::Pick,
        Anchor::Left,
        [2038.0, 806.0, 512.0, 38.0],
    ),
    (
        "pick lock in",
        Screen::Pick,
        Anchor::Left,
        [2012.0, 1187.0, 406.0, 98.0],
    ),
    (
        "pick random",
        Screen::Pick,
        Anchor::Left,
        [2436.0, 1187.0, 140.0, 98.0],
    ),
    (
        "strategy seat radiant 0",
        Screen::Strategy,
        Anchor::Centre,
        [-1131.0, 0.0, 176.0, 110.0],
    ),
    (
        "strategy seat dire 0",
        Screen::Strategy,
        Anchor::Centre,
        [207.0, 0.0, 176.0, 110.0],
    ),
    (
        "strategy ability 0",
        Screen::Strategy,
        Anchor::Left,
        [413.0, 315.0, 66.0, 66.0],
    ),
    (
        "strategy starting items",
        Screen::Strategy,
        Anchor::Right,
        [1246.0, 247.0, 1323.0, 0.0],
    ),
];

const PATIENCE: Duration = Duration::from_secs(30);
/// How far a rectangle may be from its measurement, as a share of the window's height.
const TOLERANCE: f32 = 0.01;
/// The measured middle of the strategy model across the screen, as a share of its width.
const MODEL_MIDDLE: f32 = 0.237;

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
    wait_for_the_game_scale(&mut client);
    (server, client)
}

/// Waits for the interface to take the game's design scale: while the loading screen is up the
/// interface is scaled as the menu is (`ui::MENU_DESIGN_HEIGHT`) and takes the HUD's
/// (`ui::HUD_DESIGN_HEIGHT`, which the pre-game screens are drawn in) a few frames after the
/// loading lifts. The window is of scale factor one and the settings' factor is one.
fn wait_for_the_game_scale(client: &mut Client) {
    client.within(
        PATIENCE,
        "the interface at the game's design scale",
        |client| {
            let window = window_of(client);
            let world = client.world();
            world.resource::<crate::ui::HudShown>().0
                && (world.resource::<UiScale>().0 - window.y / crate::ui::HUD_DESIGN_HEIGHT).abs()
                    < 1e-3
        },
    );
    // The layout follows the scale by a frame.
    client.frames(2);
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
    for (name, on, anchor, rect) in MEASURED.iter().filter(|(_, on, ..)| *on == screen) {
        let _ = on;
        let Some((_, have)) = laid.iter().find(|(laid_name, _)| laid_name == name) else {
            faults.push(format!("{name}: not laid out"));
            continue;
        };
        let [left, top, width, height] = *rect;
        let left = match anchor {
            Anchor::Left => left * scale,
            Anchor::Centre => window.x / 2.0 + left * scale,
            Anchor::Right => window.x - (reference_width - left) * scale,
        };
        let want = [left, top * scale, width * scale, height * scale];
        let tolerance = TOLERANCE * window.y;
        for (index, what) in ["left", "top", "width", "height"].into_iter().enumerate() {
            if index == 3 && rect[3] == 0.0 {
                continue;
            }
            let delta = have[index] - want[index];
            // The table the comparison page is made from: run with --nocapture.
            if report {
                println!(
                    "pregame-delta | {window_y:.0} | {name} | {what} | {:.1} | {:.1} | {:.2}%",
                    have[index],
                    want[index],
                    100.0 * delta / window.y
                );
            }
            if delta.abs() > tolerance {
                faults.push(format!(
                    "{name}: {what} {:.1} but measured {:.1} ({delta:+.1} px of {:.0})",
                    have[index], want[index], window.y
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
    let wanted = MODEL_MIDDLE * window.x;
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

/// What the texts under a screen's root are set in: every text but those of the shop's own
/// components (the storefront's cells and the item slots), with its string and its font.
fn texts_under(client: &mut Client, root: Entity) -> Vec<(String, TextFont)> {
    use super::harness::subtree;
    let world = client.world_mut();
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
            let font = world.get::<TextFont>(e)?.clone();
            let text = world
                .get::<Text>(e)
                .map(|t| t.0.clone())
                .or_else(|| world.get::<TextSpan>(e).map(|t| t.0.clone()))?;
            Some((text, font))
        })
        .collect()
}

/// Every text of the screen is set in one of the four faces of the interface (the original
/// game's own from the install, which a test cache holds), each resolved to a loaded font: not
/// in the plain fallback fonts, not in the default handle, not in any third font. The texts
/// that must be among those inspected are named, so a screen drawn without them fails too.
fn assert_in_the_games_faces(client: &mut Client, root: Entity, expected: &[&str], what: &str) {
    use crate::theme::Face;
    let texts = texts_under(client, root);
    let world = client.world();
    let fonts = world.resource::<crate::ui::Fonts>();
    let faces: Vec<Handle<Font>> = [Face::Display, Face::Bold, Face::Semibold, Face::Regular]
        .map(|face| fonts.face(face).clone())
        .into();
    for fallback in [&fonts.bold, &fonts.regular] {
        assert!(
            !faces.contains(fallback),
            "the cache's faces are not the plain fonts (the test cache lacks the install's faces)"
        );
    }
    let loaded = world.resource::<Assets<Font>>();
    for face in &faces {
        assert!(
            loaded.get(face).is_some(),
            "face {face:?} resolves to a loaded font"
        );
    }
    let allowed: Vec<bevy::text::FontSource> = faces.iter().cloned().map(Into::into).collect();
    let stray: Vec<&str> = texts
        .iter()
        .filter(|(_, font)| !allowed.contains(&font.font))
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(
        stray.is_empty(),
        "{what}: texts outside the faces: {stray:?}"
    );
    for want in expected {
        assert!(
            texts.iter().any(|(text, _)| text == want),
            "{what}: {want:?} was among the texts inspected ({} inspected)",
            texts.len()
        );
    }
}

#[test]
fn every_text_of_the_pick_and_the_strategy_screen_is_set_in_the_games_faces() {
    let (_server, mut client) = ten_seats("1920x1080");
    client.within(PATIENCE, "the pick screen", |c| {
        named_root(c, "pick screen").is_some()
    });
    client.click_named("pick shadow fiend");
    let root = named_root(&mut client, "pick screen").expect("the pick screen");
    client.within(PATIENCE, "the panel's texts", |c| {
        c.shown_texts().iter().any(|t| t == "SHADOW FIEND")
    });
    assert_in_the_games_faces(
        &mut client,
        root,
        &[
            "CHOOSE YOUR HERO",
            "SHADOW FIEND",
            "STRENGTH",
            "RANDOM",
            "LOCK IN",
        ],
        "the pick screen",
    );
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
    assert_in_the_games_faces(
        &mut client,
        root,
        &[
            "STARTING ITEMS",
            "REMAINING GOLD",
            "INVENTORY",
            "ENTERING BATTLE AS",
            "SHADOW FIEND",
        ],
        "the strategy screen",
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
