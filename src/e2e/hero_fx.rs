//! A hero's default ambient effects as Dota shows them (its "Ambient Effects" item): Shadow
//! Fiend wears his glow and his trail from when he stands on the map for as long as he stands,
//! past the half minute a played effect lasts at most.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::particles::{Library, ParticleFrame, Playing};

/// Seconds of the client's clock a frame of this test moves.
const FRAME: f32 = 1.0 / 30.0;

/// How many times an effect of the cache was started.
fn started(client: &Client, name: &str) -> u32 {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect(name)
        .unwrap_or_else(|| panic!("{name} in the asset cache"));
    world.resource::<Playing>().started(effect)
}

/// The cards of this frame's particles within `reach` world units of one's hero, on the ground
/// plane.
fn cards_near_me(client: &mut Client, reach: f32) -> usize {
    let body = client.body(client.me().id).expect("one's hero drawn");
    let at = client
        .world()
        .get::<GlobalTransform>(body)
        .expect("placed")
        .translation()
        .xz();
    let frame = client.world().resource::<ParticleFrame>();
    frame
        .quads
        .iter()
        .filter(|q| Vec2::new(q.head[0], q.head[2]).distance(at) < reach)
        .count()
}

/// Shadow Fiend stands at his fountain wearing his ambient glow and trail, drawn about him, and
/// still wears them, the same ones, forty seconds on.
#[test]
fn shadow_fiend_wears_his_ambient_effects_as_long_as_he_stands() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )));
    let worn = ["hero.nevermore.ambient_glow", "hero.nevermore.trail"];
    client.until("his ambient effects started", |c| {
        worn.iter().all(|name| started(c, name) == 1)
    });
    client.until("his ambient effects drawn about him", |c| {
        cards_near_me(c, 200.0) > 0
    });
    client.frames((40.0 / FRAME) as usize);
    assert!(cards_near_me(&mut client, 200.0) > 0, "still drawn");
    for name in worn {
        assert_eq!(started(&client, name), 1, "{name}: the same, never ended");
    }
}
