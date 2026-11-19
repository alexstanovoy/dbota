//! The faces of the portrait: what the cache gives to move them (Valve's phoneme table, the
//! heroes' idle expressions, each mesh's flex rig) and the line one's hero is saying.
//!
//! Dota lip-syncs the hero who speaks: a line moves the portrait's face while the portrait shows
//! that hero, whatever else is selected meanwhile (a creep's portrait never talks); the line's
//! clock is its sound's own playing position, so the mouth keeps time with what is heard.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::gltf::{GltfMeshExtras, GltfMeshName};
use bevy::mesh::morph::{MeshMorphWeights, MorphWeights};
use bevy::prelude::*;
use bota_proto::EntityId;

use crate::face::{Controls, Expression, Lipsync, Phonemes, Rig, RigScratch};
use crate::manifest::Manifest;

/// What faces move by, out of the cache.
#[derive(Resource, Default, Clone)]
pub struct Faces {
    phonemes: Phonemes,
    /// Idle expressions by the logical name of their model.
    expressions: HashMap<String, Option<Arc<Expression>>>,
    /// Rigs by glTF mesh name (none: the mesh's extras do not read).
    rigs: HashMap<String, Option<Arc<Rig>>>,
}

impl Faces {
    /// The idle expression a model's portrait plays, if it has one.
    pub fn expression(&mut self, model: &str, manifest: &Manifest) -> Option<Arc<Expression>> {
        self.expressions
            .entry(model.to_owned())
            .or_insert_with(|| {
                let (_, rel) = manifest
                    .expressions
                    .iter()
                    .find(|(name, _)| name == model)?;
                let expression = crate::cachefs::read_small(&manifest.root, rel)
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .as_deref()
                    .and_then(Expression::parse)
                    .map(Arc::new);
                if expression.is_none() {
                    warn!("{rel}: the expression does not read");
                }
                expression
            })
            .clone()
    }
}

/// A voice line's lip sync, and where in the cache it was read from: a voice pack's record
/// (`sounds/voice/pudge.bin#lipsync/12.txt`).
pub struct LineFace {
    pub lipsync: Lipsync,
    pub file: String,
}

/// How a line's face lets go once the line ends or another takes over: what is left of it decays
/// with this time constant (seconds), as Source lets a flex go (`ExponentialDecay`), five of them
/// making the 0.2 s ease out `CAudioMorphData` gives a line by default. A line whose data ends on
/// its rest leaves nothing to let go; one ending off it (or cut short) would otherwise snap back
/// in one frame. Decays of one constant add into one: whatever number of lines leave, one by one
/// or at once, what is left of them is one set of controllers decaying together.
pub const RELEASE: f32 = 0.04;
/// Seconds after which what is left of a line let go is dropped: seven time constants, when less
/// than a thousandth of what it held is left (well under any change the eye or the face tests
/// see).
pub const LET_GO: f32 = 7.0 * RELEASE;
/// Most lines left waiting to be added to what is let go, when no face takes them in.
const PENDING: usize = 8;
/// How far a sound's reported position may run from the line's own clock before the clock goes
/// after it: more than a device's buffers ahead (two of about 45 ms), so that only a sound that
/// stalled or skipped moves the face's clock, never the steps the device takes it in.
const RESYNC: f32 = 0.25;
/// How fast the line's clock goes after its sound once they parted: half a second of the line a
/// second faster or slower than the frames, so the face never jumps to it.
const SLEW: f32 = 0.5;
/// How near the sound the line's clock is back when it stops going after it: within a device's
/// buffer, the steps it reports in.
const CAUGHT_UP: f32 = 0.05;

/// The line one's hero is saying, and what is left of the lines it said before.
#[derive(Resource, Default)]
pub struct Speech {
    line: Option<Line>,
    /// Lines that ended or were taken over since a face last took them in ([`Speech::settle`]).
    leaving: Vec<Leaving>,
    /// What is left of the lines taken in: their controllers, decaying from an instant on.
    residue: Option<Residue>,
}

struct Line {
    /// The hero saying it; none for any face (a check's).
    unit: Option<EntityId>,
    face: Arc<LineFace>,
    /// The sound playing it, while it plays.
    player: Option<Entity>,
    /// Seconds since the start, at the line's start.
    anchor: f32,
    /// Whether its sound has begun: from then the line keeps the frames' clock.
    sounding: bool,
    /// The position its sound reported as it began, which every later report is taken against
    /// (the output reports ahead of what is heard by as much all along).
    began_at: f32,
    /// When the sound last reported, and whether the clock is going after it.
    heard: f32,
    catching_up: bool,
    /// Debug: the line held at this many seconds in.
    held: Option<f32>,
}

/// A line that ended or was taken over: its face where it stopped, decaying from when it did.
struct Leaving {
    unit: Option<EntityId>,
    face: Arc<LineFace>,
    /// How far into it the line stopped, and when.
    at: f32,
    since: f32,
}

/// The lines let go, as one set of controllers decaying from `since`.
struct Residue {
    unit: Option<EntityId>,
    controls: Controls,
    since: f32,
}

/// What is left `age` seconds after a line was let go.
fn release(age: f32) -> f32 {
    (-age.max(0.0) / RELEASE).exp()
}

/// What a sound's player tells of a line's sound on a frame.
#[derive(Clone, Copy, Debug)]
pub enum Heard {
    /// Asked for, not playing yet.
    Waiting,
    /// Playing, this far in as the output reports it (in the steps the device takes it in).
    At(f32),
    /// Played out, or never heard.
    Gone,
}

/// What a face says at a moment: the line and how far into it, and what is left of the lines it
/// said before.
#[derive(Clone, Copy, Default)]
pub struct Saying<'a> {
    pub line: Option<(&'a LineFace, f32)>,
    leaving: &'a [Leaving],
    residue: Option<&'a Residue>,
    unit: Option<EntityId>,
    now: f32,
}

impl Speech {
    /// `unit` starts saying a line, heard through `player` (none when not heard).
    pub fn say(
        &mut self,
        unit: Option<EntityId>,
        face: Arc<LineFace>,
        player: Option<Entity>,
        now: f32,
    ) {
        self.leave(now);
        let anchor = now + face.lipsync.lead();
        self.line = Some(Line {
            unit,
            face,
            player,
            anchor,
            sounding: false,
            began_at: 0.0,
            heard: now,
            catching_up: false,
            held: None,
        });
    }

    /// The line `unit`'s face says and how far into it, at `now`.
    pub fn of(&self, unit: Option<EntityId>, now: f32) -> Option<(&LineFace, f32)> {
        let line = self.line.as_ref()?;
        let t = line.held.unwrap_or(now - line.anchor);
        (line.unit.is_none() || line.unit == unit).then(|| (line.face.as_ref(), t))
    }

    /// What `unit`'s face says at `now`: its line, and what is left of the lines before it.
    pub fn saying(&self, unit: Option<EntityId>, now: f32) -> Saying<'_> {
        Saying {
            line: self.of(unit, now),
            leaving: &self.leaving,
            residue: self.residue.as_ref(),
            unit,
            now,
        }
    }

    /// Takes the lines let go since the last call into what is left of the lines before, at
    /// `now`: `face` gives a line's controllers where it stopped (a face's own ranges). The sum
    /// is the same before and after, so a face posed at `now` does not change by it; it only
    /// keeps what is let go one set of controllers, however many lines leave.
    pub fn settle(&mut self, now: f32, mut face: impl FnMut(&LineFace, f32) -> Controls) {
        if self.leaving.is_empty() {
            return;
        }
        let mut residue = self.residue.take();
        let unit = self.leaving.last().and_then(|l| l.unit);
        let mut controls = residue
            .as_mut()
            .map(|r| {
                let left = release(now - r.since);
                r.controls.values_mut().for_each(|v| *v *= left);
                std::mem::take(&mut r.controls)
            })
            .unwrap_or_default();
        for line in self.leaving.drain(..) {
            let left = release(now - line.since);
            for (name, value) in face(&line.face, line.at) {
                *controls.entry(name).or_insert(0.0) += value * left;
            }
        }
        self.residue = Some(Residue {
            unit,
            controls,
            since: now,
        });
    }

    /// Debug: holds the line at `t` seconds in.
    pub fn hold(&mut self, t: f32) {
        if let Some(line) = &mut self.line {
            line.player = None;
            line.held = Some(t);
        }
    }

    pub fn stop(&mut self) {
        self.line = None;
        self.leaving.clear();
        self.residue = None;
    }

    /// Keeps the line on the frames' clock from the moment its sound begins (its lead behind it),
    /// as `heard` tells of it at `now`. The output reports a sound's position in the steps its
    /// device takes it in (a buffer of about 45 ms at a time, two ahead of what is heard): the
    /// face follows the frames' clock instead. Only when the sound's position, taken against
    /// where it began, parts from that clock by more than [`RESYNC`] (the output stalled, or was
    /// suspended and resumed) does the clock go after it, at most [`SLEW`] faster or slower,
    /// until it is back within [`CAUGHT_UP`]: the face slows or hurries, it never jumps.
    pub fn hear(&mut self, now: f32, heard: Heard) {
        let Some(line) = self.line.as_mut().filter(|l| l.held.is_none()) else {
            return;
        };
        // The face trails the sound by its lead (Source's `phonemedelay`, the sound system's
        // latency: what the output holds ahead of what is heard, two device buffers of about
        // 45 ms, is about the phonemes' filter span), so it is at rest as the sound starts.
        let lead = line.face.lipsync.lead();
        match heard {
            Heard::Waiting => line.anchor = now + lead,
            Heard::At(position) if !line.sounding => {
                line.sounding = true;
                line.anchor = now + lead;
                line.began_at = position;
                line.heard = now;
            }
            Heard::At(position) => {
                // Where the anchor would be if the sound kept the frames' time since it began.
                let target = now + lead - (position - line.began_at);
                let off = target - line.anchor;
                line.catching_up |= off.abs() > RESYNC;
                if line.catching_up {
                    let most = SLEW * (now - line.heard).max(0.0);
                    line.anchor += off.clamp(-most, most);
                    line.catching_up = (target - line.anchor).abs() > CAUGHT_UP;
                }
                line.heard = now;
            }
            Heard::Gone => line.player = None,
        }
    }

    /// Ends the line once its face is still at `now`: past its lip sync's end on its clock, unless
    /// held; and lets a line that left go once it has eased out.
    pub fn finish(&mut self, now: f32) {
        let done = self
            .line
            .as_ref()
            .is_some_and(|line| line.held.is_none() && now - line.anchor > line.face.lipsync.end());
        if done {
            self.leave(now);
        }
        self.leaving.retain(|l| now - l.since <= LET_GO);
        if self
            .residue
            .as_ref()
            .is_some_and(|r| now - r.since > LET_GO)
        {
            self.residue = None;
        }
    }

    /// The line said leaves, easing out from where it is at `now`, or from its end and the
    /// instant its data ended when that was before.
    fn leave(&mut self, now: f32) {
        if let Some(line) = self.line.take() {
            let end = line.face.lipsync.end();
            let (at, since) = match line.held {
                Some(t) => (t, now),
                None if now - line.anchor > end => (end, line.anchor + end),
                None => (now - line.anchor, now),
            };
            if self.leaving.len() == PENDING {
                self.leaving.remove(0);
            }
            self.leaving.push(Leaving {
                unit: line.unit,
                face: line.face,
                at,
                since,
            });
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Faces>()
        .init_resource::<Speech>()
        .add_systems(
            First,
            crate::loading::reset::<Speech>.in_set(crate::loading::Begin),
        )
        .add_systems(Update, load.run_if(crate::loading::requested))
        .add_systems(Update, follow);
}

fn load(manifest: Res<Manifest>, mut faces: ResMut<Faces>) {
    let root = &manifest.root;
    let text =
        |rel: &str| crate::cachefs::read_small(root, rel).and_then(|b| String::from_utf8(b).ok());
    if let Some(table) = manifest.phonemes.as_deref().and_then(text) {
        match Phonemes::parse(&table) {
            Some(table) => faces.phonemes = table,
            None => warn!("the phoneme table does not read; faces say nothing"),
        }
    }
    for (model, _) in &manifest.expressions {
        faces.expression(model, &manifest);
    }
}

/// Keeps the line on its sound's clock while it plays, and ends it once its face is still.
fn follow(
    time: Res<Time<bevy::time::Real>>,
    mut speech: ResMut<Speech>,
    sinks: Query<Option<&AudioSink>>,
) {
    let now = time.elapsed_secs();
    let player = speech.line.as_ref().and_then(|line| line.player);
    if let Some(player) = player {
        let heard = match sinks.get(player) {
            // Asked for, not playing yet: the line has not begun.
            Ok(None) => Heard::Waiting,
            Ok(Some(sink)) => Heard::At(sink.position().as_secs_f32()),
            // Gone (played out, or never heard): on its own clock.
            Err(_) => Heard::Gone,
        };
        speech.hear(now, heard);
    }
    speech.finish(now);
}

/// A face on the portrait's stage: each of its meshes' morph weights and rig, the controllers'
/// ranges, its idle expression and when that began.
#[derive(Clone)]
pub struct Face {
    meshes: Vec<(Entity, Arc<Rig>)>,
    ranges: HashMap<u32, (f32, f32)>,
    expression: Option<Arc<Expression>>,
    pub began: f32,
    /// Debug: the expression held at this many seconds in.
    pub frozen: Option<f32>,
    /// The room posing works in, kept from frame to frame so that a face posed on every frame
    /// allocates nothing once it has posed once.
    scratch: Scratch,
}

/// What [`Face::pose`] works in: the controllers, a line's controllers, a mesh's weights and its
/// rig's room.
#[derive(Clone, Default)]
struct Scratch {
    controls: Controls,
    line: Controls,
    out: Vec<f32>,
    rig: RigScratch,
}

impl Face {
    /// The entities holding its meshes' morph weights, and the meshes' target names.
    pub fn nodes(&self) -> impl Iterator<Item = (Entity, &[String])> + '_ {
        self.meshes
            .iter()
            .map(|(node, rig)| (*node, rig.names.as_slice()))
    }

    /// The face of the model under `pivot`, once its scenes are ready: none when no mesh of it
    /// has a rig.
    pub fn of(
        pivot: Entity,
        expression: Option<Arc<Expression>>,
        now: f32,
        faces: &mut Faces,
        tree: &Query<&Children>,
        meshes: &Query<(&MeshMorphWeights, &GltfMeshName, &GltfMeshExtras)>,
    ) -> Option<Self> {
        let mut found: Vec<(Entity, Arc<Rig>)> = Vec::new();
        for e in tree.iter_descendants(pivot) {
            let Ok((MeshMorphWeights::Reference(node), name, extras)) = meshes.get(e) else {
                continue;
            };
            if found.iter().any(|(n, _)| n == node) {
                continue;
            }
            let rig = faces
                .rigs
                .entry(name.0.clone())
                .or_insert_with(|| {
                    let rig = Rig::from_extras(&extras.value).map(Arc::new);
                    if rig.is_none() {
                        warn!(
                            "{}: its flex rig does not read; it keeps its rest face",
                            name.0
                        );
                    }
                    rig
                })
                .clone();
            if let Some(rig) = rig {
                found.push((*node, rig));
            }
        }
        let ranges = found
            .iter()
            .flat_map(|(_, rig)| {
                rig.controllers
                    .iter()
                    .map(|&(name, min, max)| (name, (min, max)))
            })
            .collect();
        (!found.is_empty()).then_some(Self {
            meshes: found,
            ranges,
            expression,
            began: now,
            frozen: None,
            scratch: Scratch::default(),
        })
    }

    /// The same face on other entities holding its meshes' morph weights, one for each of
    /// [`Face::rigs`] in order: a test poses a copy of the portrait's face apart from the client.
    #[cfg(test)]
    pub fn on_nodes(&self, nodes: &[Entity]) -> Self {
        assert_eq!(nodes.len(), self.meshes.len(), "a node for each mesh");
        let meshes = nodes.iter().zip(&self.meshes);
        Self {
            meshes: meshes
                .map(|(&node, (_, rig))| (node, rig.clone()))
                .collect(),
            ..self.clone()
        }
    }

    /// Each of its meshes: the entity holding its morph weights, and its rig.
    pub fn rigs(&self) -> impl Iterator<Item = (Entity, &Rig)> + '_ {
        self.meshes.iter().map(|(node, rig)| (*node, rig.as_ref()))
    }

    /// The controllers of the face as it is at `now`, saying what `saying` says, into
    /// `controls`: its idle expression, the line added over it, and what is left of the lines
    /// before it (each worked out in `line`).
    fn controls(
        &self,
        now: f32,
        saying: Saying,
        faces: &Faces,
        (controls, line_room): (&mut Controls, &mut Controls),
    ) {
        controls.clear();
        if let Some(scene) = &self.expression {
            scene.apply(self.frozen.unwrap_or(now - self.began), controls);
        }
        if let Some((line, t)) = saying.line {
            line.lipsync
                .apply(t, &faces.phonemes, &self.ranges, controls);
        }
        let ours = |unit: Option<EntityId>| unit.is_none() || unit == saying.unit;
        for line in saying.leaving.iter().filter(|l| ours(l.unit)) {
            let left = release(saying.now - line.since);
            line_room.clear();
            line.face
                .lipsync
                .apply(line.at, &faces.phonemes, &self.ranges, line_room);
            for (name, value) in line_room.iter() {
                *controls.entry(*name).or_insert(0.0) += value * left;
            }
        }
        if let Some(residue) = saying.residue.filter(|r| ours(r.unit)) {
            let left = release(saying.now - residue.since);
            for (name, value) in &residue.controls {
                *controls.entry(*name).or_insert(0.0) += value * left;
            }
        }
    }

    /// The controllers a line adds to the face `t` seconds into it.
    pub fn line_controls(&self, line: &LineFace, t: f32, faces: &Faces) -> Controls {
        let mut controls = Controls::new();
        line.lipsync
            .apply(t, &faces.phonemes, &self.ranges, &mut controls);
        controls
    }

    /// Moves the face as it is at `now`, saying what `saying` says. Nothing is allocated once
    /// the face has posed once: it works in room it keeps.
    pub fn pose(
        &mut self,
        now: f32,
        saying: Saying,
        faces: &Faces,
        weights: &mut Query<&mut MorphWeights>,
    ) {
        let mut room = std::mem::take(&mut self.scratch);
        self.controls(now, saying, faces, (&mut room.controls, &mut room.line));
        for (node, rig) in self.rigs() {
            if let Ok(mut w) = weights.get_mut(node) {
                room.out.clear();
                room.out.extend_from_slice(w.weights());
                rig.weights(&room.controls, &mut room.out, &mut room.rig);
                if room.out != w.weights() {
                    w.weights_mut().copy_from_slice(&room.out);
                }
            }
        }
        self.scratch = room;
    }
}

/// Debug (`--face-trace`, for the talking-portrait test): prints the face on the stage on each
/// frame the portrait is drawn. Once a face: each of its meshes' morph targets
/// (`face-rig: MESH NAME...`, meshes by number); as it starts saying a line: the line's lip sync
/// file and the phoneme table (`face-line: FILE TABLE`); every frame: each mesh's weights as
/// drawn (`face: MESH T W...`, `T` seconds into the line, or `-` when it says none).
pub fn trace(
    stage: Res<crate::portrait::Stage>,
    speech: Res<Speech>,
    manifest: Res<Manifest>,
    time: Res<Time<bevy::time::Real>>,
    weights: Query<&MorphWeights>,
    mut traced: Local<(Option<Entity>, Option<String>)>,
) {
    let Some(actor) = stage.actor.as_ref().filter(|_| stage.drawing()) else {
        return;
    };
    let Some(face) = actor.face.as_ref() else {
        return;
    };
    if traced.0 != Some(actor.pivot) {
        traced.0 = Some(actor.pivot);
        for (i, (_, names)) in face.nodes().enumerate() {
            println!("face-rig: {i} {}", names.join(" "));
        }
    }
    let line = speech.of(stage.unit(), time.elapsed_secs());
    let file = line.map(|(line, _)| line.file.as_str());
    if file != traced.1.as_deref() {
        traced.1 = file.map(str::to_owned);
        let root = &manifest.root;
        if let (Some(file), Some(table)) = (file, manifest.phonemes.as_deref()) {
            println!(
                "face-line: {} {}",
                root.join(file).display(),
                root.join(table).display()
            );
        }
    }
    let t = line.map_or_else(|| "-".to_owned(), |(_, t)| format!("{t:.4}"));
    for (i, (node, _)) in face.nodes().enumerate() {
        let w = weights
            .get(node)
            .map(MorphWeights::weights)
            .unwrap_or_default();
        let w: Vec<String> = w.iter().map(|w| format!("{w:.4}")).collect();
        println!("face: {i} {t} {}", w.join(" "));
    }
}
