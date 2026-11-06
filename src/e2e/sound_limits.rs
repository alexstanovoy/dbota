//! The voices of the world as Dota's sound events bound them, in a real match: the blows of
//! creeps on one unit sound once for each half second, not each in full; a toggled ability's
//! looping sound lives as long as the toggle is on, follows its source, waits for the camera
//! and ends with the match; and a cast's one-shot sound plays out after the cast.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{AudioPlayer, PlaybackMode, PlaybackSettings};
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, EntityId, HeroId, Vec2 as At};

use super::harness::{Client, MID, Server};
use crate::mixer::{MERGE_WINDOW, Mixer};

const ROT: &str = "Hero_Pudge.Rot";
/// The factor Dota gives a sound from the entity of the player (`volume_on_player_mult`).
const OWN_ENTITY: f32 = 1.25;

/// The players of the world whose cache path holds `part`: what is sounding. A sound that plays
/// once is dropped at once where there is no sound device, as these tests run; a loop stays.
pub(super) fn alive(client: &mut Client, part: &str) -> usize {
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

/// A player that started: the game clock of its start and how it plays.
pub(super) type Start = (Duration, PlaybackMode);

/// The players that start from now on whose cache path holds `part`.
pub(super) fn listen(client: &mut Client, part: &'static str) -> Arc<Mutex<Vec<Start>>> {
    let starts = Arc::new(Mutex::new(Vec::new()));
    let into = starts.clone();
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>,
              players: Query<(&AudioPlayer, &PlaybackSettings)>,
              server: Res<AssetServer>,
              time: Res<Time>| {
            let Ok((player, settings)) = players.get(add.entity) else {
                return;
            };
            let path = server.get_path(player.0.id()).map(|p| p.to_string());
            if path.is_some_and(|p| p.contains(part)) {
                into.lock()
                    .expect("unpoisoned")
                    .push((time.elapsed(), settings.mode));
            }
        },
    );
    starts
}

/// Waits for Rot's sound to start and says what it is if it is not a loop: Rot's sound repeats
/// until Rot is turned off (its file has a loop start).
fn rot_starts_as_a_loop(client: &mut Client, starts: &Arc<Mutex<Vec<Start>>>) {
    client.until("Rot's sound started", |_| {
        !starts.lock().expect("unpoisoned").is_empty()
    });
    let (_, mode) = starts.lock().expect("unpoisoned")[0];
    assert!(
        matches!(mode, PlaybackMode::Loop),
        "Rot is a one-shot: its sound plays once and stops, though it repeats until Rot is off"
    );
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
pub(super) fn pudge_with_rot() -> (Server, Client, impl Fn(&mut Client)) {
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
    // Keep the victim alive through all four measured blows.
    client.cheat(Cheat::Levels { count: 24 });
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
    assert!(
        client.me().hp > 0,
        "the hero survives the four measured blow sounds"
    );
    let starts = starts.lock().expect("unpoisoned").clone();
    let tightest = starts
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
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
    let starts = listen(&mut client, "pudge/rot");
    toggle(&mut client);
    rot_starts_as_a_loop(&mut client, &starts);
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
    let starts = listen(&mut client, "pudge/rot");
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
    rot_starts_as_a_loop(&mut client, &starts);
    client.until("Rot's loop sounding at Pudge", |c| {
        alive(c, "pudge/rot") == 1
    });
    client.until("Rot at its full volume", |c| {
        rot_gain(c) == Some(OWN_ENTITY)
    });
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
    client.until("Rot at its full volume", |c| {
        rot_gain(c) == Some(OWN_ENTITY)
    });
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
    let starts = listen(&mut client, "pudge/rot");
    toggle(&mut client);
    rot_starts_as_a_loop(&mut client, &starts);
    client.until("Rot's loop sounding", |c| alive(c, "pudge/rot") == 1);
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    client.until("Rot's loop gone with the match", |c| {
        alive(c, "pudge/rot") == 0
    });
}

/// The casting row starts once with automatic completion; ending the cast never restarts it.
#[test]
fn requiems_cast_sound_starts_once_as_a_one_shot() {
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
    let starts = listen(&mut client, "requiem_cast");
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: false,
    });
    client.until("the casting sound was admitted", |_| {
        !starts.lock().expect("unpoisoned").is_empty()
    });
    client.until("the cast point over", |c| c.me().cast.is_none());
    client.frames(10);
    let starts = starts.lock().expect("unpoisoned");
    assert_eq!(starts.len(), 1, "cast completion restarted the sound");
    assert!(
        matches!(starts[0].1, PlaybackMode::Despawn),
        "the casting sound was kept as a loop"
    );
}

/// Our hero's blows on two units sound each, and a second blow on the first within the merge
/// window does not: blows merge by the unit struck, not by the hero who strikes.
#[test]
fn blows_of_a_hero_on_two_units_both_sound_and_on_one_unit_once() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let starts = listen(&mut client, "witch_doctor/projectile_impact");
    let unit = |idx| EntityId { idx, generation: 1 };
    let hero = Some(HeroId::SHADOW_FIEND);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(60),
        ));
    for target in [unit(900), unit(901), unit(900)] {
        client.core_mut().cue_blow(
            hero,
            bota_client_core::CueKind::Hit,
            0.55,
            None,
            Some(target),
        );
        client.frames(1);
    }
    let sounded = starts.lock().expect("unpoisoned").len();
    assert_eq!(sounded, 2, "blows on two units, one of them twice");
}
