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

/// Waves the random sounds of a wave name have played.
fn played(client: &Client, wave: &str) -> u32 {
    let ambience = client.world().resource::<Ambience>();
    ambience
        .sounds
        .iter()
        .zip(&ambience.played)
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
    let birds = played(&client, "birds01.wav");
    client.within(patience, "a bird singing", |c| {
        look_at(c, woods);
        played(c, "birds01.wav") > birds
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
