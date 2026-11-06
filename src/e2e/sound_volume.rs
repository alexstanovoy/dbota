//! How loud Dota's sound events play: each at the `volume` its sound event file states, so a
//! ranged creep's hit is the 0.6 of its event and not the file's whole level, and no hero's
//! sounds are louder than the rest by being left at full.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::mixer::{Mixer, Owner, Request};

/// What the client sounds `event` at when asked for `gain`.
fn played_at(client: &mut Client, event: &str, gain: f32) -> Option<f32> {
    let world = client.world_mut();
    world.resource_scope(|world, mut mixer: Mut<Mixer>| {
        let mut commands = world.commands();
        let request = Request {
            event,
            owner: Owner::None,
            merge: None,
            at: None,
            gain,
            ..Request::default()
        };
        mixer.start(&mut commands, Duration::ZERO, &request, Handle::default());
        mixer.voice_gain(event)
    })
}

/// The `volume` of each event line of the cache's `sounds/events.txt` (the twelfth word of the
/// line).
fn stated_volumes() -> Vec<(String, f32)> {
    let cache = std::path::PathBuf::from(std::env::var("DBOTA_ASSETS").expect("DBOTA_ASSETS"));
    let text =
        std::fs::read_to_string(cache.join("sounds/events.txt")).expect("the cache's events");
    text.lines()
        .filter(|line| line.starts_with("event "))
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(
                words.len(),
                17,
                "an event line states its volume after its limits: {line}"
            );
            (words[1].to_owned(), words[11].parse().expect("a volume"))
        })
        .collect()
}

/// Events whose files state a volume under 1 (Dota 7.41f's `game_sounds*.vsndevts`).
#[test]
fn events_sound_at_the_volume_of_their_sound_event() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    let stated = [
        ("Creep_Good_Range.ProjectileImpact", 0.6),
        ("Tower.Water.Attack", 0.3),
        ("General.Buy", 0.75),
        ("n_creep_Skeleton.Death", 0.25),
        ("Hero_Pudge.Rot", 1.0),
        // The pings were hand-set to these before the volumes were read from the install.
        ("General.Ping", 0.6),
        ("General.PingWarning", 0.5),
        ("PingWarning.Layer", 0.5),
    ];
    for (event, volume) in stated {
        let gain = played_at(&mut client, event, 1.0).expect("a voice");
        assert!(
            (gain - volume).abs() < 0.01,
            "{event} sounds at {gain}, not {volume}"
        );
    }
}

/// Every sound event the cache knows sounds at the volume it states, for any gain asked.
#[test]
fn every_event_of_the_cache_sounds_at_its_stated_volume_times_the_gain_asked() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    let stated = stated_volumes();
    assert!(
        stated.len() > 100,
        "the cache holds the events: {}",
        stated.len()
    );
    for (event, volume) in stated {
        let gain = played_at(&mut client, &event, 0.5).expect("a voice");
        assert!(
            (gain - 0.5 * volume).abs() < 0.005,
            "{event} sounds at {gain} for 0.5, its volume being {volume}"
        );
    }
}
