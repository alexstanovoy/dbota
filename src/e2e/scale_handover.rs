//! The interface's scale and what it is for change in the same frame. The menu is laid out in its
//! own design height (`ui::MENU_DESIGN_HEIGHT`) and the game's screens in the HUD's
//! (`ui::HUD_DESIGN_HEIGHT`); a frame that reveals the game (the loading screen lifting) or the
//! menu (leaving the game) at the other's scale is a pop the player sees, however short.
//!
//! The tests step the client frame by frame and record, for each, what the frame reveals and the
//! scale its layout was made at (read off a node's `ComputedNode`, which is the scale the frame
//! was laid out with).

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};
use crate::loading::Loading;
use crate::main_menu::{Act, MenuCore};

/// What the frames after the first need to be consecutive and settled.
const SETTLED_FRAMES: usize = 10;
/// The longest a wait for a state may last, in wall-clock time.
const PATIENCE: Duration = Duration::from_secs(120);

/// One frame: what it reveals and the scale it was laid out at.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The loading screen covers everything.
    loading: bool,
    /// The game's screens are what shows (not the loading screen, the menu or the post-game).
    game: bool,
    /// The interface scale this frame was laid out with.
    scale: f32,
    /// The window's height.
    height: f32,
}

impl Frame {
    /// The scale this frame should have: the design height of what it reveals.
    fn wanted(&self) -> f32 {
        let design = if self.game {
            crate::ui::HUD_DESIGN_HEIGHT
        } else {
            crate::ui::MENU_DESIGN_HEIGHT
        };
        self.height / design
    }

    fn pops(&self) -> bool {
        (self.scale - self.wanted()).abs() > 1e-3
    }
}

/// The frame that has just run, once an interface node exists to read its scale from.
fn frame(client: &mut Client) -> Option<Frame> {
    let world = client.world_mut();
    let window = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .ok()?
        .physical_size()
        .y as f32;
    let loading = world.resource::<Loading>().shown;
    let post = world.non_send::<crate::postgame::PostGame>().visible();
    let shows_game = world.non_send::<MenuCore>().shows_game();
    let node = world
        .query_filtered::<&ComputedNode, With<GlobalZIndex>>()
        .iter(world)
        .find(|node| node.size().x > 0.0)?;
    Some(Frame {
        loading,
        game: shows_game && !loading && !post,
        scale: 1.0 / node.inverse_scale_factor(),
        height: window,
    })
}

/// Runs the client frame by frame until `done` says the frames so far settled, recording each.
fn record(
    client: &mut Client,
    frames: &mut Vec<Frame>,
    what: &str,
    mut done: impl FnMut(&[Frame]) -> bool,
) {
    let began = Instant::now();
    while !done(frames) {
        assert!(began.elapsed() < PATIENCE, "{what}: not settled in time");
        client.frames(1);
        if let Some(seen) = frame(client) {
            frames.push(seen);
        }
    }
}

/// The last `SETTLED_FRAMES` frames all reveal the same (and are at the right scale).
fn settled(frames: &[Frame], game: bool) -> bool {
    frames.len() >= SETTLED_FRAMES
        && frames[frames.len() - SETTLED_FRAMES..]
            .iter()
            .all(|f| if game { f.game } else { !f.game && !f.loading })
}

/// The frames that reveal something at the scale of something else, from the first frame at
/// which the interface had taken the loading screen's scale (the start-up frames before it are
/// the window opening).
fn pops(frames: &[Frame]) -> Vec<Frame> {
    let from = frames
        .iter()
        .position(|f| f.loading && !f.pops())
        .unwrap_or(frames.len());
    frames[from..]
        .iter()
        .filter(|f| f.pops())
        .copied()
        .collect()
}

/// The frames that reveal something at the scale of something else.
fn wrong(frames: &[Frame]) -> Vec<Frame> {
    frames.iter().filter(|f| f.pops()).copied().collect()
}

fn playing() -> (Server, Client) {
    let server = Server::start(2, &["--pick-secs", "600", "--tactical-secs", "600"]);
    quiet_seat(&server, HeroId::PUDGE);
    let client = Client::run(&["--addr", &server.address, "--window", "1920x1080"]);
    (server, client)
}

/// The loading screen lifts onto the pick screen: no frame shows it at the menu's scale.
#[test]
fn no_frame_of_the_loading_screens_lifting_shows_the_game_at_the_menus_scale() {
    let (_server, mut client) = playing();
    let mut frames = Vec::new();
    record(&mut client, &mut frames, "the game revealed", |f| {
        settled(f, true)
    });
    assert!(
        frames.iter().any(|f| f.loading),
        "the loading screen was seen up ({} frames)",
        frames.len()
    );
    let popped = pops(&frames);
    assert!(popped.is_empty(), "frames at the wrong scale: {popped:#?}");
}

/// Leaving the game reveals the menu: no frame shows it at the game's scale.
#[test]
fn no_frame_of_leaving_the_game_shows_the_menu_at_the_games_scale() {
    let (_server, mut client) = playing();
    let mut frames = Vec::new();
    record(&mut client, &mut frames, "the game revealed", |f| {
        settled(f, true)
    });
    let before = frames.len();
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .act(Act::Disconnect);
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .act(Act::Answer(true));
    record(&mut client, &mut frames, "the menu revealed", |f| {
        f.len() >= before + SETTLED_FRAMES && settled(f, false)
    });
    assert!(
        frames[before..].iter().any(|f| !f.game),
        "the menu was seen after the game"
    );
    let popped = wrong(&frames[before..]);
    assert!(popped.is_empty(), "frames at the wrong scale: {popped:#?}");
}
