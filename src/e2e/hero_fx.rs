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

/// How long Shadow Fiend's trail has stepped (its first system's age), whether it sleeps, and
/// after how long out of view its definition says it does.
fn trail(client: &Client) -> Option<(f32, bool, f32)> {
    let world = client.world();
    let effect = world.resource::<Library>().effect("hero.nevermore.trail")?;
    let instance = world.resource::<Playing>().instances_of(effect).next()?;
    let sleep_after = instance.effect.defs[0].system.sleep_after;
    Some((instance.nodes[0].age, instance.asleep(), sleep_after))
}

/// Shadow Fiend's trail, out of every view for longer than its definition's sleep time (the
/// schema's 8 s, as Dota's file leaves it), stops being simulated, as Source puts a system no
/// view draws to sleep; seen again, it steps on at once and is drawn about him.
#[test]
fn an_unseen_trail_sleeps_and_wakes_when_seen() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )));
    client.until("his trail drawn about him", |c| {
        trail(c).is_some() && cards_near_me(c, 200.0) > 0
    });
    let (age, asleep, sleep_after) = trail(&client).expect("his trail");
    assert!(!asleep, "seen, it is awake");
    assert_eq!(
        sleep_after,
        heatel::particle::SLEEP_AFTER,
        "the trail's file leaves it unsaid"
    );
    // The camera off to the far corner of the map, away from him.
    client.core_mut().pinned = None;
    {
        let world = client.world_mut();
        let mut camera = world
            .query::<&mut crate::camera::RtsCamera>()
            .single_mut(world)
            .expect("the game's camera");
        camera.focus = Vec3::new(16_000.0, 0.0, -16_000.0);
    }
    client.frames(((sleep_after - 1.0) / FRAME) as usize);
    let (later, asleep, _) = trail(&client).expect("his trail");
    assert!(
        !asleep && later > age,
        "unseen for less than its sleep time, it steps on"
    );
    client.frames((2.0 / FRAME) as usize);
    let (slept, asleep, _) = trail(&client).expect("his trail");
    assert!(asleep, "unseen past its sleep time, it sleeps");
    client.frames(30);
    let (still, asleep, _) = trail(&client).expect("his trail");
    assert!(
        asleep && still == slept,
        "asleep, it is not stepped: {slept} then {still}"
    );
    client.follow_me();
    client.frames(3);
    let (woke, asleep, _) = trail(&client).expect("his trail");
    assert!(!asleep && woke > still, "seen again, it steps on");
    client.until("his trail drawn about him again", |c| {
        cards_near_me(c, 200.0) > 0
    });
}
