//! Shadow Fiend's persistent rocks orbit nearby instead of climbing out of the scene.
use std::time::Duration;

use bevy::{gltf::Gltf, prelude::*, time::TimeUpdateStrategy, world_serialization::WorldAssetRoot};
use bota_proto::HeroId;
use heatel::{DirSource, Resource, Vpk, find_game_dir, kv3::Value};

use super::harness::{Client, Server, subtree};
use crate::particles::{Library, Playing};

#[test]
fn shadow_fiends_persistent_rocks_remain_near_him_for_the_whole_orbit() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 30.0,
        )));
    let (definition, count) = authored_limits(&client);
    let (offset, radius) = authored_sphere();
    client.until("the trail's persistent rocks", |c| {
        rock_positions(c, definition).len() == count
    });
    client.until("a persistent rock's actual mesh drawn", |c| {
        !drawn_rocks(c, definition).is_empty()
    });
    server.frozen(|| {
        for second in 1..=40 {
            client.frames(30);
            let particles = rock_positions(&client, definition);
            assert_eq!(particles.len(), count, "the same authored burst survives at {second}s");
            let body = client.body(client.me().id).expect("hero drawn");
            let hero = client.world().get::<GlobalTransform>(body).expect("hero frame").translation();
            let center = hero + offset;
            let drawn = drawn_rocks(&mut client, definition);
            assert!(!drawn.is_empty(), "the persistent rocks remain submitted at {second}s");
            for position in particles.into_iter().chain(drawn) {
                // Position operators after the constraint can add a small displacement.
                // Check every direction, including submitted mesh origins, with a 25-unit margin.
                assert!(position.distance(center) <= radius + 25.0,
                    "orbit left its authored sphere at {second}s: distance {}, radius {radius}, position {position:?}", position.distance(center));
            }
        }
    });
}

#[test]
fn the_rock_orbit_sets_its_declared_force_center_from_its_previous_bounds() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let (definition, count) = authored_limits(&client);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 30.0,
        )));
    client.until("the persistent rock burst", |c| {
        rock_positions(c, definition).len() == count
    });
    let data = authored_source();
    let vector = |key: &str, default: f32| {
        data.get(key)
            .and_then(Value::as_array)
            .map_or(Vec3::splat(default), |v| {
                Vec3::from_array(std::array::from_fn(|i| {
                    v[i].as_f64().expect("bound") as f32
                }))
            })
    };
    let (padding_low, padding_high) = (
        vector("m_BoundingBoxMin", -10.0),
        vector("m_BoundingBoxMax", 10.0),
    );
    server.frozen(|| {
        client.frames(3);
        for _ in 0..20 {
            let world = client.world();
            let effect = world
                .resource::<Library>()
                .effect("hero.nevermore.trail")
                .expect("trail");
            let node = &world
                .resource::<Playing>()
                .instances_of(effect)
                .next()
                .expect("playing")
                .nodes
                .iter()
                .find(|n| n.def == definition)
                .expect("rock node");
            let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
            for (i, position) in node.parts.vectors[0].iter().enumerate() {
                let radius =
                    Vec3::splat(node.parts.scalar_field(heatel::particle::Field::RADIUS, i));
                low = low.min(*position - radius);
                high = high.max(*position + radius);
            }
            let expected = (low + padding_low + high + padding_high) * 0.5;
            client.frames(1);
            let world = client.world();
            let effect = world
                .resource::<Library>()
                .effect("hero.nevermore.trail")
                .expect("trail");
            let node = world
                .resource::<Playing>()
                .instances_of(effect)
                .next()
                .expect("playing")
                .nodes
                .iter()
                .find(|n| n.def == definition)
                .expect("rock node");
            assert!(
                node.control_points[2].position.distance(expected) < 0.01,
                "the asset sets CP2 to its last bounds' center {expected:?}, got {:?}",
                node.control_points[2].position
            );
        }
    });
}

fn authored_source() -> Value {
    let provided = std::env::var_os("DBOTA_DOTA_PATH")
        .or_else(|| std::env::var_os("HEATEL_DOTA_PATH"))
        .map(std::path::PathBuf::from);
    let source =
        DirSource::new(find_game_dir(provided.as_deref()).expect("explicit local install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let bytes = pak
        .read_file(
            &source,
            "particles/units/heroes/hero_nevermore/nevermore_trail_rocks.vpcf_c",
        )
        .expect("rocks");
    Resource::parse(&bytes)
        .expect("resource")
        .data()
        .expect("DATA")
        .expect("KV3")
}

fn authored_sphere() -> (Vec3, f32) {
    let data = authored_source();
    let sphere = data
        .get("m_Constraints")
        .and_then(Value::as_array)
        .expect("constraints")
        .iter()
        .find(|value| value.get("_class").and_then(Value::as_str) == Some("C_OP_ConstrainDistance"))
        .expect("sphere constraint");
    let offset = sphere
        .get("m_CenterOffset")
        .and_then(Value::as_array)
        .expect("offset");
    let component = |i: usize| offset[i].as_f64().expect("offset component") as f32;
    let radius = sphere
        .get("m_fMaxDistance")
        .and_then(Value::as_f64)
        .expect("outer radius") as f32;
    (Vec3::new(component(0), component(2), -component(1)), radius)
}

fn authored_limits(client: &Client) -> (usize, usize) {
    let data = authored_source();
    let list = |key: &str| {
        data.get(key)
            .and_then(Value::as_array)
            .expect("function list")
    };
    let count = list("m_Emitters")[0]
        .get("m_nParticlesToEmit")
        .and_then(|v| v.get("m_flLiteralValue"))
        .and_then(Value::as_f64)
        .expect("burst count") as usize;
    let path = client
        .world()
        .resource::<crate::Args>()
        .cache_dir
        .join("particles/nevermore_trail.json");
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("cached trail")).expect("JSON");
    let definition = cache["systems"]
        .as_array()
        .expect("systems")
        .iter()
        .position(|system| {
            system["path"]
                .as_str()
                .is_some_and(|p| p.ends_with("/nevermore_trail_rocks.vpcf"))
        })
        .expect("the same source system in the effect");
    (definition, count)
}

fn rock_positions(client: &Client, definition: usize) -> Vec<Vec3> {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect("hero.nevermore.trail")
        .expect("trail");
    let Some(instance) = world.resource::<Playing>().instances_of(effect).next() else {
        return Vec::new();
    };
    let node = instance
        .nodes
        .iter()
        .find(|n| n.def == definition)
        .expect("rock system");
    node.parts.vectors[0]
        .iter()
        .map(|p| Vec3::new(p.x, p.z, -p.y))
        .collect()
}

fn drawn_rocks(client: &mut Client, definition: usize) -> Vec<Vec3> {
    let particles = rock_positions(client, definition);
    let model = client
        .world()
        .resource::<Library>()
        .models
        .iter()
        .find(|m| m.file.ends_with("/fx_rocks.vmdl.skin0.glb"))
        .expect("authored rock model")
        .gltf
        .clone();
    let world = client.world_mut();
    let Some(scene) = world
        .resource::<Assets<Gltf>>()
        .get(&model)
        .and_then(|g| g.scenes.first())
        .cloned()
    else {
        return Vec::new();
    };
    let roots: Vec<_> = world
        .query::<(
            Entity,
            &WorldAssetRoot,
            &Visibility,
            &GlobalTransform,
            Option<&crate::warmup::Warmup>,
        )>()
        .iter(world)
        .filter(|(_, asset, visibility, _, warmup)| {
            asset.0 == scene && **visibility != Visibility::Hidden && warmup.is_none()
        })
        .filter(|(_, _, _, transform, _)| {
            particles
                .iter()
                .any(|p| p.distance(transform.translation()) < 0.01)
        })
        .map(|(root, _, _, transform, _)| (root, transform.translation()))
        .collect();
    roots
        .into_iter()
        .filter(|(root, _)| {
            subtree(world, *root).iter().any(|e| {
                world.get::<Mesh3d>(*e).is_some()
                    && world
                        .get::<InheritedVisibility>(*e)
                        .is_some_and(|v| v.get())
            })
        })
        .map(|(_, at)| at)
        .collect()
}
