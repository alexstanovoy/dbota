//! The game camera: the long lens from far out.

use bevy::camera::Projection;
use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server};

/// A fresh client's game camera looks down at 58 degrees through a lens of 25 degrees from
/// 4400 units out (Dota's own 1200 units, 60 degrees and 70-degree lens were tried and this
/// one taken back).
#[test]
fn the_game_camera_is_the_long_lens_from_far_out() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    let world = client.world_mut();
    let (rts, place, projection) = world
        .query::<(&crate::camera::RtsCamera, &GlobalTransform, &Projection)>()
        .single(world)
        .expect("the game's camera");
    assert!((rts.distance - 4400.0).abs() < 1.0, "{} out", rts.distance);
    let down = place.forward().as_vec3();
    let pitch = (-down.y).asin().to_degrees();
    assert!((pitch - 58.0).abs() < 0.5, "{pitch} degrees down");
    let Projection::Perspective(lens) = projection else {
        panic!("a perspective lens");
    };
    assert!(
        (lens.fov.to_degrees() - 25.0).abs() < 0.5,
        "a lens of {}",
        lens.fov.to_degrees()
    );
}
