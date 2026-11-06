//! The Tormentor's chasms while no Tormentor stands in one: the shutter lies closed over the pit,
//! and it is the ground, as Dota's traces find it, flat, so a hero stands on it and an order's
//! ring lies on it (the hero sank into the pit under the shutter, the ring bent over its relief).

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::overlay::GOOD;

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);

/// The middle of the Tormentor's chasm by the Radiant's bottom lane (the map's
/// `sentinel_base_radiant`, Dota's (7744, -6208)).
const CHASM: (f32, f32) = (16960.0, 3008.0);

/// How far the heights of what lies on the shutter may differ: its plates' relief.
const FLAT: f32 = 6.0;

/// One's hero stands on the closed shutter and is ordered across it: the order's ring lies flat
/// on the shutter, and the hero's feet are on it, not under it.
#[test]
fn a_closed_chasm_is_flat_ground_a_hero_stands_on() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.within(SOON, "the map's ground known", |c| {
        c.world().resource::<crate::map::Ground>().known()
    });
    client.stand_at((CHASM.0 as i32 - 60, CHASM.1 as i32));
    client
        .core_mut()
        .act(Intent::MoveTo((CHASM.0 + 60.0, CHASM.1)));
    let good = GOOD.to_linear();
    let mut ring = Vec::new();
    client.within(SOON, "the move order's ring", |c| {
        ring = c
            .line_strips()
            .into_iter()
            .find(|(points, colour)| {
                colour.red == good.red && colour.green == good.green && points.len() > 8
            })
            .map(|(points, _)| points)
            .unwrap_or_default();
        !ring.is_empty()
    });
    let heights: Vec<f32> = ring.iter().map(|p| p.y).collect();
    let (low, high) = heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
    assert!(
        high - low < FLAT,
        "the ring lies flat: heights {low} to {high}"
    );
    // The shutter lies level with the ground about the chasm, past its stone rim.
    let around = client
        .world()
        .resource::<crate::map::Ground>()
        .at(CHASM.0, CHASM.1 + 450.0);
    let lid = (low + high) / 2.0 - crate::overlay::LIFT;
    assert!(
        (lid - around).abs() < FLAT * 2.0,
        "the ring lies on the shutter at {lid}, the ground about the chasm at {around}"
    );
    let me = client.me().id;
    client.within(SOON, "one's hero across the shutter", |c| {
        c.view()
            .unit(me)
            .is_some_and(|u| (u.pos.x.to_f32() - CHASM.0 - 60.0).abs() < 8.0)
    });
    client.frames(10);
    let body = client.body(me).expect("one's hero drawn");
    let feet = client
        .world()
        .get::<GlobalTransform>(body)
        .expect("placed")
        .translation()
        .y;
    assert!(
        (feet - lid).abs() < FLAT * 2.0,
        "the hero stands on the shutter: feet {feet}, shutter {lid}"
    );
}
