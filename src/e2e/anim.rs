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
