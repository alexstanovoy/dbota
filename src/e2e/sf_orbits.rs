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
    let (definition, count, ceiling) = authored_limits(&client);
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
            let ground = client.world().get::<GlobalTransform>(body).expect("hero frame").translation().y;
            let drawn = drawn_rocks(&mut client, definition);
            assert!(!drawn.is_empty(), "the persistent rocks remain submitted at {second}s");
            for position in particles.into_iter().chain(drawn) {
                assert!(position.y - ground <= ceiling + 0.1,
                    "orbit rose out of its authored shell at {second}s: height {}, ceiling {ceiling}, position {position:?}", position.y - ground);
            }
        }
    });
}

#[test]
fn the_rock_orbit_sets_its_declared_force_center_from_its_previous_bounds() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let (definition, count, _) = authored_limits(&client);
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
    let source = DirSource::new(find_game_dir(None).expect("install"));
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

fn authored_limits(client: &Client) -> (usize, usize, f32) {
    let source = DirSource::new(find_game_dir(None).expect("install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let path = "particles/units/heroes/hero_nevermore/nevermore_trail_rocks.vpcf_c";
    let bytes = pak.read_file(&source, path).expect("rock definition");
    let data = Resource::parse(&bytes)
        .expect("resource")
        .data()
        .expect("DATA")
        .expect("KV3");
    let list = |key: &str| {
        data.get(key)
            .and_then(Value::as_array)
            .expect("function list")
    };
    let class =
        |value: &Value, name: &str| value.get("_class").and_then(Value::as_str) == Some(name);
    let sphere = list("m_Constraints")
        .iter()
        .find(|v| class(v, "C_OP_ConstrainDistance"))
        .expect("sphere constraint");
    let vector_z = |value: &Value, key: &str| {
        value.get(key).and_then(Value::as_array).expect("vector")[2]
            .as_f64()
            .expect("component") as f32
    };
    let max = sphere
        .get("m_fMaxDistance")
        .and_then(Value::as_f64)
        .expect("maximum distance") as f32;
    let mut ceiling = vector_z(sphere, "m_CenterOffset") + max;
    // These position operators run after BasicMovement's constraints. Their bounded offsets
    // can move a rock above the shell again in the same frame; rotations around Z cannot.
    for operator in list("m_Operators") {
        if operator.get("m_nOpEndCapState").and_then(Value::as_str)
            == Some("PARTICLE_ENDCAP_ENDCAP_ON")
        {
            continue;
        }
        if class(operator, "C_OP_VectorNoise") {
            ceiling += vector_z(operator, "m_vecOutputMax");
        }
        if class(operator, "C_OP_OscillateVector") {
            ceiling += vector_z(operator, "m_RateMax");
        }
    }
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
    (definition, count, ceiling)
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
