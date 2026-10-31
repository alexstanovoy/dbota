//! The map's ambient effects end to end: the particle systems Dota's map places play where it
//! places them, by day or by night as it says, only near the camera and within their budget.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId};

use super::harness::{Client, MID, Server};
use crate::ambient::{Ambient, MAX_ACTIVE};

/// The emitter of an effect nearest a point (bota's units): its index and place.
fn nearest(client: &Client, effect: &str, to: Vec2) -> (usize, Vec2) {
    let ambient = client.world().resource::<Ambient>();
    ambient
        .emitters
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name == format!("map.{effect}"))
        .map(|(i, e)| (i, e.at))
        .min_by(|a, b| a.1.distance(to).total_cmp(&b.1.distance(to)))
        .unwrap_or_else(|| panic!("the map places {effect}"))
}

/// Whether the emitter at a place plays now.
fn plays(client: &Client, at: Vec2) -> bool {
    let ambient = client.world().resource::<Ambient>();
    ambient.active().any(|e| e.at == at)
}

/// The cards of this frame's particles within `reach` of a place on the ground (bota's units).
fn cards_near(client: &Client, at: Vec2, reach: f32) -> usize {
    let frame = client.world().resource::<crate::particles::ParticleFrame>();
    frame
        .quads
        .iter()
        .filter(|q| Vec2::new(q.head[0], -q.head[2]).distance(at) < reach)
        .count()
}

/// Asserts the budget: no more playing than [`MAX_ACTIVE`].
fn within_budget(client: &Client) {
    let playing = client.world().resource::<Ambient>().active().count();
    assert!(playing <= MAX_ACTIVE, "{playing} ambient effects play");
}

/// The dragonflies the map sets over the river near the middle fly by day and are drawn there,
/// with the night's fireflies off; by night the fireflies near them glow there and the
/// dragonflies are gone; the Dire's lava across the map never plays while the camera is here.
#[test]
fn the_rivers_dragonflies_fly_by_day_and_its_fireflies_by_night() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let middle = Vec2::new(MID.0 as f32, MID.1 as f32);
    let (_, flies) = nearest(&client, "dragonflies_group", middle);
    let (_, glows) = nearest(&client, "fireflies", flies);
    let (_, lava) = nearest(&client, "dire_lava_gloops_10sec", flies);
    let patience = Duration::from_secs(60);
    client.cheat(Cheat::TimeOfDay { night: false });
    client.stand_at((flies.x as i32, flies.y as i32));
    client.within(patience, "day", |c| {
        c.world().resource::<crate::daylight::Daylight>().night == 0.0
    });
    client.within(patience, "the dragonflies drawn by day", |c| {
        plays(c, flies) && cards_near(c, flies, 500.0) > 0
    });
    assert!(!plays(&client, glows), "no fireflies by day");
    assert!(!plays(&client, lava), "nothing plays across the map");
    within_budget(&client);
    client.cheat(Cheat::TimeOfDay { night: true });
    client.stand_at((glows.x as i32, glows.y as i32));
    client.within(patience, "the fireflies drawn by night", |c| {
        plays(c, glows) && cards_near(c, glows, 500.0) > 0
    });
    assert!(!plays(&client, flies), "no dragonflies by night");
    within_budget(&client);
}

/// The particle lights shining now within `reach` of a place on the ground (bota's units).
fn lights_near(client: &mut Client, at: Vec2, reach: f32) -> usize {
    let world = client.world_mut();
    world
        .query::<(&PointLight, &GlobalTransform)>()
        .iter(world)
        .filter(|(light, place)| {
            let p = place.translation();
            light.intensity != 0.0 && Vec2::new(p.x, -p.z).distance(at) < reach
        })
        .count()
}

/// A lamp's flame lights the ground about it with one light: the few lights it keeps flickering
/// a few tens of units apart, each reaching hundreds, shine as one, as every light costs each
/// pixel it reaches.
#[test]
fn a_lamps_flame_lights_the_ground_with_one_light() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let middle = Vec2::new(MID.0 as f32, MID.1 as f32);
    let (_, lamp) = nearest(&client, "lamp_flame_tintable", middle);
    client.stand_at((lamp.x as i32 + 200, lamp.y as i32));
    client.within(Duration::from_secs(60), "the lamp's flame lit", |c| {
        plays(c, lamp) && lights_near(c, lamp, 100.0) > 0
    });
    for _ in 0..60 {
        client.frames(1);
        let lights = lights_near(&mut client, lamp, 100.0);
        assert_eq!(lights, 1, "the lamp's flame shines as {lights} lights");
    }
    within_budget(&client);
}
