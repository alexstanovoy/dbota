//! The voices of the world as Dota's sound events bound them, in a real match: the blows of
//! creeps on one unit sound once for each half second, not each in full; a toggled ability's
//! looping sound lives as long as the toggle is on, follows its source, waits for the camera
//! and ends with the match; and a cast's one-shot sound plays out after the cast.

use std::time::Duration;

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, EntityId, HeroId, Vec2 as At};

use super::harness::{Client, MID, Server};
use crate::mixer::{MERGE_WINDOW, Mixer};

const ROT: &str = "Hero_Pudge.Rot";

/// The voices of the world's mixer sounding an event now.
fn sounding(client: &Client, event: &str) -> usize {
    client.world().resource::<Mixer>().sounding(event)
}

/// The mixer's clock: what the voices' ages are counted by.
fn clock(client: &Client) -> Duration {
    client.world().resource::<Time>().elapsed()
}

/// The volume Rot's loop has now.
fn rot_gain(client: &Client) -> Option<f32> {
    client.world().resource::<Mixer>().loop_gain(ROT)
}

/// Puts the camera's focus on a point of the world, let go of the hero it followed.
fn look_at(client: &mut Client, (x, y): (f32, f32)) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let mut camera = world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("the game's camera");
    camera.focus = Vec3::new(x, 0.0, -y);
}

/// A Pudge on the map with Rot learned and the toggle that turns it on and off.
fn pudge_with_rot() -> (Server, Client, impl Fn(&mut Client)) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Levels { count: 5 });
    client.cheat(Cheat::Wtf { on: true });
    let slot = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == AbilityId::ROT)
        .expect("Rot") as u8;
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: true,
    });
    client.until("Rot learned", |c| {
        c.me().abilities[usize::from(slot)].level > 0
    });
    let toggle = move |client: &mut Client| {
        client.core_mut().act(Intent::Press {
            slot: Slot::Ability(slot),
            ctrl: false,
        });
    };
    (server, client, toggle)
}

/// Many melee creeps strike one hero for seconds: the blows on him sound once for each merge
/// window of the mixer's clock, and some do sound.
#[test]
fn blows_of_creeps_on_one_hero_merge_within_half_a_second() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Wtf { on: true });
    let me = client.me().id;
    for dx in [-120, 0, 120] {
        client.cheat(Cheat::CreateCreeps {
            enemy: true,
            at: At::from_ints(MID.0 + dx, MID.1 + 150),
        });
    }
    let blows = |c: &Client| {
        let mixer = c.world().resource::<Mixer>();
        mixer.starts_of("Creep_Good_Melee.Attack")
    };
    client.until("the creeps' blows sounded", |c| blows(c).len() >= 4);
    let mut starts: Vec<(EntityId, Duration)> = blows(&client);
    assert!(starts.iter().all(|(target, _)| *target == me), "{starts:?}");
    starts.sort_by_key(|(_, at)| *at);
    let tightest = starts
        .windows(2)
        .map(|pair| pair[1].1 - pair[0].1)
        .min()
        .expect("two blows");
    assert!(
        tightest >= MERGE_WINDOW,
        "two blows on one hero sounded {tightest:?} apart"
    );
}

/// Pudge's Rot toggled on, off and on: its looping sound is alive exactly while the toggle is on,
/// one voice and never two.
#[test]
fn rot_sounds_one_loop_while_it_is_on_and_none_after_it_is_turned_off() {
    let (_server, mut client, toggle) = pudge_with_rot();
    toggle(&mut client);
    client.until("Rot's loop sounding", |c| sounding(c, ROT) == 1);
    client.frames(30);
    assert_eq!(sounding(&client, ROT), 1, "Rot's loop stacked");
    toggle(&mut client);
    client.until("Rot's loop stopped with Rot", |c| sounding(c, ROT) == 0);
    toggle(&mut client);
    client.until("Rot's loop sounding again", |c| sounding(c, ROT) >= 1);
    client.frames(30);
    assert_eq!(sounding(&client, ROT), 1, "Rot's loop stacked");
}

/// Rot turned on with the camera far from Pudge is not heard; when the camera comes to him it
/// is, and it is as loud as the camera's distance leaves it from then on.
#[test]
fn rot_turned_on_out_of_earshot_sounds_when_the_camera_comes_and_follows_the_camera() {
    let (_server, mut client, toggle) = pudge_with_rot();
    let pudge = client.me().pos;
    let at = (pudge.x.to_f32(), pudge.y.to_f32());
    look_at(&mut client, (at.0 + 9000.0, at.1));
    client.frames(5);
    toggle(&mut client);
    client.until("Rot worn", |c| {
        c.me()
            .effects
            .iter()
            .any(|e| e.id == bota_proto::EffectId::ROT)
    });
    client.frames(60);
    assert_eq!(sounding(&client, ROT), 0, "Rot heard from across the map");
    look_at(&mut client, at);
    client.until("Rot's loop sounding at Pudge", |c| sounding(c, ROT) == 1);
    client.until("Rot at its full volume", |c| rot_gain(c) == Some(1.0));
    look_at(&mut client, (at.0 + 3000.0, at.1));
    client.until("Rot quieter with the distance", |c| {
        rot_gain(c).is_some_and(|gain| gain < 0.6)
    });
}

/// Rot on when the match is gone (the menu's idle session in its place) is not left sounding.
#[test]
fn rot_does_not_sound_on_after_the_match_is_gone() {
    let (_server, mut client, toggle) = pudge_with_rot();
    toggle(&mut client);
    client.until("Rot's loop sounding", |c| sounding(c, ROT) == 1);
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    client.until("Rot's loop gone with the match", |c| sounding(c, ROT) == 0);
    let mut players = client.world_mut().query::<&AudioPlayer>();
    let left = players
        .iter(client.world())
        .filter(|p| {
            p.0.path()
                .is_some_and(|path| path.path().to_string_lossy().contains("pudge/rot"))
        })
        .count();
    assert_eq!(left, 0, "Rot's player outlived the match");
}

/// Requiem's cast sound is five seconds long and plays out after the cast point is over.
#[test]
fn requiems_cast_sound_plays_out_after_the_cast_point() {
    const CAST: &str = "Hero_Nevermore.RequiemOfSoulsCast";
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Levels { count: 6 });
    client.cheat(Cheat::Wtf { on: true });
    let slot = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == AbilityId::REQUIEM)
        .expect("Requiem") as u8;
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: true,
    });
    client.until("Requiem learned", |c| {
        c.me().abilities[usize::from(slot)].level > 0
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: false,
    });
    client.until("the cast sounded", |c| sounding(c, CAST) == 1);
    client.until("the cast point over", |c| c.me().cast.is_none());
    let over = clock(&client);
    client.until("a second past the cast point", |c| {
        clock(c) >= over + Duration::from_secs(1)
    });
    assert_eq!(sounding(&client, CAST), 1, "the cast sound was cut short");
}
