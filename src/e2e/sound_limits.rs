//! The voices of the world as Dota's sound events bound them, in a real match: the blows of
//! creeps on one unit sound once for each half second, not each in full, and a toggled ability's
//! looping sound lives exactly as long as the toggle is on.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, HeroId, Vec2 as At};

use super::harness::{Client, MID, Server};

/// The voices sounding now whose sound file's path holds `part`.
fn voices(client: &mut Client, part: &str) -> usize {
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

/// The instants the client starts a sound whose cache path holds `part`.
fn listen(client: &mut Client, part: &'static str) -> Arc<Mutex<Vec<Instant>>> {
    let starts = Arc::new(Mutex::new(Vec::new()));
    let into = starts.clone();
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>, players: Query<&AudioPlayer>, server: Res<AssetServer>| {
            let path = players
                .get(add.entity)
                .ok()
                .and_then(|p| server.get_path(p.0.id()))
                .map(|p| p.to_string());
            if path.is_some_and(|p| p.contains(part)) {
                into.lock().expect("unpoisoned").push(Instant::now());
            }
        },
    );
    starts
}

/// Many melee creeps strike one hero for seconds: their blows sound once for each half second
/// at most (the window in which blows on one unit merge), and some do sound.
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
        .map(|pair| pair[1].duration_since(pair[0]))
        .min()
        .expect("two blows");
    // Slack for the pace of frames: the merge window is 0.5 s of the game's clock.
    assert!(
        tightest >= Duration::from_millis(400),
        "two blows on one hero sounded {tightest:?} apart"
    );
}

/// Pudge's Rot toggled on, off and on: its looping sound is alive exactly while the toggle is on,
/// one voice and never two.
#[test]
fn rot_sounds_one_loop_while_it_is_on_and_none_after_it_is_turned_off() {
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
    let toggle = |client: &mut Client| {
        client.core_mut().act(Intent::Press {
            slot: Slot::Ability(slot),
            ctrl: false,
        });
    };
    toggle(&mut client);
    client.until("Rot's loop sounding", |c| voices(c, "pudge/rot") == 1);
    client.frames(30);
    assert_eq!(voices(&mut client, "pudge/rot"), 1, "Rot's loop stacked");
    toggle(&mut client);
    client.until("Rot's loop stopped with Rot", |c| {
        voices(c, "pudge/rot") == 0
    });
    toggle(&mut client);
    client.until("Rot's loop sounding again", |c| voices(c, "pudge/rot") >= 1);
    client.frames(30);
    assert_eq!(voices(&mut client, "pudge/rot"), 1, "Rot's loop stacked");
}
