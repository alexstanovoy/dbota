//! In-place locomotion keeps hero heading and the default cosmetics on the animated skeleton.
use super::{
    creep_walk::{ControlledFrames, authored_walk, check_sample, root_joints, tell},
    harness::{Client, Server, pose_of, subtree},
};
use bevy::{mesh::skinning::SkinnedMesh, prelude::*, time::TimeUpdateStrategy};
use bota_proto::{Angle, HeroId, Vec2 as Spot};
use heatel::{DirSource, Vpk};
use std::{collections::HashMap, time::Duration};

#[test]
fn both_heroes_walk_without_trajectory_or_heading_changes_and_keep_their_wearables() {
    let source = DirSource::new(super::harness::game_dir());
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let mut failures = Vec::new();
    for hero in [HeroId::SHADOW_FIEND, HeroId::PUDGE] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, hero, &[]);
        client.follow_me();
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 60.0,
            )));
        client.until(
            "hero body, animation and default wearables bound",
            |client| {
                let Some(body) = client.body(client.me().id) else {
                    return false;
                };
                client.world().get::<crate::units::Rig>(body).is_some()
                    && wearables_ready(client, body)
            },
        );
        server.frozen(|| {
            let _live = std::mem::replace(
                &mut client.core_mut().source,
                bota_client_core::Source::Live(Box::new(ControlledFrames)),
            );
            walk(&mut client, &source, &pak, &mut failures);
        });
    }
    assert!(failures.is_empty(), "hero locomotion: {failures:#?}");
}

fn walk(client: &mut Client, source: &DirSource, pak: &Vpk, failures: &mut Vec<String>) {
    let mut unit = client.me().clone();
    let origin = unit.pos;
    let speed = unit.move_speed.to_int();
    unit.facing = Angle { brads: 0 };
    let body = client.body(unit.id).expect("body");
    let roots = root_joints(client, body);
    let (activity_error, bounds) = authored_walk(client, body, source, pak);
    assert!(activity_error.is_none(), "{activity_error:?}");
    let mut poses = Vec::new();
    let mut positions = Vec::new();
    for step in 1..=36 {
        unit.pos = origin + Spot::from_ints(speed * step / 30, 0);
        tell(client, unit.clone());
        client.frames(2);
        if [12, 24, 36].contains(&step) {
            check_sample(
                client,
                body,
                &roots,
                &bounds,
                &format!("hero {:?}", unit.hero),
                failures,
            );
            let pivot = client
                .world()
                .get::<crate::units::ModelOf>(body)
                .expect("pivot")
                .0;
            let rotation = client
                .world()
                .get::<Transform>(pivot)
                .expect("model transform")
                .rotation;
            assert!(
                rotation
                    .dot(crate::coords::sim_facing_to_rotation(0.0))
                    .abs()
                    > 0.9999,
                "exported motion must not change hero heading: {rotation:?}"
            );
            assert_wearables(client, body);
            poses.push(
                pose_of(client, body)
                    .into_iter()
                    .filter(|(id, _)| *id != body)
                    .collect::<Vec<_>>(),
            );
            positions.push(
                client
                    .world()
                    .get::<GlobalTransform>(body)
                    .expect("placed")
                    .translation(),
            );
        }
    }
    assert!(positions.windows(2).all(|pair| pair[1].x > pair[0].x + 1.0));
    assert!(
        positions
            .iter()
            .all(|at| (at.z + origin.y.to_f32()).abs() < 0.01),
        "no lateral body slide"
    );
    assert!(
        poses
            .windows(2)
            .all(|pair| pair[0].iter().any(|(id, a)| pair[1].iter().any(
                |(other, b)| id == other && a.rotation.angle_between(b.rotation).abs() > 1e-4
            ))),
        "the hero's animated skeleton keeps moving"
    );
    eprintln!(
        "hero {:?}: checked heading, root poses, moving joints and bound wearables",
        unit.hero
    );
}

fn wearables_ready(client: &Client, body: Entity) -> bool {
    let mut count = 0;
    for entity in subtree(client.world(), body) {
        if let Some(wearable) = client.world().get::<crate::units::Wearable>(entity) {
            count += 1;
            if !wearable.bound() {
                return false;
            }
        }
    }
    count > 0
}

fn assert_wearables(client: &Client, body: Entity) {
    let world = client.world();
    let mut bound_joints = 0;
    let mut worn = 0;
    for root in subtree(world, body) {
        let Some(wearable) = world.get::<crate::units::Wearable>(root) else {
            continue;
        };
        worn += 1;
        assert!(wearable.bound());
        let names: HashMap<_, _> = subtree(world, wearable.wearer())
            .into_iter()
            .filter_map(|entity| {
                Some((
                    world.get::<Name>(entity)?.as_str().to_ascii_lowercase(),
                    entity,
                ))
            })
            .collect();
        for mesh in subtree(world, root) {
            let Some(skin) = world.get::<SkinnedMesh>(mesh) else {
                continue;
            };
            for joint in &skin.joints {
                let name = world
                    .get::<Name>(*joint)
                    .expect("joint name")
                    .as_str()
                    .to_ascii_lowercase();
                if let Some(expected) = names.get(&name) {
                    assert_eq!(
                        joint, expected,
                        "cosmetics must use their animated hero joint"
                    );
                    bound_joints += 1;
                }
            }
        }
    }
    assert!(
        worn > 0 && bound_joints > 0,
        "default cosmetics must be present and driven by hero joints"
    );
}
