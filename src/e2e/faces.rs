//! Every hero's face moves as its authored data moves it, and never jumps: the portrait's face
//! (its morph weights as drawn, and the joints its meshes are skinned to) has no discontinuity
//! while the hero stands idle, while it runs and answers its order, and through each of its voice
//! lines (every kind the cache holds: moves, attacks, runes, the Bottle...) over its idle
//! expression, wherever in the expression's loop a line starts, and when one line takes over from
//! another; and the face shown on a display's frames moves as the time of each frame says.
//!
//! The client draws the face on simulated frames 2 ms apart: through a whole loop of the idle
//! clip, the run through a whole loop of its clip with the answer its hero says, and one whole
//! line. The portrait is drawn on every one of them (a frame it is not drawn on holds the face
//! still, to jump it on at the next). Each drawn frame must be the face [`Face::pose`] gives at
//! that frame's moment and line (the face's rigs set every morph weight, so that is all of them),
//! posed by clocks that do not jump (the expression's never restarts; a line's moves on by the
//! frame), and its joints may move [`MAX_TURN`] and [`MAX_MOVE`] a frame. The joints come from
//! the body's clips alone (the expression and the lines move morph weights, nothing else), so
//! these frames cover them whole.
//!
//! Whether that face jumps is then judged apart from the client: a copy of the portrait's face,
//! posed by the client's own [`Face::pose`] and [`Speech`] at any instant, plays its idle
//! expression through a whole loop and every line of the pack said as the hero's voice says it
//! (its face trailing its sound's start by the line's lead), starting at each of [`PHASES`]
//! instants of the expression's loop, and taken over halfway by the next line at every
//! [`TAKEOVER_EVERY`]th, in frames of 2 ms (the idle) and 4 ms (a line). A frame on which a weight
//! moves by more than half [`JUMP`] off what the frames on either side say it should (motion moves
//! alike on neighbouring frames, whatever its sign; a jump moves one frame alone), or all of them
//! together by more than half [`TOTAL_JUMP`], is halved down to [`FINEST`], following each half whose motion can still exceed a cutoff: a jump is a
//! change still more than [`JUMP`] in a weight there, or more than [`TOTAL_JUMP`] in all of them
//! together (a jump does not shrink as the step does; motion does). No speed is capped: a frame
//! with no jump moves a weight up to 0.158 in 4 ms and all of them together up to 1.087 (Pudge's
//! lines taking over one another), 0.0025 and 0.017 in [`FINEST`]. No jump is allowed anywhere: a line that ends, or that another takes
//! over, is let go of gradually ([`crate::talk::RELEASE`]), and so are three lines said within a
//! fifth of a second of each other.
//!
//! The motion sweep uses 30, 60, 144 and 240 Hz and uneven browser frames. The presentation
//! fixture supplies the media time of the audio samples heard on each frame. It also checks
//! delayed startup and resumed playback separately from the uninterrupted frame sequence.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::mesh::morph::MorphWeights;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, EntityId, HeroId, Order, Target, Vec2 as At};

use super::face_frames::Pacing;
use super::harness::{Client, Server};
use crate::portrait::Stage;
use crate::talk::{Face, Faces, Heard, LineFace, Saying, Speech};

/// How near each controller of a line's own face must be to rest where its data ends for the
/// line to end at rest.
const REST: f32 = 0.05;
/// Seconds between frames.
const STEP: f32 = 0.002;
/// How far a line's clock may be off a drawn frame's step: the client's f32 clock is exact to a
/// few hundred microseconds over its first half hour.
const CLOCK_SLACK: f32 = 0.000_25;
/// A change of a morph weight bigger than this at once is a jump: eight times the most any weight
/// moves in [`FINEST`] without one (0.0025, a 4 ms frame moving it 0.158: Pudge's line 26 taking
/// over 25).
const JUMP: f32 = 0.02;
/// The step a frame is halved down to when judging a jump: 1/64 of a line's frame, which the
/// clock ([`MAX_CLOCK`] seconds of f32) resolves to within 4 µs.
const FINEST: f32 = LINE_STEP / 64.0;
/// A change of all morph weights together (the sum of their changes) bigger than this at once is
/// a jump too, a pop spread over several weights each under [`JUMP`]: 2.4 times the most all of
/// them move in [`FINEST`] without one (0.017, a 4 ms frame moving them 1.087: Pudge's line 69
/// taking over 68 while 68 is let go).
const TOTAL_JUMP: f32 = 0.04;
/// The latest instant the copy is posed at, in seconds.
const MAX_CLOCK: f32 = 32.0;
/// Instants of the idle expression's loop each line starts at, [`PHASE_STEP`] apart: 7 s, a
/// whole loop of every hero's expression (Pudge's lasts 7 s, Shadow Fiend's 5.2 s).
const PHASES: usize = 350;
const PHASE_STEP: f32 = 0.02;
/// Most threads playing the lines apart from the client.
const MAX_THREADS: usize = 8;
/// Most a face joint may turn (degrees) or move (inches, against the model's pivot) a frame.
const MAX_TURN: f32 = 1.0;
const MAX_MOVE: f32 = 0.5;
/// Frames of the idle expression: long enough for every hero's to loop once.
const IDLE_FRAMES: usize = 4500;
/// Frames the client draws past the end of a clip's loop (the idle's, the run's), so that its
/// seam, its end back to its start, is checked too.
const SEAM_FRAMES: usize = 50;
/// Most frames the answer to the move order takes to be said whole.
const ANSWER_FRAMES: usize = 2500;
/// Frames played before a line starts and after it ends, twice as many after (the face its own
/// before it, back to its own after).
const LEAD_FRAMES: usize = 25;
/// Seconds between the copy's frames through a whole line.
const LINE_STEP: f32 = 0.004;
/// How many buffers ahead of what is heard a device takes a sound.
const DEVICE_AHEAD: f32 = 2.0;
/// How much more or less than its time's face a face shown on a frame may move (all its weights
/// together): what float rounding leaves.
const SHOWN_SLACK: f32 = 0.02;

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

/// The face on the portrait's stage.
fn stage_face(client: &Client) -> &Face {
    let actor = client.world().resource::<Stage>().actor.as_ref();
    let face = actor.expect("the stage's model").face.as_ref();
    face.expect("the stage's face")
}

/// A copy of the portrait's face, posed apart from the client by the client's own [`Face::pose`]:
/// its meshes' morph weights (as drawn when copied) on entities of a world of its own.
struct Mirror {
    world: World,
    weights: QueryState<&'static mut MorphWeights>,
    nodes: Vec<Entity>,
    face: Face,
}

impl Mirror {
    fn of(client: &Client) -> Mirror {
        let face = stage_face(client);
        let mut world = World::new();
        let nodes: Vec<Entity> = face
            .rigs()
            .map(|(node, _)| {
                let drawn = client.world().get::<MorphWeights>(node);
                world
                    .spawn(drawn.expect("a face mesh's weights").clone())
                    .id()
            })
            .collect();
        let face = face.on_nodes(&nodes);
        let weights = world.query::<&mut MorphWeights>();
        Mirror {
            world,
            weights,
            nodes,
            face,
        }
    }

    /// Whether the face's rigs set every morph weight of its meshes: posed over weights none of
    /// its rules can give (they are held within 0 and 1), none is left.
    fn sets_every_weight(&mut self, faces: &Faces) -> bool {
        let mut weights = self.weights.query_mut(&mut self.world);
        for mut mesh in weights.iter_mut() {
            mesh.weights_mut().fill(-1.0);
        }
        self.pose(0.0, Saying::default(), faces)
            .iter()
            .all(|w| *w >= 0.0)
    }

    /// The face posed at `now` saying what `saying` says: every morph weight of its meshes in
    /// turn.
    fn pose(&mut self, now: f32, saying: Saying, faces: &Faces) -> Vec<f32> {
        let mut weights = self.weights.query_mut(&mut self.world);
        self.face.pose(now, saying, faces, &mut weights);
        let world = &self.world;
        let weights = self
            .nodes
            .iter()
            .map(|&node| world.get::<MorphWeights>(node));
        let weights = weights.map(|w| w.expect("a copied mesh's weights").weights());
        weights.flatten().copied().collect()
    }
}

/// Whether the face drawn now is the face its expression and line give at this moment.
fn drawn_is_posed(client: &Client) -> bool {
    let world = client.world();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    let saying = world.resource::<Speech>().saying(Some(client.me().id), now);
    let drawn = drawn_face(client).map(|(weights, _)| weights);
    Some(Mirror::of(client).pose(now, saying, world.resource())) == drawn
}

/// The controllers of the face on the portrait's stage and their ranges, by token, from its
/// meshes' flex rigs.
fn ranges(client: &mut Client) -> HashMap<u32, (f32, f32)> {
    let world = client.world_mut();
    let pivot = world
        .resource::<Stage>()
        .actor
        .as_ref()
        .expect("the stage's model")
        .pivot;
    let mut out = HashMap::new();
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
/// each controller's rest (by the face's `ranges`) at its last key: read from the record itself.
fn ends_at_rest(ranges: &HashMap<u32, (f32, f32)>, text: &str) -> bool {
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
    names.iter().zip(&last).all(|(name, v)| {
        ranges
            .get(name)
            .is_none_or(|&(min, max)| (min + v * (max - min)).abs() <= REST)
    })
}

/// How far into the line it says the hero is now.
fn said(client: &Client) -> Option<f32> {
    spoken(client).map(|(_, t)| t)
}

fn spoken(client: &Client) -> Option<(String, f32)> {
    let world = client.world();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    let line = world.resource::<Speech>().of(Some(client.me().id), now);
    line.map(|(face, t)| (face.file.clone(), t))
}

/// The morph weight that changed most from `before` to `after`, and by how much.
fn largest_change(before: &[f32], after: &[f32]) -> (usize, f32) {
    let weight = before.iter().zip(after).map(|(a, b)| (a - b).abs());
    weight
        .enumerate()
        .fold((0, 0.0), |m, (i, s)| if s > m.1 { (i, s) } else { m })
}

/// How much the morph weights changed from `before` to `after`, all together.
fn total_change(before: &[f32], after: &[f32]) -> f32 {
    before.iter().zip(after).map(|(a, b)| (a - b).abs()).sum()
}

fn presented_start(client: &Client) -> Option<f32> {
    let world = client.world();
    let time = world.resource::<Time<bevy::time::Real>>();
    let speech = world.resource::<Speech>();
    let (face, _) = speech.of(Some(client.me().id), time.elapsed_secs())?;
    let (clock, _) = speech.active_playback()?;
    let sample = world
        .resource::<crate::audio_output::Output>()
        .presentation(time.elapsed())?;
    match sample(clock, true) {
        Heard::At(position) => Some(position - crate::talk::PHONEME_CENTRE),
        Heard::Scheduled(_) => Some(-face.lipsync.lead()),
        _ => None,
    }
}

/// How a line's clock moved from one drawn frame to the next, when it jumped: a line said on
/// both moves on by the frame's `step` (or waits at its lead for its sound to begin); one
/// starting is said from its lead before its start (or earlier, a line registered ahead).
fn clock_jump(
    before: &Option<(String, f32)>,
    after: &Option<(String, f32)>,
    step: f32,
    presented_start: Option<f32>,
) -> Option<String> {
    match (before, after) {
        // A line waits at its lead before its first viseme, the face at rest, until its sound
        // begins: it may hold there.
        (Some((was, t0)), Some((file, t1)))
            if was == file && t1 == t0 && *t0 <= -crate::face::PHONEME_FILTER + CLOCK_SLACK =>
        {
            None
        }
        (Some((was, t0)), Some((file, t1)))
            if was == file && *t0 < -crate::talk::PHONEME_CENTRE =>
        {
            (*t1 + CLOCK_SLACK < *t0).then(|| format!("{file}'s scheduled preroll ran backwards"))
        }
        (Some((was, t0)), Some((file, t1))) if was == file => ((t1 - t0 - step).abs()
            > CLOCK_SLACK)
            .then(|| format!("{file}'s clock moved {:.5} s in a {step} s frame", t1 - t0)),
        (_, Some((file, t))) => {
            let wrong = presented_start.map_or_else(
                || *t > -crate::face::PHONEME_FILTER + CLOCK_SLACK,
                |want| (*t - want).abs() > CLOCK_SLACK,
            );
            wrong
                .then(|| format!("{file} started {t:.5} s in; presented start {presented_start:?}"))
        }
        (_, None) => None,
    }
}

/// Runs `frames` frames of `step` seconds, each checked against the one before: the frames
/// something jumped on. A frame jumps when a joint moves more than its limits, when the face
/// drawn is not the face [`Face::pose`] gives at the frame's moment and line (or none is drawn),
/// or when the clocks it is posed by jump: the expression's restarts, or a line's ([`clock_jump`]);
/// and a frame the portrait is not drawn on holds the face still, to jump it on at the next.
fn watch(client: &mut Client, frames: usize, step: f32, phase: &str) -> Vec<String> {
    watch_until(client, frames, step, phase, |_| false)
}

/// [`watch`], ended after the first frame `done` holds on.
fn watch_until(
    client: &mut Client,
    frames: usize,
    step: f32,
    phase: &str,
    mut done: impl FnMut(&Client) -> bool,
) -> Vec<String> {
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            step,
        )));
    let mut out = Vec::new();
    let began = |client: &Client| drawn_face(client).map(|_| stage_face(client).began);
    let mut last = (drawn_face(client), spoken(client), began(client));
    for frame in 0..frames {
        client.frames(1);
        let now = (drawn_face(client), spoken(client), began(client));
        let mut what = Vec::new();
        match (&last.0, &now.0) {
            (Some((w0, j0)), Some((w1, j1))) => {
                assert_eq!(
                    (w0.len(), j0.len()),
                    (w1.len(), j1.len()),
                    "{phase}: one face"
                );
                let started = now
                    .1
                    .as_ref()
                    .is_some_and(|(file, _)| last.1.as_ref().is_none_or(|(was, _)| was != file));
                let presented = if started {
                    presented_start(client)
                } else {
                    None
                };
                what.extend(clock_jump(&last.1, &now.1, step, presented));
                if last.2 != now.2 {
                    what.push(format!(
                        "the expression restarted: {:?} -> {:?}",
                        last.2, now.2
                    ));
                }
                for (k, ((r0, t0), (r1, t1))) in j0.iter().zip(j1).enumerate() {
                    let turn = r0.angle_between(*r1).to_degrees();
                    let moved = t0.distance(*t1);
                    if turn > MAX_TURN || moved > MAX_MOVE {
                        what.push(format!("joint {k} turned {turn:.2} deg, moved {moved:.2}"));
                    }
                }
                if !drawn_is_posed(client) {
                    what.push("not the face of its expression and line now".into());
                }
                if !client.world().resource::<Stage>().drawing() {
                    what.push("the portrait not drawn: the face held from a frame before".into());
                }
            }
            _ => what.push("no face drawn".into()),
        }
        if !what.is_empty() {
            out.push(format!(
                "{phase} frame {frame}: {}; speech {:?} -> {:?}",
                what.join(", "),
                last.1,
                now.1
            ));
        }
        last = now;
        if done(client) {
            break;
        }
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

/// The frames of `step` a whole line is played over, and the lead it starts after: the face its
/// own for the lead before it, then the line, then back to its own for twice the lead.
fn line_frames(authored: &AuthoredLine, step: f32) -> (usize, f32) {
    let lead = registration_lead(&authored.text);
    let end = authored.face.lipsync.end();
    (((lead + end + 2.0 * lead) / step).ceil() as usize, lead)
}

/// The same sample should drive the same pose whatever latency the output reports.
#[test]
fn the_mouth_follows_the_presented_samples_at_different_output_latencies() {
    let face = timed_face(97);
    for pacing in [
        Pacing::Hz30,
        Pacing::Hz60,
        Pacing::Hz144,
        Pacing::Hz240,
        Pacing::Browser,
    ] {
        for buffers in [1.0, 2.0, 4.0] {
            let latency = buffers * pacing.device_buffer().as_secs_f32();
            let mut speech = Speech::default();
            speech.say(None, face.clone(), Some(Entity::PLACEHOLDER), 0.0);
            let mut now = 0.0;
            for frame in pacing.frames(1.0) {
                now += frame.as_secs_f32();
                let audible = (now - latency).max(0.0);
                speech.hear(now, Heard::At(audible));
                // The centre of the forward phoneme filter is at the presented sample.
                let want = audible - crate::talk::PHONEME_CENTRE;
                let (said, jaw) = timed_mouth(&speech, now);
                if audible == 0.0 {
                    assert_eq!(jaw, 0.0, "{} before {latency:.4}", pacing.name());
                } else {
                    assert!(
                        (said - want).abs() < 0.0002,
                        "{} at {now:.4}: face {said:.4}, expected {want:.4}",
                        pacing.name()
                    );
                    assert!(
                        (jaw - mouth(&face, want)).abs() < 0.0002,
                        "{} at {now:.4}: jaw {jaw:.4}",
                        pacing.name()
                    );
                }
            }
        }
    }
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

/// The line the client plays whole: the shortest whose own face ends off rest (and so eases out
/// of the face as it ends), else the shortest.
fn drawn_line<'a>(
    lines: &'a [AuthoredLine],
    ranges: &HashMap<u32, (f32, f32)>,
) -> &'a AuthoredLine {
    let key = |line: &AuthoredLine| (ends_at_rest(ranges, &line.text), line_frames(line, STEP).0);
    let line = lines.iter().min_by_key(|line| key(line));
    line.expect("a line with a lip sync")
}

/// Plays a whole line on the client, every frame checked at [`STEP`]. The hero starts
/// saying it a lead from now, the face still its own until then: no frame of the face is left
/// unchecked by it.
fn say_line(client: &mut Client, name: &str, authored: &AuthoredLine) -> Vec<String> {
    let line = &authored.face;
    let file = &line.file;
    let (frames, lead) = line_frames(authored, STEP);
    assert!(
        said(client).is_none(),
        "{name} {file}: no line said before it"
    );
    let unit = client.me().id;
    let world = client.world_mut();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    let mut speech = world.resource_mut::<Speech>();
    speech.say(Some(unit), line.clone(), None, now + lead);
    let found = watch(client, frames, STEP, &format!("{name} {file}"));
    let jumps = found;
    assert!(
        said(client).is_none(),
        "{name} {file}: the line over, now {:?}",
        spoken(client)
    );
    jumps
}

/// The lines said in turn, as the hero's voice says them: each line's face, and the instant it
/// is said at (the line at its start then, the one before it leaving).
type Script<'s> = &'s [(Arc<LineFace>, f32)];

/// The most a frame that held no jump moved one weight, and all of them together, and where.
#[derive(Default)]
struct Fastest {
    weight: (f32, String),
    total: (f32, String),
}

impl Fastest {
    fn note(&mut self, (weight, total): (f32, f32), at: impl Fn() -> String) {
        if weight > self.weight.0 {
            self.weight = (weight, at());
        }
        if total > self.total.0 {
            self.total = (total, at());
        }
    }

    fn join(&mut self, other: Fastest) {
        self.note((other.weight.0, 0.0), || other.weight.1.clone());
        self.note((0.0, other.total.0), || other.total.1.clone());
    }
}

/// The client's face played apart from the client, its expression's clock started at 0: at any
/// instant, a [`Speech`] of the client's is told the lines said by then (each at its instant)
/// and ends them by that instant, then [`Face::pose`] poses the copy, as a client frame does.
struct Played<'a> {
    mirror: Mirror,
    faces: &'a Faces,
    unit: EntityId,
}

impl<'a> Played<'a> {
    fn of(mut mirror: Mirror, faces: &'a Faces, unit: EntityId) -> Self {
        mirror.face.began = 0.0;
        Played {
            mirror,
            faces,
            unit,
        }
    }

    /// The face at `now` with `script` said by then, and how far into its line the hero says it
    /// is.
    fn at(&mut self, now: f32, script: Script) -> (Vec<f32>, Option<f32>) {
        assert!(
            now < MAX_CLOCK,
            "posed within the clock's resolution: {now} s"
        );
        let mut speech = Speech::default();
        for (face, said) in script.iter().filter(|(_, said)| *said <= now) {
            speech.finish(*said);
            self.settle(&mut speech, *said);
            speech.say(Some(self.unit), face.clone(), None, *said);
        }
        speech.finish(now);
        self.posed(now, &mut speech)
    }

    /// Takes the lines `speech` let go into what is left of the lines before, as the portrait
    /// does on each frame.
    fn settle(&mut self, speech: &mut Speech, now: f32) {
        self.mirror
            .face
            .settle(speech, Some(self.unit), now, self.faces);
    }

    /// The face `speech` gives at `now` (taking in the lines it let go first, as the portrait
    /// does), and how far into its line the hero says it is.
    fn posed(&mut self, now: f32, speech: &mut Speech) -> (Vec<f32>, Option<f32>) {
        self.settle(speech, now);
        let saying = speech.saying(Some(self.unit), now);
        let t = saying.line.map(|(_, t)| t);
        (self.mirror.pose(now, saying, self.faces), t)
    }

    /// Plays `frames` frames of `step` seconds from `start`, saying `script`, each checked
    /// against the one before: a frame on which a weight moves by more than half [`JUMP`] off
    /// what the frames on either side say it should (or all of them together by more than half
    /// [`TOTAL_JUMP`]) is halved down to [`FINEST`], and holds a jump
    /// when the change is still more than [`JUMP`] in a weight there (or [`TOTAL_JUMP`] in all).
    fn play(
        &mut self,
        (start, frames, step): (f32, usize, f32),
        script: Script,
        phase: &str,
        fastest: &mut Fastest,
    ) -> Vec<String> {
        let halvings = (step / FINEST).log2().round() as u32;
        let at = |frame: usize| start + frame as f32 * step;
        let faces: Vec<_> = (0..=frames).map(|k| self.at(at(k), script)).collect();
        let change = |k: usize| -> Option<Vec<f32>> {
            let (a, b) = (&faces.get(k.checked_sub(1)?)?.0, &faces.get(k)?.0);
            Some(b.iter().zip(a).map(|(b, a)| b - a).collect())
        };
        let mut out = Vec::new();
        for frame in 1..=frames {
            let this = change(frame).expect("a frame's change");
            // What the frames around it say it should move: motion moves alike on frames side by
            // side whatever its sign or speed, a jump moves one frame alone.
            let around: Vec<Vec<f32>> = [frame - 1, frame + 1]
                .into_iter()
                .filter_map(change)
                .collect();
            let off = |w: usize| {
                let expected =
                    around.iter().map(|d| d[w]).sum::<f32>() / around.len().max(1) as f32;
                (this[w] - expected).abs()
            };
            let alone = (0..this.len()).map(off).fold(0.0, f32::max);
            let alone_total: f32 = (0..this.len()).map(off).sum();
            let (before, after) = (&faces[frame - 1], &faces[frame]);
            let jump = (alone > JUMP / 2.0 || alone_total > TOTAL_JUMP / 2.0)
                .then(|| {
                    self.halve(
                        (at(frame - 1), before),
                        (at(frame), after),
                        halvings,
                        script,
                    )
                })
                .flatten();
            match jump {
                Some((said, what)) => out.push(format!(
                    "{phase} frame {frame}: {what} at once; said {said:?}"
                )),
                None => {
                    let (i, by) = largest_change(&before.0, &after.0);
                    let total = total_change(&before.0, &after.0);
                    fastest.note((by, total), || format!("{phase} frame {frame}, weight {i}"));
                }
            }
        }
        out
    }

    /// Halves the step from `before` to `after` `halvings` times. A jump in one weight can
    /// occur in a different half from the largest summed motion. At the finest step, a jump
    /// changes a weight by more than
    /// [`JUMP`] or all of them more than [`TOTAL_JUMP`] (how far into the line it is said on both
    /// sides, and what jumped).
    fn halve(
        &mut self,
        before: (f32, &(Vec<f32>, Option<f32>)),
        after: (f32, &(Vec<f32>, Option<f32>)),
        halvings: u32,
        script: Script,
    ) -> Option<((Option<f32>, Option<f32>), String)> {
        Self::subframe_jump(before, after, halvings, &mut |at| self.at(at, script))
    }

    fn subframe_jump(
        (lo, before): (f32, &(Vec<f32>, Option<f32>)),
        (hi, after): (f32, &(Vec<f32>, Option<f32>)),
        halvings: u32,
        pose: &mut impl FnMut(f32) -> (Vec<f32>, Option<f32>),
    ) -> Option<((Option<f32>, Option<f32>), String)> {
        let (i, by) = largest_change(&before.0, &after.0);
        let total = total_change(&before.0, &after.0);
        if halvings == 0 {
            let said = (before.1, after.1);
            return match (by > JUMP, total > TOTAL_JUMP) {
                (true, _) => Some((said, format!("weight {i} by {by:.3}"))),
                (false, true) => Some((said, format!("all weights by {total:.3}"))),
                (false, false) => None,
            };
        }
        let mid = (lo + hi) / 2.0;
        let middle = pose(mid);
        let explore =
            |a: &[f32], b: &[f32]| largest_change(a, b).1 > JUMP || total_change(a, b) > TOTAL_JUMP;
        if explore(&before.0, &middle.0)
            && let Some(jump) =
                Self::subframe_jump((lo, before), (mid, &middle), halvings - 1, pose)
        {
            return Some(jump);
        }
        if explore(&middle.0, &after.0) {
            return Self::subframe_jump((mid, &middle), (hi, after), halvings - 1, pose);
        }
        None
    }

    /// Says lines `start` seconds into the expression's loop, as the hero's voice says them (each
    /// from its start at once), each `after` seconds after the one before (the first a lead
    /// after `start`), from the face at rest before them to the face let go back to its own
    /// after: the jumps.
    fn say(
        &mut self,
        start: f32,
        lines: &[(&AuthoredLine, f32)],
        fastest: &mut Fastest,
    ) -> Vec<String> {
        let lead = LINE_STEP * LEAD_FRAMES as f32;
        let mut said = start + lead;
        let mut script = Vec::new();
        for (line, after) in lines {
            said += after;
            script.push((line.face.clone(), said));
        }
        let end = said + script.last().map_or(0.0, |(f, _)| f.lipsync.end());
        let frames = ((end + crate::talk::LET_GO + lead - start) / LINE_STEP).ceil() as usize;
        let names: Vec<&str> = script.iter().map(|(f, _)| f.file.as_str()).collect();
        let phase = format!("{} from {:.0} ms", names.join(" then "), start * 1000.0);
        let found = self.play((start, frames, LINE_STEP), &script, &phase, fastest);
        let after = self.at(start + frames as f32 * LINE_STEP, &script).1;
        assert!(after.is_none(), "{phase}: the line over, now {after:?}");
        found
    }

    /// The line's face on the frames of `pacing` as the client shows it, against the face its
    /// frames' times give: the line is requested on a frame and heard after the output delay. On
    /// each frame the face shown must move as much as the face of its time does, within
    /// [`SHOWN_SLACK`] of all its weights together: the frames it moves more or less on (it
    /// holds still and then jumps on), and the most it did.
    fn shown(&mut self, line: &AuthoredLine, pacing: Pacing) -> (Vec<String>, f32) {
        let frames = pacing.frames(line.face.lipsync.end() + crate::talk::LET_GO + 0.3);
        let times: Vec<f32> = frames
            .iter()
            .scan(0.0, |t, step| {
                *t += step.as_secs_f32();
                Some(*t)
            })
            .collect();
        let (said, heard) = (
            times[0],
            times[1] + pacing.device_buffer().as_secs_f32() * DEVICE_AHEAD,
        );
        let mut speech = Speech::default();
        let mut last: Option<(Vec<f32>, Vec<f32>)> = None;
        let (mut out, mut most) = (Vec::new(), 0.0f32);
        for (frame, &now) in times.iter().enumerate() {
            if frame == 0 {
                speech.say(
                    Some(self.unit),
                    line.face.clone(),
                    Some(Entity::PLACEHOLDER),
                    now,
                );
            }
            let media = now - heard;
            speech.hear(
                now,
                if media < 0.0 {
                    Heard::Scheduled(media)
                } else {
                    Heard::At(media)
                },
            );
            speech.finish(now);
            let shown = self.posed(now, &mut speech).0;
            let lead = line.face.lipsync.lead();
            let centre = crate::talk::PHONEME_CENTRE;
            let start = (heard - (lead - centre)).max(said);
            // An output with a short scheduling horizon starts at rest and reaches the
            // aligned pose at sample zero. The copy has neither device nor playback tracking.
            let ideal = if now < heard {
                let progress = ((now - start) / (heard - start)).clamp(0.0, 1.0);
                let at = -lead + (lead - centre) * progress;
                let mut reference = Speech::default();
                reference.say(Some(self.unit), line.face.clone(), None, now);
                reference.hold(at);
                self.posed(now, &mut reference).0
            } else {
                self.at(now, &[(line.face.clone(), heard + centre - lead)])
                    .0
            };
            let own = ideal;
            if let Some((shown_before, own_before)) = &last {
                let off: f32 = shown
                    .iter()
                    .zip(shown_before)
                    .zip(own.iter().zip(own_before))
                    .map(|((s, s0), (o, o0))| ((s - s0) - (o - o0)).abs())
                    .sum();
                most = most.max(off);
                if off > SHOWN_SLACK {
                    out.push(format!(
                        "{} at {}: frame {frame} ({now:.4} s) moves {off:.3} off its time's face",
                        line.face.file,
                        pacing.name()
                    ));
                }
            }
            last = Some((shown, own));
        }
        (out, most)
    }
}

/// Every [`TAKEOVER_EVERY`]th phase, each line is also taken over halfway by the next one, and
/// said with the next two 0.06 s apart.
const TAKEOVER_EVERY: usize = 10;

/// The face's jumps apart from the client: its idle expression through a whole loop, and every
/// line starting at each of [`PHASES`] instants of the loop (and taken over by the next line at
/// some of them), the phases shared among threads (a copy of the face, `copies[0]` for the idle
/// and one more for each thread).
fn played_apart(
    (mut copies, faces, unit): (Vec<Mirror>, &Faces, EntityId),
    name: &str,
    lines: &[AuthoredLine],
) -> Vec<String> {
    let mut fastest = Fastest::default();
    let idle = (0.0, IDLE_FRAMES, STEP);
    let mut played = Played::of(copies.remove(0), faces, unit);
    let found = played.play(idle, &[], &format!("{name} idle"), &mut fastest);
    let mut jumps: Vec<(usize, String)> = found.into_iter().map(|j| (0, j)).collect();
    let threads = copies.len();
    let copies = copies
        .into_iter()
        .map(|mirror| Played::of(mirror, faces, unit));
    std::thread::scope(|scope| {
        let workers: Vec<_> = copies
            .into_iter()
            .enumerate()
            .map(|(first, mut played)| {
                scope.spawn(move || {
                    let mut fastest = Fastest::default();
                    let mut jumps = Vec::new();
                    for phase in (first..PHASES).step_by(threads) {
                        let start = phase as f32 * PHASE_STEP;
                        for (k, line) in lines.iter().enumerate() {
                            let next = &lines[(k + 1) % lines.len()];
                            let third = &lines[(k + 2) % lines.len()];
                            let mut found = played.say(start, &[(line, 0.0)], &mut fastest);
                            if phase % TAKEOVER_EVERY == 0 {
                                let half = line.face.lipsync.end() / 2.0;
                                let over = [(line, 0.0), (next, half)];
                                found.extend(played.say(start, &over, &mut fastest));
                                // Keep the 60 ms case and cover three starts within 100 ms.
                                for spacing in [0.06, 0.045] {
                                    let quick = [(line, 0.0), (next, spacing), (third, spacing)];
                                    found.extend(played.say(start, &quick, &mut fastest));
                                }
                            }
                            jumps.extend(found.into_iter().map(|j| (phase, format!("{name} {j}"))));
                        }
                    }
                    (jumps, fastest)
                })
            })
            .collect();
        for worker in workers {
            let (found, theirs) = worker.join().expect("a thread playing the lines");
            jumps.extend(found);
            fastest.join(theirs);
        }
    });
    jumps.sort();
    let mut phases: Vec<usize> = jumps.iter().map(|(phase, _)| *phase).collect();
    phases.dedup();
    let mut out: Vec<String> = jumps.iter().take(20).map(|(_, j)| j.clone()).collect();
    if !jumps.is_empty() {
        out.push(format!(
            "{} jumps in all, at {} of {PHASES} phases; fastest without a jump: a weight {:.3} at {}, \
             all {:.3} at {}",
            jumps.len(),
            phases.len(),
            fastest.weight.0,
            fastest.weight.1,
            fastest.total.0,
            fastest.total.1
        ));
    }
    out
}

/// How the sound's output misbehaves while a line plays: a frame of half a second (the client
/// stalled; the output played on), or the output suspended for 0.6 s and resumed (a browser
/// suspends its audio when its tab is hidden) while the frames go on.
#[derive(Clone, Copy, Debug)]
enum Stall {
    Hitch,
    Suspended,
}

/// How long a stall lasts, and how much faster or slower than the frames a line's clock may run
/// while it goes after its sound (the client's half, and float slack).
const STALL: f32 = 0.5;
const SUSPENDED: f32 = 0.6;
const CLOCK_SPEED: f32 = 0.51;

/// Every line's clock, with presentation stalled as `stall` says, from
/// the frame its sound is heard to its end: the frames its clock jumped on (it may go after the
/// sound only at most half a second a second faster or slower than the frames), and whether it
/// went after the sound at all on some line.
fn clocks_hold(unit: EntityId, lines: &[AuthoredLine], stall: Stall) -> (Vec<String>, bool) {
    let pacing = match stall {
        Stall::Hitch => Pacing::Hz240,
        Stall::Suspended => Pacing::Browser,
    };
    let buffer = pacing.device_buffer().as_secs_f32();
    let (mut out, mut went_after) = (Vec::new(), false);
    for line in lines {
        let end = line.face.lipsync.end();
        let mut frames = pacing.frames(end + 2.0);
        let middle = frames.len() / 3;
        if let Stall::Hitch = stall {
            frames.insert(middle, Duration::from_secs_f32(STALL));
        }
        let times: Vec<f32> = frames
            .iter()
            .scan(0.0, |t, step| {
                *t += step.as_secs_f32();
                Some(*t)
            })
            .collect();
        let (heard, stalled) = (times[1] + buffer * DEVICE_AHEAD, times[middle]);
        // Keep the last presented sample during suspension. On resume, let the output reach
        // the end of one queued buffer before the renderer next samples its clock.
        let played = |now: f32| match stall {
            Stall::Suspended if now >= stalled + SUSPENDED => now - SUSPENDED - heard + buffer,
            Stall::Suspended if now >= stalled => stalled - heard,
            _ => now - heard,
        };
        let mut speech = Speech::default();
        let mut last: Option<(f32, f32)> = None;
        for (frame, &now) in times.iter().enumerate() {
            if frame == 0 {
                speech.say(
                    Some(unit),
                    line.face.clone(),
                    Some(Entity::PLACEHOLDER),
                    now,
                );
            }
            let position = played(now).max(0.0);
            speech.hear(now, Heard::At(position));
            speech.finish(now);
            let Some((_, t)) = speech.of(Some(unit), now) else {
                assert!(
                    position == 0.0 || position >= end,
                    "face ended while media played at {position:.4} of {end:.4}"
                );
                continue;
            };
            if let Some((before, t0)) = last.filter(|_| now > heard) {
                let (dt, moved) = (now - before, t - t0);
                let available = (position - played(before).max(0.0)).max(0.0);
                went_after |= t + crate::talk::PHONEME_CENTRE < position - CLOCK_SLACK;
                let slowest = (dt * (1.0 - CLOCK_SPEED)).min(available);
                if moved < slowest - CLOCK_SLACK
                    || moved > (1.0 + CLOCK_SPEED) * dt + CLOCK_SLACK
                    || t + crate::talk::PHONEME_CENTRE > position + CLOCK_SLACK
                {
                    out.push(format!(
                        "{} {stall:?}: frame {frame} ({now:.3} s) moved the line {moved:.4} s in \
                         {dt:.4} s",
                        line.face.file
                    ));
                }
            }
            last = Some((now, t));
        }
    }
    out.truncate(20);
    (out, went_after)
}

/// Every line's face against its presentation time at each frame pacing.
fn shown_apart(played: &mut Played, name: &str, lines: &[AuthoredLine]) -> Vec<String> {
    let mut out = Vec::new();
    let mut most = 0.0f32;
    for pacing in [
        Pacing::Hz30,
        Pacing::Hz60,
        Pacing::Hz144,
        Pacing::Hz240,
        Pacing::Browser,
    ] {
        for line in lines {
            let (off, worst) = played.shown(line, pacing);
            most = most.max(worst);
            out.extend(off.into_iter().map(|o| format!("{name} {o}")));
        }
    }
    let count = out.len();
    out.truncate(20);
    if count > 0 {
        out.push(format!(
            "{count} frames shown off their time's face; the most {most:.3} of all weights"
        ));
    }
    out
}

/// The idle/run phase hears real answers. Injected playback starts only once those answers,
/// including an already chosen delayed line, finish; no transition goes unchecked.
fn finish_automatic(client: &mut Client, name: &str) -> Vec<String> {
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .voice = false;
    let mut patience = super::harness::Patience::of(Duration::from_secs(60));
    let mut jumps = Vec::new();
    loop {
        let pending = client
            .world()
            .resource::<crate::hero_voice::HeroVoice>()
            .resident()
            .is_some_and(|voice| voice.has_pending_line());
        if !pending
            && !client.world().resource::<Speech>().has_waiting_line()
            && said(client).is_none()
        {
            return jumps;
        }
        patience.pass("the automatic voice line to finish before injected playback");
        let phase = format!("{name} finishing automatic speech");
        let found = watch(client, 1, STEP, &phase);
        jumps.extend(found);
    }
}

/// The frames of [`STEP`] that play the stage model's clip `name` through a whole loop and past
/// its seam.
fn loop_frames(client: &Client, name: &str) -> usize {
    let stage = client.world().resource::<Stage>();
    let model = stage.actor.as_ref().expect("the stage's model").model();
    let bank = client.world().resource::<crate::units::ModelBank>();
    let clip = bank.0[model].clips.iter().find(|(clip, ..)| clip == name);
    let seconds = clip.unwrap_or_else(|| panic!("the model's {name} clip")).2;
    assert!(
        seconds.is_finite() && seconds > 0.0 && seconds < 60.0,
        "the {name} clip lasts some seconds: {seconds}"
    );
    (seconds / STEP).ceil() as usize + SEAM_FRAMES
}

/// The client draws the face idle through a loop of its clip, running through a loop of its
/// clip and answering the order whole, and saying `line` whole.
fn drawn_on_the_client(client: &mut Client, name: &str, line: &AuthoredLine) -> Vec<String> {
    let idle = loop_frames(client, "idle");
    let found = watch(client, idle, STEP, &format!("{name} idle"));
    if !found.is_empty() {
        return found;
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
    let running = format!("{name} running");
    let mut found = watch(client, soon, STEP, &running);
    assert!(
        said(client).is_some(),
        "{name}: the move order answered with a line, being said now"
    );
    let run = loop_frames(client, "run").saturating_sub(soon);
    found.extend(watch(client, run, STEP, &running));
    found.extend(watch_until(client, ANSWER_FRAMES, STEP, &running, |c| {
        said(c).is_none()
    }));
    let mut jumps = found;
    jumps.extend(finish_automatic(client, name));
    if jumps.is_empty() {
        jumps.extend(say_line(client, name, line));
    }
    jumps
}

fn check_hero(hero: HeroId) -> Vec<String> {
    // A lockstep view can advance each client frame. Its clock must not run sixteen times
    // faster than these 2 ms frames and interrupt a still-playing line with a later game event.
    let rate = (1.0 / STEP).round().to_string();
    let server = Server::start(1, &["--tick-rate", &rate]);
    let mut client = Client::join(&server, hero, &[]);
    // The answer's sound plays to an output a device buffer at a time on the client's own clock,
    // as a player's device takes it: the line's clock must keep the frames' time through it
    // ([`clock_jump`]), the steps of the reported position never moving it.
    client
        .world_mut()
        .insert_resource(crate::audio_output::DeviceBuffers {
            buffer: Pacing::Hz60.device_buffer(),
            ahead: DEVICE_AHEAD as u32,
            sample_rate: Pacing::Hz60.sample_rate(),
        });
    client.cheat(Cheat::StartGame);
    // From the start: 2 ms apart, so the first face drawn is the first one checked.
    let world = client.world_mut();
    world.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
        STEP,
    )));
    client.within(Duration::from_secs(120), "the portrait's face posed", |c| {
        drawn_face(c).is_some()
    });
    // The portrait is drawn while the HUD is on screen, not under the loading screen.
    super::hud_layout::on_screen(&mut client);
    let name = format!("{hero:?}");
    let (lines, silent) = lines(&client, hero);
    assert!(!lines.is_empty(), "{name}: lines with a lip sync");
    assert_eq!(silent, 0, "{name}: every voice line has its lip sync");
    let ranges = ranges(&mut client);
    let faces = client.world().resource();
    assert!(
        Mirror::of(&client).sets_every_weight(faces),
        "{name}: the face's rigs set every morph weight of its meshes"
    );
    let line = drawn_line(&lines, &ranges);
    let jumps = drawn_on_the_client(&mut client, &name, line);
    if !jumps.is_empty() {
        return jumps;
    }
    // The copies taken, the client and the server go: other tests' clients may run meanwhile.
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut copies: Vec<Mirror> = (0..=threads.clamp(1, MAX_THREADS))
        .map(|_| Mirror::of(&client))
        .collect();
    let shown = Mirror::of(&client);
    let faces: Faces = client.world().resource::<Faces>().clone();
    let unit = client.me().id;
    drop(client);
    drop(server);
    let mut jumps = shown_apart(&mut Played::of(shown, &faces, unit), &name, &lines);
    for stall in [Stall::Hitch, Stall::Suspended] {
        let (jumped, went_after) = clocks_hold(unit, &lines, stall);
        jumps.extend(jumped);
        if let Stall::Suspended = stall {
            assert!(
                went_after,
                "{name}: a suspended output put some line's clock after its sound"
            );
        }
    }
    jumps.extend(played_apart(
        (std::mem::take(&mut copies), &faces, unit),
        &name,
        &lines,
    ));
    jumps
}

/// A test a hero, one for every hero.
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
    let jumps = check_hero(hero);
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

fn timed_face(code: u32) -> Arc<LineFace> {
    let data = format!("dbota-lipsync 1\nphoneme 0.0 0.4 {code}\nphoneme 0.4 2.0 98\n");
    Arc::new(LineFace {
        lipsync: crate::face::Lipsync::parse(&data).expect("a timed face"),
        file: format!("line {code}"),
    })
}

fn mouth(face: &LineFace, position: f32) -> f32 {
    let table = crate::face::Phonemes::parse(
        "dbota-phonemes 1\nkeys jaw\nphoneme 97 1.0\nphoneme 98 0.25\n",
    )
    .expect("a phoneme table");
    let mut controls = crate::face::Controls::new();
    face.lipsync
        .apply(position, &table, &HashMap::new(), &mut controls);
    controls
        .get(&crate::face::token("jaw"))
        .copied()
        .unwrap_or_default()
}

fn timed_mouth(speech: &Speech, now: f32) -> (f32, f32) {
    speech.of(None, now).map_or((0.0, 0.0), |(face, position)| {
        (position, mouth(face, position))
    })
}

#[test]
fn playback_clock_stays_at_rest_until_the_first_audible_sample() {
    let line = timed_face(97);
    let mut speech = Speech::default();
    speech.say(None, line, Some(Entity::PLACEHOLDER), 0.0);
    for frame in 1..=12 {
        let now = frame as f32 / 60.0;
        speech.hear(now, Heard::At(0.0));
        assert_eq!(
            timed_mouth(&speech, now).1,
            0.0,
            "no sample audible at {now:.3}"
        );
    }
}

#[test]
fn playback_clock_keeps_the_media_position_at_the_first_frame_after_a_hitch() {
    let line = timed_face(97);
    for rate in [30, 60, 144, 240] {
        let mut speech = Speech::default();
        speech.say(None, line.clone(), Some(Entity::PLACEHOLDER), 0.0);
        let now = 0.5 + 1.0 / rate as f32;
        let played = now - 0.1;
        speech.hear(now, Heard::At(played));
        let expected = played - crate::talk::PHONEME_CENTRE;
        let (position, jaw) = timed_mouth(&speech, now);
        assert!(
            (position - expected).abs() < CLOCK_SLACK,
            "{rate} Hz: media position {position:.4} vs {expected:.4}"
        );
        assert!(
            (jaw - mouth(&line, expected)).abs() < CLOCK_SLACK,
            "{rate} Hz: jaw {jaw:.4}"
        );
    }
}

#[test]
fn playback_clock_holds_on_stall_and_rejoins_the_audible_position() {
    let line = timed_face(97);
    let mut speech = Speech::default();
    speech.say(None, line.clone(), Some(Entity::PLACEHOLDER), 0.0);
    speech.hear(0.2, Heard::At(0.2));
    let (position, jaw) = timed_mouth(&speech, 0.2);
    speech.hear(0.7, Heard::At(0.2));
    let held = timed_mouth(&speech, 0.7);
    assert!(
        (held.0 - position).abs() < CLOCK_SLACK,
        "the stalled clock moved from {position:.4} to {:.4}",
        held.0
    );
    assert!(
        (held.1 - jaw).abs() < CLOCK_SLACK,
        "the stalled jaw moved from {jaw:.4} to {:.4}",
        held.1
    );
    for frame in 1..=240 {
        let now = 0.7 + frame as f32 / 240.0;
        let played = now - 0.5;
        speech.hear(now, Heard::At(played));
        speech.finish(now);
        let expected = played - crate::talk::PHONEME_CENTRE;
        let (position, jaw) = timed_mouth(&speech, now);
        assert!(
            position <= expected + CLOCK_SLACK,
            "face {position:.4} ahead of media {expected:.4}"
        );
        if frame == 240 {
            assert!(
                (position - expected).abs() < CLOCK_SLACK,
                "face {position:.4} never rejoined media {expected:.4}"
            );
            assert!(
                (jaw - mouth(&line, expected)).abs() < CLOCK_SLACK,
                "jaw never rejoined sound: {jaw:.4}"
            );
        }
    }
}

#[test]
fn playback_clock_rests_when_the_output_disappears() {
    for heard_before_loss in [None, Some(0.2)] {
        let line = timed_face(97);
        let mut speech = Speech::default();
        speech.say(None, line, Some(Entity::PLACEHOLDER), 0.0);
        if let Some(played) = heard_before_loss {
            speech.hear(0.2, Heard::At(played));
            speech.hear(0.3, Heard::At(played + 0.1));
        }
        speech.hear(0.31, Heard::Gone);
        speech.finish(0.5);
        let (position, jaw) = timed_mouth(&speech, 0.5);
        assert_eq!(
            (position, jaw),
            (0.0, 0.0),
            "output lost at {heard_before_loss:?}: face {position:.4}, jaw {jaw:.4}"
        );
    }
}

#[test]
fn playback_clock_keeps_the_outgoing_face_until_the_takeover_is_audible() {
    let first = timed_face(97);
    let second = timed_face(98);
    let mut speech = Speech::default();
    speech.say(None, first.clone(), Some(Entity::PLACEHOLDER), 0.0);
    speech.hear(0.2, Heard::At(0.2));
    speech.hear(0.3, Heard::At(0.3));
    speech.say(
        None,
        second.clone(),
        Some(Entity::from_raw_u32(1).unwrap()),
        0.31,
    );
    speech.hear(0.32, Heard::Waiting);
    let (position, jaw) = timed_mouth(&speech, 0.32);
    assert!(
        (position - (0.3 - crate::talk::PHONEME_CENTRE)).abs() < CLOCK_SLACK,
        "outgoing media position changed to {position:.4}"
    );
    assert!(
        (jaw - mouth(&first, position)).abs() < CLOCK_SLACK,
        "outgoing jaw changed to {jaw:.4}"
    );
    speech.hear(0.35, Heard::At(0.04));
    let (position, jaw) = timed_mouth(&speech, 0.35);
    assert!(
        (position - (0.04 - crate::talk::PHONEME_CENTRE)).abs() < CLOCK_SLACK,
        "incoming media position {position:.4}"
    );
    assert!(
        (jaw - mouth(&second, position)).abs() < CLOCK_SLACK,
        "incoming jaw {jaw:.4}"
    );
}

#[test]
fn a_small_jump_is_detected_beside_larger_smooth_movement() {
    let mut pose = |at: f32| {
        let jump = if at >= 0.125 { 0.025 } else { 0.0 };
        let motion = (at - 0.5).max(0.0) * 0.2;
        (vec![jump, motion, motion, motion], Some(at))
    };
    let before = pose(0.0);
    let after = pose(1.0);
    let jump = Played::subframe_jump((0.0, &before), (1.0, &after), 6, &mut pose);
    assert_eq!(
        jump.map(|(_, what)| what).as_deref(),
        Some("weight 0 by 0.025")
    );
}

fn queue_test_face(client: &mut Client, face: Arc<LineFace>) -> crate::audio_output::VoiceClock {
    use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
    let mut bytes = std::io::Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wave = hound::WavWriter::new(&mut bytes, spec).expect("a wave writer");
    for _ in 0..spec.sample_rate * 2 {
        wave.write_sample(4096i16).expect("a sample");
    }
    wave.finalize().expect("a complete wave");
    let world = client.world_mut();
    let source = world
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: bytes.into_inner().into(),
        });
    let clock = crate::audio_output::VoiceClock::new(1.0);
    let player = world
        .spawn((
            AudioPlayer::new(source),
            PlaybackSettings::DESPAWN,
            clock.clone(),
        ))
        .id();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    world
        .resource_mut::<Speech>()
        .say_heard(None, face, player, clock.clone(), now);
    clock
}

fn rendered_weights(client: &Client) -> Vec<f32> {
    assert!(
        client.world().resource::<Stage>().drawing(),
        "the portrait draws this frame"
    );
    drawn_face(client).expect("the rendered morph weights").0
}

fn frozen_portrait(client: &mut Client, output: bool) -> Vec<f32> {
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .voice = false;
    if output {
        client
            .world_mut()
            .insert_resource(crate::audio_output::DeviceBuffers {
                buffer: Pacing::Hz60.device_buffer(),
                ahead: 3,
                sample_rate: Pacing::Hz60.sample_rate(),
            });
    }
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 60.0,
        )));
    super::hud_layout::on_screen(client);
    client.until("the portrait has drawn its posed jaw", |client| {
        client.world().resource::<Stage>().drawing() && drawn_face(client).is_some()
    });
    client
        .world_mut()
        .resource_mut::<Stage>()
        .actor
        .as_mut()
        .unwrap()
        .face
        .as_mut()
        .unwrap()
        .frozen = Some(0.0);
    client.step_frames(1);
    rendered_weights(client)
}

fn start_rendered_jaw(client: &mut Client, rest: &[f32], face: &Arc<LineFace>) {
    queue_test_face(client, face.clone());
    for _ in 0..60 {
        client.step_frames(1);
        let world = client.world();
        let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
        if world
            .resource::<Speech>()
            .of(None, now)
            .is_some_and(|(_, position)| (0.12..0.30).contains(&position))
        {
            let opening = largest_change(rest, &rendered_weights(client)).1;
            assert!(
                opening > 0.15,
                "the jaw must be wide open before interruption, got {opening:.4}"
            );
            return;
        }
    }
    panic!("the rendered jaw never opened");
}

fn expect_release(client: &mut Client, rest: &[f32], what: &str, change: impl FnOnce(&mut Client)) {
    let face = timed_face(97);
    start_rendered_jaw(client, rest, &face);
    let before = rendered_weights(client);
    let now = client
        .world()
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs();
    let position = client.world().resource::<Speech>().of(None, now).unwrap().1;
    let mut mirror = Mirror::of(client);
    let (index, opening) = largest_change(rest, &before);
    change(client);
    let mut natural = Speech::default();
    let mut previous = before[index];
    for frame in 0..=20 {
        client.step_frames(1);
        let world = client.world();
        let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
        if frame == 0 {
            natural.say(None, face.clone(), None, now);
            natural.hold(position);
            natural.say(
                None,
                Arc::new(LineFace {
                    lipsync: Default::default(),
                    file: String::new(),
                }),
                None,
                now,
            );
        }
        natural.finish(now);
        let expected = mirror.pose(now, natural.saying(None, now), world.resource())[index];
        let actual = rendered_weights(client)[index];
        assert!(
            (actual - expected).abs() < 0.002,
            "{what} frame {frame}: drawn jaw {actual:.4}, natural release {expected:.4}, previous {previous:.4}, opening {opening:.4}"
        );
        previous = actual;
    }
    assert!(
        (previous - rest[index]).abs() < 0.002,
        "{what}: the jaw settles back to rest"
    );
}

#[test]
fn rendered_jaw_eases_when_replay_playback_is_interrupted() {
    use bota_client_core::{Intent, Source};
    let mut live = super::fog_pov::Spectated::hero("0", HeroId::PUDGE);
    let start = live.client.tick();
    live.wait("enough recorded playback", |_, c| c.tick() > start + 360);
    let file = live.save();
    let mut client = Client::run(&[
        "--replay",
        file.path().to_str().expect("a replay path"),
        "--pov",
        "0",
    ]);
    let rest = frozen_portrait(&mut client, true);
    client.within(Duration::from_secs(60), "the loaded replay is playing", |client| {
        client.world().resource::<crate::loading::Loading>().done()
            && matches!(&client.core().source, Source::Replay(player) if !player.paused && !player.finished())
    });
    client.frames(1);
    for command in ["pause", "speed", "scrub"] {
        expect_release(&mut client, &rest, command, |client| match command {
            "pause" => client.core_mut().act(Intent::TogglePause),
            "speed" => client.core_mut().act(Intent::Faster),
            _ => {
                let Source::Replay(player) = &mut client.core_mut().source else {
                    panic!("a replay")
                };
                player.advance_ticks(120.0);
            }
        });
        if command == "pause" {
            client.core_mut().act(Intent::TogglePause);
            client.frames(1);
        }
    }
}

#[test]
fn rendered_jaw_eases_when_the_device_or_presentation_clock_is_lost() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::StartGame);
    let rest = frozen_portrait(&mut client, true);
    for (what, failure) in [("device lost", true), ("presentation unavailable", false)] {
        expect_release(&mut client, &rest, what, |client| {
            let world = client.world_mut();
            world.remove_resource::<crate::audio_output::DeviceBuffers>();
            world
                .resource_mut::<crate::audio_output::Output>()
                .disconnect(failure);
        });
        client
            .world_mut()
            .insert_resource(crate::audio_output::DeviceBuffers {
                buffer: Pacing::Hz60.device_buffer(),
                ahead: 3,
                sample_rate: Pacing::Hz60.sample_rate(),
            });
        client.step_frames(1);
    }
}

#[test]
fn a_voice_requested_without_a_device_leaves_the_rendered_jaw_at_rest() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::StartGame);
    let rest = frozen_portrait(&mut client, false);
    queue_test_face(&mut client, timed_face(97));
    for _ in 0..30 {
        client.step_frames(1);
        assert_eq!(
            rendered_weights(&client),
            rest,
            "the unheard line moved the jaw"
        );
    }
}

#[test]
fn rendered_jaw_transitions_align_with_the_heard_line_at_60_hz() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::StartGame);
    let rest = frozen_portrait(&mut client, true);
    for first in [0.0, -0.08] {
        let face = Arc::new(LineFace {
            lipsync: crate::face::Lipsync::parse(&format!(
                "dbota-lipsync 1\nphoneme {first} 0.40 97\nphoneme 0.48 1.2 98\n"
            ))
            .unwrap(),
            file: format!("timed onset {first}"),
        });
        let mut mirror = Mirror::of(&client);
        let expected = |mirror: &mut Mirror, at: f32, client: &Client| {
            let mut reference = Speech::default();
            reference.say(None, face.clone(), None, 0.0);
            reference.hold(at - crate::face::PHONEME_FILTER / 2.0);
            mirror.pose(0.0, reference.saying(None, 0.0), client.world().resource())
        };
        let wide = expected(&mut mirror, 0.2, &client);
        let (jaw, opening) = largest_change(&rest, &wide);
        assert!(opening > 0.15, "the fixture needs a wide opening");
        let midpoint = (rest[jaw] + wide[jaw]) * 0.5;
        let clock = queue_test_face(&mut client, face.clone());
        let mut drawn_open = None;
        let mut drawn_close = None;
        let mut sample_open = None;
        let mut sample_close = None;
        for frame in 0usize..100 {
            client.step_frames(1);
            let world = client.world();
            let now = world.resource::<Time<bevy::time::Real>>().elapsed();
            let drawn = rendered_weights(&client)[jaw];
            let Some(presentation) = world
                .resource::<crate::audio_output::Output>()
                .presentation(now)
            else {
                continue;
            };
            let media = match presentation(&clock, true) {
                Heard::Scheduled(at) | Heard::At(at) => at,
                _ => continue,
            };
            let want = expected(&mut mirror, media, &client)[jaw];
            let ref_closed = at_or_below(want, midpoint, rest[jaw], wide[jaw]);
            let drawn_closed = at_or_below(drawn, midpoint, rest[jaw], wide[jaw]);
            if media < 0.20 {
                if !ref_closed {
                    sample_open.get_or_insert(frame);
                }
                if !drawn_closed {
                    drawn_open.get_or_insert(frame);
                }
            }
            if media > 0.30 {
                if ref_closed {
                    sample_close.get_or_insert(frame);
                }
                if drawn_closed {
                    drawn_close.get_or_insert(frame);
                }
            }
        }
        let open_error = drawn_open
            .expect("a visible jaw opening")
            .abs_diff(sample_open.expect("the aligned opening"));
        let close_error = drawn_close
            .expect("a visible jaw close")
            .abs_diff(sample_close.expect("the audible transition"));
        assert!(
            open_error <= 1 && close_error <= 1,
            "first phoneme {first}: opening error {open_error}, closing error {close_error} frames"
        );
        println!(
            "jaw alignment {first}: opening {open_error}, closing {close_error} frames at 60 Hz"
        );
        client.step_frames(70);
    }
}

fn at_or_below(weight: f32, midpoint: f32, rest: f32, wide: f32) -> bool {
    (weight - midpoint) * (wide - rest) <= 0.0
}

#[test]
fn playback_clock_catches_up_without_jumping_when_a_buffer_returns() {
    let line = timed_face(97);
    let mut speech = Speech::default();
    speech.say(None, line.clone(), Some(Entity::PLACEHOLDER), 0.0);
    speech.hear(0.2, Heard::At(0.2));
    speech.hear(0.7, Heard::At(0.2));
    let mut previous = timed_mouth(&speech, 0.7);
    for frame in 1..=180 {
        let now = 0.7 + frame as f32 / 240.0;
        let output = now - 0.4;
        speech.hear(now, Heard::At(output));
        let current = timed_mouth(&speech, now);
        assert!(
            current.0 - previous.0 <= 1.5 / 240.0 + CLOCK_SLACK,
            "the recovering face jumped by {:.4} s",
            current.0 - previous.0
        );
        assert!(
            current.0 <= output - crate::talk::PHONEME_CENTRE + CLOCK_SLACK,
            "the recovering face passed its sound"
        );
        previous = current;
    }
    let end = 1.05 - crate::talk::PHONEME_CENTRE;
    assert!(
        (previous.0 - end).abs() < CLOCK_SLACK,
        "the face never rejoined: {previous:?}"
    );
    assert!(
        (previous.1 - mouth(&line, end)).abs() < CLOCK_SLACK,
        "the jaw never rejoined: {previous:?}"
    );
}

#[test]
fn a_covered_portrait_leaves_the_camera_untouched_until_shown() {
    use bevy::input::ButtonState;
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::StartGame);
    super::hud_layout::on_screen(&mut client);
    client.until("the portrait has drawn a pose", |c| {
        c.world().resource::<Stage>().drawing()
    });
    let camera = client
        .world_mut()
        .query_filtered::<Entity, With<crate::portrait::PortraitCamera>>()
        .single(client.world())
        .expect("the portrait camera");
    let toggle = |c: &mut Client| {
        c.key(KeyCode::Escape, ButtonState::Pressed);
        c.key(KeyCode::Escape, ButtonState::Released);
    };
    toggle(&mut client);
    assert!(!client.world().resource::<crate::ui::HudShown>().0);
    let tagged = {
        let mut camera = client
            .world_mut()
            .get_mut::<Transform>(camera)
            .expect("camera transform");
        camera.translation.x += 16.0;
        camera.translation
    };
    client.frames(3);
    assert_eq!(
        client.world().get::<Transform>(camera).unwrap().translation,
        tagged,
        "the hidden portrait re-aimed its camera"
    );
    toggle(&mut client);
    client.frames(3);
    assert_ne!(
        client.world().get::<Transform>(camera).unwrap().translation,
        tagged,
        "the visible portrait stopped tracking its actor"
    );
}
