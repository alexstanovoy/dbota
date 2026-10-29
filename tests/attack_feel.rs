//! How an attack looks and sounds, end to end: a match is played on a real
//! bota_server (cheats on, one seat) and recorded, then the real client plays
//! the replay headless with `--trace-feel`, which prints what every unit's
//! model shows each frame and each creep sound decided. Drawn on the GPU in a
//! headless compositor, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it.
//!
//! - a swing broken off before its strike is drawn as a cancel, never as a
//!   blow (the clip is cut, not carried to its attack point);
//! - a change of clip is a 0.2 s cross-fade, not a cut;
//! - melee creeps whoosh as a swing begins and hit as it lands;
//! - the Ogre Bruiser's Ogre Smash plays its own sequence through its 2.8 s
//!   cast point.

mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bota_client_core::{Ingested, IssuedOrder, Link, Seat, SessionOptions, Transport};
use bota_core::NeutralKind;
use bota_proto::{
    Cheat, ClientMsg, EntityId, HeroId, Order, Target, Team, UnitKind, Vec2, WorldView,
};

/// Seconds a run may take.
const PATIENCE: Duration = Duration::from_secs(300);

/// What a recording's driver wants done on a tick.
enum Step {
    Wait,
    Order(Order),
    Done,
}

fn cheat(cheat: Cheat) -> Step {
    Step::Order(Order::Cheat { cheat })
}

/// Plays one seat of Shadow Fiend on a server of its own with cheats on, a
/// driver deciding the orders from each tick's view, and returns the replay
/// it wrote.
fn record(
    scratch: &Path,
    name: &str,
    map: &str,
    seed: u64,
    mut driver: impl FnMut(&WorldView) -> Step,
) -> (PathBuf, u32) {
    let replay = scratch.join(format!("{name}.brp"));
    let mut server: Child = Command::new(common::server_binary())
        .args(["--map", map, "--port", "0", "--bind", "127.0.0.1"])
        .args(["--players", "1", "--tactical-secs", "0", "--cheats"])
        .args(["--mode", "lockstep", "--empty-after-secs", "1"])
        .args(["--seed", &seed.to_string(), "--replay"])
        .arg(&replay)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("bota_server starts");
    let mut output = BufReader::new(server.stdout.take().expect("piped"));
    let address = loop {
        let mut line = String::new();
        assert!(output.read_line(&mut line).expect("the server") > 0);
        if let Some(address) = line.trim().strip_prefix("bota_server listening on ") {
            break address.to_owned();
        }
    };
    std::thread::spawn(move || for _ in output.lines() {});
    let (mut link, seated) =
        Link::join(&address, "feel", HeroId::SHADOW_FIEND, None).expect("a seat");
    let mut seat = Seat::new(seated, SessionOptions::default());
    let started = Instant::now();
    let mut last_order = 0;
    let mut ticks = 0;
    loop {
        assert!(started.elapsed() < PATIENCE, "the recording ran too long");
        let msg = link.recv().expect("link").expect("open");
        match seat.ingest(msg, &mut ()).expect("stream") {
            Ingested::TickComplete { .. } => {
                let session = seat.session_mut().expect("started");
                let view = session.current().expect("a view").clone();
                ticks = view.tick;
                let step = if view.tick < last_order + 2 {
                    Step::Wait
                } else {
                    driver(&view)
                };
                match step {
                    Step::Order(order) => {
                        last_order = view.tick;
                        let out = session.issue(IssuedOrder::hero(order)).expect("issued");
                        link.send(&out.message()).expect("sent");
                    }
                    Step::Done => break,
                    Step::Wait => {}
                }
                if let Some(tick) = session.end_turn() {
                    link.send(&ClientMsg::Ack { tick }).expect("acked");
                }
            }
            Ingested::Over { .. } => break,
            _ => {}
        }
    }
    drop(link);
    let _ = server.wait();
    (replay, ticks)
}

/// Plays a replay in the client with the trace on and returns its lines.
fn trace(scratch: &Path, replay: &Path, seconds: u32) -> Vec<String> {
    let cache =
        std::env::var("DBOTA_ASSETS").map_or_else(|_| scratch.join("assets"), PathBuf::from);
    let script = format!("wait:{seconds};exit");
    let mut child = common::client()
        .args(["--replay"])
        .arg(replay)
        .args(["--pov", "0", "--trace-feel", "--mute", "--no-vsync"])
        .args([
            "--display-mode",
            "windowed",
            "--script",
            &script,
            "--config",
        ])
        .arg(scratch.join("client.conf"))
        .env("DBOTA_ASSETS", cache)
        .env("XDG_DATA_HOME", scratch.join("data"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbota runs");
    let stdout = child.stdout.take().expect("its stdout");
    let said = std::thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter(|l| l.starts_with("anim ") || l.starts_with("sound "))
            .collect::<Vec<_>>()
    });
    let started = Instant::now();
    while child.try_wait().expect("waited for").is_none() {
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            panic!("the client ran too long");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    said.join().expect("its output")
}

/// One `anim` line.
#[derive(Debug, Clone)]
struct Frame {
    at: f32,
    tick: u32,
    unit: u32,
    pos: String,
    kind: String,
    clip: String,
    t: f32,
    point: Option<f32>,
    share: f32,
    fading: String,
}

fn frames(lines: &[String]) -> Vec<Frame> {
    lines
        .iter()
        .filter(|l| l.starts_with("anim "))
        .map(|l| {
            let get = |key: &str| {
                l.split(' ')
                    .find_map(|w| w.strip_prefix(key))
                    .unwrap_or_else(|| panic!("{key} in {l}"))
            };
            Frame {
                at: get("at=").parse().expect("at"),
                tick: get("tick=").parse().expect("tick"),
                unit: get("unit=").parse().expect("unit"),
                pos: get("pos=").to_owned(),
                kind: get("kind=").to_owned(),
                clip: get("clip=").to_owned(),
                t: get("t=").parse().expect("t"),
                point: get("point=").parse().ok(),
                share: get("share=").parse().expect("share"),
                fading: get("fading=").to_owned(),
            }
        })
        .collect()
}

/// A scratch folder of this test run.
fn scratch(name: &str) -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// Fighting enemy creeps at the lane: waves of both sides are made where the
/// hero stands, a swing of his is broken off by a stop right after the order,
/// then he fights on.
fn lane_fight(scratch: &Path) -> (PathBuf, u32) {
    let mut phase = 0;
    let mut waited = 0u32;
    let mut target: Option<EntityId> = None;
    let mut ordered_at = 0;
    let at = Vec2::from_ints(-1500, -1200);
    record(scratch, "lane", "0", 1, |view| {
        let me = view.units.iter().find(|u| u.hero.is_some());
        let enemy_creep = view
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::CreepMelee && u.team == Team::Dire)
            .min_by_key(|u| me.map_or(0, |m| (u.pos.x - m.pos.x).to_int().abs()));
        match phase {
            0 => {
                phase = 1;
                cheat(Cheat::StartGame)
            }
            1 => {
                phase = 2;
                cheat(Cheat::AllVision { on: true })
            }
            2 => {
                phase = 3;
                cheat(Cheat::Wtf { on: true })
            }
            3 => {
                phase = 4;
                cheat(Cheat::Teleport { at })
            }
            4 => {
                phase = 5;
                cheat(Cheat::CreateCreeps { enemy: true, at })
            }
            5 => {
                phase = 6;
                waited = view.tick;
                cheat(Cheat::CreateCreeps { enemy: false, at })
            }
            // Let the creeps fight a while.
            6 if view.tick > waited + 90 => {
                phase = 7;
                match enemy_creep {
                    Some(c) => {
                        target = Some(c.id);
                        ordered_at = view.tick;
                        Step::Order(Order::Attack {
                            target: Target::Unit(c.id),
                        })
                    }
                    None => Step::Wait,
                }
            }
            // Stand down two ticks into the windup.
            7 if view.tick >= ordered_at + 3 => {
                phase = 8;
                waited = view.tick;
                Step::Order(Order::Move {
                    target: Target::None,
                })
            }
            8 if view.tick > waited + 20 => {
                phase = 9;
                waited = view.tick;
                Step::Order(Order::Attack {
                    target: Target::Unit(target.expect("a target")),
                })
            }
            9 if view.tick > waited + 240 => Step::Done,
            _ => Step::Wait,
        }
    })
}

/// A swing broken off in its windup is cut, not played on to a blow; a swing
/// that lands plays the blow through; every change of clip is a 0.2 s fade;
/// melee creeps whoosh and hit.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_broken_off_swing_is_a_cancel_and_clips_fade_and_melee_creeps_sound() {
    let dir = scratch("lane");
    let (replay, ticks) = lane_fight(&dir);
    let lines = trace(&dir, &replay, ticks / 30 + 25);
    let all = frames(&lines);
    assert!(!all.is_empty(), "the client traced nothing");
    let mut by_unit: HashMap<u32, Vec<&Frame>> = HashMap::new();
    for f in &all {
        by_unit.entry(f.unit).or_default().push(f);
    }
    // Cancel: a unit whose attack clip stopped before its point never shows
    // an attack clip past its point until a new windup begins.
    let mut cancelled = 0;
    let mut shots = Vec::new();
    let mut landed = 0;
    for frames in by_unit.values() {
        let mut windup_peak: Option<f32> = None;
        for pair in frames.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let attacking = |f: &Frame| f.clip.starts_with("attack");
            if attacking(a) && a.t < a.point.unwrap_or(0.0) {
                windup_peak = Some(a.t);
            }
            if attacking(a) && !attacking(b) && a.t < a.point.unwrap_or(0.0) - 0.02 {
                cancelled += 1;
                if a.kind == "CreepMelee" {
                    shots.push(("cancelled-creep-swing", a.pos.clone(), a.tick));
                }
            }
            if attacking(a) && a.point.is_some_and(|p| a.t >= p) {
                landed += 1;
            }
            if windup_peak.is_some()
                && !attacking(a)
                && attacking(b)
                && b.t > b.point.unwrap_or(f32::MAX)
            {
                panic!("a broken-off windup was drawn as a blow: {a:?} then {b:?}");
            }
        }
    }
    assert!(cancelled > 0, "no swing was broken off in the match");
    assert!(landed > 0, "no swing landed in the match");
    // Cross-fade: a change of a unit's clip starts a fade whose weight climbs
    // by a frame's time over Dota's 0.2 s each frame, to one, the clip it
    // left held under it. (A clip that comes back before it faded picks up
    // from the weight it had left.)
    let mut fades = 0;
    for frames in by_unit.values() {
        for (i, pair) in frames.windows(2).enumerate() {
            if pair[0].clip == pair[1].clip || pair[1].share >= 1.0 {
                continue;
            }
            let start = pair[1];
            assert!(
                !start.fading.is_empty(),
                "a clip change fades the one before: {start:?}"
            );
            let mut last = start;
            for next in &frames[i + 2..] {
                if next.clip != start.clip || last.share >= 1.0 {
                    break;
                }
                let climb = f64::from(next.share - last.share);
                let expected = f64::from(((next.at - last.at) / 0.2).min(1.0 - last.share));
                assert!(
                    (climb - expected).abs() < 0.03,
                    "a fade climbs 1 in 0.2 s: {last:?} then {next:?}"
                );
                last = next;
            }
            fades += i32::from(start.share < 0.5 && last.share >= 1.0);
        }
    }
    assert!(fades > 0, "no clip change was seen to fade from the start");
    // Sound: melee creeps whoosh as a swing begins and hit as it lands.
    let heard = |kind: &str| {
        lines
            .iter()
            .filter(|l| l.starts_with("sound ") && l.contains(&format!("name={kind} ")))
            .collect::<Vec<_>>()
    };
    for kind in ["sound.creep.melee.preattack", "sound.creep.melee.attack"] {
        let said = heard(kind);
        assert!(!said.is_empty(), "no {kind} was decided");
        assert!(
            said.iter().all(|l| !l.ends_with("sources=0")),
            "{kind} has no sound in the cache"
        );
    }
    keep(&replay, "lane", &shots);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `ATTACK_FEEL_KEEP` set to a folder, a run keeps its replay there and
/// says which ticks and units are worth a picture (`tools/headless.sh` with
/// `--screenshot-at TICK --follow` takes them).
fn keep(replay: &Path, name: &str, shots: &[(&str, String, u32)]) {
    let Some(folder) = std::env::var_os("ATTACK_FEEL_KEEP") else {
        return;
    };
    let target = Path::new(&folder).join(format!("{name}.brp"));
    std::fs::copy(replay, &target).expect("the replay is kept");
    for (what, unit, tick) in shots.iter().take(3) {
        println!("keep: {} {what} at {unit} tick {tick}", target.display());
    }
}

/// A hero walks up to the camps until one holds a neutral of `kind`, attacks
/// it until one of that kind casts, and watches the cast through.
fn neutral_cast(scratch: &Path, name: &str, kind: NeutralKind) -> (PathBuf, u32) {
    use bota_proto::MapId;
    let camps = bota_core::map_of(MapId(0)).camps;
    let mut setup = vec![
        Cheat::StartGame,
        Cheat::AllVision { on: true },
        Cheat::Levels { count: 10 },
        Cheat::Wtf { on: true },
        Cheat::KillCreeps {
            which: bota_proto::CreepSide::All,
        },
        Cheat::SpawnNeutrals,
    ];
    setup.reverse();
    let (mut camp, mut arrived, mut cast_seen, mut target, mut fought) =
        (0usize, 0u32, None::<u32>, None::<EntityId>, 0u32);
    let of = |view: &WorldView, wanted: NeutralKind| {
        view.units
            .iter()
            .filter(|u| u.neutral.and_then(NeutralKind::of) == Some(wanted))
            .map(|u| (u.id, u.cast.is_some()))
            .collect::<Vec<_>>()
    };
    record(scratch, name, "0", 3, move |view| {
        if let Some(next) = setup.pop() {
            return cheat(next);
        }
        let of_kind = of(view, kind);
        if target.is_none() {
            if arrived != 0 && view.tick < arrived + 10 {
                return Step::Wait;
            }
            if let Some((id, _)) = of_kind.first().filter(|_| arrived != 0) {
                target = Some(*id);
                fought = view.tick;
            } else {
                assert!(camp < camps.len(), "no camp holds a {kind:?}");
                let pos = camps[camp].pos;
                camp += 1;
                arrived = view.tick;
                return cheat(Cheat::Teleport {
                    at: Vec2::from_ints(pos.x.to_int() - 200, pos.y.to_int()),
                });
            }
        }
        assert!(view.tick < fought + 3000, "no {kind:?} cast in the fight");
        match cast_seen {
            None if of_kind.iter().any(|(_, casting)| *casting) => {
                cast_seen = Some(view.tick);
                Step::Wait
            }
            None if (view.tick - fought) % 60 == 21 => Step::Order(Order::Attack {
                target: Target::Unit(target.expect("a target")),
            }),
            Some(at) if view.tick > at + 150 => Step::Done,
            _ => Step::Wait,
        }
    })
}

/// The Ogre Bruiser's Ogre Smash plays its own sequence through the whole
/// cast, which lands on the 2.8 s cast point. (The Frostmage's Ice Armor is
/// not cast by the simulation yet; its clip's point is pinned to the
/// ability's by `ability_art`'s tests.)
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn the_ogre_bruiser_plays_the_smash_sequence_through_its_cast() {
    let dir = scratch("ogre");
    let kind = NeutralKind::OgreMauler;
    let (replay, ticks) = neutral_cast(&dir, "smash", kind);
    let lines = trace(&dir, &replay, ticks / 30 + 25);
    let tag = format!("neutral=Some({kind:?})");
    let ogre: Vec<String> = lines.iter().filter(|l| l.contains(&tag)).cloned().collect();
    let casting: Vec<Frame> = frames(&ogre)
        .into_iter()
        .filter(|f| f.clip == "cast2")
        .collect();
    assert!(!casting.is_empty(), "the Bruiser never played its Smash");
    let first = casting.first().expect("a frame");
    let last = casting.iter().map(|f| f.t).fold(0.0, f32::max);
    assert!(
        first.t < 0.2,
        "the cast begins at the start of the clip: {first:?}"
    );
    assert!(
        (last - 2.8).abs() < 0.1,
        "it reaches the 2.8 s cast point: {last}"
    );
    assert_eq!(first.point, Some(2.8), "the clip's point");
    let late = casting.iter().find(|f| f.t > 2.2).expect("a late frame");
    keep(
        &replay,
        "smash",
        &[("ogre-smash", late.pos.clone(), late.tick)],
    );
    let _ = std::fs::remove_dir_all(&dir);
}
