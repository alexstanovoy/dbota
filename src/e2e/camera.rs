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

/// The game camera: where its focus is on the ground, in bota's units.
fn focus_of(client: &mut Client) -> Vec2 {
    let world = client.world_mut();
    let focus = world
        .query::<&crate::camera::RtsCamera>()
        .single(world)
        .expect("the game's camera")
        .focus;
    Vec2::new(focus.x, -focus.z)
}

/// A match begins with the camera on one's hero and the hero neither picked nor followed: the
/// hero walks off and the camera stays, until the player pins it (a second tap on the portrait)
/// or holds the camera lock.
#[test]
fn the_hero_is_neither_selected_nor_followed_when_the_match_begins() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.until("the camera on the hero where it stands", |c| {
        let hero = c.me().pos;
        let hero = Vec2::new(hero.x.to_f32(), hero.y.to_f32());
        focus_of(c).distance(hero) < 50.0
    });
    assert_eq!(
        client.core().selected,
        None,
        "nothing is picked, so no ring is drawn"
    );
    assert_eq!(
        client.core().commanded(),
        client.core().my_hero(),
        "the keys and the panel are still about the hero"
    );
    assert_eq!(client.core().pinned, None, "the camera is not pinned");
    let placed = focus_of(&mut client);
    client.cheat(bota_proto::Cheat::Teleport {
        at: bota_proto::Vec2::from_ints(8000, 9600),
    });
    client.until("the hero far from the camera", |c| {
        let hero = c.me().pos;
        Vec2::new(hero.x.to_f32(), hero.y.to_f32()).distance(placed) > 3000.0
    });
    client.frames(120);
    let now = focus_of(&mut client);
    assert!(
        now.distance(placed) < 1.0,
        "the camera stayed at {placed}, not {now}, while the hero walked off"
    );
}

/// A replay watched through a seat follows that seat's hero: the camera is on it as it walks.
#[test]
fn a_replay_watched_through_a_seat_follows_that_seats_hero() {
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("target/camera-pov-{}.brp", std::process::id()));
    let end = {
        let mut server = Server::start(
            1,
            &[
                "--replay",
                replay.to_str().expect("UTF-8"),
                "--empty-after-secs",
                "1",
            ],
        );
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        client.cheat(bota_proto::Cheat::Teleport {
            at: bota_proto::Vec2::from_ints(13000, 13000),
        });
        client.until("the hero far off", |c| {
            let hero = c.me().pos;
            Vec2::new(hero.x.to_f32(), hero.y.to_f32()).distance(Vec2::new(13000.0, 13000.0)) < 50.0
        });
        client.frames(30);
        let end = client.tick();
        drop(client);
        server.wait();
        end
    };
    let mut client = Client::run(&["--replay", replay.to_str().expect("UTF-8"), "--pov", "0"]);
    // Held at the first tick, with the hero at its fountain, the camera comes to it there.
    client.until("the replay's first view", |c| {
        if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
            player.paused = true;
        }
        c.core().view().is_some()
    });
    client.frames(30);
    let at_start = focus_of(&mut client);
    assert!(
        at_start.distance(Vec2::new(13000.0, 13000.0)) > 3000.0,
        "the hero starts far from where it walks to: {at_start}"
    );
    client.until(
        "the replay where the hero walked, the camera on the hero",
        |c| {
            let now = c.tick();
            if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
                if now < end {
                    player.advance_ticks(f64::from(end - now));
                }
                player.paused = now >= end;
            }
            let Some(hero) = c.core().my_hero().and_then(|id| c.view().unit(id)) else {
                return false;
            };
            let hero = Vec2::new(hero.pos.x.to_f32(), hero.pos.y.to_f32());
            now >= end && focus_of(c).distance(hero) < 50.0
        },
    );
    assert!(
        focus_of(&mut client).distance(Vec2::new(13000.0, 13000.0)) < 300.0,
        "the camera went with the hero from where it started to where it walked"
    );
}
