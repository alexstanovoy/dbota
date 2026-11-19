//! The faces of the portrait: what the cache gives to move them (Valve's phoneme table, the
//! heroes' idle expressions, each mesh's flex rig) and the line one's hero is saying.
//!
//! Dota lip-syncs the hero who speaks: a line moves the portrait's face while the portrait shows
//! that hero, whatever else is selected meanwhile (a creep's portrait never talks); the line's
//! clock follows the samples scheduled for presentation by the audio output.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bevy::audio::AudioPlayer;
use bevy::gltf::{GltfMeshExtras, GltfMeshName};
use bevy::mesh::morph::{MeshMorphWeights, MorphWeights};
use bevy::prelude::*;
use bota_proto::EntityId;

pub use crate::audio_output::Played as Heard;
use crate::audio_output::VoiceClock;
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
/// A newly available output buffer may be ahead of a face that held through an underrun.
const CATCH_UP_RATE: f32 = 1.5;
/// A phoneme contributes across a forward window, so its centre belongs at the audio cursor.
pub const PHONEME_CENTRE: f32 = crate::face::PHONEME_FILTER / 2.0;
/// If a line takes the mouth while another is audible, build its pose from zero as the old
/// pose is released. Its phoneme clock can still start exactly at the new line's sample zero.
const TAKEOVER_RISE: f32 = 0.04;

#[derive(Resource, Default)]
pub struct Speech {
    line: Option<Line>,
    waiting: VecDeque<Line>,
    leaving: Vec<Leaving>,
    residues: Vec<Residue>,
}

struct Line {
    unit: Option<EntityId>,
    face: Arc<LineFace>,
    player: Option<Entity>,
    voice: Option<VoiceClock>,
    began: f32,
    lead: f32,
    end: f32,
    /// Last pose time and media position. An unstarted line stays in pre-roll.
    presented: Option<(f32, f32)>,
    preroll: Option<f32>,
    takeover: Option<f32>,
    presented_end: Option<f32>,
    held: Option<f32>,
}

impl Line {
    fn speed(&self) -> f32 {
        self.voice.as_ref().map_or(1.0, VoiceClock::speed)
    }

    fn at(&self, now: f32) -> f32 {
        if let Some(held) = self.held {
            return held;
        }
        if self.player.is_none() {
            return now - self.began - self.lead;
        }
        let Some((_, position)) = self.presented else {
            return -self.lead;
        };
        if let Some(from) = self.preroll.filter(|_| position < 0.0) {
            let progress = ((position - from) / -from).clamp(0.0, 1.0);
            return -self.lead + (self.lead - PHONEME_CENTRE) * progress;
        }
        position - PHONEME_CENTRE
    }

    fn gain(&self, now: f32) -> f32 {
        self.takeover
            .map_or(1.0, |began| ((now - began) / TAKEOVER_RISE).clamp(0.0, 1.0))
    }

    fn schedule(&mut self, now: f32, position: f32) {
        if position >= 0.0 {
            return;
        }
        self.preroll
            .get_or_insert(position.max(PHONEME_CENTRE - self.lead));
        self.present(now, position);
    }

    fn present(&mut self, now: f32, position: f32) {
        let speed = self.speed();
        let position = self.presented.map_or(position, |(last, previous)| {
            position.min(previous + speed * CATCH_UP_RATE * (now - last).max(0.0))
        });
        let end = self.end + PHONEME_CENTRE;
        if position > end
            && let Some((last, previous)) = self.presented
            && previous <= end
        {
            self.presented_end =
                Some(last + (now - last) * ((end - previous) / (position - previous)));
        }
        self.presented = Some((now, position));
    }
}

/// A line that ended or was taken over: its face where it stopped, decaying from when it did.
struct Leaving {
    unit: Option<EntityId>,
    face: Arc<LineFace>,
    /// How far into it the line stopped, and when.
    at: f32,
    gain: f32,
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

/// The active voice and the expressions still releasing at this instant.
#[derive(Clone, Copy, Default)]
pub struct Saying<'a> {
    pub line: Option<(&'a LineFace, f32)>,
    gain: f32,
    leaving: &'a [Leaving],
    residues: &'a [Residue],
    unit: Option<EntityId>,
    now: f32,
}

impl Speech {
    pub fn say(
        &mut self,
        unit: Option<EntityId>,
        face: Arc<LineFace>,
        player: Option<Entity>,
        now: f32,
    ) {
        self.queue(unit, face, player, None, now);
    }

    pub fn say_heard(
        &mut self,
        unit: Option<EntityId>,
        face: Arc<LineFace>,
        player: Entity,
        voice: VoiceClock,
        now: f32,
    ) {
        self.queue(unit, face, Some(player), Some(voice), now);
    }

    fn queue(
        &mut self,
        unit: Option<EntityId>,
        face: Arc<LineFace>,
        player: Option<Entity>,
        voice: Option<VoiceClock>,
        now: f32,
    ) {
        let lead = face.lipsync.lead();
        let end = face.lipsync.end();
        let line = Line {
            unit,
            face,
            player,
            voice,
            began: now,
            lead,
            end,
            presented: None,
            preroll: None,
            takeover: None,
            presented_end: None,
            held: None,
        };
        if player.is_none() {
            self.leave(now);
            self.line = Some(line);
        } else {
            if self.waiting.len() == PENDING {
                self.waiting.pop_front();
            }
            self.waiting.push_back(line);
        }
    }

    pub fn of(&self, unit: Option<EntityId>, now: f32) -> Option<(&LineFace, f32)> {
        let line = self.line.as_ref()?;
        (line.unit.is_none() || line.unit == unit).then(|| (line.face.as_ref(), line.at(now)))
    }

    #[cfg(test)]
    pub fn has_waiting_line(&self) -> bool {
        !self.waiting.is_empty()
    }

    pub fn active_playback(&self) -> Option<(&VoiceClock, Entity)> {
        let line = self.line.as_ref()?;
        Some((line.voice.as_ref()?, line.player?))
    }

    pub fn saying(&self, unit: Option<EntityId>, now: f32) -> Saying<'_> {
        Saying {
            line: self.of(unit, now),
            gain: self.line.as_ref().map_or(1.0, |line| line.gain(now)),
            leaving: &self.leaving,
            residues: &self.residues,
            unit,
            now,
        }
    }

    /// Fold only after the departing unit's rig is available. Controller ranges belong to that
    /// rig, and an earlier spectator target can be selected again while its expression releases.
    pub fn settle(
        &mut self,
        unit: Option<EntityId>,
        now: f32,
        mut evaluate: impl FnMut(&LineFace, f32, &mut Controls),
        room: &mut Controls,
    ) {
        if self.leaving.is_empty() {
            return;
        }
        self.settle_unit(unit, now, &mut evaluate, room);
        if unit.is_some() {
            self.settle_unit(None, now, &mut evaluate, room);
        }
    }

    fn settle_unit(
        &mut self,
        unit: Option<EntityId>,
        now: f32,
        evaluate: &mut impl FnMut(&LineFace, f32, &mut Controls),
        room: &mut Controls,
    ) {
        if !self.leaving.iter().any(|line| line.unit == unit) {
            return;
        }
        let residue = self
            .residues
            .iter()
            .position(|residue| residue.unit == unit);
        let residue = match residue {
            Some(index) => &mut self.residues[index],
            None => {
                if self.residues.len() == PENDING {
                    self.residues.remove(0);
                }
                self.residues.push(Residue {
                    unit,
                    controls: Controls::new(),
                    since: now,
                });
                self.residues.last_mut().expect("residue just inserted")
            }
        };
        let left = release(now - residue.since);
        residue
            .controls
            .values_mut()
            .for_each(|value| *value *= left);
        residue.since = now;
        for line in self.leaving.extract_if(.., |line| line.unit == unit) {
            room.clear();
            evaluate(&line.face, line.at, room);
            let left = line.gain * release(now - line.since);
            for (&name, &value) in room.iter() {
                *residue.controls.entry(name).or_insert(0.0) += value * left;
            }
        }
    }

    /// Debug: holds the line at `t` seconds in.
    pub fn hold(&mut self, t: f32) {
        if let Some(line) = &mut self.line {
            line.held = Some(t);
        }
    }

    pub fn stop(&mut self) {
        self.line = None;
        self.waiting.clear();
        self.leaving.clear();
        self.residues.clear();
    }

    pub fn interrupt(&mut self, now: f32) {
        self.leave(now);
        self.waiting.clear();
    }

    #[cfg(test)]
    pub fn hear(&mut self, now: f32, heard: Heard) {
        let player = self
            .waiting
            .back()
            .or(self.line.as_ref())
            .and_then(|line| line.player);
        self.present(now, |line| {
            if line.player == player {
                heard
            } else {
                Heard::Waiting
            }
        });
    }

    fn present(&mut self, now: f32, mut heard: impl FnMut(&Line) -> Heard) {
        if let Some(line) = self.line.as_mut().filter(|line| line.player.is_some()) {
            match heard(line) {
                Heard::At(position) => line.present(now, position),
                Heard::Scheduled(position) => line.schedule(now, position),
                Heard::Ended | Heard::Gone => self.leave(now),
                Heard::Waiting => {}
            }
        }
        self.waiting.retain_mut(|line| match heard(line) {
            Heard::At(position) if position > 0.0 => {
                line.present(now, position);
                true
            }
            Heard::Ended | Heard::Gone => false,
            Heard::Scheduled(position) => {
                line.schedule(now, position);
                true
            }
            Heard::At(_) | Heard::Waiting => true,
        });
        let has_line = self.line.is_some();
        let latest = self
            .waiting
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, line)| {
                let position = line.presented?.1;
                (!has_line || position > 0.0).then_some((index, position / line.speed()))
            })
            .min_by(|(_, a), (_, b)| match (*a >= 0.0, *b >= 0.0) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (false, false) => b.total_cmp(a),
                _ => a.total_cmp(b),
            });
        if let Some((index, _)) = latest {
            self.leave(now);
            self.line = self.waiting.remove(index);
            if has_line && let Some(line) = &mut self.line {
                line.takeover = Some(now);
            }
            self.waiting
                .retain(|line| line.presented.is_none_or(|(_, position)| position <= 0.0));
        }
    }

    pub fn finish(&mut self, now: f32) {
        if self
            .line
            .as_ref()
            .is_some_and(|line| line.held.is_none() && line.at(now) > line.end)
        {
            self.leave(now);
        }
        self.leaving.retain(|line| now - line.since <= LET_GO);
        self.residues
            .retain(|residue| now - residue.since <= LET_GO);
    }

    fn leave(&mut self, now: f32) {
        let Some(line) = self.line.take() else { return };
        let at = line.at(now);
        let since = if line.player.is_none() && line.held.is_none() && at > line.end {
            line.began + line.lead + line.end
        } else {
            line.presented_end.unwrap_or(now)
        };
        if self.leaving.len() == PENDING {
            self.leaving.remove(0);
        }
        self.leaving.push(Leaving {
            unit: line.unit,
            gain: line.gain(now),
            face: line.face,
            at: at.min(line.end),
            since,
        });
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
        .add_systems(
            PostUpdate,
            follow.in_set(crate::audio_output::BeforePortrait),
        );
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

fn follow(
    time: Res<Time<bevy::time::Real>>,
    mut speech: ResMut<Speech>,
    output: Res<crate::audio_output::Output>,
    players: Query<(), With<AudioPlayer>>,
) {
    if speech.line.is_none()
        && speech.waiting.is_empty()
        && speech.leaving.is_empty()
        && speech.residues.is_empty()
    {
        return;
    }
    let now = time.elapsed_secs();
    if speech
        .line
        .as_ref()
        .is_some_and(|line| line.voice.is_some())
        || !speech.waiting.is_empty()
    {
        let Some(presentation) = output.presentation(time.elapsed()) else {
            speech.interrupt(now);
            speech.finish(now);
            return;
        };
        speech.present(now, |line| {
            let Some(voice) = &line.voice else {
                return Heard::Waiting;
            };
            presentation(
                voice,
                line.player.is_some_and(|player| players.contains(player)),
            )
        });
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
            if saying.gain >= 1.0 {
                line.lipsync
                    .apply(t, &faces.phonemes, &self.ranges, controls);
            } else if saying.gain > 0.0 {
                line_room.clear();
                line_room.extend(controls.iter().map(|(&name, &weight)| (name, weight)));
                line.lipsync
                    .apply(t, &faces.phonemes, &self.ranges, line_room);
                for (&name, &weight) in line_room.iter() {
                    let value = controls.entry(name).or_insert(0.0);
                    *value += saying.gain * (weight - *value);
                }
            }
        }
        let ours = |unit: Option<EntityId>| unit.is_none() || unit == saying.unit;
        for line in saying.leaving.iter().filter(|l| ours(l.unit)) {
            let left = line.gain * release(saying.now - line.since);
            line_room.clear();
            line.face
                .lipsync
                .apply(line.at, &faces.phonemes, &self.ranges, line_room);
            for (name, value) in line_room.iter() {
                *controls.entry(*name).or_insert(0.0) += value * left;
            }
        }
        for residue in saying.residues.iter().filter(|residue| ours(residue.unit)) {
            let left = release(saying.now - residue.since);
            for (name, value) in &residue.controls {
                *controls.entry(*name).or_insert(0.0) += value * left;
            }
        }
    }

    pub fn settle(&mut self, speech: &mut Speech, unit: Option<EntityId>, now: f32, faces: &Faces) {
        speech.settle(
            unit,
            now,
            |line, at, out| {
                line.lipsync.apply(at, &faces.phonemes, &self.ranges, out);
            },
            &mut self.scratch.line,
        );
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

pub(crate) fn trace_line(message: std::fmt::Arguments<'_>) {
    #[cfg(target_arch = "wasm32")]
    info!("{message}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{message}");
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
    args: Res<crate::Args>,
    portrait: Query<(&ComputedNode, &UiGlobalTransform), With<crate::hud::PortraitRender>>,
    output: Res<crate::audio_output::Output>,
) {
    let Some(actor) = stage.actor.as_ref().filter(|_| stage.drawing()) else {
        return;
    };
    let Some(face) = actor.face.as_ref() else {
        return;
    };
    if traced.0 != Some(actor.pivot) {
        traced.0 = Some(actor.pivot);
        if args.speech_check
            && let Ok((node, transform)) = portrait.single()
        {
            let size = node.size();
            let top_left = transform.translation - size / 2.0;
            trace_line(format_args!(
                "speech-check: crop {:.1} {:.1} {:.1} {:.1}",
                top_left.x, top_left.y, size.x, size.y
            ));
        }
        for (i, (_, names)) in face.nodes().enumerate() {
            trace_line(format_args!("face-rig: {i} {}", names.join(" ")));
        }
    }
    let line = speech.of(stage.unit(), time.elapsed_secs());
    let file = line.map(|(line, _)| line.file.as_str());
    if file != traced.1.as_deref() {
        traced.1 = file.map(str::to_owned);
        let root = &manifest.root;
        if let (Some(file), Some(table)) = (file, manifest.phonemes.as_deref()) {
            trace_line(format_args!(
                "face-line: {} {}",
                root.join(file).display(),
                root.join(table).display()
            ));
        }
    }
    if args.speech_check
        && let Some((clock, _)) = speech.active_playback()
        && let Some(present) = output.presentation(time.elapsed())
        && let Heard::At(position) = present(clock, true)
    {
        trace_line(format_args!("speech-check: presented {position:.5}"));
    }
    let t = line.map_or_else(|| "-".to_owned(), |(_, t)| format!("{t:.4}"));
    for (i, (node, _)) in face.nodes().enumerate() {
        let w = weights
            .get(node)
            .map(MorphWeights::weights)
            .unwrap_or_default();
        let w: Vec<String> = w.iter().map(|w| format!("{w:.4}")).collect();
        trace_line(format_args!("face: {i} {t} {}", w.join(" ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_releasing_expression_is_resolved_on_its_own_portrait() {
        let first = EntityId {
            idx: 1,
            generation: 0,
        };
        let second = EntityId {
            idx: 2,
            generation: 0,
        };
        let jaw = crate::face::token("jaw");
        let line = Arc::new(LineFace {
            lipsync: Lipsync::parse(&format!(
                "dbota-lipsync 1\nmorph 0 0 {jaw}\nkey 0 1\nkey 1 1\n"
            ))
            .expect("a timed jaw"),
            file: "line".to_owned(),
        });
        let face = |open| Face {
            meshes: vec![],
            ranges: HashMap::from([(jaw, (0.0, open))]),
            expression: None,
            began: 0.0,
            frozen: None,
            scratch: Scratch::default(),
        };
        let mut first_face = face(1.0);
        let mut second_face = face(0.25);
        let faces = Faces::default();
        let mut speech = Speech::default();
        speech.say(Some(first), line, None, 0.0);
        speech.say(
            Some(second),
            Arc::new(LineFace {
                lipsync: Lipsync::default(),
                file: "other".to_owned(),
            }),
            None,
            0.2,
        );
        second_face.settle(&mut speech, Some(second), 0.21, &faces);
        first_face.settle(&mut speech, Some(first), 0.22, &faces);
        let mut controls = Controls::new();
        let mut room = Controls::new();
        first_face.controls(
            0.22,
            speech.saying(Some(first), 0.22),
            &faces,
            (&mut controls, &mut room),
        );
        assert!(
            (controls.get(&jaw).copied().unwrap_or(0.0) - (-0.02f32 / RELEASE).exp()).abs() < 1e-5,
            "the first hero lost its jaw release: {controls:?}"
        );
        second_face.controls(
            0.22,
            speech.saying(Some(second), 0.22),
            &faces,
            (&mut controls, &mut room),
        );
        assert_eq!(
            controls.get(&jaw).copied().unwrap_or(0.0),
            0.0,
            "the second hero inherited the jaw"
        );
    }
    #[test]
    fn three_audible_lines_preserve_both_outgoing_expressions() {
        let unit = EntityId {
            idx: 1,
            generation: 0,
        };
        let jaw = crate::face::token("jaw");
        let line = Arc::new(LineFace {
            lipsync: Lipsync::parse("dbota-lipsync 1\nphoneme 0 1 97\n").expect("one phoneme"),
            file: "line".to_owned(),
        });
        let faces = Faces {
            phonemes: Phonemes::parse("dbota-phonemes 1\nkeys jaw\nphoneme 97 1\n")
                .expect("one jaw"),
            ..Faces::default()
        };
        let mut face = Face {
            meshes: vec![],
            ranges: HashMap::new(),
            expression: None,
            began: 0.0,
            frozen: None,
            scratch: Scratch::default(),
        };
        let mut speech = Speech::default();
        speech.say(
            Some(unit),
            line.clone(),
            Some(Entity::from_raw_u32(1).unwrap()),
            0.0,
        );
        speech.hear(0.044, Heard::At(0.044));
        speech.say(
            Some(unit),
            line.clone(),
            Some(Entity::from_raw_u32(2).unwrap()),
            0.045,
        );
        speech.hear(0.046, Heard::At(0.001));
        face.settle(&mut speech, Some(unit), 0.046, &faces);
        speech.hear(0.089, Heard::At(0.044));
        speech.say(
            Some(unit),
            line,
            Some(Entity::from_raw_u32(3).unwrap()),
            0.09,
        );
        speech.hear(0.091, Heard::At(0.001));
        face.settle(&mut speech, Some(unit), 0.091, &faces);

        let mut controls = Controls::new();
        let mut room = Controls::new();
        face.controls(
            0.092,
            speech.saying(Some(unit), 0.092),
            &faces,
            (&mut controls, &mut room),
        );
        let first = (-0.046f32 / RELEASE).exp();
        let second = (-0.001f32 / RELEASE).exp();
        let current =
            (0.001 + PHONEME_CENTRE) / crate::face::PHONEME_FILTER * (0.001 / TAKEOVER_RISE);
        let opening = controls.get(&jaw).copied().unwrap_or_default();
        // Neutral emphasis scales the table; derive that scale from an isolated phoneme.
        let mut full = Controls::new();
        Lipsync::parse("dbota-lipsync 1\nphoneme 0 1 97\n")
            .unwrap()
            .apply(0.08, &faces.phonemes, &HashMap::new(), &mut full);
        let wanted = (first + second + current) * full[&jaw];
        assert!(
            (opening - wanted).abs() < 1e-5,
            "the settled jaw was {opening:.4}; both releases give {wanted:.4}"
        );
    }
}
