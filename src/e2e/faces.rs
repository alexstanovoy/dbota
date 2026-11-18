//! Every hero's face moves as its authored data moves it, and never jumps: the portrait's face
//! (its morph weights as drawn, and the joints its meshes are skinned to) has no discontinuity
//! while the hero stands idle, while it runs and answers its order, and through each of its voice
//! lines (every kind the cache holds: moves, attacks, runes, the Bottle...) over its idle
//! expression, wherever in the expression's loop the line starts.
//!
//! The client draws the face on simulated frames 2 ms apart: through a whole loop of the idle
//! clip, the run through a whole loop of its clip with the answer its hero says, and one whole
//! line. Each drawn frame must be the face [`Face::pose`] gives at that frame's moment and line
//! (the face's rigs set every morph weight, so that is all of them), posed by clocks that do not
//! jump (the expression's never restarts; a line's moves on by the frame), and its joints may
//! move [`MAX_TURN`] and [`MAX_MOVE`] a frame. The joints come from the body's clips alone (the
//! expression and the lines move morph weights, nothing else), so these frames cover them whole.
//!
//! Whether that face jumps is then judged apart from the client: a copy of the portrait's face,
//! posed by the client's own [`Face::pose`] and [`Speech`] at any instant, plays its idle
//! expression through a whole loop and every line of the pack starting at each of [`PHASES`]
//! instants of the expression's loop, in frames of 2 ms (the idle) and 4 ms (a line). A frame on
//! which a weight moves more than [`EXAMINE`], or all of them together more than
//! [`EXAMINE_TOTAL`], is halved down to [`FINEST`], following the half all the weights together
//! move more in: a jump is a change still more than [`JUMP`] in a weight there, or more than
//! [`TOTAL_JUMP`] in all of them together (a jump does not shrink as the step does; motion does).
//! No speed is capped: a frame with no jump moves a weight up to 0.123 in 4 ms (Pudge's line 25
//! started 3480 ms into the loop, weight 7) and all of them together up to 0.553 (Pudge's line 159
//! started 2780 ms in), 0.0019 and 0.0086 in [`FINEST`].
//!
//! The client drops a line on the first frame past its data's end ([`Speech::finish`]) and draws
//! the expression alone from that frame, with no blend, so a line steps back once there by what
//! its last key holds, and that one jump is allowed it. A line whose own authored face does not
//! return to rest where its data ends (read here from its lip sync record) steps back by up to
//! 0.52 of a weight (Pudge's line 25); one that does, within [`REST`] of each controller, by a
//! step spread over the weights, each under [`JUMP`] (Pudge's twelve such lines, 4 to 158, by 0.20
//! to 0.36 in all, at every phase).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::mesh::morph::MorphWeights;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, EntityId, HeroId, Order, Target, Vec2 as At};

use super::harness::{Client, Server};
use crate::portrait::Stage;
use crate::talk::{Face, Faces, LineFace, Speech};

/// How near each controller of a line's own face must be to rest where its data ends for the
/// line to end at rest.
const REST: f32 = 0.05;
/// Seconds between frames.
const STEP: f32 = 0.002;
/// How far a line's clock may be off a drawn frame's step: the client's f32 clock is exact to a
/// few hundred microseconds over its first half hour.
const CLOCK_SLACK: f32 = 0.000_25;
/// A change of a morph weight bigger than this at once is a jump.
const JUMP: f32 = 0.1;
/// The step a frame is halved down to when judging a jump: 1/64 of a line's frame. The fastest
/// motion (0.123 of a weight and 0.553 of all together in a 4 ms frame) moves 0.0019 and 0.0086
/// in it, a fiftieth of [`JUMP`] and a twenty-third of [`TOTAL_JUMP`], and the clock
/// ([`MAX_CLOCK`] seconds of f32) resolves it to within 4 µs.
const FINEST: f32 = LINE_STEP / 64.0;
/// A frame on which a weight moves more than this is halved to see whether it jumped: a jump
/// over [`JUMP`] hides from it only behind three times as much opposite motion of that weight in
/// the same frame.
const EXAMINE: f32 = JUMP / 4.0;
/// A change of all morph weights together (the sum of their changes) bigger than this at once is
/// a jump too: a pop spread over several weights, each under [`JUMP`] (four weights by 0.06 is
/// one).
const TOTAL_JUMP: f32 = 0.2;
/// A frame on which all weights together move more than this is halved too.
const EXAMINE_TOTAL: f32 = TOTAL_JUMP / 4.0;
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
        self.pose(0.0, None, faces).iter().all(|w| *w >= 0.0)
    }

    /// The face posed at `now` saying `line`: every morph weight of its meshes in turn.
    fn pose(&mut self, now: f32, line: Option<(&LineFace, f32)>, faces: &Faces) -> Vec<f32> {
        let mut weights = self.weights.query_mut(&mut self.world);
        self.face.pose(now, line, faces, &mut weights);
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
    let line = world.resource::<Speech>().of(Some(client.me().id), now);
    let drawn = drawn_face(client).map(|(weights, _)| weights);
    Some(Mirror::of(client).pose(now, line, world.resource())) == drawn
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

/// A frame that jumped: how far into the line the hero says it was before and after it (none:
/// saying none), what jumped on it, and whether only all the weights together did (each under
/// [`JUMP`]).
struct Jump {
    said: (Option<f32>, Option<f32>),
    what: String,
    spread: bool,
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

/// How a line's clock moved from one drawn frame to the next, when it jumped: a line said on
/// both moves on by the frame's `step`; one starting is said from its start (or before it, a
/// line registered ahead).
fn clock_jump(
    before: &Option<(String, f32)>,
    after: &Option<(String, f32)>,
    step: f32,
) -> Option<String> {
    match (before, after) {
        (Some((was, t0)), Some((file, t1))) if was == file => ((t1 - t0 - step).abs()
            > CLOCK_SLACK)
            .then(|| format!("{file}'s clock moved {:.5} s in a {step} s frame", t1 - t0)),
        (_, Some((file, t))) => (*t > CLOCK_SLACK).then(|| format!("{file} started {t:.5} s in")),
        (_, None) => None,
    }
}

/// Runs `frames` frames of `step` seconds, each checked against the one before: the frames
/// something jumped on. A frame jumps when a joint moves more than its limits, when the face
/// drawn is not the face [`Face::pose`] gives at the frame's moment and line (or none is drawn),
/// or when the clocks it is posed by jump: the expression's restarts, or a line's ([`clock_jump`]).
fn watch(client: &mut Client, frames: usize, step: f32, phase: &str) -> Vec<Jump> {
    watch_until(client, frames, step, phase, |_| false)
}

/// [`watch`], ended after the first frame `done` holds on.
fn watch_until(
    client: &mut Client,
    frames: usize,
    step: f32,
    phase: &str,
    mut done: impl FnMut(&Client) -> bool,
) -> Vec<Jump> {
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
                what.extend(clock_jump(&last.1, &now.1, step));
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
            }
            _ => what.push("no face drawn".into()),
        }
        if !what.is_empty() {
            out.push(Jump {
                spread: false,
                said: (last.1.as_ref().map(|s| s.1), now.1.as_ref().map(|s| s.1)),
                what: format!(
                    "{phase} frame {frame}: {}; speech {:?} -> {:?}",
                    what.join(", "),
                    last.1,
                    now.1
                ),
            });
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

/// The jumps of a whole line that count: all but one on the frame its face leaves on (said up to
/// its end before it, a frame landing on the end itself still saying it; saying none, or past
/// its end, after it). A line whose own face ends off rest may step back there by any amount; one
/// whose face ends at rest only by a spread step, each weight under [`JUMP`] (what its last key
/// holds within [`REST`] of each controller's rest, dropped with no blend).
fn counted(jumps: Vec<Jump>, end: f32, rests: bool) -> impl Iterator<Item = String> {
    let mut allowed = true;
    jumps.into_iter().filter_map(move |jump| {
        let ends_here = matches!(jump.said, (Some(before), after) if before <= end && after.is_none_or(|t| t > end));
        if allowed && ends_here && (!rests || jump.spread) {
            allowed = false;
            return None;
        }
        Some(jump.what)
    })
}

#[test]
fn a_line_ending_off_rest_steps_back_once_on_a_frame_landing_on_its_end() {
    let end = 2.28;
    let jump = |said, what: &str| Jump {
        said,
        what: what.to_owned(),
        spread: false,
    };
    let jumps = vec![
        jump((Some(end - 0.004), Some(end)), "a jump onto its end"),
        jump((Some(end), None), "the step back"),
    ];
    let counted: Vec<String> = counted(jumps, end, false).collect();
    assert_eq!(
        counted,
        ["a jump onto its end"],
        "only the step back is allowed"
    );
}

#[test]
fn a_line_ending_at_rest_steps_back_only_by_a_spread_residual() {
    let end = 2.28;
    let leaving = |what: &str, spread| Jump {
        said: (Some(end), None),
        what: what.to_owned(),
        spread,
    };
    let residual = counted(vec![leaving("a spread residual", true)], end, true);
    assert_eq!(residual.count(), 0, "a spread residual is its step back");
    let weight = counted(vec![leaving("a weight's jump", false)], end, true);
    assert_eq!(
        weight.collect::<Vec<_>>(),
        ["a weight's jump"],
        "no weight jumps as a line at rest ends"
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

/// The line the client plays whole: the shortest whose own face ends off rest (and so has its
/// one step back), else the shortest.
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
fn say_line(
    client: &mut Client,
    name: &str,
    authored: &AuthoredLine,
    ranges: &HashMap<u32, (f32, f32)>,
) -> Vec<String> {
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
    let rests = ends_at_rest(ranges, &authored.text);
    let jumps = counted(found, line.lipsync.end(), rests).collect();
    assert!(
        said(client).is_none(),
        "{name} {file}: the line over, now {:?}",
        spoken(client)
    );
    jumps
}

/// A line registered with a [`Speech`]: its face, and the instant registered to start at.
type Said = (Arc<LineFace>, f32);

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
/// instant, a [`Speech`] of the client's is given the line said (if any) and ends it by that
/// instant, then [`Face::pose`] poses the copy, as a client frame does.
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

    /// The face at `now` saying `line`, and how far into it the hero says it is.
    fn at(&mut self, now: f32, line: Option<&Said>) -> (Vec<f32>, Option<f32>) {
        assert!(
            now < MAX_CLOCK,
            "posed within the clock's resolution: {now} s"
        );
        let mut speech = Speech::default();
        if let Some((face, start)) = line {
            speech.say(Some(self.unit), face.clone(), None, *start);
        }
        speech.finish(now);
        let said = speech.of(Some(self.unit), now);
        (
            self.mirror.pose(now, said, self.faces),
            said.map(|(_, t)| t),
        )
    }

    /// Plays `frames` frames of `step` seconds from `start`, saying `line`, each checked against
    /// the one before: a frame on which a weight moves more than [`EXAMINE`] is halved down to
    /// [`FINEST`], following the half that moves more, and holds a jump when the change is still
    /// more than [`JUMP`] there. The jumps, each with how far into the line it is said on both
    /// sides of the finest step.
    fn play(
        &mut self,
        (start, frames, step): (f32, usize, f32),
        line: Option<&Said>,
        phase: &str,
        fastest: &mut Fastest,
    ) -> Vec<Jump> {
        let halvings = (step / FINEST).log2().round() as u32;
        let at = |frame: usize| start + frame as f32 * step;
        let mut last = self.at(at(0), line);
        let mut out = Vec::new();
        for frame in 0..frames {
            let now = self.at(at(frame + 1), line);
            let (i, by) = largest_change(&last.0, &now.0);
            let total = total_change(&last.0, &now.0);
            let jump = (by > EXAMINE || total > EXAMINE_TOTAL)
                .then(|| self.halve((at(frame), &last), (at(frame + 1), &now), halvings, line))
                .flatten();
            match jump {
                Some((said, what, spread)) => out.push(Jump {
                    said,
                    what: format!("{phase} frame {frame}: {what} at once; said {said:?}"),
                    spread,
                }),
                None => fastest.note((by, total), || format!("{phase} frame {frame}, weight {i}")),
            }
            last = now;
        }
        out
    }

    /// Halves the step from `before` to `after` `halvings` times, keeping the half the face
    /// moves more in, all its weights together: the jump there, if a weight changes more than
    /// [`JUMP`] or all of them more than [`TOTAL_JUMP`] (how far into the line it is said on both
    /// sides, and what jumped).
    fn halve(
        &mut self,
        before: (f32, &(Vec<f32>, Option<f32>)),
        after: (f32, &(Vec<f32>, Option<f32>)),
        halvings: u32,
        line: Option<&Said>,
    ) -> Option<((Option<f32>, Option<f32>), String, bool)> {
        let (mut lo, mut hi) = ((before.0, before.1.clone()), (after.0, after.1.clone()));
        for _ in 0..halvings {
            let mid = (lo.0 + hi.0) / 2.0;
            let face = (mid, self.at(mid, line));
            let first = total_change(&lo.1.0, &face.1.0);
            let second = total_change(&face.1.0, &hi.1.0);
            if first >= second {
                hi = face;
            } else {
                lo = face;
            }
        }
        let (i, by) = largest_change(&lo.1.0, &hi.1.0);
        let total = total_change(&lo.1.0, &hi.1.0);
        let said = (lo.1.1, hi.1.1);
        match (by > JUMP, total > TOTAL_JUMP) {
            (true, _) => Some((said, format!("weight {i} by {by:.3}"), false)),
            (false, true) => Some((said, format!("all weights by {total:.3}"), true)),
            (false, false) => None,
        }
    }

    /// Plays a whole line starting `start` seconds into the expression's loop, as [`say_line`]
    /// does on the client: the jumps that count.
    fn say(
        &mut self,
        start: f32,
        authored: &AuthoredLine,
        ranges: &HashMap<u32, (f32, f32)>,
        fastest: &mut Fastest,
    ) -> Vec<String> {
        let line = &authored.face;
        let (frames, lead) = line_frames(authored, LINE_STEP);
        let said: Said = (line.clone(), start + lead);
        let phase = format!("{} from {:.0} ms", line.file, start * 1000.0);
        let found = self.play((start, frames, LINE_STEP), Some(&said), &phase, fastest);
        let rests = ends_at_rest(ranges, &authored.text);
        let jumps = counted(found, line.lipsync.end(), rests).collect();
        let end = start + frames as f32 * LINE_STEP;
        let after = self.at(end, Some(&said)).1;
        assert!(after.is_none(), "{phase}: the line over, now {after:?}");
        jumps
    }
}

/// The face's jumps apart from the client: its idle expression through a whole loop, and every
/// line starting at each of [`PHASES`] instants of the loop, the phases shared among threads (a
/// copy of the face, `copies[0]` for the idle and one more for each thread).
fn played_apart(
    (mut copies, faces, unit): (Vec<Mirror>, &Faces, EntityId),
    name: &str,
    lines: &[AuthoredLine],
    ranges: &HashMap<u32, (f32, f32)>,
) -> Vec<String> {
    let mut fastest = Fastest::default();
    let idle = (0.0, IDLE_FRAMES, STEP);
    let mut played = Played::of(copies.remove(0), faces, unit);
    let found = played.play(idle, None, &format!("{name} idle"), &mut fastest);
    let mut jumps: Vec<(usize, String)> = found.into_iter().map(|j| (0, j.what)).collect();
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
                        for line in lines {
                            let found = played.say(start, line, ranges, &mut fastest);
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
        if !pending && said(client).is_none() {
            return jumps;
        }
        patience.pass("the automatic voice line to finish before injected playback");
        let phase = format!("{name} finishing automatic speech");
        let found = watch(client, 1, STEP, &phase);
        jumps.extend(found.into_iter().map(|jump| jump.what));
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
fn drawn_on_the_client(
    client: &mut Client,
    name: &str,
    line: &AuthoredLine,
    ranges: &HashMap<u32, (f32, f32)>,
) -> Vec<String> {
    let idle = loop_frames(client, "idle");
    let found = watch(client, idle, STEP, &format!("{name} idle"));
    if !found.is_empty() {
        return found.into_iter().map(|j| j.what).collect();
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
    let mut jumps: Vec<String> = found.into_iter().map(|j| j.what).collect();
    jumps.extend(finish_automatic(client, name));
    if jumps.is_empty() {
        jumps.extend(say_line(client, name, line, ranges));
    }
    jumps
}

fn check_hero(hero: HeroId) -> Vec<String> {
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
    let jumps = drawn_on_the_client(&mut client, &name, line, &ranges);
    if !jumps.is_empty() {
        return jumps;
    }
    // The copies taken, the client and the server go: other tests' clients may run meanwhile.
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let copies: Vec<Mirror> = (0..=threads.clamp(1, MAX_THREADS))
        .map(|_| Mirror::of(&client))
        .collect();
    let faces: Faces = client.world().resource::<Faces>().clone();
    let unit = client.me().id;
    drop(client);
    drop(server);
    played_apart((copies, &faces, unit), &name, &lines, &ranges)
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
