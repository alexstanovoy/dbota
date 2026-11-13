//! Dota's soundscapes end to end: the map's ambience heard where the camera is and by the time
//! of day, as the map's soundscapes lay it out (the day's and the night's, each with its loops
//! at the bases, the waterfall and the river, and its birds, bats and owl now and then).

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId};

use super::harness::{Client, Server};
use crate::soundscape::Ambience;

/// bota = Dota + this, on x and y.
const DOTA_TO_BOTA: f32 = 9216.0;

/// Puts the camera's focus on a Dota point, let go of the hero it followed.
fn look_at(client: &mut Client, (x, y): (f32, f32)) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let mut camera = world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("the game's camera");
    camera.focus = Vec3::new(x + DOTA_TO_BOTA, 0.0, -(y + DOTA_TO_BOTA));
}

/// The name of the soundscape heard and how far it has faded in.
fn heard(client: &Client) -> Option<(String, f32)> {
    let ambience = client.world().resource::<Ambience>();
    let at = ambience.heard?;
    Some((ambience.scapes[at].clone(), ambience.weights[at]))
}

/// The loudest gain of the sounds of a soundscape whose wave ends with `wave`, loops or not.
fn gain(client: &Client, scape: &str, wave: &str, looped: bool) -> f32 {
    sounds_of(client, scape, wave, looped)
        .map(|i| client.world().resource::<Ambience>().gains[i])
        .fold(0.0, f32::max)
}

/// The sounds of soundscape `scape` (by the scape's name, as the file's lines name it) whose
/// first wave ends with `wave`: indices into the ambience's lists.
fn sounds_of<'a>(
    client: &'a Client,
    scape: &'a str,
    wave: &'a str,
    looped: bool,
) -> impl Iterator<Item = usize> + 'a {
    let ambience = client.world().resource::<Ambience>();
    let at = ambience.scapes.iter().position(|s| s == scape);
    ambience
        .sounds
        .iter()
        .enumerate()
        .filter(move |(_, (s, w, l))| Some(*s) == at && w.ends_with(wave) && *l == looped)
        .map(|(i, _)| i)
}

/// Eligible random wave draws, independently of mute and mixer admission.
fn scheduled(client: &Client, wave: &str) -> u32 {
    let ambience = client.world().resource::<Ambience>();
    ambience
        .sounds
        .iter()
        .zip(&ambience.scheduled)
        .filter(|((_, w, looped), _)| !looped && w.ends_with(wave))
        .map(|(_, n)| *n)
        .sum()
}

/// By day at the Radiant base the day's soundscape is heard whole: its bed and the Radiant
/// spawn's loop play, the Dire spawn's (across the map) does not; at the river its loop plays;
/// near the bottom woods the day's birds sing now and then. By night the night's soundscape
/// takes over: its bed plays and the day's bed has faded out.
#[test]
fn dotas_soundscapes_play_by_place_and_by_time_of_day() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::TimeOfDay { night: false });
    let patience = Duration::from_secs(60);
    client.within(patience, "day", |c| {
        c.world().resource::<crate::daylight::Daylight>().night == 0.0
    });
    look_at(&mut client, (-7350.0, -6825.0));
    client.within(patience, "the day's soundscape heard whole", |c| {
        heard(c) == Some(("default_day".to_owned(), 1.0))
    });
    client.frames(2);
    assert!(gain(&client, "default_day", "default_day_lp.wav", true) > 0.0);
    assert!(gain(&client, "default_day", "radiant_spawn_loop_02.wav", true) > 0.0);
    assert_eq!(
        gain(&client, "default_day", "dire_spawn_loop_01.wav", true),
        0.0
    );
    look_at(&mut client, (1750.0, -8000.0));
    client.frames(2);
    assert!(gain(&client, "default_day", "river_med_lp_01.wav", true) > 0.0);
    // The camera held over the bottom woods while the birds are waited for.
    let woods = (1000.0, -5400.0);
    look_at(&mut client, woods);
    let birds = scheduled(&client, "birds01.wav");
    client.within(patience, "a bird singing", |c| {
        look_at(c, woods);
        scheduled(c, "birds01.wav") > birds
    });
    client.cheat(Cheat::TimeOfDay { night: true });
    client.within(patience, "the night's soundscape heard whole", |c| {
        heard(c) == Some(("default_night".to_owned(), 1.0))
    });
    client.frames(2);
    assert!(gain(&client, "default_night", "default_night_lp.wav", true) > 0.0);
    assert_eq!(
        gain(&client, "default_day", "default_day_lp.wav", true),
        0.0
    );
}

/// The Ancients' humming loops are left out (the owner: an unpleasant continuous sound on the
/// base): with the camera held over either Ancient, by day and by night, no ancient loop sounds
/// while the base's other loops (the spawn's) still do.
#[test]
fn the_ancients_humming_loops_are_not_heard_at_either_base() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let patience = Duration::from_secs(60);
    for night in [false, true] {
        client.cheat(Cheat::TimeOfDay { night });
        let scape = if night {
            "default_night"
        } else {
            "default_day"
        };
        for (ancient, spawn) in [
            ((-5850.0, -5300.0), "radiant_spawn_loop_02.wav"),
            ((5250.0, 4700.0), "dire_spawn_loop_01.wav"),
        ] {
            look_at(&mut client, ancient);
            client.within(patience, "the soundscape heard whole", |c| {
                look_at(c, ancient);
                heard(c) == Some((scape.to_owned(), 1.0))
            });
            client.frames(2);
            for wave in ["radiant_ancient_lp_01.wav", "dire_ancient_lp_01.wav"] {
                assert_eq!(
                    gain(&client, scape, wave, true),
                    0.0,
                    "{wave} sounds at {ancient:?} in {scape}"
                );
                assert_eq!(
                    sounds_of(&client, scape, wave, true).count(),
                    0,
                    "{wave} is bound in {scape}"
                );
            }
            assert!(
                sounds_of(&client, scape, spawn, true).count() > 0,
                "the base's other loops are kept ({spawn} in {scape})"
            );
        }
    }
}

/// Roshan's pits sound only while Roshan stands in them: with none in the match, the camera held
/// over either pit hears neither pit's loop, by day and by night, as Dota's map places them in the
/// soundscape for him alone.
#[test]
fn an_empty_roshan_pit_is_silent() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let patience = Duration::from_secs(60);
    for night in [false, true] {
        client.cheat(Cheat::TimeOfDay { night });
        let scape = if night {
            "default_night"
        } else {
            "default_day"
        };
        // The bottom pit, then the top one.
        for pit in [(3000.0, -3000.0), (-3000.0, 2400.0)] {
            look_at(&mut client, pit);
            client.within(patience, "the soundscape heard whole", |c| {
                look_at(c, pit);
                heard(c) == Some((scape.to_owned(), 1.0))
            });
            client.frames(2);
            let pit_loops = sounds_of(&client, scape, "dire_pit_loop_01_l.wav", true).count();
            assert_eq!(pit_loops, 2, "the {scape}'s two pit loops");
            assert_eq!(
                gain(&client, scape, "dire_pit_loop_01_l.wav", true),
                0.0,
                "a pit loop sounds at {pit:?} {scape} with no Roshan"
            );
        }
    }
}

/// Occupancy comes from the unit catalogue's NPC key, on the same real server stream the
/// model test draws. The other pit stays silent while Roshan stands in this one.
#[test]
fn an_occupied_roshan_pit_sounds_and_the_empty_one_does_not() {
    let server = Server::start(1, &["--objectives", "roshan", "--immortal-heroes"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::TimeOfDay { night: false });
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.within(Duration::from_secs(60), "the occupied pit loop", |c| {
        look_at(c, (-3194.28, 2394.62));
        gain(c, "default_day", "dire_pit_loop_01_l.wav", true) > 0.0
    });
    look_at(&mut client, (2859.76, -2764.77));
    client.frames(4);
    assert_eq!(
        gain(&client, "default_day", "dire_pit_loop_01_l.wav", true),
        0.0
    );
}

/// A loop of the ambience that is already sounding takes the master volume once and the ambience's
/// level and mute as they move, and a second loop of the day's scape sounds as the first.
#[test]
fn the_ambience_loops_follow_the_balance_with_the_master_applied_once() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    super::channels::listen(&mut client);
    client.cheat(Cheat::TimeOfDay { night: false });
    look_at(&mut client, (-7350.0, -6825.0));
    client.within(Duration::from_secs(60), "day ambience", |c| {
        heard(c) == Some(("default_day".to_owned(), 1.0))
    });
    client.frames(3);
    let world = client.world_mut();
    let sound = world
        .query::<(Entity, &bevy::audio::AudioPlayer, &crate::channels::Tuned)>()
        .iter(world)
        .find(|(_, player, tuned)| {
            tuned.channel == crate::channels::Channel::Ambience
                && player
                    .0
                    .path()
                    .is_some_and(|path| path.to_string().ends_with("default_day_lp.wav"))
        })
        .map(|(entity, ..)| entity)
        .expect("the day's ambience loop");
    let raw = gain(&client, "default_day", "default_day_lp.wav", true);
    let tuned = client
        .world()
        .get::<crate::channels::Tuned>(sound)
        .expect("tuned")
        .base;
    assert!(
        raw > 0.0 && (tuned - raw).abs() < 1e-3,
        "the loop's own loudness, before the balance, is the scape's: {tuned} against {raw}"
    );
    super::channels::follows_the_balance(&mut client, sound, crate::channels::Channel::Ambience);
}
