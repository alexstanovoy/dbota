//! The game's sounds end with the game: leaving the match (the menu's idle session takes the
//! seat) and a connection that drops each silence every sound of the world, its loops and
//! ambience included, and nothing starts one again.

use std::time::Duration;

use bevy::audio::{AudioPlayer, AudioSource};
use bevy::prelude::*;

use super::harness::Client;
use super::sound_limits::{alive, listen, pudge_with_rot};
use crate::mixer::{Mixer, Owner, Request};

/// A cast sound five seconds long, which plays out unless it is ended.
const LONG: &str = "Hero_Nevermore.RequiemOfSoulsCast";

/// Every sound player of the world: what is sounding, game or not.
fn players(client: &mut Client) -> usize {
    let world = client.world_mut();
    world.query::<&AudioPlayer>().iter(world).count()
}

/// A match with Rot sounding and the day's ambience under it.
fn match_sounding() -> (super::harness::Server, Client) {
    let (server, mut client, toggle) = pudge_with_rot();
    let starts = listen(&mut client, "pudge/rot");
    toggle(&mut client);
    client.until("Rot's loop sounding", |c| alive(c, "pudge/rot") == 1);
    drop(starts);
    client.within(Duration::from_secs(30), "the ambience sounding too", |c| {
        players(c) >= 2
    });
    start_long_sound(&mut client);
    (server, client)
}

/// Starts the long sound through the mixer, as a cast would; a machine with a sound device
/// plays it out, one without drops its player at once, so the mixer's own voices are the
/// witness.
fn start_long_sound(client: &mut Client) {
    let world = client.world_mut();
    let now = world.resource::<Time>().elapsed();
    world.resource_scope(|world, mut mixer: Mut<Mixer>| {
        let mut commands = world.commands();
        let request = Request {
            event: LONG,
            owner: Owner::None,
            merge: None,
            at: None,
            gain: 1.0,
            speed: 1.0,
            ..Request::default()
        };
        mixer.start(&mut commands, now, &request, Handle::default());
    });
    assert_eq!(sounding(client), 1, "the long sound started");
}

fn sounding(client: &Client) -> usize {
    client.world().resource::<Mixer>().sounding(LONG)
}

fn assert_silent_for_a_while(client: &mut Client, what: &str) {
    client.until(what, |c| players(c) == 0);
    client.frames(60);
    assert_eq!(players(client), 0, "{what}: a sound started again");
    assert_eq!(sounding(client), 0, "{what}: a cast sound plays on");
}

#[test]
fn leaving_the_match_silences_every_game_sound() {
    let (_server, mut client) = match_sounding();
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    assert_silent_for_a_while(&mut client, "every game sound gone with the match");
}

/// After the match is left the mixer starts nothing, whoever asks and however long after, until
/// a match is in view again.
#[test]
fn no_game_sound_starts_after_leaving_the_match() {
    let (_server, mut client) = match_sounding();
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    assert_silent_for_a_while(&mut client, "every game sound gone with the match");
    start_refused(&mut client);
    client.frames(120);
    start_refused(&mut client);
}

fn start_refused(client: &mut Client) {
    let world = client.world_mut();
    let now = world.resource::<Time>().elapsed();
    let started = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
        let mut commands = world.commands();
        let request = Request {
            event: LONG,
            owner: Owner::None,
            merge: None,
            at: None,
            gain: 1.0,
            speed: 1.0,
            ..Request::default()
        };
        mixer.start(&mut commands, now, &request, Handle::default())
    });
    assert_eq!(started, crate::mixer::Started::Refused);
}

/// Leaving a game in its pick phase, before any match was in view, ends what its voices
/// started too.
#[test]
fn leaving_before_the_first_view_ends_what_the_game_started() {
    let server = super::harness::Server::start(1, &[]);
    let mut client = Client::pick(&server, bota_proto::HeroId::PUDGE, &[]);
    let line = client
        .world_mut()
        .spawn((
            crate::mixer::GameAudio,
            AudioPlayer::<AudioSource>::new(Handle::default()),
            bevy::audio::PlaybackSettings::LOOP,
        ))
        .id();
    client.frames(2);
    assert!(client.world().get_entity(line).is_ok(), "the line plays");
    *client.world_mut().non_send_mut::<crate::session::Session>() = crate::main_menu::idle();
    client
        .world_mut()
        .resource_mut::<crate::main_menu::GameEpoch>()
        .0 += 1;
    client.until("the pick phase's line ended", |c| {
        c.world().get_entity(line).is_err()
    });
}

#[test]
fn a_dropped_connection_silences_every_game_sound() {
    let (server, mut client) = match_sounding();
    drop(server);
    client.until("the connection seen lost", |c| c.core().lost);
    assert_silent_for_a_while(&mut client, "every game sound gone with the connection");
}
