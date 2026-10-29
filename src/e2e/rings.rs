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
