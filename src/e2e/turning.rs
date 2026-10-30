//! A unit turns at its hero's turn rate on screen, whatever the clicks: a new order mid-turn
//! neither snaps the drawn model round nor stops it turning.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, MID, Server};

/// Radians of a sim angle in brads.
fn radians(brads: i32) -> f32 {
    brads as f32 / 65536.0 * std::f32::consts::TAU
}

/// The yaw the model's pivot is drawn at, as the facing it shows (radians counter-clockwise
/// from east).
fn drawn_facing(client: &mut Client) -> f32 {
    let id = client.me().id;
    let body = client.body(id).expect("drawn");
    let pivot = client
        .world()
        .get::<crate::units::ModelOf>(body)
        .expect("a model")
        .0;
    let rotation = client
        .world()
        .get::<Transform>(pivot)
        .expect("posed")
        .rotation;
    rotation.to_euler(EulerRot::YXZ).0 + std::f32::consts::FRAC_PI_2
}

/// The shortest signed turn from one angle to another.
fn delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// A click on alternating sides every other frame, as fast as a player can: on no frame does
/// the drawn model turn by more than its hero's turn rate allows for the ticks that passed
/// (plus the one tick the drawing trails the server by), and over the clicks it does turn
/// round, frame after frame.
#[test]
fn rapid_clicks_on_alternating_sides_turn_the_model_at_the_heros_rate() {
    for hero in [HeroId::SHADOW_FIEND, HeroId::PUDGE] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, hero, &[]);
        client.stand_at(MID);
        let rate = radians(i32::from(
            bota_core::hero_def(hero).expect("a hero").unit.turn_rate,
        ));
        let (mut before, mut tick) = (drawn_facing(&mut client), client.tick());
        let (mut turning_frames, mut widest) = (0, 0.0_f32);
        for frame in 0..160_u32 {
            if frame.is_multiple_of(2) {
                let side = if (frame / 2).is_multiple_of(2) {
                    2000.0
                } else {
                    -2000.0
                };
                client
                    .core_mut()
                    .act(Intent::MoveTo((MID.0 as f32 + side, MID.1 as f32)));
            }
            client.frames(1);
            let now = drawn_facing(&mut client);
            let passed = client.tick().saturating_sub(tick);
            let step = delta(before, now).abs();
            let allowed = rate * (passed + 1) as f32 + 1e-3;
            assert!(
                step <= allowed,
                "{hero:?}: frame {frame} drew a turn of {step} rad, more than {allowed} ({passed} ticks)"
            );
            turning_frames += usize::from(step > 1e-4);
            widest = widest.max(step);
            (before, tick) = (now, client.tick());
        }
        assert!(
            turning_frames > 20,
            "{hero:?}: the model turned on {turning_frames} frames only"
        );
        assert!(widest > 0.0);
    }
}
