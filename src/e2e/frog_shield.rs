//! A real Marshmage cast must produce the bubble's imported sphere around its recipient.
use bevy::{gltf::Gltf, prelude::*, world_serialization::WorldAssetRoot};
use bota_client_core::Intent;
use bota_proto::{Cheat, EffectId, EntityId, HeroId, ModifierSpec};

use super::harness::{Client, Server};

#[test]
fn a_marshmages_water_bubble_draws_its_shield_around_the_hurt_ally() {
    with_bubble(assert_drawn_bubbles);
}

#[test]
#[should_panic(expected = "drawn radius")]
fn the_shield_radius_guard_rejects_a_hundredfold_error_in_the_drawn_scale() {
    with_bubble(|client, frog| {
        // Deliberately corrupt the submitted model frame, without changing its CP1.
        // This is a negative control of the same quantitative guard used by the real cast.
        let sphere = visible_bubbles(client)[0];
        {
            let mut transform = client
                .world_mut()
                .get_mut::<GlobalTransform>(sphere)
                .expect("sphere frame");
            *transform = transform.mul_transform(Transform::from_scale(Vec3::splat(0.01)));
        }
        assert_drawn_bubbles(client, frog);
    });
}

#[test]
fn a_bubble_keeps_its_last_radius_while_the_recipient_scene_is_rebuilt() {
    with_bubble(|client, frog| {
        let before = bubble_radius(client);
        let scene = recipient_scene(client, frog);
        let radius = client
            .world_mut()
            .entity_mut(scene)
            .take::<crate::units::RenderRadius>()
            .expect("radius");
        client.frames(3);
        assert_eq!(
            bubble_radius(client),
            before,
            "a temporarily unavailable radius is not zero"
        );
        client.world_mut().entity_mut(scene).insert(radius);
    });
}

#[test]
fn a_bubble_with_unavailable_bounds_uses_a_visible_selection_volume_fallback() {
    with_bubble(|client, frog| {
        let _live = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(super::creep_walk::ControlledFrames)),
        );
        let original = client.view().unit(frog).expect("frog").clone();
        let mut bare = original.clone();
        bare.effects.retain(|e| e.id != EffectId(54));
        super::creep_walk::tell(client, bare);
        client.frames(120);
        let scene = recipient_scene(client, frog);
        client
            .world_mut()
            .entity_mut(scene)
            .insert(crate::units::RenderRadius::default());
        let effect = client
            .world()
            .resource::<crate::particles::Library>()
            .effect("neutral.water_bubble")
            .expect("bubble")
            .clone();
        let before = client
            .world()
            .resource::<crate::particles::Playing>()
            .started(&effect);
        super::creep_walk::tell(client, original);
        client.frames(30);
        assert_eq!(
            client
                .world()
                .resource::<crate::particles::Playing>()
                .started(&effect),
            before + 1,
            "missing imported bounds must not silently prevent the effect from starting"
        );
        // Froglet's fallback box is 48 x 48 x 90 units: its half-diagonal is sqrt(3177).
        const EXPECTED: f32 = 56.364_883;
        assert!(
            (bubble_radius(client) - EXPECTED).abs() < 0.001,
            "missing metadata uses the Froglet selection box's half-diagonal {EXPECTED}"
        );
        assert_eq!(visible_bubbles(client).len(), 2);
    });
}

fn recipient_scene(client: &Client, frog: EntityId) -> Entity {
    let world = client.world();
    let body = client.body(frog).expect("recipient");
    let pivot = world.get::<crate::units::ModelOf>(body).expect("pivot").0;
    world
        .get::<Children>(pivot)
        .expect("children")
        .iter()
        .find(|e| world.get::<crate::units::RenderRadius>(*e).is_some())
        .expect("body scene")
}

fn bubble_radius(client: &Client) -> f32 {
    let world = client.world();
    let effect = world
        .resource::<crate::particles::Library>()
        .effect("neutral.water_bubble")
        .expect("bubble");
    world
        .resource::<crate::particles::Playing>()
        .instances_of(effect)
        .find(|i| i.nodes[0].parts.len() == 2)
        .expect("active spheres")
        .nodes[0]
        .control_points[1]
        .position
        .x
}

fn with_bubble(check: impl FnOnce(&mut Client, EntityId)) {
    let server = Server::start(1, &["--map", "0", "--seed", "5"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::SpawnNeutrals);
    let camp = bota_core::CAMPS
        .iter()
        .find(|c| c.flooded && c.kind == bota_core::CampKind::Medium)
        .expect("froglet camp");
    client.stand_at((camp.pos.x.to_f32() as i32 + 250, camp.pos.y.to_f32() as i32));
    let frog = hurt_frog(&mut client);
    client.until("the Marshmage's real Water Bubble", |c| {
        c.view()
            .unit(frog)
            .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId(54)))
    });
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f32(1.0 / 60.0),
            ));
        client.until("the recipient model ready", |c| {
            c.body(frog).is_some_and(|body| {
                c.world()
                    .get::<crate::units::Attachments>(body)
                    .is_some_and(|a| a.hitloc.is_some())
            })
        });
        client.frames(30);
        check(&mut client, frog);
    });
}

fn assert_drawn_bubbles(client: &mut Client, frog: EntityId) {
    assert_bubble_binding(client, frog);
    let expected = bubble_radius(client);
    let model_radius = sphere_radius_metres(client);
    let spheres = visible_bubbles(client);
    assert_eq!(
        spheres.len(),
        2,
        "both authored bubble spheres are submitted for drawing"
    );
    for sphere in spheres {
        let transform = client
            .world()
            .get::<GlobalTransform>(sphere)
            .expect("sphere frame");
        let drawn_radius = model_radius * transform.to_scale_rotation_translation().0.max_element();
        // The nominal 8-unit sphere's packed vertices stop slightly short of its axes.
        // Allow 2% for that tessellation and float conversion, never arbitrary undersizing.
        assert!(
            (drawn_radius / expected - 1.0).abs() <= 0.02,
            "drawn radius {drawn_radius} must match CP1 radius {expected} within 2%"
        );
        let body = client.body(frog).expect("recipient drawn");
        let hit = client
            .world()
            .get::<crate::units::Attachments>(body)
            .expect("attachments")
            .hitloc
            .expect("hit location");
        let center = client
            .world()
            .get::<GlobalTransform>(hit)
            .expect("attachment frame")
            .translation();
        assert!(
            transform.translation().distance(center) < 0.1,
            "the bubble surrounds the drawn recipient's hit location"
        );
    }
}

fn sphere_radius_metres(client: &Client) -> f32 {
    let world = client.world();
    let model = world
        .resource::<crate::particles::Library>()
        .models
        .iter()
        .find(|m| m.file.ends_with("/sphere.vmdl.skin5.glb"))
        .expect("sphere skin");
    let glb = std::fs::read(world.resource::<crate::Args>().cache_dir.join(&model.file))
        .expect("sphere GLB");
    let length = u32::from_le_bytes(glb[12..16].try_into().expect("JSON length")) as usize;
    let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).expect("JSON");
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for mesh in json["meshes"].as_array().expect("meshes") {
        for p in mesh["primitives"].as_array().expect("primitives") {
            let a = &json["accessors"]
                [p["attributes"]["POSITION"].as_u64().expect("position") as usize];
            let point = |key: &str| {
                Vec3::from_array(std::array::from_fn(|i| {
                    a[key][i].as_f64().expect("extreme") as f32
                }))
            };
            low = low.min(point("min"));
            high = high.max(point("max"));
        }
    }
    (high - low).max_element() * 0.5
}

fn assert_bubble_binding(client: &Client, frog: EntityId) {
    let world = client.world();
    let body = client.body(frog).expect("recipient drawn");
    let radius = imported_radius(client, body);
    let pivot = world.get::<crate::units::ModelOf>(body).expect("pivot").0;
    let scene = world
        .get::<Children>(pivot)
        .expect("scene children")
        .iter()
        .find(|e| world.get::<crate::units::RenderRadius>(*e).is_some())
        .expect("body scene");
    let scale = world
        .get::<Transform>(scene)
        .expect("drawn scale")
        .scale
        .abs()
        .max_element();
    let expected = radius * scale;
    let stored = world
        .get::<crate::units::RenderRadius>(scene)
        .expect("imported radius")
        .0
        .expect("valid imported bounds");
    assert!(
        (stored - expected).abs() < 0.01,
        "imported model radius {stored}, accessor reference {expected}"
    );
    let effect = world
        .resource::<crate::particles::Library>()
        .effect("neutral.water_bubble")
        .expect("bubble");
    let instance = world
        .resource::<crate::particles::Playing>()
        .instances_of(effect)
        .next()
        .expect("playing");
    assert!(
        (instance.nodes[0].control_points[1].position.x - expected).abs() < 0.01,
        "CP1 is the recipient's imported render radius {expected}"
    );
}

fn imported_radius(client: &Client, body: Entity) -> f32 {
    let world = client.world();
    let model = world
        .get::<crate::units::Unit>(body)
        .expect("unit")
        .model
        .expect("model");
    let model = &world.resource::<crate::units::ModelBank>().0[model].model;
    let path = model.path.split_once("://").expect("cache path").1;
    let glb = std::fs::read(world.resource::<crate::Args>().cache_dir.join(path))
        .expect("imported model");
    let length = u32::from_le_bytes(glb[12..16].try_into().expect("JSON length")) as usize;
    let gltf: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).expect("glTF");
    // Accessor extrema are written by the exporter, independently of the client's bounds reader.
    let (mut low, mut high) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for mesh in gltf["meshes"].as_array().expect("meshes") {
        for primitive in mesh["primitives"].as_array().expect("primitives") {
            let index = primitive["attributes"]["POSITION"]
                .as_u64()
                .expect("positions") as usize;
            let accessor = &gltf["accessors"][index];
            let point = |key: &str| {
                Vec3::from_array(std::array::from_fn(|i| {
                    accessor[key][i].as_f64().expect("bound") as f32
                }))
            };
            low = low.min(point("min"));
            high = high.max(point("max"));
        }
    }
    (high - low).length() * 0.5
}

fn hurt_frog(client: &mut Client) -> EntityId {
    let frog_kind = bota_core::NeutralKind::Froglet.id();
    client.until("the flooded camp drawn", |c| {
        c.view().units.iter().any(|u| u.neutral == Some(frog_kind))
    });
    let frog = client
        .view()
        .units
        .iter()
        .find(|u| u.neutral == Some(frog_kind))
        .expect("froglet");
    let (id, at) = (frog.id, frog.pos);
    client.cheat(Cheat::ApplyModifier {
        target: bota_proto::Target::None,
        spec: ModifierSpec {
            physical_damage: 20_000,
            ..ModifierSpec::NOMINAL
        },
        ticks: 3000,
    });
    client.core_mut().act(Intent::AttackAt {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(id),
    });
    client.until("the frog below half health", |c| {
        let frog = c.view().unit(id).expect("the hurt frog survives");
        frog.hp * 2 < frog.max_hp
    });
    client.core_mut().act(Intent::Stop);
    id
}

fn visible_bubbles(client: &mut Client) -> Vec<Entity> {
    let model = client
        .world()
        .resource::<crate::particles::Library>()
        .models
        .iter()
        .find(|m| m.file.ends_with("/sphere.vmdl.skin5.glb"))
        .expect("bubble's selected skin")
        .gltf
        .clone();
    let world = client.world_mut();
    let scene = world
        .resource::<Assets<Gltf>>()
        .get(&model)
        .and_then(|g| g.scenes.first())
        .expect("bubble scene")
        .clone();
    let roots: Vec<_> = world
        .query::<(
            Entity,
            &WorldAssetRoot,
            &Visibility,
            Option<&crate::warmup::Warmup>,
        )>()
        .iter(world)
        .filter(|(_, asset, visibility, warmup)| {
            asset.0 == scene && **visibility != Visibility::Hidden && warmup.is_none()
        })
        .map(|(entity, ..)| entity)
        .collect();
    roots
        .into_iter()
        .filter(|root| {
            super::harness::subtree(world, *root).iter().any(|part| {
                world.get::<Mesh3d>(*part).is_some()
                    && world
                        .get::<InheritedVisibility>(*part)
                        .is_some_and(|v| v.get())
            })
        })
        .collect()
}
