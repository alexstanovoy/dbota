//! The browser's keys left to the browser, the client's half: its window
//! cancels none of the page's events, so the page alone (web/keys.js, `smoke.py --keys-page`)
//! decides which keys are the game's.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::harness::Client;

/// The client's window, as `main` makes it, lets every event through to the page (Bevy's
/// default cancels them all, Ctrl+R and F12 among them).
#[test]
fn the_window_cancels_none_of_the_pages_events() {
    let mut client = Client::run(&[]);
    let world = client.world_mut();
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    assert!(!window.prevent_default_event_handling);
}
