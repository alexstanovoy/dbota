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
