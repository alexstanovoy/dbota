//! A hero's model as Dota moves it from clip to clip: standing, it plays its rare idle now and
//! then; set running, its idle blends into the run over a moment instead of cutting to it.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::anim::{FADE_SECONDS, Pose, RARE_IDLE};

/// Seconds of the client's clock a frame of this test moves: steady, whatever the machine's
/// load, so a blend spans the frames its length says.
const FRAME: f32 = 1.0 / 60.0;

/// One's hero's pose.
fn pose(client: &Client) -> Option<&Pose> {
    let body = client.body(client.me().id)?;
    client.world().get::<Pose>(body)
}

/// The weights of the clips one's hero's model plays now.
fn weights(client: &Client) -> Vec<f32> {
    let body = client.body(client.me().id).expect("one's hero drawn");
    let rig = client
        .world()
        .get::<crate::units::Rig>(body)
        .expect("one's hero's rig")
        .0;
    let player = client
        .world()
        .get::<AnimationPlayer>(rig)
        .expect("its animation player");
    player
        .playing_animations()
        .map(|(_, a)| a.weight())
        .collect()
}

/// Shadow Fiend standing at his fountain plays his rare idle after some loops of his idle and
/// goes back to idling; ordered to walk, for a few frames his idle and his run both weigh, less
/// than whole and adding up to one, and once the blend is over only the run does.
#[test]
fn a_standing_hero_plays_its_rare_idle_and_blends_into_a_run() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )));
    let clip = |c: &mut Client| pose(c).map(Pose::clip);
    client.until("the hero idling", |c| clip(c) == Some("idle"));
    client.until("the rare idle", |c| clip(c) == Some(RARE_IDLE));
    client.until("the idle again", |c| clip(c) == Some("idle"));
    client.until("the blend into the idle over", |c| {
        pose(c).is_some_and(|p| !p.blending())
    });
    let at = client.me().pos;
    let to = (at.x.to_f32() + 800.0, at.y.to_f32());
    client.core_mut().act(Intent::MoveTo(to));
    client.within(Duration::from_secs(60), "the run", |c| {
        clip(c) == Some("run")
    });
    let mixed = weights(&client);
    assert_eq!(mixed.len(), 2, "idle and run both play: {mixed:?}");
    assert!(
        mixed.iter().all(|w| *w > 0.0 && *w < 1.0),
        "each weighs part: {mixed:?}"
    );
    assert!(
        (mixed.iter().sum::<f32>() - 1.0).abs() < 1e-3,
        "the parts add up to one: {mixed:?}"
    );
    let frames = (FADE_SECONDS / FRAME).ceil() as usize + 1;
    client.frames(frames);
    assert!(pose(&client).is_some_and(|p| !p.blending()));
    assert_eq!(clip(&mut client), Some("run"), "still running");
    assert_eq!(weights(&client), [1.0], "the run alone, whole");
}

/// Whether one's hero's model is posed: its player has its clips' graph, and every skinned mesh
/// of it has bounds that follow its joints; or `None` while its model is not drawn.
fn posed(client: &Client) -> Option<(bool, bool)> {
    use bevy::camera::visibility::DynamicSkinnedMeshBounds;
    let body = client.body(client.me().id)?;
    let world = client.world();
    let rig = world.get::<crate::units::Rig>(body)?.0;
    let pivot = world.get::<crate::units::ModelOf>(body)?.0;
    let meshes: Vec<Entity> = super::harness::subtree(world, pivot)
        .into_iter()
        .filter(|e| world.get::<bevy::mesh::skinning::SkinnedMesh>(*e).is_some())
        .collect();
    assert!(!meshes.is_empty(), "the hero's model is skinned");
    let bounded = meshes
        .iter()
        .map(|e| world.get::<DynamicSkinnedMeshBounds>(*e).is_some());
    let bounded: Vec<bool> = bounded.collect();
    assert!(
        bounded.iter().all(|b| *b == bounded[0]),
        "all of its meshes alike: {bounded:?}"
    );
    Some((world.get::<AnimationGraphHandle>(rig).is_some(), bounded[0]))
}

/// Puts the camera's focus at a bota point, without following anyone.
fn look_at(client: &mut Client, (x, y): (f32, f32)) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let mut camera = world.query::<&mut crate::camera::RtsCamera>();
    camera.single_mut(world).expect("the game's camera").focus = Vec3::new(x, 0.0, -y);
}

/// A hero the camera has left far behind is not posed (no clip evaluated, no bounds following
/// its joints); when the camera jumps back to it, it is posed in the very first frame there, its
/// joints moved on from where they were left, never a frame late in a stale pose.
#[test]
fn a_hero_far_from_the_view_is_not_posed_and_is_posed_in_the_first_frame_the_view_jumps_back() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(super::harness::MID);
    client.until("the hero posed", |c| posed(c) == Some((true, true)));
    let at = client.me().pos;
    let at = (at.x.to_f32(), at.y.to_f32());
    look_at(&mut client, (at.0 - 9000.0, at.1 - 9000.0));
    client.until("the hero no longer posed", |c| {
        posed(c) == Some((false, false))
    });
    let body = client.body(client.me().id).expect("one's hero drawn");
    let left = super::harness::pose_of(&client, body);
    client.frames(2);
    assert_eq!(
        super::harness::pose_of(&client, body),
        left,
        "unposed, its joints hold still"
    );
    look_at(&mut client, at);
    client.frames(1);
    assert_eq!(
        posed(&client),
        Some((true, true)),
        "posed in the first frame after the jump"
    );
    let first = super::harness::pose_of(&client, body);
    assert_ne!(first, left, "its joints are posed in that frame");
    client.frames(1);
    assert_ne!(super::harness::pose_of(&client, body), first, "and play on");
}

/// A unit's model is posed from the first frame it is drawn in the view, its skinned meshes'
/// bounds following its joints, not only once it has left the view and come back.
#[test]
fn a_hero_drawn_in_the_view_is_posed_from_its_first_frame() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client.until("the hero's model drawn", |c| posed(c).is_some());
    assert_eq!(
        posed(&client),
        Some((true, true)),
        "posed, bounds following its joints"
    );
}

/// The widest reach either side of its feet and the height over them of a model's bind pose, in
/// world units, read from its file in the cache (the bounds glTF keeps of every mesh's positions).
fn bind_pose_extent(file: &std::path::Path, scale: f32) -> (f32, f32) {
    let file = std::fs::read(file).expect("a model file of the cache");
    let json_len = u32::from_le_bytes(file[12..16].try_into().expect("a GLB header")) as usize;
    let doc: serde_json::Value =
        serde_json::from_slice(&file[20..20 + json_len]).expect("its JSON");
    let k = crate::map::UNITS_PER_METRE * scale;
    let (mut reach, mut height) = (0.0f32, 0.0f32);
    for mesh in doc["meshes"].as_array().into_iter().flatten() {
        for primitive in mesh["primitives"].as_array().into_iter().flatten() {
            let Some(at) = primitive["attributes"]["POSITION"].as_u64() else {
                continue;
            };
            let accessor = &doc["accessors"][at as usize];
            let corner = |key: &str| -> Vec<f32> {
                let values = accessor[key].as_array().into_iter().flatten();
                values
                    .filter_map(|v| v.as_f64())
                    .map(|v| v as f32)
                    .collect()
            };
            let (min, max) = (corner("min"), corner("max"));
            for v in [min[0], max[0], min[2], max[2]] {
                reach = reach.max(v.abs() * k);
            }
            height = height.max(max[1] * k);
        }
    }
    (reach, height)
}

/// Every model a unit is posed with (one with clips, and what is worn on one), at every size a
/// unit wears it, fits the box it is posed by with room for its clips to reach past its bind
/// pose, a quarter more again.
#[test]
fn every_posed_model_fits_the_box_it_is_posed_by() {
    use clap::Parser;
    use dbota_import::catalog::{Line as CatalogLine, parse};
    let root = crate::Args::parse_from(["dbota"]).cache_dir;
    let catalog = parse(crate::manifest::MANIFEST, crate::manifest::IDS).expect("the manifest");
    let models: Vec<_> = catalog
        .lines
        .iter()
        .filter_map(|l| match l {
            CatalogLine::Model(m) => Some(m),
            _ => None,
        })
        .collect();
    let posed = |name: &str| {
        models
            .iter()
            .any(|m| m.logical == name && !m.anims.is_empty())
    };
    let mut measured = 0;
    for model in &models {
        let body = model.attach.as_deref().unwrap_or(&model.logical);
        if !posed(body) {
            continue;
        }
        let units = catalog.lines.iter().filter_map(|l| match l {
            CatalogLine::Unit(u) if u.model.as_deref() == Some(body) => u.model_scale,
            _ => None,
        });
        let own = models
            .iter()
            .find(|m| m.logical == body)
            .and_then(|m| m.model_scale);
        for scale in units.chain([own.unwrap_or(1.0)]) {
            let (reach, height) = bind_pose_extent(&root.join(&model.out), scale);
            let name = &model.logical;
            assert!(
                reach * 1.25 <= crate::anim::MODEL_REACH,
                "{name} at {scale} reaches {reach:.0}"
            );
            assert!(
                height * 1.25 <= crate::anim::MODEL_HEIGHT,
                "{name} at {scale} stands {height:.0}"
            );
            measured += 1;
        }
    }
    assert!(
        measured > 50,
        "the cache's posed models were measured: {measured}"
    );
}
