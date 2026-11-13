//! The sounds of creeps' attacks, in a real match, as Dota's sound sets give them: a unit's
//! `SoundSet` (`npc_units.txt`) plays `<set>.PreAttack` as a swing begins, `<set>.Attack` as a
//! melee blow lands or a missile leaves, and `<set>.ProjectileImpact` as a missile lands. They
//! are world sounds: heard from wherever the camera is whoever the creeps strike, not for the
//! hero one plays alone. What sounded is read from the audio event log: every sound the mixer
//! started, with the sound event it plays, when, and how loud.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{AudioPlayer, PlaybackSettings};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, EntityId, HeroId, UnitKind, Vec2 as At};

use super::harness::{Client, MID, Server};
use crate::mixer::Mixer;

/// How far from one's own hero the fight is: out of every creep's reach.
const APART: i32 = 3500;
/// The game clock one frame of these tests advances by.
const FRAME: Duration = Duration::from_millis(16);

/// A sound the mixer started.
#[derive(Clone, Debug)]
struct Played {
    /// The clock of the client when it started.
    at: Duration,
    /// The Dota sound event it plays.
    event: String,
    /// Its volume as started, before the player's volume.
    gain: f32,
}

type Log = Arc<Mutex<Vec<Played>>>;

/// The log of every sound the mixer starts from now on.
fn audio_log(client: &mut Client) -> Log {
    let log: Log = Arc::default();
    let into = log.clone();
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>,
              settings: Query<&PlaybackSettings>,
              mixer: Res<Mixer>,
              time: Res<Time>| {
            let Some((event, _)) = mixer.event_at(add.entity) else {
                return;
            };
            let gain = settings
                .get(add.entity)
                .map_or(0.0, |s| s.volume.to_linear());
            into.lock().expect("unpoisoned").push(Played {
                at: time.elapsed(),
                event: event.to_owned(),
                gain,
            });
        },
    );
    log
}

/// The sounds of the log that play `event`, in order.
fn of(log: &Log, event: &str) -> Vec<Played> {
    log.lock()
        .expect("unpoisoned")
        .iter()
        .filter(|p| p.event == event)
        .cloned()
        .collect()
}

/// Puts the camera on a point of the world, let go of the hero it followed: the sounds are heard
/// from where the player looks.
fn look_at(client: &mut Client, (x, y): (i32, i32)) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let mut camera = world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("the game's camera");
    camera.focus = bevy::prelude::Vec3::new(x as f32, 0.0, -(y as f32));
}

/// A match with the hero set apart and invulnerable, and the camera over a lane fight of both
/// sides' creeps (one wave of each, facing each other): no sound is of the hero's.
fn lane_fight() -> (Server, Client, Log, (i32, i32)) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Invulnerable { on: true });
    let fight = (MID.0 + APART, MID.1);
    client.cheat(Cheat::CreateCreeps {
        enemy: false,
        at: At::from_ints(fight.0, fight.1),
    });
    client.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: At::from_ints(fight.0 + 500, fight.1),
    });
    look_at(&mut client, fight);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let log = audio_log(&mut client);
    (server, client, log, fight)
}

/// Both sides' melee creeps fighting each other: each whooshes as a swing begins and clashes as
/// it lands, with their own side's sound event, and the hero one plays is not struck.
#[test]
fn melee_creeps_fighting_each_other_whoosh_then_clash() {
    let (_server, mut client, log, _) = lane_fight();
    client.until("both sides' melee blows sounded", |_| {
        of(&log, "Creep_Good_Melee.Attack").len() >= 3
            && of(&log, "Creep_Bad_Melee.Attack").len() >= 3
    });
    for side in ["Creep_Good_Melee", "Creep_Bad_Melee"] {
        let swings = of(&log, &format!("{side}.PreAttack"));
        let blows = of(&log, &format!("{side}.Attack"));
        let first_swing = swings.first().unwrap_or_else(|| panic!("no {side} swing"));
        let first_blow = blows.first().expect("a blow");
        assert!(
            first_swing.at + FRAME <= first_blow.at,
            "{side}: the swing sounds before its blow lands ({first_swing:?} then {first_blow:?})"
        );
        assert!(
            blows.iter().all(|blow| blow.gain > 0.0),
            "{side}: every blow is audible: {blows:?}"
        );
    }
}

/// Both sides' ranged creeps: a missile sounds as it leaves (while one is in flight) and again
/// as it lands, in that order.
#[test]
fn ranged_creeps_sound_their_missiles_leaving_and_landing() {
    let (_server, mut client, log, _) = lane_fight();
    let mut seen = 0;
    client.until("both sides' ranged launches and landings sounded", |c| {
        let launches =
            of(&log, "Creep_Good_Range.Attack").len() + of(&log, "Creep_Bad_Range.Attack").len();
        if launches > seen {
            seen = launches;
            let view = c.view();
            assert!(
                view.projectiles.iter().any(|p| {
                    p.source
                        .and_then(|id| view.unit(id))
                        .is_some_and(|u| u.kind == UnitKind::CreepRanged)
                }),
                "a launch sounds while a ranged creep's missile is in the air"
            );
        }
        ["Good", "Bad"].iter().all(|side| {
            !of(&log, &format!("Creep_{side}_Range.Attack")).is_empty()
                && !of(&log, &format!("Creep_{side}_Range.ProjectileImpact")).is_empty()
        })
    });
    for side in ["Good", "Bad"] {
        let launch = of(&log, &format!("Creep_{side}_Range.Attack"))[0].clone();
        let landing = of(&log, &format!("Creep_{side}_Range.ProjectileImpact"))[0].clone();
        assert!(
            launch.at + FRAME <= landing.at,
            "{side}: the launch sounds before the landing ({launch:?}, {landing:?})"
        );
    }
}

/// Most creep sounds that may start within one second of the test's clock in a full wave
/// fighting. A creep makes a swing sound and a blow or launch sound once an attack time, and a
/// dozen creeps fight; 26 were measured at the busiest second (189 over 600 frames), the mixer's
/// merge and repeat limits having taken the rest. The bound leaves room above that and is broken
/// by creep sounds that nothing limits.
const MOST_CREEP_SOUNDS_A_SECOND: usize = 40;

/// A full wave fighting fills the air with blows, but not to noise: by the voice limits the
/// install's sound events give (a repeat within 50 ms and 100 units is not played) and the
/// merge of blows on one unit, no more than [`MOST_CREEP_SOUNDS_A_SECOND`] creep sounds start in
/// any second, and none starts louder than the event's own volume.
#[test]
fn a_full_wave_fighting_does_not_stack_into_noise() {
    let (_server, mut client, log, _) = lane_fight();
    client.frames(600);
    let played = log.lock().expect("unpoisoned").clone();
    assert!(
        played.len() >= 20,
        "a wave fighting is heard ({} sounds)",
        played.len()
    );
    let busiest = played
        .iter()
        .map(|p| {
            played
                .iter()
                .filter(|q| q.at >= p.at && q.at < p.at + Duration::from_secs(1))
                .count()
        })
        .max()
        .expect("sounds");
    assert!(
        busiest <= MOST_CREEP_SOUNDS_A_SECOND,
        "{busiest} creep sounds in a second is a roar"
    );
    let loudest = played.iter().map(|p| p.gain).fold(0.0, f32::max);
    assert!(
        loudest <= 1.0,
        "no creep sound is started above full volume"
    );
}

/// Creeps beating a hero are heard whichever hero it is: one's own, an ally's or an enemy's.
fn blows_on_hero(own: bool, enemy_hero: bool) -> usize {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Wtf { on: true });
    client.cheat(Cheat::AllVision { on: true });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let me = client.me().id;
    let (at, victim): ((i32, i32), Option<EntityId>) = if own {
        (MID, None)
    } else {
        let at = (MID.0 + APART, MID.1);
        client.cheat(Cheat::CreateHero {
            hero: HeroId::PUDGE,
            enemy: enemy_hero,
            at: At::from_ints(at.0, at.1),
        });
        client.until("the other hero stands", |c| {
            c.view()
                .units
                .iter()
                .any(|u| u.kind == UnitKind::Hero && u.id != me)
        });
        let id = client
            .view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Hero && u.id != me)
            .map(|u| u.id);
        (at, id)
    };
    // The creeps of the side opposed to the one struck.
    let creeps_are_enemies_of_seat = if own { true } else { !enemy_hero };
    for dy in [-100, 100] {
        client.cheat(Cheat::CreateCreeps {
            enemy: creeps_are_enemies_of_seat,
            at: At::from_ints(at.0 + 300, at.1 + dy),
        });
    }
    look_at(&mut client, at);
    let log = audio_log(&mut client);
    client.until("the creeps hurt the hero", |c| {
        let hp = |id: EntityId| c.view().unit(id).map(|u| (u.hp, u.max_hp));
        match victim {
            Some(id) => hp(id).is_some_and(|(hp, full)| hp < full),
            None => c.me().hp < c.me().max_hp,
        }
    });
    client.frames(120);
    of(&log, "Creep_Good_Melee.Attack").len() + of(&log, "Creep_Bad_Melee.Attack").len()
}

#[test]
fn creeps_beating_an_allied_hero_are_heard() {
    assert!(blows_on_hero(false, false) > 0);
}

#[test]
fn creeps_beating_an_enemy_hero_are_heard() {
    assert!(blows_on_hero(false, true) > 0);
}

#[test]
fn creeps_beating_ones_own_hero_are_heard() {
    assert!(blows_on_hero(true, false) > 0);
}

/// Siege creeps come with the eleventh wave: the game is played up to it, then both sides' waves
/// are made to meet. Each side's catapult sounds as its missile leaves and as it lands.
#[test]
fn siege_creeps_sound_their_missiles_leaving_and_landing() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::StartGame);
    let wave_eleven = client.core().pregame_ticks() + 10 * bota_core::rules::WAVE_PERIOD_TICKS;
    client.until("the eleventh wave's time", |c| {
        c.view().game_tick() >= wave_eleven + 10
    });
    let fight = (MID.0 + APART, MID.1);
    for (enemy, dx) in [(false, 0), (true, 500)] {
        client.cheat(Cheat::CreateCreeps {
            enemy,
            at: At::from_ints(fight.0 + dx, fight.1),
        });
    }
    look_at(&mut client, fight);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let log = audio_log(&mut client);
    client.until("both sides' catapults launched and landed", |_| {
        ["Good", "Bad"].iter().all(|side| {
            !of(&log, &format!("Creep_{side}_Engine.Attack")).is_empty()
                && !of(&log, &format!("Creep_{side}_Engine.ProjectileImpact")).is_empty()
        })
    });
    for side in ["Good", "Bad"] {
        let launch = of(&log, &format!("Creep_{side}_Engine.Attack"))[0].clone();
        let landing = of(&log, &format!("Creep_{side}_Engine.ProjectileImpact"))[0].clone();
        assert!(
            launch.at + FRAME <= landing.at,
            "{side}: the launch sounds before the landing ({launch:?}, {landing:?})"
        );
    }
}

/// The sound set the manifest gives a Dota unit.
fn set_of(client: &Client, npc: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .unit_art(npc)
        .and_then(|art| art.sound_set.clone())
        .unwrap_or_else(|| panic!("{npc} has a sound set"))
}

/// A neutral creep of the wanted attack stands up in the match and a hero beside it is attacked:
/// the sounds of its attack, and the set it should sound them through.
fn neutral_attack_sounds(ranged: bool) -> (String, Log) {
    let server = Server::start(1, &["--map", "0", "--seed", "5"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    // Strong enough to stand the blows: a swing at an invulnerable hero is called off.
    client.cheat(Cheat::Levels { count: 24 });
    client.cheat(Cheat::Wtf { on: true });
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::SpawnNeutrals);
    let wanted = |u: &bota_proto::UnitView| {
        u.kind == UnitKind::CreepNeutral
            && u.neutral
                .and_then(bota_core::NeutralKind::of)
                .is_some_and(|k| k.neutral().def.projectile_speed.is_some() == ranged)
    };
    // Neutrals are seen where a hero stands: the hero walks the camps until one has such a
    // creep in sight.
    let mut found = None;
    for camp in bota_core::CAMPS {
        client.stand_at((camp.pos.x.to_int() + 250, camp.pos.y.to_int()));
        client.frames(10);
        found = client.view().units.iter().find(|u| wanted(u)).cloned();
        if found.is_some() {
            break;
        }
    }
    let neutral = found.expect("a camp with a neutral of the wanted attack");
    let npc = crate::units::npc_unit(&neutral).expect("its Dota unit");
    let set = set_of(&client, npc);
    client.stand_at((neutral.pos.x.to_int() + 200, neutral.pos.y.to_int()));
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let log = audio_log(&mut client);
    let wanted_event = format!("{set}.attack");
    client.until("the neutral's attack sounded", |_| {
        log.lock()
            .expect("unpoisoned")
            .iter()
            .any(|p| p.event.to_lowercase() == wanted_event)
    });
    (set, log)
}

#[test]
fn melee_neutrals_whoosh_then_clash_through_their_sound_set() {
    let (set, log) = neutral_attack_sounds(false);
    let events = log.lock().expect("unpoisoned").clone();
    let first = |name: &str| {
        events
            .iter()
            .find(|p| p.event.to_lowercase() == format!("{set}.{name}"))
            .cloned()
    };
    let swing = first("preattack").expect("a swing sounded");
    let blow = first("attack").expect("a blow sounded");
    assert!(swing.at + FRAME <= blow.at, "{swing:?} {blow:?}");
}

#[test]
fn ranged_neutrals_sound_their_missiles_leaving_through_their_sound_set() {
    let (set, log) = neutral_attack_sounds(true);
    assert!(
        log.lock()
            .expect("unpoisoned")
            .iter()
            .any(|p| p.event.to_lowercase() == format!("{set}.attack")),
        "a missile left"
    );
}
