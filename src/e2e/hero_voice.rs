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
use bota_proto::{Cheat, CreepSide, HeroId, Order, RuneKind, Target};
use dbota_import::voice::concept::{Concept, Fact};
use dbota_import::voice::speaker::Silence;

use super::harness::{Client, MID, Server, scripted_seat};
use super::runes::{drawn, point_at, spawn_rune, top_spot};
use crate::hero_voice::{Answer, HeroVoice, Line, Said};

/// How long a wait on the match may take before the test calls it stuck: a hang guard only, the
/// waits are on the match's own state (a loaded machine runs it slowly).
const HANG: Duration = Duration::from_secs(240);

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
        move |add: On<Add<AudioPlayer>>,
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
    let deadline = Instant::now() + HANG;
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
            "no line to {concept:?} {value:?} within {HANG:?}; answers: {asked:?}; pack {state}, \
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
    client.within(HANG, "the hero quiet", |c| {
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
    client.within(HANG, "a line answering the order", |c| {
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

/// What the enemy Pudge of [`foe_seat`] is told next.
enum Send {
    /// Stand up at its fountain (`-respawn`).
    Respawn,
    /// Stand at a spot.
    To(bota_proto::Vec2),
}

/// The queue of what [`foe_seat`]'s Pudge does next.
type Sends = Arc<Mutex<std::collections::VecDeque<Send>>>;

/// An enemy Pudge, held by a seat with no front end, doing what is pushed to the queue returned,
/// one thing a view, standing still between.
fn foe_seat(server: &Server) -> Sends {
    let sends: Sends = Arc::default();
    let queue = sends.clone();
    scripted_seat(server, HeroId::PUDGE, move |session| {
        if session.own_hero().is_none() {
            return Vec::new();
        }
        let next = queue.lock().expect("the foe's queue").pop_front();
        let orders = match next {
            None => return Vec::new(),
            Some(Send::Respawn) => vec![Order::Cheat {
                cheat: Cheat::Respawn,
            }],
            Some(Send::To(at)) => vec![
                Order::Cheat {
                    cheat: Cheat::Teleport { at },
                },
                Order::Move {
                    target: Target::None,
                },
            ],
        };
        orders.into_iter().map(IssuedOrder::hero).collect()
    });
    sends
}

/// The enemy seat's scoreboard row.
fn foe_player(c: &Client) -> Option<&bota_proto::PlayerView> {
    let view = c.core().view()?;
    let mine = view.player(c.core().my_slot?)?.team;
    view.players.iter().find(|p| p.team != mine)
}

/// The enemy hero, as the view shows it.
fn foe(c: &Client) -> Option<&bota_proto::UnitView> {
    c.core().view()?.unit(foe_player(c)?.unit?)
}

/// Sends the foe to `at` (standing it up first when it lies dead) and waits until it stands
/// there.
fn send_foe(client: &mut Client, sends: &Sends, at: bota_proto::Vec2) {
    if foe_player(client).is_none_or(|p| p.respawn_left > 0) {
        sends
            .lock()
            .expect("the foe's queue")
            .push_back(Send::Respawn);
        client.until("the foe standing", |c| {
            foe_player(c).is_some_and(|p| p.respawn_left == 0) && foe(c).is_some()
        });
    }
    sends
        .lock()
        .expect("the foe's queue")
        .push_back(Send::To(at));
    client.until("the foe at its spot", |c| {
        foe(c).is_some_and(|u| u.pos.within(at, bota_proto::Fixed::from_int(100)))
    });
}

/// Shadow Fiend (Dire) and an enemy Pudge on the map without creeps, everything in sight, the
/// battle begun; `before` runs first. Shadow Fiend then kills Pudge: the first attack order, at
/// the foe far off, is answered first, so that the killing one falls in the attack's own
/// cooldown and the hero is quiet when the kill comes. The answer to the kill, and the players.
fn kill_scene(before: impl FnOnce(&mut Client, &Sends)) -> (Client, Players, Answer) {
    let server = Server::start(2, &["--map", "0", "--runes", "none"]);
    let sends = foe_seat(&server);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let players = players(&mut client);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    begun(&mut client);
    // Strong enough to kill the foe in a few blows.
    client.cheat(Cheat::Levels { count: 24 });
    client.stand_at(MID);
    before(&mut client, &sends);
    send_foe(
        &mut client,
        &sends,
        bota_proto::Vec2::from_ints(MID.0 - 2000, MID.1),
    );
    let attack = |c: &mut Client| {
        let foe = foe(c).expect("the foe");
        let (at, unit) = ((foe.pos.x.to_f32(), foe.pos.y.to_f32()), Some(foe.id));
        c.core_mut().act(Intent::AttackAt { at, unit });
    };
    first_line(&mut client, Concept::Attack, |c, _| attack(c));
    client.core_mut().act(Intent::Stop);
    quiet(&mut client);
    send_foe(
        &mut client,
        &sends,
        bota_proto::Vec2::from_ints(MID.0 + 250, MID.1),
    );
    attack(&mut client);
    let mut kill = None;
    client.within(HANG, "the kill answered", |c| {
        kill = answers_to(c, Concept::Kill).into_iter().next();
        kill.as_ref()
            .is_some_and(|a| a.said.as_ref().map_or(true, |l| l.player.is_some()))
    });
    (client, players, kill.expect("an answer"))
}

/// The kill was answered by Shadow Fiend's first-blood rule, which outscores his plain kill
/// rule, with the line it played.
fn first_blood_answered(client: &mut Client, players: &Players, kill: Answer) {
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
    checked_line(client, players, &line);
}

/// Shadow Fiend kills an enemy Pudge, the match's first hero kill.
#[test]
fn a_first_kill_is_answered_by_the_first_blood_rule() {
    let (mut client, players, kill) = kill_scene(|_, _| {});
    first_blood_answered(&mut client, &players, kill);
}

/// A hero that fell to a tower alone was no one's kill: the server keeps First Blood for the
/// first kill a hero is credited with, and so does the hero's line.
#[test]
fn a_death_to_a_tower_leaves_first_blood_to_the_first_kill() {
    let (mut client, players, kill) = kill_scene(|client, sends| {
        let tower = bota_core::rules::DIRE_TOWERS
            .iter()
            .find(|(lane, tier, _)| *lane == bota_core::rules::LANE_MID && *tier == 1)
            .expect("Dire's middle tower")
            .2;
        let deaths = |c: &Client| {
            let mine = c
                .view()
                .player(c.core().my_slot.expect("a seat"))
                .map(|p| p.team);
            c.view()
                .players
                .iter()
                .filter(|p| Some(p.team) != mine)
                .map(|p| p.deaths)
                .sum::<u16>()
        };
        sends
            .lock()
            .expect("the foe's queue")
            .push_back(Send::To(tower + bota_proto::Vec2::from_ints(-400, 0)));
        client.until("the foe fallen to the tower", |c| deaths(c) > 0);
    });
    first_blood_answered(&mut client, &players, kill);
}

/// Pudge last-hits an enemy creep: the line's query counts his last hits as the scoreboard
/// does (his creep score), the one count his thirteenth's rule (`ButchersDozen`) reads.
///
/// Nothing else takes the creeps: no lane spawns, no creep of his own, the river away from every
/// tower; and nothing takes him (invulnerable). One attack-move, so that the order's own line is
/// long said when a creep of full health falls.
#[test]
fn a_last_hit_counts_the_heros_creep_score() {
    let server = Server::start(1, &["--map", "0", "--runes", "none"]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    begun(&mut client);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::Invulnerable { on: true });
    client.stand_at(MID);
    quiet(&mut client);
    let last_hits = |c: &Client| {
        c.core()
            .my_slot
            .and_then(|slot| c.view().player(slot)?.creep_score)
            .map_or(0, |score| score.last_hits)
    };
    let before = last_hits(&client);
    let since = client.tick();
    let camp = (MID.0 + 300, MID.1);
    client.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: bota_proto::Vec2::from_ints(camp.0, camp.1),
    });
    client.core_mut().act(Intent::AttackAt {
        at: (camp.0 as f32, camp.1 as f32),
        unit: None,
    });
    client.within(HANG, "a creep last-hit", |c| last_hits(c) > before);
    let own = |c: &Client| {
        answers_to(c, Concept::LastHit)
            .into_iter()
            .find(|a| a.tick >= since)
    };
    client.until("the last hit asked", |c| own(c).is_some());
    let hit = own(&client).expect("the last hit's query");
    assert_eq!(
        hit.query.value("lasthitcount"),
        Some((before + 1).to_string().as_str())
    );
}

/// Takes a Bounty Rune spawned at the top river spot: the hero stands beside it, and a right
/// click on it takes it.
fn take_bounty(client: &mut Client) {
    quiet(client);
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(client, RuneKind::Bounty, spot);
    client.within(HANG, "the Bounty Rune drawn", |c| {
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
    let deadline = Instant::now() + HANG;
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
            "bounty answer {n} not within {HANG:?}; answers: {asked:?}, voice {state:?}, shown {done}"
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
    // Frames the first line may take to come: the spawn line comes on the first frames of the
    // game, a walk's within a few hundred.
    const FRAME_BUDGET: usize = 3000;
    let mut frames = Vec::new();
    let mut line_at = None;
    while line_at.is_none_or(|at| frames.len() < at + 120) {
        assert!(
            frames.len() < FRAME_BUDGET,
            "no line said in {FRAME_BUDGET} frames"
        );
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

/// Only the watched seat's hero is asked anything, of what happens to it: a spectator watching
/// no seat asks nobody, a replay watched as the seat whose hero gained levels asks that hero of
/// them, and the replay watched as the other seat, or as no seat, asks nothing of them.
#[test]
fn only_the_watched_seats_hero_is_asked_of_its_levels() {
    let mut game = super::fog_pov::Spectated::start("0");
    game.cheat(Cheat::Levels { count: 3 });
    let spectator = game.client.world().resource::<HeroVoice>();
    assert_eq!(
        spectator.hero(),
        None,
        "a spectator watching no seat speaks for no hero"
    );
    assert!(answers(&game.client).is_empty(), "nor is anything asked");
    let leveled = game.driver.my_slot.expect("the driving seat").0;
    let file = game.save();
    let asked_of_levels = |slot: Option<u8>| {
        let client = super::fog_pov::replay_observed(&file, slot, &[], |_| {});
        let hero = client
            .world()
            .resource::<HeroVoice>()
            .hero()
            .map(str::to_owned);
        (hero, answers_to(&client, Concept::LevelUp).len())
    };
    let (hero, levels) = asked_of_levels(Some(leveled));
    assert_eq!(hero.as_deref(), Some("nevermore"));
    assert!(levels > 0, "the watched seat's hero is asked of its levels");
    let (_, levels) = asked_of_levels(Some(1 - leveled));
    assert_eq!(levels, 0, "the other seat's hero is asked nothing of them");
    let (hero, levels) = asked_of_levels(None);
    assert_eq!(
        (hero, levels),
        (None, 0),
        "a replay watched as no seat asks nobody"
    );
}

/// The driving seat's Shadow Fiend (levelled, invulnerable, no lane creeps) last-hits a camp of
/// enemy creeps at the river and then kills the other seat's Shadow Fiend there: the match as a
/// spectator recorded it, and the seat whose hero did it.
fn kills_and_last_hits() -> (super::fog_pov::ReplayFile, bota_proto::SlotId) {
    let mut game = super::fog_pov::Spectated::pair("0");
    for cheat in [
        Cheat::CreepSpawning { on: false },
        Cheat::KillCreeps {
            which: CreepSide::All,
        },
        Cheat::Invulnerable { on: true },
        Cheat::Levels { count: 24 },
        Cheat::Teleport {
            at: bota_proto::Vec2::from_ints(MID.0, MID.1),
        },
    ] {
        game.cheat(cheat);
    }
    let slot = game.driver.my_slot.expect("the driving seat");
    let camp = (MID.0 + 300, MID.1);
    game.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: bota_proto::Vec2::from_ints(camp.0, camp.1),
    });
    // Two of the camp's creeps, each struck until it falls: an attack-move may leave the
    // camp to walk off.
    let camp_creeps = |driver: &bota_client_core::App| {
        let view = driver.view()?;
        let mine = view.player(slot)?.team;
        let creeps: Vec<_> = view
            .units
            .iter()
            .filter(|u| u.team != mine && u.kind == bota_proto::UnitKind::CreepMelee)
            .map(|u| (u.id, u.pos))
            .take(2)
            .collect();
        (creeps.len() == 2).then_some(creeps)
    };
    game.wait("the camp standing", |driver, _| {
        camp_creeps(driver).is_some()
    });
    for (creep, at) in camp_creeps(&game.driver).expect("the camp") {
        game.driver.act(Intent::AttackAt {
            at: (at.x.to_f32(), at.y.to_f32()),
            unit: Some(creep),
        });
        game.wait("the creep struck down", |driver, _| {
            driver.view().is_some_and(|view| view.unit(creep).is_none())
        });
    }
    game.wait("two creeps last-hit", |driver, _| {
        driver
            .view()
            .and_then(|view| view.player(slot)?.creep_score)
            .is_some_and(|score| score.last_hits >= 2)
    });
    // Back to the river the creeps were chased from, to see the other hero come.
    game.cheat(Cheat::Teleport {
        at: bota_proto::Vec2::from_ints(MID.0, MID.1),
    });
    let other = |driver: &bota_client_core::App| {
        let view = driver.view()?;
        view.unit(view.players.iter().find(|p| p.slot != slot)?.unit?)
            .map(|u| (u.id, u.pos))
    };
    // Its own seat's cheat brings it: a walk from its fountain would fill the recording.
    game.other
        .as_mut()
        .expect("the other seat")
        .act(Intent::Cheat(Cheat::Teleport {
            at: bota_proto::Vec2::from_ints(MID.0 - 200, MID.1),
        }));
    game.wait("the other hero come to the river", |driver, _| {
        other(driver).is_some_and(|(_, at)| {
            at.within(
                bota_proto::Vec2::from_ints(MID.0, MID.1),
                bota_proto::Fixed::from_int(600),
            )
        })
    });
    let (foe, at) = other(&game.driver).expect("the other hero");
    game.driver.act(Intent::AttackAt {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(foe),
    });
    // The recording names units as the full stream does, not as the driver's view does.
    let fallen = game
        .client
        .core()
        .view()
        .and_then(|view| view.players.iter().find(|p| p.slot != slot)?.unit)
        .expect("the other hero in the full stream");
    game.until_death(fallen);
    (game.save(), slot)
}

/// What the listener raises for `slot` while `file` plays, from the first frame `from` holds
/// for on, each with its view's tick; `memory` is what it remembers from before.
fn heard(
    file: &super::fog_pov::ReplayFile,
    slot: bota_proto::SlotId,
    from: impl Fn(&bota_client_core::App) -> bool,
    memory: &mut crate::hero_voice::listen::Memory,
) -> Vec<(u32, Concept, Vec<(Fact, String)>)> {
    use clap::Parser;
    let path = file.path().to_str().expect("a UTF-8 path");
    let seat = slot.0.to_string();
    let args = crate::Args::parse_from(["dbota", "--replay", path, "--pov", &seat]);
    let mut core = crate::session::open(&args).expect("the replay opens").0;
    let fog = crate::fog::FogGrid::default();
    let mut raised = Vec::new();
    let mut listening = false;
    // A frame a tick, and a bound far past any test's match.
    for _ in 0..1_000_000 {
        core.pump(1.0 / f32::from(core.tick_rate().max(1)));
        if let Some(tick) = core.view().map(|view| view.tick) {
            listening |= from(&core);
            if listening {
                let queries = memory.queries(&core, Some(slot), &fog);
                raised.extend(queries.into_iter().map(|(c, f)| (tick, c, f)));
            }
        }
        if let bota_client_core::Source::Replay(player) = &core.source
            && (player.finished() || player.error().is_some())
        {
            return raised;
        }
    }
    panic!("the replay never ended")
}

fn kills_and_last_hits_of(
    raised: &[(u32, Concept, Vec<(Fact, String)>)],
) -> Vec<&(u32, Concept, Vec<(Fact, String)>)> {
    raised
        .iter()
        .filter(|(_, concept, _)| matches!(concept, Concept::Kill | Concept::LastHit))
        .collect()
}

/// A replay heard again from its start (the view's tick going back) raises what it raised
/// the first time, with the same facts: First Blood and the last hit counts among them.
#[test]
fn a_replay_heard_again_from_its_start_raises_what_it_raised() {
    let (file, slot) = kills_and_last_hits();
    let mut memory = crate::hero_voice::listen::Memory::default();
    let first = heard(&file, slot, |_| true, &mut memory);
    let told = kills_and_last_hits_of(&first);
    assert!(
        told.iter().any(|(_, c, facts)| *c == Concept::Kill
            && facts.contains(&(Fact::FirstBlood, "1".to_owned()))),
        "{told:?}"
    );
    assert!(
        told.iter().any(|(_, c, facts)| *c == Concept::LastHit
            && facts.contains(&(Fact::LastHitCount, "2".to_owned()))),
        "{told:?}"
    );
    let again = heard(&file, slot, |_| true, &mut memory);
    assert_eq!(kills_and_last_hits_of(&again), told);
}

/// Listening begun on the frame of a kill or a last hit raises it as listening from the start
/// does: the frame's own snapshot already counts it, and it is not counted twice.
#[test]
fn listening_begun_on_a_kill_or_a_last_hit_raises_it_as_from_the_start() {
    let (file, slot) = kills_and_last_hits();
    let whole = heard(
        &file,
        slot,
        |_| true,
        &mut crate::hero_voice::listen::Memory::default(),
    );
    let told = kills_and_last_hits_of(&whole);
    assert!(told.len() >= 3, "{told:?}");
    for event in told {
        let late = heard(
            &file,
            slot,
            |core| core.view().is_some_and(|view| view.tick >= event.0),
            &mut crate::hero_voice::listen::Memory::default(),
        );
        assert_eq!(kills_and_last_hits_of(&late).first(), Some(&event));
    }
}

/// After a kill two heroes shared (a tower finished the foe, the driving seat's hero had hurt
/// it: no hero credited, the assist counted), the next kill is no First Blood, whether the
/// listener heard the shared kill or began listening after it.
#[test]
fn a_listener_begun_after_a_shared_kill_keeps_first_blood_spent() {
    use clap::Parser;
    let game = super::fog_pov::Spectated::pair("0");
    let slot = game.driver.my_slot.expect("the driving seat");
    let file = game.save();
    let path = file.path().to_str().expect("a UTF-8 path");
    let seat = slot.0.to_string();
    let args = crate::Args::parse_from(["dbota", "--replay", path, "--pov", &seat]);
    let mut core = crate::session::open(&args).expect("the replay opens").0;
    let both = |core: &bota_client_core::App| {
        core.view()
            .is_some_and(|view| view.players.iter().all(|p| p.unit.is_some()))
    };
    for _ in 0..100_000 {
        if both(&core) {
            break;
        }
        core.pump(1.0 / 30.0);
    }
    let template = core.view().expect("a view of both heroes").clone();
    let mine = template
        .player(slot)
        .and_then(|p| p.unit)
        .expect("one's hero");
    let foe = template
        .players
        .iter()
        .find(|p| p.slot != slot)
        .and_then(|p| p.unit)
        .expect("the foe");
    let tower = template
        .units
        .iter()
        .find(|u| {
            u.kind == bota_proto::UnitKind::Tower
                && u.team == template.player(slot).expect("seat").team
        })
        .expect("a tower of one's own")
        .id;
    // The view after each death: its tick on, the driving seat's counts changed.
    let after = |ticks: u32, kills: u16, assists: u16| {
        let mut view = template.clone();
        view.tick += ticks;
        let me = view
            .players
            .iter_mut()
            .find(|p| p.slot == slot)
            .expect("the driving seat");
        me.kills += kills;
        me.assists += assists;
        bota_proto::ServerMsg::Snapshot { view }
    };
    let death =
        |killer: bota_proto::EntityId, credited: bool, assists: u16| bota_proto::EventKind::Died {
            unit: foe,
            killer: Some(killer),
            denied: false,
            gold: 0,
            bounty: Some(0),
            assists,
            awards: None,
            credited_hero: credited.then(|| Box::new(killer)),
        };
    let fog = crate::fog::FogGrid::default();
    let heard = |core: &mut bota_client_core::App,
                 memory: &mut crate::hero_voice::listen::Memory,
                 view,
                 event| {
        core.handle(view);
        core.told = vec![event];
        memory.queries(core, Some(slot), &fog)
    };
    let shared = || (after(1, 0, 1), death(tower, false, 1 << slot.0));
    let kill = || (after(60, 1, 1), death(mine, true, 0));
    let mut whole = crate::hero_voice::listen::Memory::default();
    let (view, event) = shared();
    heard(&mut core, &mut whole, view, event);
    let (view, event) = kill();
    let heard_through = heard(&mut core, &mut whole, view, event);
    let (view, event) = kill();
    let begun_after = heard(
        &mut core,
        &mut crate::hero_voice::listen::Memory::default(),
        view,
        event,
    );
    let first_blood = |raised: &[(Concept, Vec<(Fact, String)>)]| {
        raised
            .iter()
            .find(|(c, _)| *c == Concept::Kill)
            .and_then(|(_, facts)| facts.iter().find(|(f, _)| *f == Fact::FirstBlood))
            .map(|(_, v)| v.clone())
    };
    assert_eq!(first_blood(&heard_through), Some("0".to_owned()));
    assert_eq!(first_blood(&begun_after), Some("0".to_owned()));
}

/// A line the hero is saying sounds in the hero voices' channel, whatever the effects' level is, and
/// its sink follows that channel's balance while the line plays.
#[test]
fn a_line_the_hero_says_follows_the_hero_voices_balance() {
    let (_server, mut client, _players) = alone(HeroId::PUDGE);
    super::channels::listen(&mut client);
    let line = first_line(&mut client, Concept::Move, walk);
    let sound = line.player.expect("the mixer gave the line a voice");
    super::channels::follows_the_balance(&mut client, sound, crate::channels::Channel::Voices);
}
