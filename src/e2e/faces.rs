//! Every hero's face moves as its authored data moves it, and never jumps: the portrait's face
//! (its morph weights as drawn, and the joints its meshes are skinned to) changes smoothly from
//! one frame to the next while the hero stands idle, while it runs and answers its order, and
//! through each of its voice lines (every kind the cache holds: moves, attacks, runes, the
//! Bottle...) over its idle expression, every frame checked by the same limits. The frames are
//! 2 ms apart through the idle loop and the run and 4 ms through each whole line, so an authored
//! motion moves the face a little each frame (a blink, the fastest, about 0.03 of a weight at
//! 2 ms) and a jump stands out whole. A line
//! whose own authored face does not return to rest where its data ends (read here from its lip
//! sync record) steps back once there: that one frame (the one its data ends on), and no other,
//! is allowed it.

use std::sync::Arc;
use std::time::Duration;

use bevy::mesh::morph::MorphWeights;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, HeroId, Order, Target, Vec2 as At};

use super::harness::{Client, Server};
use crate::portrait::Stage;
use crate::talk::{LineFace, Speech};

/// How near each controller of a line's own face must be to rest where its data ends for the
/// line to end at rest.
const REST: f32 = 0.05;
/// Seconds between frames.
const STEP: f32 = 0.002;
/// Most a morph weight may change from one frame to the next.
const MAX_WEIGHT_STEP: f32 = 0.1;
/// Most a face joint may turn (degrees) or move (inches, against the model's pivot) a frame.
const MAX_TURN: f32 = 1.0;
const MAX_MOVE: f32 = 0.5;
/// Frames idle: long enough for every hero's idle expression to loop once.
const IDLE_FRAMES: usize = 4500;
/// Frames after the move order: the run, the answer said, and back to idle.
const RUN_FRAMES: usize = 2500;
/// Frames played before a line starts and after it ends, twice as many after (the face its own
/// before it, back to its own after).
const LEAD_FRAMES: usize = 25;
/// Seconds between frames through a whole line: twice the idle's, its limits the same (so a
/// line is held to half the speed of change the idle is).
const LINE_STEP: f32 = 0.004;

/// The portrait's face as drawn: every morph weight of its meshes, and every joint they are
/// skinned to (turn and place against the stage's pivot). None while the stage has no face.
fn drawn_face(client: &Client) -> Option<(Vec<f32>, Vec<(Quat, Vec3)>)> {
    let world = client.world();
    let actor = world.resource::<Stage>().actor.as_ref()?;
    let face = actor.face.as_ref()?;
    let pivot = world
        .get::<GlobalTransform>(actor.pivot)?
        .affine()
        .inverse();
    let mut weights = Vec::new();
    let mut joints = Vec::new();
    for (node, _) in face.nodes() {
        weights.extend_from_slice(world.get::<MorphWeights>(node)?.weights());
        let meshes = world.get::<Children>(node).into_iter().flatten();
        for skin in meshes.filter_map(|e| world.get::<SkinnedMesh>(*e)) {
            for joint in &skin.joints {
                let at = pivot * world.get::<GlobalTransform>(*joint)?.affine();
                let (_, turn, place) = at.to_scale_rotation_translation();
                joints.push((turn, place));
            }
        }
    }
    Some((weights, joints))
}

/// The controllers of the face on the portrait's stage and their ranges, by token, from its
/// meshes' flex rigs.
fn ranges(client: &mut Client) -> std::collections::HashMap<u32, (f32, f32)> {
    let world = client.world_mut();
    let pivot = world
        .resource::<Stage>()
        .actor
        .as_ref()
        .expect("the stage's model")
        .pivot;
    let mut out = std::collections::HashMap::new();
    let mut meshes = world.query::<(Entity, &bevy::gltf::GltfMeshExtras)>();
    let extras: Vec<(Entity, String)> = meshes
        .iter(world)
        .map(|(e, x)| (e, x.value.clone()))
        .collect();
    for (mesh, text) in extras {
        let on_stage = std::iter::successors(Some(mesh), |e| {
            world.get::<ChildOf>(*e).map(ChildOf::parent)
        })
        .any(|e| e == pivot);
        if let Some(rig) = crate::face::Rig::from_extras(&text).filter(|_| on_stage) {
            out.extend(
                rig.controllers
                    .iter()
                    .map(|&(name, min, max)| (name, (min, max))),
            );
        }
    }
    out
}

/// Whether a line's own face, as its authored lip sync record holds it, is back at
/// each controller's rest at its last key: read from the record itself.
fn ends_at_rest(client: &mut Client, text: &str) -> bool {
    let mut names: Vec<u32> = Vec::new();
    let mut last: Vec<f32> = Vec::new();
    for words in text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
    {
        match words.as_slice() {
            ["morph", _, _, tokens @ ..] => {
                names = tokens.iter().map(|t| t.parse().expect("a token")).collect();
            }
            ["key", _, values @ ..] => {
                last = values.iter().map(|v| v.parse().expect("a value")).collect();
            }
            _ => {}
        }
    }
    let ranges = ranges(client);
    names.iter().zip(&last).all(|(name, v)| {
        ranges
            .get(name)
            .is_none_or(|&(min, max)| (min + v * (max - min)).abs() <= REST)
    })
}

/// A frame that jumped: how far into the line the hero says it was before and after it (none:
/// saying none), and what jumped on it.
struct Jump {
    said: (Option<f32>, Option<f32>),
    what: String,
}

/// How far into the line it says the hero is now.
fn said(client: &Client) -> Option<f32> {
    let now = client
        .world()
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs();
    let unit = client.me().id;
    let speech = client.world().resource::<Speech>();
    speech.of(Some(unit), now).map(|(_, t)| t)
}

fn spoken(client: &Client) -> Option<(String, f32)> {
    let now = client
        .world()
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs();
    client
        .world()
        .resource::<Speech>()
        .of(Some(client.me().id), now)
        .map(|(face, t)| (face.file.clone(), t))
}

/// Runs `frames` frames of `step` seconds, each checked against the one before by the same
/// limits whatever the step: the frames something jumped on. A frame with no face drawn is a
/// jump in itself.
fn watch(client: &mut Client, frames: usize, step: f32, phase: &str) -> Vec<Jump> {
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            step,
        )));
    let mut out = Vec::new();
    let mut last = (drawn_face(client), said(client), spoken(client));
    for frame in 0..frames {
        client.frames(1);
        let now = (drawn_face(client), said(client), spoken(client));
        let mut what = Vec::new();
        match (&last.0, &now.0) {
            (Some((w0, j0)), Some((w1, j1))) => {
                assert_eq!(
                    (w0.len(), j0.len()),
                    (w1.len(), j1.len()),
                    "{phase}: one face"
                );
                let weight = w0.iter().zip(w1).map(|(a, b)| (a - b).abs());
                let (i, by) = weight
                    .enumerate()
                    .fold((0, 0.0), |m, (i, s)| if s > m.1 { (i, s) } else { m });
                if by > MAX_WEIGHT_STEP {
                    what.push(format!("weight {i} by {by:.3}"));
                }
                for (k, ((r0, t0), (r1, t1))) in j0.iter().zip(j1).enumerate() {
                    let turn = r0.angle_between(*r1).to_degrees();
                    let moved = t0.distance(*t1);
                    if turn > MAX_TURN || moved > MAX_MOVE {
                        what.push(format!("joint {k} turned {turn:.2} deg, moved {moved:.2}"));
                    }
                }
            }
            _ => what.push("no face drawn".into()),
        }
        if !what.is_empty() {
            out.push(Jump {
                said: (last.1, now.1),
                what: format!(
                    "{phase} frame {frame}: {}; speech {:?} -> {:?}",
                    what.join(", "),
                    last.2,
                    now.2
                ),
            });
        }
        last = now;
    }
    out
}

/// A line loaded as the client loads it, with its authored text kept independently to judge
/// whether its final key returns to rest.
struct AuthoredLine {
    face: Arc<LineFace>,
    text: String,
}

/// Register before the earliest phoneme's lookahead, with two sampled frames to observe rest.
fn registration_lead(text: &str) -> f32 {
    let first = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("phoneme"), Some(start)) => {
                    Some(start.parse::<f32>().expect("a phoneme time"))
                }
                _ => None,
            }
        })
        .fold(0.0, f32::min);
    let lead = (LINE_STEP * LEAD_FRAMES as f32)
        .max(-first + crate::face::PHONEME_FILTER + 2.0 * LINE_STEP);
    // Bound the fixture's extra frames even when its cache metadata is unreasonable.
    assert!(
        lead.is_finite() && lead <= 1.0,
        "voice pre-roll exceeds a second: {lead}"
    );
    lead
}

#[test]
fn a_line_is_registered_before_its_authored_phoneme_preroll() {
    let first = -0.08;
    let text = format!("dbota-lipsync 1\nphoneme {first} 0.1 97\n");
    let lead = registration_lead(&text);
    assert!(
        -lead + LINE_STEP < first - crate::face::PHONEME_FILTER,
        "the first sampled frame precedes phoneme lookahead, lead {lead}"
    );
}

/// Every voice line of the hero's resident pack, of every kind, with its lip sync; and how many
/// of its lines have no lip sync.
fn lines(client: &Client, hero: HeroId) -> (Vec<AuthoredLine>, usize) {
    use std::io::Read;

    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let name = manifest.dota_name("hero", hero.0).expect("the hero's name");
    let rel = dbota_import::voice::pack_path(name);
    let limit = dbota_import::voice::pack::MAX_PACK_BYTES;
    let mut bytes = Vec::new();
    std::fs::File::open(manifest.root.join(&rel))
        .expect("the hero's voice pack")
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .expect("the voice pack read");
    assert!(bytes.len() <= limit, "{rel}: the pack is within its bound");
    let voice = dbota_import::voice::open(name, &bytes).expect("the hero's voice pack reads");
    let texts: Vec<Option<String>> = voice
        .sounds
        .iter()
        .map(|sound| {
            sound.lipsync.map(|text| {
                std::str::from_utf8(text)
                    .expect("the authored lip sync is text")
                    .to_owned()
            })
        })
        .collect();
    let loaded = crate::hero_voice::loaded(voice, &rel);
    let mut out = Vec::new();
    let mut silent = 0;
    for ((_, face), text) in loaded.sounds.into_iter().zip(texts) {
        match (face, text) {
            (Some(face), Some(text)) => out.push(AuthoredLine { face, text }),
            _ => silent += 1,
        }
    }
    (out, silent)
}

/// The hero starts saying `line` `ahead` seconds from now, the face still its own until then:
/// no frame of the face is left unchecked by it.
fn say_ahead(client: &mut Client, line: &Arc<LineFace>, ahead: f32) {
    let unit = client.me().id;
    let world = client.world_mut();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    world
        .resource_mut::<Speech>()
        .say(Some(unit), line.clone(), None, now + ahead);
}

/// Plays a whole line, every frame checked at [`LINE_STEP`]. A line whose authored face ends off
/// rest may step back once, on the frame its data ends.
fn say_line(client: &mut Client, name: &str, authored: &AuthoredLine, jumps: &mut Vec<String>) {
    let line = &authored.face;
    let file = &line.file;
    let lead = registration_lead(&authored.text);
    assert!(
        said(client).is_none(),
        "{name} {file}: no line said before it"
    );
    say_ahead(client, line, lead);
    let end = line.lipsync.end();
    let frames = ((lead + end + 2.0 * lead) / LINE_STEP).ceil() as usize;
    let rests = ends_at_rest(client, &authored.text);
    let mut allowed = !rests;
    for jump in watch(client, frames, LINE_STEP, &format!("{name} {file}")) {
        // The frame its data ends on: said before it, and either saying none after or past the end.
        let ends_here = matches!(jump.said, (Some(before), after) if before < end && after.is_none_or(|t| t >= end));
        if allowed && ends_here {
            allowed = false;
            continue;
        }
        jumps.push(jump.what);
    }
    assert!(
        said(client).is_none(),
        "{name} {file}: the line over, now {:?}",
        spoken(client)
    );
}

/// The idle/run phase hears real answers. Exhaustive injected playback starts only once those
/// answers, including an already chosen delayed line, finish; no transition goes unchecked.
fn finish_automatic(client: &mut Client, name: &str, jumps: &mut Vec<String>) {
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .voice = false;
    let mut patience = super::harness::Patience::of(Duration::from_secs(60));
    loop {
        let pending = client
            .world()
            .resource::<crate::hero_voice::HeroVoice>()
            .resident()
            .is_some_and(|voice| voice.has_pending_line());
        if !pending && said(client).is_none() {
            break;
        }
        patience.pass("the automatic voice line to finish before injected playback");
        jumps.extend(
            watch(
                client,
                1,
                STEP,
                &format!("{name} finishing automatic speech"),
            )
            .into_iter()
            .map(|jump| jump.what),
        );
    }
}

fn check_hero(hero: HeroId, jumps: &mut Vec<String>) {
    // A lockstep view can advance each client frame. Its clock must not run sixteen times
    // faster than these 2 ms frames and interrupt a still-playing line with a later game event.
    let rate = (1.0 / STEP).round().to_string();
    let server = Server::start(1, &["--tick-rate", &rate]);
    let mut client = Client::join(&server, hero, &[]);
    client.cheat(Cheat::StartGame);
    // From the start: posed on every frame (not at most 30 times a second), 2 ms apart, so the
    // first face drawn is the first one checked.
    let world = client.world_mut();
    world.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
        STEP,
    )));
    world.resource_mut::<Stage>().every = true;
    client.within(Duration::from_secs(120), "the portrait's face posed", |c| {
        drawn_face(c).is_some()
    });
    let mut found = Vec::new();
    let name = format!("{hero:?}");
    found.extend(watch(
        &mut client,
        IDLE_FRAMES,
        STEP,
        &format!("{name} idle"),
    ));
    if !found.is_empty() {
        jumps.extend(found.into_iter().map(|j| j.what));
        return;
    }
    let away = client.me().pos + At::from_ints(-1500, -1500);
    client.core_mut().send_order_to(
        None,
        Order::Move {
            target: Target::Pos(away),
        },
    );
    // The answer is under way within the first fifth of a second.
    let soon = 100;
    found.extend(watch(&mut client, soon, STEP, &format!("{name} running")));
    assert!(
        said(&client).is_some(),
        "{name}: the move order answered with a line, being said now"
    );
    found.extend(watch(
        &mut client,
        RUN_FRAMES - soon,
        STEP,
        &format!("{name} running"),
    ));
    jumps.extend(found.into_iter().map(|j| j.what));
    if !jumps.is_empty() {
        return;
    }
    finish_automatic(&mut client, &name, jumps);
    if !jumps.is_empty() {
        return;
    }
    let (lines, silent) = lines(&client, hero);
    assert!(!lines.is_empty(), "{name}: lines with a lip sync");
    assert_eq!(silent, 0, "{name}: every voice line has its lip sync");
    for line in lines {
        say_line(&mut client, &name, &line, jumps);
        if !jumps.is_empty() {
            return;
        }
    }
}

/// A test a hero (each some minutes long, and run on its own), one for every hero.
const TESTED: [HeroId; 2] = [HeroId::PUDGE, HeroId::SHADOW_FIEND];

/// Whether `tested` holds every hero once: as many as there are, each of them.
const fn every_hero_once(tested: &[HeroId]) -> bool {
    let heroes = &bota_core::HEROES;
    if tested.len() != heroes.len() {
        return false;
    }
    let mut h = 0;
    while h < heroes.len() {
        let (mut t, mut found) = (0, 0);
        while t < tested.len() {
            if tested[t].0 == heroes[h].id.0 {
                found += 1;
            }
            t += 1;
        }
        if found != 1 {
            return false;
        }
        h += 1;
    }
    true
}

const _: () = assert!(
    every_hero_once(&TESTED),
    "every hero needs its face test here, once"
);

#[test]
fn the_face_tests_hold_every_hero_once() {
    assert!(
        !every_hero_once(&[HeroId::PUDGE, HeroId::PUDGE]),
        "twice one hero is not both"
    );
    assert!(!every_hero_once(&[HeroId::PUDGE]), "one hero is not both");
    assert!(every_hero_once(&[HeroId::SHADOW_FIEND, HeroId::PUDGE]));
}

fn face_moves_without_a_jump(hero: HeroId) {
    assert!(TESTED.contains(&hero), "{hero:?} is one of TESTED");
    let mut jumps = Vec::new();
    check_hero(hero, &mut jumps);
    assert!(jumps.is_empty(), "{hero:?}'s face jumped: {jumps:#?}");
}

#[test]
fn pudges_face_moves_without_a_jump_idle_running_and_speaking() {
    face_moves_without_a_jump(HeroId::PUDGE);
}

#[test]
fn shadow_fiends_face_moves_without_a_jump_idle_running_and_speaking() {
    face_moves_without_a_jump(HeroId::SHADOW_FIEND);
}
