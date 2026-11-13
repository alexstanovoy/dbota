//! The Shrines of Wisdom and the Lotus Pools as Dota shows them: neither is picked by the
//! mouse (untargetable, part of the map), a hero walks onto a Shrine and stands in it, a ready
//! Shrine lights its ring on the ground and fills it as a side gathers, and a Lotus Pool plays its
//! ring out from the middle as a hero plucks.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, EffectId, EntityId, HeroId, UnitKind, Vec2 as Spot};

use super::harness::{Client, Server};
use crate::particles::{Library, Playing};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);
/// Patience for what the game clock brings on: minutes, at twenty times Dota's pace.
const LATER: Duration = Duration::from_secs(150);

/// A match of one seat with the objective named, its clock at twenty times Dota's pace (on the
/// wall clock, a `mode` of `realtime`, however fast the client takes it in), one's Shadow Fiend
/// on the map and the game begun.
fn objective_match(objective: &str, mode: &str) -> (Server, Client) {
    let server = Server::start(
        1,
        &[
            "--map",
            "0",
            "--objectives",
            objective,
            "--tick-rate",
            "600",
            "--mode",
            mode,
        ],
    );
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    (server, client)
}

/// The unit of a kind standing at a place of the map.
fn unit_at(client: &Client, kind: UnitKind, at: Spot) -> EntityId {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == kind && u.pos == at)
        .expect("it stands in the view")
        .id
}

/// What the mouse picks with the pointer on a unit's middle, and on its body above it, and
/// what a left click there selects.
fn picked_at(client: &mut Client, at: Spot) -> Vec<Option<EntityId>> {
    let (x, y) = (at.x.to_f32(), at.y.to_f32());
    let mut picked = Vec::new();
    for lift in [0.0, 120.0] {
        let ground = client.world().resource::<crate::map::Ground>().at(x, y);
        let world = client.world_mut();
        let (camera, place) = world
            .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>()
            .single(world)
            .expect("the game's camera");
        let on = camera
            .world_to_viewport(place, Vec3::new(x, ground + lift, -y))
            .expect("on screen");
        client.mouse(on);
        let hover = *client.world().resource::<crate::pick::Hover>();
        picked.extend([hover.select, hover.attack].map(|hit| hit.map(|(id, _)| id)));
        client.click(MouseButton::Left);
        picked.push(client.core().selected);
    }
    picked
}

/// The instances of an effect, those ended (playing out) too or not: each one's control point
/// `cp`, as it was last fed.
fn instances(client: &Client, effect: &str, cp: usize, ended_too: bool) -> Vec<Vec3> {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect(effect)
        .unwrap_or_else(|| panic!("{effect} in the asset cache"));
    world
        .resource::<Playing>()
        .instances_of(effect)
        .filter(|instance| ended_too || !instance.ending())
        .map(|instance| instance.nodes[0].control_points[cp].position)
        .collect()
}

/// The playing instances of an effect, not ended: each one's control point `cp`.
fn playing(client: &Client, effect: &str, cp: usize) -> Vec<Vec3> {
    instances(client, effect, cp, false)
}

/// Ground-facing, nontransparent geometry submitted for this effect at the objective.
fn ground_ring(client: &Client, logical: &str, at: Spot) -> bool {
    let world = client.world();
    let library = world.resource::<Library>();
    let effect = library.effect(logical).expect("ring effect imported");
    let looks: Vec<_> = effect
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .collect();
    let frame = world.resource::<crate::particles::ParticleFrame>();
    let ground = world.resource::<crate::map::Ground>();
    frame
        .batches
        .iter()
        .filter(|batch| !batch.stage && looks.contains(&batch.look))
        .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
        .any(|card| {
            let head = Vec3::from(card.head);
            let tail = Vec3::from(card.tail);
            let middle = (head + tail) * 0.5;
            card.head_color[3] > 0.01
                && card.tail_color[3] > 0.01
                && Vec2::new(middle.x - at.x.to_f32(), -middle.z - at.y.to_f32()).length() < 450.0
                && middle.y >= ground.surface_at(middle.x, -middle.z) - 0.5
                && middle.y - ground.surface_at(middle.x, -middle.z) < 30.0
                && (head.y - tail.y).abs() < 2.0
                && Vec3::from(card.head_side).length() + head.distance(tail) > 8.0
        })
}

/// Keep the witnessed gameplay state while its real particle simulation produces a frame.
fn assert_ground_ring(server: &Server, client: &mut Client, logical: &str, at: Spot) {
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f64(1.0 / 30.0),
            ));
        let mut drawn = false;
        for _ in 0..30 {
            client.frames(1);
            drawn |= ground_ring(client, logical, at);
        }
        assert!(
            drawn,
            "{logical} must submit visible geometry on the drawn ground"
        );
    });
}

/// A Shrine of Wisdom: the mouse picks nothing on it; a hero sent to its middle stands there;
/// once ready it lights its ambient effect; while gathering the ground ring fills
/// (control point 1 feeds the authored countdown remap) when the hero comes back to gather it.
#[test]
fn a_shrine_is_walked_onto_never_picked_and_shows_its_ring_filling() {
    let (server, mut client) = objective_match("shrines", "realtime");
    let at = bota_core::DOTA_SHRINES[0];
    client.stand_at((at.x.to_int() - 400, at.y.to_int()));
    let shrine = unit_at(&client, UnitKind::Shrine, at);
    let me = client.me().id;
    assert!(
        picked_at(&mut client, at)
            .iter()
            .all(|picked| *picked != Some(shrine)),
        "the shrine is never picked"
    );
    client
        .core_mut()
        .act(Intent::MoveTo((at.x.to_f32(), at.y.to_f32())));
    client.within(SOON, "one's hero in the shrine's middle", |c| {
        c.view()
            .unit(me)
            .is_some_and(|u| u.pos.within(at, bota_core::rules::units(24)))
    });
    // Out of its reach while it readies, so that nobody gathers it before it is seen ready.
    client.stand_at((at.x.to_int() - 400, at.y.to_int()));
    client.within(LATER, "the shrine ready, its ring lit", |c| {
        c.view()
            .unit(shrine)
            .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId::SHRINE_READY))
            && !playing(c, "shrine.ambient", 1).is_empty()
    });
    assert!(
        playing(&client, "shrine.active", 1).is_empty(),
        "the countdown starts only during gathering"
    );
    client
        .core_mut()
        .act(Intent::MoveTo((at.x.to_f32(), at.y.to_f32())));
    // At twenty times Dota's pace the three seconds pass in a few frames, and the ring ends with
    // the charge it shows: it keeps what it was fed last.
    client.within(SOON, "the ring filled as one's hero gathers", |c| {
        instances(c, "shrine.active", 4, true)
            .iter()
            .any(|share| share.y > 0.0)
    });
    assert_ground_ring(&server, &mut client, "shrine.active", at);
    // The ring's outer radius is the one the game feeds the countdown, and stays fixed while its
    // share grows (a halved radius would still draw a ring, only a smaller one).
    let ring = instances(&client, "shrine.active", 1, true);
    assert!(!ring.is_empty(), "the countdown plays");
    assert!(ring.iter().all(|cp| cp.x == 300.0), "{ring:?}");
}

/// A Lotus Pool: the mouse picks nothing on it, and a hero standing in it plays the ring that
/// runs out from the middle (control point 1's y grows while its x keeps the outer radius).
#[test]
fn a_lotus_pool_is_never_picked_and_plays_its_ring_while_a_hero_plucks() {
    let (server, mut client) = objective_match("lotus_pools", "lockstep");
    let at = bota_core::DOTA_LOTUS_POOLS[0];
    client.stand_at((at.x.to_int() - 500, at.y.to_int()));
    let pool = unit_at(&client, UnitKind::LotusPool, at);
    assert!(
        picked_at(&mut client, at)
            .iter()
            .all(|picked| *picked != Some(pool)),
        "the pool is never picked"
    );
    client.within(LATER, "the pool's first lotus", |client| {
        client
            .view()
            .pools
            .iter()
            .any(|held| held.id == pool && held.lotuses > 0)
    });
    client.stand_at((at.x.to_int() - 200, at.y.to_int()));
    let mut radii = Vec::new();
    client.within(SOON, "the ring running out from the middle", |c| {
        if let Some(ring) = playing(c, "lotus_pool.gathering", 1).first() {
            assert_eq!(ring.x, 350.0, "the outer radius stays fixed");
            radii.push(ring.y);
        }
        radii.windows(2).any(|w| w[1] > w[0] && w[0] > 0.0)
    });
    assert_ground_ring(&server, &mut client, "lotus_pool.gathering", at);
}
