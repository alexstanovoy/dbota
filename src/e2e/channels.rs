//! The balance of the channels as the sinks of sounds that are playing hear it: the master volume
//! and the channel's level and mute reach a sound that is already sounding as they reach one that
//! starts, each exactly once, whichever part of the client made the sound.
//!
//! The client under test has no sound device, so every sound that starts is given a sink of an
//! in-memory mixer that plays silence for good (see [`listen`]); the test reads what the sink is
//! told.

use std::time::Duration;

use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::channels::{Channel, Preview, Tuned};
use crate::config::Settings;
use crate::mixer::{Mixer, Request};

/// Gives every sound of a channel that starts from now on a sink of an in-memory mixer, which
/// plays silence endlessly so that the sound stays for the test to read.
pub(super) fn listen(client: &mut Client) {
    let (output, source) = rodio::mixer::mixer(
        2.try_into().expect("two channels"),
        48_000.try_into().expect("a rate"),
    );
    // The mixer's source keeps the sinks connected for as long as the test runs.
    std::mem::forget(source);
    client
        .world_mut()
        .add_observer(move |add: On<Add<Tuned>>, mut commands: Commands| {
            let player = rodio::Player::connect_new(&output);
            player.append(rodio::source::Zero::new(
                2.try_into().expect("two channels"),
                48_000.try_into().expect("a rate"),
            ));
            commands.entity(add.entity).insert(AudioSink::new(player));
        });
}

/// What the sink of a sound is told now; nothing for a sound that has ended (a loop of the
/// ambience ends when its channel is muted, and starts again when it is not).
fn sink_of(client: &Client, sound: Entity) -> f32 {
    let ended = |entity: &EntityRef| entity.contains::<crate::audio_output::Ending>();
    client
        .world()
        .get_entity(sound)
        .ok()
        .filter(|e| !ended(e))
        .map_or(0.0, |entity| {
            entity
                .get::<AudioSink>()
                .expect("a sink in the sound's entity")
                .volume()
                .to_linear()
        })
}

fn set(client: &mut Client, change: impl FnOnce(&mut Settings)) {
    change(&mut client.world_mut().resource_mut::<Settings>());
    client.frames(2);
}

/// The sound's sink follows the master volume and its channel's level and mute, as they are when
/// it starts and as they change while it plays; another channel's balance does not reach it.
pub(super) fn follows_the_balance(client: &mut Client, sound: Entity, channel: Channel) {
    if client.world().get::<AudioSink>(sound).is_none() {
        // The sound started before anyone listened.
        let (output, source) = rodio::mixer::mixer(
            2.try_into().expect("two channels"),
            48_000.try_into().expect("a rate"),
        );
        std::mem::forget(source);
        let player = rodio::Player::connect_new(&output);
        player.append(rodio::source::Zero::new(
            2.try_into().expect("two channels"),
            48_000.try_into().expect("a rate"),
        ));
        std::mem::forget(output);
        client
            .world_mut()
            .entity_mut(sound)
            .insert(AudioSink::new(player));
    }
    let other = Channel::ALL
        .into_iter()
        .find(|other| *other != channel)
        .expect("another channel");
    let wanted = |client: &Client, master: f32, level: f32| -> f32 {
        let tuned = client.world().get::<Tuned>(sound).expect("a tuned sound");
        assert_eq!(tuned.channel, channel, "the sound is of its channel");
        tuned.base * master * level
    };
    let settings = |client: &Client| client.world().resource::<Settings>().clone();
    set(client, |s| {
        s.volume = 0.7;
        s.muted = false;
        for channel in Channel::ALL {
            s.mix.set_level(channel, 1.0);
            s.mix.set_muted(channel, false);
        }
    });
    let close = |got: f32, want: f32, what: &str| {
        assert!(
            (got - want).abs() < 1e-5,
            "{channel:?} {what}: the sink is told {got}, not {want}"
        );
    };
    close(
        sink_of(client, sound),
        wanted(client, 0.7, 1.0),
        "at the master",
    );
    set(client, |s| s.mix.set_level(channel, 0.4));
    close(
        sink_of(client, sound),
        wanted(client, 0.7, 0.4),
        "after its level moved",
    );
    set(client, |s| s.mix.set_level(other, 0.1));
    close(
        sink_of(client, sound),
        wanted(client, 0.7, 0.4),
        "after another channel's level moved",
    );
    set(client, |s| s.volume = 0.5);
    close(
        sink_of(client, sound),
        wanted(client, 0.5, 0.4),
        "after the master moved (once)",
    );
    set(client, |s| s.mix.set_muted(channel, true));
    close(sink_of(client, sound), 0.0, "after its mute");
    set(client, |s| s.mix.set_muted(channel, false));
    if client
        .world()
        .get_entity(sound)
        .is_ok_and(|entity| entity.contains::<crate::audio_output::Ending>())
        || client.world().get_entity(sound).is_err()
    {
        // A loop the mute ended: the next one is the soundscape's to start, at the balance.
        return;
    }
    close(
        sink_of(client, sound),
        wanted(client, 0.5, 0.4),
        "after its unmute",
    );
    set(client, |s| s.muted = true);
    close(sink_of(client, sound), 0.0, "after mute-all");
    assert!(settings(client).muted);
    set(client, |s| s.muted = false);
}

/// The sound of the last voice the mixer started.
fn newest_voice(client: &mut Client, event: &str, channel: Channel, ambience: bool) -> Entity {
    let world = client.world_mut();
    let now = world.resource::<Time>().elapsed();
    world.resource_scope(|world, mut mixer: Mut<Mixer>| {
        let mut commands = world.commands();
        let request = Request {
            event,
            channel,
            ..Request::default()
        };
        if ambience {
            mixer.start_ambience(&mut commands, now, &request, Handle::default());
        } else {
            mixer.start(&mut commands, now, &request, Handle::default());
        }
    });
    world.flush();
    let mut tuned = world.query::<(Entity, &Tuned)>();
    tuned
        .iter(world)
        .filter(|(_, tuned)| tuned.channel == channel)
        .map(|(entity, _)| entity)
        .last()
        .expect("the voice's sound")
}

/// Effects, hero voices and ambience that the mixer sounds follow the balance, long ones and
/// short ones alike.
#[test]
fn the_mixers_voices_follow_the_balance_while_they_sound() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    listen(&mut client);
    for (channel, event, ambience) in [
        (Channel::Effects, "General.Buy", false),
        (Channel::Voices, "General.Ping", false),
        (Channel::Ambience, "Tower.Water.Attack", true),
    ] {
        let sound = newest_voice(&mut client, event, channel, ambience);
        follows_the_balance(&mut client, sound, channel);
    }
}

/// A line the announcer is already speaking falls silent with the announcer's mute and follows its
/// fader.
#[test]
fn the_announcers_line_follows_the_balance_while_it_is_spoken() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    listen(&mut client);
    let now = client.world().resource::<Time>().elapsed_secs();
    client
        .world_mut()
        .resource_mut::<crate::voice::Announcer>()
        .push(Some(Handle::default()), 0.9, now);
    let mut line = None;
    client.within(
        Duration::from_secs(30),
        "an announcer's line sounding",
        |c| {
            let world = c.world_mut();
            line = world
                .query::<(Entity, &Tuned)>()
                .iter(world)
                .find(|(_, tuned)| tuned.channel == Channel::Announcer)
                .map(|(entity, _)| entity);
            line.is_some()
        },
    );
    let line = line.expect("the announcer's line started");
    follows_the_balance(&mut client, line, Channel::Announcer);
}

/// A try of a fader sounds in the channel's balance too: its mute silences the sample that is
/// playing.
#[test]
fn a_preview_that_is_playing_follows_the_balance() {
    let mut client = super::menu::local_menu();
    listen(&mut client);
    client
        .world_mut()
        .write_message(Preview(Channel::Interface));
    client.frames(3);
    let world = client.world_mut();
    let sample = world
        .query::<(Entity, &Tuned)>()
        .iter(world)
        .find(|(_, tuned)| tuned.channel == Channel::Interface)
        .map(|(entity, _)| entity)
        .expect("the interface's click sounds");
    follows_the_balance(&mut client, sample, Channel::Interface);
}

/// Every sound the client makes belongs to a channel: a player with none would sound outside the
/// balance, and out of the master volume.
#[test]
fn every_sound_player_of_a_match_carries_its_channel() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client
        .core_mut()
        .act(bota_client_core::Intent::MoveTo((7400.0, 9600.0)));
    client.world_mut().write_message(Preview(Channel::Effects));
    for _ in 0..300 {
        client.frames(1);
        let world = client.world_mut();
        let untuned = world
            .query_filtered::<Entity, (With<bevy::audio::AudioPlayer>, Without<Tuned>)>()
            .iter(world)
            .count();
        assert_eq!(untuned, 0, "a sound player without a channel");
    }
}
