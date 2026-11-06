//! One's hero speaks from its own response rules, through the real client against a real
//! server, out of this build's asset cache: a move order is answered by the hero's own move
//! rule with one of its lines, a kill by its kill rule, a line's own `respeakdelay` (the Bounty
//! Rune's 300 s) holds on the match's clock and lets the line be said again once past, and the
//! pack, read from the pick on, is resident before the battle, its first line starting without a
//! frame-time spike.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bota_client_core::{Intent, IssuedOrder};
use bota_proto::{Cheat, CreepSide, HeroId, ModifierSpec, Order, RuneKind, Target};
use dbota_import::voice::concept::{Concept, Fact};
use dbota_import::voice::speaker::Silence;

use super::harness::{Client, MID, Server, scripted_seat};
use super::runes::{SOON, drawn, point_at, spawn_rune, top_spot};
use crate::hero_voice::{Answer, HeroVoice, Line, Said};

fn answers(client: &Client) -> Vec<Answer> {
    client
        .world()
        .resource::<Said>()
        .0
        .iter()
        .cloned()
        .collect()
}

/// The answers to `concept` so far.
fn answers_to(client: &Client, concept: Concept) -> Vec<Answer> {
    answers(client)
        .into_iter()
        .filter(|a| a.query.concept == concept)
        .collect()
}

/// Whether the hero said a line to `concept` whose query's fact of that concept is `value` (any
/// line when `value` is empty): for the other tests of what a hero says.
pub(super) fn spoke(client: &Client, concept: Concept, value: &str) -> bool {
    answers_to(client, concept).iter().any(|a| {
        a.said.is_ok()
            && (value.is_empty()
                || a.query
                    .values()
                    .any(|(fact, v)| fact != Fact::Concept && v == value))
    })
}

/// Every sound player started, with the sound it plays and the mixer's event for it.
type Players = Arc<Mutex<Vec<(Entity, AssetId<AudioSource>, Option<String>)>>>;

/// Records every sound player the client starts from now on.
fn players(client: &mut Client) -> Players {
    let players: Players = Arc::default();
    let into = players.clone();
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>,
              playing: Query<&AudioPlayer>,
              mixer: Res<crate::mixer::Mixer>| {
            let Ok(player) = playing.get(add.entity) else {
                return;
            };
            let event = mixer
                .event_at(add.entity)
                .map(|(event, _)| event.to_owned());
            into.lock()
                .expect("players")
                .push((add.entity, player.0.id(), event));
        },
    );
    players
}

/// Waits for the hero to say a line to `concept` ([`spoke`]); a timeout names what it was asked
/// and answered meanwhile.
pub(super) fn wait_spoken(client: &mut Client, concept: Concept, value: &str) {
    let deadline = Instant::now() + SOON;
    while !spoke(client, concept, value) {
        let asked: Vec<_> = answers(client)
            .iter()
            .map(|a| (a.tick, a.query.concept, a.said.as_ref().err().copied()))
            .collect();
        let voice = client.world().resource::<HeroVoice>();
        let state = match &voice.pack {
            crate::hero_voice::Pack::None => "no hero",
            crate::hero_voice::Pack::Loading { .. } => "loading",
            crate::hero_voice::Pack::Resident(_) => "resident",
            crate::hero_voice::Pack::Failed { .. } => "failed",
        };
        let shown = client.world().resource::<crate::loading::Loading>().done();
        assert!(
            Instant::now() < deadline,
            "no line to {concept:?} {value:?} within {SOON:?}; answers: {asked:?}; pack {state}, \
             shown {shown}, voice setting {}",
            client.world().resource::<crate::config::Settings>().voice
        );
        client.frames(1);
    }
}

/// The line said, checked against the pack the client holds: the rule is the hero's own, the
/// line one of that rule's, and the voice that sounds plays exactly the line's sound, through the
/// mixer as the line's event.
fn checked_line(client: &mut Client, players: &Players, line: &Line) {
    let voice = client.world().resource::<HeroVoice>();
    let resident = voice.resident().expect("the hero's pack resident");
    let rules = &resident.rules;
    let rule = rules
        .rules
        .iter()
        .find(|r| r.name == line.rule)
        .unwrap_or_else(|| panic!("{} is a rule of the pack", line.rule));
    let events: Vec<&str> = rule
        .lines
        .iter()
        .map(|l| rules.events[l.event].name.as_str())
        .collect();
    assert!(
        events.contains(&line.event.as_str()),
        "{} is a line of {}",
        line.event,
        line.rule
    );
    let event = rules
        .events
        .iter()
        .find(|e| e.name == line.event)
        .expect("its event");
    assert!(
        event.sounds.contains(&line.sound),
        "the line's sound is its event's"
    );
    let expected = resident.sounds[line.sound].id();
    let player = line.player.expect("the mixer gave the line a voice");
    let started = players.lock().expect("players").clone();
    let (_, sound, event) = started
        .iter()
        .find(|(entity, _, _)| *entity == player)
        .expect("the line's sound player started");
    assert_eq!(*sound, expected, "the voice plays the line's own sound");
    assert_eq!(
        event.as_deref(),
        Some(line.event.as_str()),
        "as the line's event"
    );
}

/// Waits for the game to show (the hero says nothing before), the battle to begin and the
/// hero's line for it to end.
fn begun(client: &mut Client) {
    client.until("the game shown", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client.cheat(Cheat::StartGame);
    client.until("the battle begun", |c| {
        crate::hero_voice::game_time(c.core()) >= 0.0
    });
    client.frames(3);
    quiet(client);
}

/// Waits until the hero says nothing: its last line is over on the match's clock, which its
/// speaker counts by (the sound itself plays on the wall's).
pub(super) fn quiet(client: &mut Client) {
    client.within(SOON, "the hero quiet", |c| {
        let rate = f64::from(c.core().tick_rate().max(1));
        let now = f64::from(c.tick()) / rate;
        let voice = c.world().resource::<HeroVoice>();
        voice.resident().is_none_or(|r| now >= r.speaking_until())
    });
}

/// Waits for the hero's first line to `concept`, asking again with `ask` every few frames
/// until one is said.
fn first_line(
    client: &mut Client,
    concept: Concept,
    mut ask: impl FnMut(&mut Client, u32),
) -> Line {
    quiet(client);
    let mut frames = 0u32;
    let mut said = None;
    client.within(SOON, "a line answering the order", |c| {
        said = answers_to(c, concept).into_iter().find_map(|a| a.said.ok());
        if said.as_ref().is_some_and(|l| l.player.is_some()) {
            return true;
        }
        if said.is_none() && frames.is_multiple_of(20) {
            ask(c, frames / 20);
        }
        frames += 1;
        false
    });
    said.expect("a line")
}

/// A walk order, to one of two spots by `n`.
fn walk(client: &mut Client, n: u32) {
    let x = if n.is_multiple_of(2) { 7400.0 } else { 7800.0 };
    client.core_mut().act(Intent::MoveTo((x, 9600.0)));
}

/// `hero` joined alone, the battle begun, standing mid, its sound players recorded.
fn alone(hero: HeroId) -> (Server, Client, Players) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, hero, &[]);
    let players = players(&mut client);
    begun(&mut client);
    client.stand_at(MID);
    (server, client, players)
}

fn answers_a_move_order_from_its_own_rule(
    hero: HeroId,
    rule: &str,
    criteria: [(&str, &str, &str); 2],
) {
    let (_server, mut client, players) = alone(hero);
    let line = first_line(&mut client, Concept::Move, walk);
    assert_eq!(line.rule, rule);
    let matched: Vec<(&str, &str, &str)> = line
        .criteria
        .iter()
        .map(|(n, k, e)| (n.as_str(), k.as_str(), e.as_str()))
        .collect();
    assert_eq!(matched, criteria);
    // The concept's criterion weighs 1 (the default), the hero's own 5.
    assert_eq!(line.score, 6.0);
    checked_line(&mut client, &players, &line);
}

#[test]
fn pudge_answers_a_move_order_from_his_own_move_rule() {
    answers_a_move_order_from_its_own_rule(
        HeroId::PUDGE,
        "pudge_Move",
        [
            ("Move", "Concept", "TLK_DOTA_MOVE"),
            ("Ispudge", "classname", "npc_dota_hero_pudge"),
        ],
    );
}

#[test]
fn shadow_fiend_answers_a_move_order_from_his_own_move_rule() {
    answers_a_move_order_from_its_own_rule(
        HeroId::SHADOW_FIEND,
        "nevermore_Move",
        [
            ("Move", "Concept", "TLK_DOTA_MOVE"),
            ("Isnevermore", "classname", "npc_dota_hero_Nevermore"),
        ],
    );
}

/// An enemy Pudge, held by a seat with no front end, that comes to `at` and is made frail (its
/// health cut to the least) once the flag returned is set.
fn frail_foe(server: &Server, at: bota_proto::Vec2) -> Arc<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let come = Arc::new(AtomicBool::new(false));
    let called = come.clone();
    let mut came = false;
    scripted_seat(server, HeroId::PUDGE, move |session| {
        if came || !called.load(Ordering::SeqCst) || session.own_hero().is_none() {
            return Vec::new();
        }
        came = true;
        let frail = ModifierSpec {
            max_hp: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        };
        [
            Order::Cheat {
                cheat: Cheat::Teleport { at },
            },
            Order::Cheat {
                cheat: Cheat::ApplyModifier {
                    target: Target::None,
                    spec: frail,
                    ticks: 100_000,
                },
            },
            Order::Move {
                target: Target::None,
            },
        ]
        .into_iter()
        .map(IssuedOrder::hero)
        .collect()
    });
    come
}

/// Shadow Fiend kills an enemy Pudge, the match's first hero kill: his first-blood rule answers
/// (its weight outscores his plain kill rule's), and the query names the hero killed. The first
/// attack order, at the foe far off, is answered first, so that the killing one falls in the
/// attack's own cooldown and the hero is quiet when the kill comes.
#[test]
fn a_first_kill_is_answered_by_the_first_blood_rule() {
    let server = Server::start(2, &["--runes", "none"]);
    let victim_at = bota_proto::Vec2::from_ints(MID.0 + 250, MID.1);
    let come = frail_foe(&server, victim_at);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let players = players(&mut client);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    begun(&mut client);
    client.stand_at(MID);
    fn foe(c: &Client) -> Option<&bota_proto::UnitView> {
        let view = c.core().view()?;
        let mine = view.player(c.core().my_slot?)?.team;
        let player = view.players.iter().find(|p| p.team != mine)?;
        view.unit(player.unit?)
    }
    client.until("the foe in sight", |c| foe(c).is_some());
    let attack = |c: &mut Client| {
        let foe = foe(c).expect("the foe");
        let (at, unit) = ((foe.pos.x.to_f32(), foe.pos.y.to_f32()), Some(foe.id));
        c.core_mut().act(Intent::AttackAt { at, unit });
    };
    first_line(&mut client, Concept::Attack, |c, _| attack(c));
    client.core_mut().act(Intent::Stop);
    quiet(&mut client);
    come.store(true, std::sync::atomic::Ordering::SeqCst);
    client.until("the foe beside the hero", |c| {
        foe(c).is_some_and(|u| u.pos.within(victim_at, bota_proto::Fixed::from_int(100)))
    });
    attack(&mut client);
    let mut kill = None;
    client.within(SOON, "the kill answered", |c| {
        kill = answers_to(c, Concept::Kill).into_iter().next();
        kill.as_ref()
            .is_some_and(|a| a.said.as_ref().map_or(true, |l| l.player.is_some()))
    });
    let kill = kill.expect("an answer");
    assert_eq!(
        kill.query.value("targetclassname"),
        Some("npc_dota_hero_pudge")
    );
    assert_eq!(kill.query.value("firstblood"), Some("1"));
    let line = kill.said.expect("a line");
    assert_eq!(line.rule, "nevermore_KillIsFirstBlood");
    // Kill 1, Isnevermore 5, IsFirstBlood 50: his plain kill rule scores 6.
    assert_eq!(line.score, 56.0);
    assert!(
        line.criteria
            .iter()
            .any(|(name, key, expr)| name == "IsFirstBlood" && key == "firstblood" && expr == "1"),
        "{:?}",
        line.criteria
    );
    checked_line(&mut client, &players, &line);
}

/// Takes a Bounty Rune spawned at the top river spot: the hero stands beside it, and a right
/// click on it takes it.
fn take_bounty(client: &mut Client) {
    quiet(client);
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(client, RuneKind::Bounty, spot);
    client.within(SOON, "the Bounty Rune drawn", |c| {
        drawn(c, RuneKind::Bounty).is_some()
    });
    point_at(client, (spot.0 as f32, spot.1 as f32));
    client.click(MouseButton::Right);
}

/// The answers to the Bounty Rune, waiting for the `n`th.
fn bounty_answer(client: &mut Client, n: usize) -> Answer {
    let bounty = |c: &Client| -> Vec<Answer> {
        answers_to(c, Concept::Powerup)
            .into_iter()
            .filter(|a| a.query.value("poweruptype") == Some("DOTA_RUNE_BOUNTY"))
            .collect()
    };
    let deadline = Instant::now() + SOON;
    while bounty(client).len() < n {
        let asked: Vec<_> = answers(client)
            .iter()
            .map(|a| (a.tick, a.query.concept, a.said.as_ref().err().copied()))
            .collect();
        let voice = client.world().resource::<HeroVoice>();
        let state = (voice.hero().map(str::to_owned), voice.resident().is_some());
        let done = client.world().resource::<crate::loading::Loading>().done();
        assert!(
            Instant::now() < deadline,
            "bounty answer {n} not within {SOON:?}; answers: {asked:?}, voice {state:?}, shown {done}"
        );
        client.frames(1);
    }
    bounty(client).swap_remove(n - 1)
}

/// The Bounty Rune's line waits its own 300 s `respeakdelay` (the pack's, from the install's
/// rules) on the match's clock: a second rune inside it gets no line, a third past it does.
#[test]
fn a_lines_own_respeak_delay_holds_on_the_match_clock() {
    let server = Server::start(1, &["--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    begun(&mut client);
    take_bounty(&mut client);
    let first = bounty_answer(&mut client, 1);
    let line = first.said.as_ref().expect("the first bounty answered");
    assert_eq!(line.rule, "nevermore_PowerupIsPowerupBounty");
    let respeak = {
        let resident = client
            .world()
            .resource::<HeroVoice>()
            .resident()
            .expect("resident");
        let rule = resident
            .rules
            .rules
            .iter()
            .find(|r| r.name == line.rule)
            .expect("its rule");
        rule.lines[0].respeak
    };
    assert_eq!(respeak, 300.0, "the install's respeakdelay of the line");
    let rate = u32::from(client.core().tick_rate());
    take_bounty(&mut client);
    let second = bounty_answer(&mut client, 2);
    assert!(second.tick - first.tick < 300 * rate);
    assert_eq!(second.said.as_ref().err(), Some(&Silence::NoLine));
    let past = first.tick + 300 * rate;
    client.until("the match clock past the respeak delay", |c| {
        c.tick() >= past
    });
    take_bounty(&mut client);
    let third = bounty_answer(&mut client, 3);
    assert!(third.tick >= past);
    assert_eq!(
        third.said.as_ref().map(|l| l.rule.as_str()),
        Ok(line.rule.as_str())
    );
}

/// The hero's pack, read from the pick on, is resident on the battle's first frame of play,
/// and the frames its first line starts on cost no more than the frames around them do: the
/// line plays from what is resident, decoded as it plays.
#[test]
fn the_pack_is_resident_from_the_pick_and_the_first_line_costs_no_frame() {
    let server = Server::start(1, &["--tactical-secs", "5"]);
    let mut client = Client::pick(&server, HeroId::PUDGE, &[]);
    client.until("the hero on the map", |c| c.core().ears().is_some());
    let voice = client.world().resource::<HeroVoice>();
    let resident = voice
        .resident()
        .expect("the pack resident on the first frame of play");
    assert_eq!(resident.hero, "pudge");
    client.until("the game shown", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    let started = |c: &Client| {
        answers(c)
            .iter()
            .any(|a| matches!(&a.said, Ok(l) if l.player.is_some()))
    };
    let deadline = Instant::now() + SOON;
    let mut frames = Vec::new();
    let mut line_at = None;
    while line_at.is_none_or(|at| frames.len() < at + 120) {
        assert!(Instant::now() < deadline, "no line said within {SOON:?}");
        // The spawn line, else a walk's.
        if line_at.is_none() && frames.len() >= 120 && frames.len().is_multiple_of(20) {
            walk(&mut client, frames.len() as u32 / 20);
        }
        let began = Instant::now();
        client.frames(1);
        frames.push(began.elapsed());
        if line_at.is_none() && started(&client) {
            line_at = Some(frames.len() - 1);
        }
    }
    let at = line_at.expect("a line");
    let line_frames = [frames[at], frames[at + 1]];
    let mut others: Vec<Duration> = frames
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != at && *i != at + 1)
        .map(|(_, d)| *d)
        .collect();
    others.sort();
    let median = others[others.len() / 2];
    // The machine runs many other jobs: a frame now and then takes a few times the median. A
    // pack read, or a line decoded whole, on the frame would take far longer (Pudge's pack is
    // 2.8 MB of compressed audio).
    let bound = median * 4 + Duration::from_millis(8);
    for (i, frame) in line_frames.iter().enumerate() {
        assert!(
            *frame <= bound,
            "frame {i} of the first line took {frame:?}, the median frame {median:?}"
        );
    }
    println!(
        "first line frames {line_frames:?}, median {median:?}, {} frames",
        frames.len()
    );
}
