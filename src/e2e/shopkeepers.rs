//! The map's shop keepers as Dota shows them: standing at the places the map gives them, not
//! baked into the ground's mesh in a T-pose, and playing their idle loops.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::map_props::MapProp;

/// Every entity under `root`.
fn subtree(world: &World, root: Entity) -> Vec<Entity> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        found.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

fn pose_of(world: &World, root: Entity) -> Vec<Transform> {
    subtree(world, root)
        .into_iter()
        .filter_map(|e| world.get::<Transform>(e).copied())
        .collect()
}

#[test]
fn the_maps_shop_keepers_stand_apart_from_the_ground_and_play_their_idle_loops() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let cache = std::path::PathBuf::from(std::env::var("DBOTA_ASSETS").expect("DBOTA_ASSETS"));
    let wanted = std::fs::read_to_string(cache.join("map/mid/buildings.txt"))
        .expect("the cache's buildings")
        .lines()
        .filter(|line| line.starts_with("shopkeeper"))
        .count();
    assert!(wanted >= 3, "the map places its keepers: {wanted}");
    client.within(Duration::from_secs(60), "every keeper standing", |c| {
        let world = c.world_mut();
        world.query::<&MapProp>().iter(world).count() == wanted
    });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            500,
        )));
    let props: Vec<Entity> = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<MapProp>>()
            .iter(world)
            .collect()
    };
    client.within(
        Duration::from_secs(60),
        "every keeper's loop playing",
        |c| {
            let world = c.world();
            props.iter().all(|prop| {
                subtree(world, *prop).into_iter().any(|e| {
                    world
                        .get::<AnimationPlayer>(e)
                        .is_some_and(|p| p.playing_animations().next().is_some())
                })
            })
        },
    );
    let before: Vec<_> = props.iter().map(|p| pose_of(client.world(), *p)).collect();
    client.frames(4);
    let after: Vec<_> = props.iter().map(|p| pose_of(client.world(), *p)).collect();
    for (at, (was, now)) in before.iter().zip(&after).enumerate() {
        assert!(
            was != now,
            "keeper {at} stands in the same pose two seconds on"
        );
    }
    // The baked copies are gone from the map's own file, which the ground's mesh is made of.
    let map: serde_json::Value = serde_json::from_slice(
        &std::fs::read(cache.join("map/mid/mid.gltf")).expect("the cache's map"),
    )
    .expect("the map's json");
    let baked = map["nodes"]
        .as_array()
        .expect("the map's nodes")
        .iter()
        .filter(|n| {
            n["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("shopkeeper"))
        })
        .count();
    assert_eq!(baked, 0, "no keeper left in the map's mesh");
}
