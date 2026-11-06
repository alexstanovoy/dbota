//! The mix of Valve's sounds from the install: how far each event carries (its sound level or
//! `distance_max`), which mix group it is in, what the group's level is, and what the mixer's
//! layers do while the announcer speaks.

use std::collections::HashMap;

use std::time::Duration;

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::mixer::{MIX_MAKEUP_GAIN, Mixer};
use crate::voice::Announcer;

/// One `event` line of the cache's `sounds/events.txt`, read apart from the client's own reader.
struct Line {
    name: String,
    volume: f32,
    level: f32,
    distance_max: Option<f32>,
    group: Option<String>,
}

fn cache(file: &str) -> String {
    let cache = std::path::PathBuf::from(std::env::var("DBOTA_ASSETS").expect("DBOTA_ASSETS"));
    std::fs::read_to_string(cache.join(file)).expect("the cache's file")
}

fn event_lines() -> Vec<Line> {
    cache("sounds/events.txt")
        .lines()
        .filter(|l| l.starts_with("event "))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(w.len(), 17, "{l}");
            let number = |at: usize| w[at].parse::<f32>().expect("a number");
            Line {
                name: w[1].to_owned(),
                volume: number(11),
                level: number(12),
                distance_max: Some(number(13)).filter(|d| *d > 0.0),
                group: Some(w[14].to_owned()).filter(|g| g != "-"),
            }
        })
        .collect()
}

/// The volume of each group in the default mix.
fn group_volumes() -> HashMap<String, f32> {
    cache("sounds/mixer.txt")
        .lines()
        .filter(|l| l.starts_with("group "))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let volume: f32 = w[2].parse().expect("a volume");
            let mute: f32 = w[4].parse().expect("a mute");
            (w[1].to_owned(), volume * (1.0 - mute))
        })
        .collect()
}

fn client() -> (Server, Client) {
    let server = Server::start(1, &[]);
    let client = Client::join(&server, HeroId::PUDGE, &[]);
    (server, client)
}

/// Every event carries as far as Dota's data says: `distance_max`, else the reach of its sound
/// level (81 dB, 2500 units, 20 log10 per decade), bounded by the client's documented maximum
/// reach. This approximation falls linearly to nothing there and sounds as loud as its volume
/// times its group's level times the makeup gain.
#[test]
fn every_event_carries_and_sounds_as_its_sound_level_and_group_say() {
    let (_server, client) = client();
    let mixer = client.world().resource::<Mixer>();
    let volumes = group_volumes();
    let lines = event_lines();
    assert!(lines.len() > 100, "{}", lines.len());
    for line in &lines {
        let reach = line.distance_max.unwrap_or_else(|| {
            (2500.0 * 10f32.powf((line.level - 81.0) / 20.0)).min(crate::attenuation::LONGEST_REACH)
        });
        let group = line
            .group
            .as_ref()
            .map_or(1.0, |g| *volumes.get(g).unwrap_or(&1.0));
        for distance in [0.0, 300.0, 1000.0, 2000.0, 3500.0, 20_000.0] {
            let falloff = (1.0 - distance / reach).clamp(0.0, 1.0);
            let expected = (line.volume * falloff * group * MIX_MAKEUP_GAIN).clamp(0.0, 1.0);
            let got = mixer.gain_for(&line.name, (distance, 0.0), (0.0, 0.0));
            assert!(
                (got - expected).abs() < 1e-3,
                "{} at {distance}: {got}, not {expected} (reach {reach}, group {:?})",
                line.name,
                line.group
            );
        }
    }
}

/// A sample of Dota's events in the groups its mixer puts them in: by the path of their files,
/// the first group of the mixer's file that matches, or by the group the event names.
#[test]
fn events_are_in_the_mix_groups_valves_mixer_gives_them() {
    let lines = event_lines();
    let group_of = |name: &str| {
        lines
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("{name} in the cache"))
            .group
            .clone()
    };
    for (event, group) in [
        // By the path of its files: shared creep sounds, though `weapons` comes later in the file.
        ("Creep_Good_Melee.Attack", "Creeps"),
        // Damage to the player's own hero.
        ("Tower.HeroImpact", "PlayerDamage"),
        ("General.Buy", "UI"),
        ("DOTA_Item.BlinkDagger.Activate", "Weapons"),
        ("Hero_Pudge.Rot", "Weapons"),
        // By the group the event names.
        ("General.Ping", "Ping"),
        ("Creep_Good_Range.ProjectileImpact", "Creeps"),
        ("Hero_Nevermore.Shadowraze", "Weapons"),
    ] {
        assert_eq!(group_of(event).as_deref(), Some(group), "{event}");
    }
}

/// The ping levels as the client had them set by hand before the mixer was read from the install.
#[test]
fn the_ping_levels_stay_where_they_were() {
    let (_server, client) = client();
    let mixer = client.world().resource::<Mixer>();
    for (event, level) in [
        ("General.Ping", 0.6),
        ("General.PingWarning", 0.5),
        ("PingWarning.Layer", 0.5),
    ] {
        let got = mixer.gain_for(event, (0.0, 0.0), (0.0, 0.0));
        assert!((got - level).abs() < 1e-3, "{event}: {got}, not {level}");
    }
}

/// The install's `Announcer` layer ducks the world's weapons while the announcer speaks, through
/// the game's own `speak` system: lines called on the announcer, one frame of a tenth of a second
/// at a time, and let go over the layer's release after they stop.
#[test]
fn the_announcer_ducks_the_weapons_of_the_world_in_a_client() {
    let (_server, mut client) = client();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    let weapons = "DOTA_Item.BlinkDagger.Activate";
    let beside = |client: &Client| {
        client
            .world()
            .resource::<Mixer>()
            .gain_for(weapons, (0.0, 0.0), (0.0, 0.0))
    };
    let before = beside(&client);
    let mut ducked = false;
    for _ in 0..100 {
        let now = client.world().resource::<Time>().elapsed_secs();
        client
            .world_mut()
            .resource_mut::<Announcer>()
            .push(Some(Handle::default()), 0.8, now);
        client.frames(1);
        if beside(&client) < before * 0.8 {
            ducked = true;
            break;
        }
    }
    assert!(
        ducked,
        "the announcer speaking ducked nothing in ten seconds of game time"
    );
    // The lines end: whatever still plays is ended, and the layer lets go over its release.
    let players: Vec<Entity> = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(world)
            .collect()
    };
    for player in players {
        client.world_mut().despawn(player);
    }
    client
        .world_mut()
        .resource_mut::<Announcer>()
        .waiting_clear();
    for _ in 0..100 {
        client.frames(1);
        if beside(&client) > before * 0.99 {
            return;
        }
    }
    panic!("the weapons never came back after the announcer fell silent");
}

/// The mixer file of the install is what the client's rules assume: the groups in Valve's order
/// (Creeps before Weapons, so shared creep sounds are Creeps'), the default mix at sound level
/// factor 1, and the Announcer layer triggered by the Announcer group.
#[test]
fn the_installs_mixer_has_the_order_levels_and_trigger_the_client_relies_on() {
    let text = cache("sounds/mixer.txt");
    let groups: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("group "))
        .map(|l| l.split_whitespace().nth(1).expect("a name"))
        .collect();
    let at = |name: &str| groups.iter().position(|g| *g == name).expect(name);
    assert!(at("Creeps") < at("Weapons"), "{groups:?}");
    assert!(
        text.lines()
            .filter(|l| l.starts_with("group "))
            .all(|l| l.split_whitespace().nth(3) == Some("1.000")),
        "the default mix gives a group a sound level factor but 1"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("layer Announcer Announcer ")),
        "the Announcer layer is triggered by the Announcer group"
    );
}
