//! The voices of the world as Dota's sound events bound them, in a real match: the blows of
//! creeps on one unit sound once for each half second, not each in full; a toggled ability's
//! looping sound lives as long as the toggle is on, follows its source, waits for the camera
//! and ends with the match; and a cast's one-shot sound plays out after the cast.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, HeroId, Vec2 as At};

use super::harness::{Client, MID, Server};
use crate::mixer::{MERGE_WINDOW, Mixer};

const ROT: &str = "Hero_Pudge.Rot";

/// The players of the world whose cache path holds `part`: what is sounding. A sound that plays
/// once is dropped at once where there is no sound device, as these tests run; a loop stays.
fn alive(client: &mut Client, part: &str) -> usize {
    let world = client.world_mut();
    let mut players = world.query::<&AudioPlayer>();
    players
        .iter(world)
        .filter(|player| {
            player
                .0
                .path()
                .is_some_and(|path| path.path().to_string_lossy().contains(part))
        })
        .count()
}

/// The players that start from now on whose cache path holds `part`, with the game clock of
/// each start.
fn listen(client: &mut Client, part: &'static str) -> Arc<Mutex<Vec<Duration>>> {
    let starts = Arc::new(Mutex::new(Vec::new()));
    let into = starts.clone();
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>,
              players: Query<&AudioPlayer>,
              server: Res<AssetServer>,
              time: Res<Time>| {
            let path = players
                .get(add.entity)
                .ok()
                .and_then(|p| server.get_path(p.0.id()))
                .map(|p| p.to_string());
            if path.is_some_and(|p| p.contains(part)) {
                into.lock().expect("unpoisoned").push(time.elapsed());
            }
        },
    );
    starts
}

/// The voices of the world's mixer sounding an event now.
fn sounding(client: &Client, event: &str) -> usize {
    client.world().resource::<Mixer>().sounding(event)
}

/// The game clock.
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

/// Many melee creeps strike one hero for seconds: the sounds of their blows on him start no
/// closer than the merge window of the game clock, and some do sound.
#[test]
fn blows_of_creeps_on_one_hero_merge_within_half_a_second() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Wtf { on: true });
    let starts = listen(&mut client, "melee_hit");
    for dx in [-120, 0, 120] {
        client.cheat(Cheat::CreateCreeps {
            enemy: true,
            at: At::from_ints(MID.0 + dx, MID.1 + 150),
        });
    }
    client.until("the creeps' blows sounded", |_| {
        starts.lock().expect("unpoisoned").len() >= 4
    });
    let starts = starts.lock().expect("unpoisoned").clone();
    let tightest = starts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
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
    client.until("Rot's loop sounding", |c| alive(c, "pudge/rot") == 1);
    client.frames(30);
    assert_eq!(alive(&mut client, "pudge/rot"), 1, "Rot's loop stacked");
    toggle(&mut client);
    client.until("Rot's loop stopped with Rot", |c| {
        alive(c, "pudge/rot") == 0
    });
    toggle(&mut client);
    client.until("Rot's loop sounding again", |c| alive(c, "pudge/rot") >= 1);
    client.frames(30);
    assert_eq!(alive(&mut client, "pudge/rot"), 1, "Rot's loop stacked");
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
    assert_eq!(
        alive(&mut client, "pudge/rot"),
        0,
        "Rot heard from across the map"
    );
    look_at(&mut client, at);
    client.until("Rot's loop sounding at Pudge", |c| {
        alive(c, "pudge/rot") == 1
    });
    client.until("Rot at its full volume", |c| rot_gain(c) == Some(1.0));
    look_at(&mut client, (at.0 + 3000.0, at.1));
    client.until("Rot quieter with the distance", |c| {
        rot_gain(c).is_some_and(|gain| gain < 0.6)
    });
}

/// Pudge walking away from a camera that stays lowers the volume of his Rot.
#[test]
fn rot_is_quieter_when_pudge_walks_away_from_the_camera() {
    let (_server, mut client, toggle) = pudge_with_rot();
    let pudge = client.me().pos;
    let at = (pudge.x.to_f32(), pudge.y.to_f32());
    look_at(&mut client, at);
    toggle(&mut client);
    client.until("Rot at its full volume", |c| rot_gain(c) == Some(1.0));
    client.cheat(Cheat::Teleport {
        at: At::from_ints(MID.0 + 3000, MID.1),
    });
    client.until("Rot quieter with Pudge gone", |c| {
        rot_gain(c).is_some_and(|gain| gain < 0.6)
    });
}

/// Rot on when the match is gone (the menu's idle session in its place) is not left sounding.
#[test]
fn rot_does_not_sound_on_after_the_match_is_gone() {
    let (_server, mut client, toggle) = pudge_with_rot();
    toggle(&mut client);
    client.until("Rot's loop sounding", |c| alive(c, "pudge/rot") == 1);
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    client.until("Rot's loop gone with the match", |c| {
        alive(c, "pudge/rot") == 0
    });
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
