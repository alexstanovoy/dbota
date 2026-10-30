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

use crate::face::{Controls, Expression, Lipsync, Phonemes, Rig};
use crate::manifest::Manifest;

/// What faces move by, out of the cache.
#[derive(Resource, Default)]
pub struct Faces {
    phonemes: Phonemes,
    /// Idle expressions by the logical name of their model.
    expressions: HashMap<String, Arc<Expression>>,
    /// Rigs by glTF mesh name (none: the mesh's extras do not read).
    rigs: HashMap<String, Option<Arc<Rig>>>,
}

impl Faces {
    /// The idle expression a model's portrait plays, if it has one.
    pub fn expression(&self, model: &str) -> Option<Arc<Expression>> {
        self.expressions.get(model).cloned()
    }
}

/// A voice line's lip sync, and the cache file it was read from.
pub struct LineFace {
    pub lipsync: Lipsync,
    pub file: String,
}

/// The line one's hero is saying.
#[derive(Resource, Default)]
pub struct Speech(Option<Line>);

struct Line {
    /// The hero saying it; none for any face (a check's).
    unit: Option<EntityId>,
    face: Arc<LineFace>,
    /// The sound playing it, while it plays.
    player: Option<Entity>,
    /// Seconds since the start, at the line's start.
    anchor: f32,
    /// Debug: the line held at this many seconds in.
    held: Option<f32>,
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
        self.0 = Some(Line {
            unit,
            face,
            player,
            anchor: now,
            held: None,
        });
    }

    /// The line `unit`'s face says and how far into it, at `now`.
    pub fn of(&self, unit: Option<EntityId>, now: f32) -> Option<(&LineFace, f32)> {
        let line = self.0.as_ref()?;
        let t = line.held.unwrap_or(now - line.anchor);
        (line.unit.is_none() || line.unit == unit).then(|| (line.face.as_ref(), t))
    }

    /// Debug: holds the line at `t` seconds in.
    pub fn hold(&mut self, t: f32) {
        if let Some(line) = &mut self.0 {
            line.player = None;
            line.held = Some(t);
        }
    }

    pub fn stop(&mut self) {
        self.0 = None;
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Faces>()
        .init_resource::<Speech>()
        .add_systems(Startup, load)
        .add_systems(Update, follow);
}

fn load(manifest: Res<Manifest>, mut faces: ResMut<Faces>) {
    let Some(root) = manifest.root.as_deref() else {
        return;
    };
    let text =
        |rel: &str| crate::cachefs::read_small(root, rel).and_then(|b| String::from_utf8(b).ok());
    if let Some(table) = manifest.phonemes.as_deref().and_then(text) {
        match Phonemes::parse(&table) {
            Some(table) => faces.phonemes = table,
            None => warn!("the phoneme table does not read; faces say nothing"),
        }
    }
    for (model, rel) in &manifest.expressions {
        match text(rel).as_deref().and_then(Expression::parse) {
            Some(scene) => {
                faces.expressions.insert(model.clone(), Arc::new(scene));
            }
            None => warn!("{rel}: the expression does not read"),
        }
    }
}

/// The lip sync of a voice line (its cache path), if the cache has it.
pub fn lipsync(manifest: &Manifest, sound: &str) -> Option<Arc<LineFace>> {
    let rel = dbota_import::import::lipsync_path(sound);
    let bytes = crate::cachefs::read_small(manifest.root.as_deref()?, &rel)?;
    let parsed = String::from_utf8(bytes)
        .ok()
        .as_deref()
        .and_then(Lipsync::parse);
    if parsed.is_none() {
        warn!("{rel}: the lip sync does not read");
    }
    parsed.map(|lipsync| Arc::new(LineFace { lipsync, file: rel }))
}

/// Keeps the line on its sound's clock while it plays, and ends it once its face is still.
fn follow(
    time: Res<Time<bevy::time::Real>>,
    mut speech: ResMut<Speech>,
    sinks: Query<Option<&AudioSink>>,
) {
    let now = time.elapsed_secs();
    let Some(line) = speech.0.as_mut() else {
        return;
    };
    match line.player.map(|p| sinks.get(p)) {
        // Asked for, not playing yet: the line has not begun.
        Some(Ok(None)) => line.anchor = now,
        Some(Ok(Some(sink))) => line.anchor = now - sink.position().as_secs_f32(),
        // Gone (played out, or never heard): on its own clock.
        Some(Err(_)) => line.player = None,
        None => {}
    }
    if line.held.is_none() && now - line.anchor > line.face.lipsync.end() {
        speech.0 = None;
    }
}

/// A face on the portrait's stage: each of its meshes' morph weights and rig, the controllers'
/// ranges, its idle expression and when that began.
pub struct Face {
    meshes: Vec<(Entity, Arc<Rig>)>,
    ranges: HashMap<u32, (f32, f32)>,
    expression: Option<Arc<Expression>>,
    pub began: f32,
    /// Debug: the expression held at this many seconds in.
    pub frozen: Option<f32>,
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
        })
    }

    /// Moves the face as it is at `now`, saying `line` (and how far into it) if any.
    pub fn pose(
        &self,
        now: f32,
        line: Option<(&LineFace, f32)>,
        faces: &Faces,
        weights: &mut Query<&mut MorphWeights>,
    ) {
        let mut controls = Controls::new();
        if let Some(scene) = &self.expression {
            scene.apply(self.frozen.unwrap_or(now - self.began), &mut controls);
        }
        if let Some((line, t)) = line {
            line.lipsync
                .apply(t, &faces.phonemes, &self.ranges, &mut controls);
        }
        for (node, rig) in &self.meshes {
            if let Ok(mut w) = weights.get_mut(*node) {
                let mut out = w.weights().to_vec();
                rig.weights(&controls, &mut out);
                if out != w.weights() {
                    w.weights_mut().copy_from_slice(&out);
                }
            }
        }
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
        let root = manifest.root.as_deref().unwrap_or(std::path::Path::new(""));
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
