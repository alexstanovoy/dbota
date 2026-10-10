//! The forest: one Dota tree at every tree of the match, the model, turn
//! and size taken from where the real map puts its tree there, and a stump
//! where one has been cut down. Trees are merged into one mesh per model and
//! map chunk, rebuilt when a tree in the chunk falls or grows back.

use std::collections::HashMap;

use bevy::{
    gltf::{Gltf, GltfMesh},
    mesh::Indices,
    prelude::*,
};

use crate::manifest::Manifest;
use crate::map::{DOTA_ORIGIN, Ground, UNITS_PER_METRE};
use crate::session::Session;

/// World units within which a tree of the real map stands for one of the
/// match.
const MATCH_REACH: f32 = 48.0;
/// World units of a side of a forest chunk.
const CHUNK: f32 = 2304.0;

/// Whether the scenery a photograph of the map should show is in place:
/// the ground and the forest.
#[derive(Resource, Default)]
pub struct SceneryReady(pub bool);

/// One tree of the real map: model, place in bota's units, turn, size.
struct MapTree {
    model: String,
    x: f32,
    y: f32,
    yaw: f32,
    scale: f32,
}

/// One tree of the match: where it stands, how it is turned and sized, and
/// which model it is.
struct Planted {
    model: usize,
    at: Vec3,
    yaw: f32,
    scale: f32,
}

/// The parts of one tree model once its file has loaded.
struct Kind {
    gltf: Handle<Gltf>,
    tree: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    stump: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
}

/// The forest: every tree, the models, the chunk meshes and what they were
/// built for.
#[derive(Resource, Default)]
struct Forest {
    trees: Vec<Planted>,
    kinds: Vec<Kind>,
    /// Chunk and model to the entities drawing its trees and its stumps.
    chunks: HashMap<(i32, i32, usize), Vec<Entity>>,
    /// The felled trees the chunks were built for.
    felled: Vec<u32>,
    /// Whether every model has loaded and the chunks are built.
    built: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Forest>()
        .init_resource::<SceneryReady>()
        .add_systems(Update, (plant, read_kinds, rebuild).chain());
}

/// Reads the real map's trees, if the cache has them.
fn map_trees(manifest: &Manifest) -> Vec<MapTree> {
    let Some(root) = &manifest.root else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(root.join("map/mid/trees.txt")) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let w: Vec<&str> = line.split_whitespace().collect();
            let [model, x, y, _, yaw, scale] = w.as_slice() else {
                return None;
            };
            Some(MapTree {
                model: (*model).to_string(),
                x: x.parse::<f32>().ok()? + DOTA_ORIGIN,
                y: y.parse::<f32>().ok()? + DOTA_ORIGIN,
                yaw: yaw.parse::<f32>().ok()?.to_radians(),
                scale: scale.parse().ok()?,
            })
        })
        .collect()
}

/// Places one tree at every tree of the match once the match and the ground
/// are known, and starts loading the models they need.
#[allow(clippy::too_many_arguments)]
fn plant(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    ground: Res<Ground>,
    server: Res<AssetServer>,
    mut forest: ResMut<Forest>,
    args: Res<crate::Args>,
    mut done: Local<bool>,
) {
    let core = &session.0;
    if *done || core.trees.is_empty() || !ground.known() || args.without.iter().any(|w| w == "trees") {
        return;
    }
    *done = true;
    let known = map_trees(&manifest);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, t) in known.iter().enumerate() {
        grid.entry(((t.x / 64.0) as i32, (t.y / 64.0) as i32)).or_default().push(i);
    }
    let mut names: Vec<String> = Vec::new();
    for (index, &(x, y)) in core.trees.iter().enumerate() {
        let (cx, cy) = ((x / 64.0) as i32, (y / 64.0) as i32);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .filter_map(|key| grid.get(&key))
            .flatten()
            .map(|i| &known[*i])
            .filter(|t| (t.x - x).hypot(t.y - y) <= MATCH_REACH)
            .min_by(|a, b| (a.x - x).hypot(a.y - y).total_cmp(&(b.x - x).hypot(b.y - y)));
        let fallback = if x + y < 18432.0 { "tree_oak_01" } else { "dire_tree004" };
        let (name, yaw, scale) = near.map_or((fallback, (index as f32 * 2.4) % 6.28, 1.0), |t| (t.model.as_str(), t.yaw, t.scale));
        if manifest.model(&format!("tree.{name}")).is_none() {
            continue;
        }
        let model = match names.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                names.push(name.to_string());
                names.len() - 1
            }
        };
        forest.trees.push(Planted {
            model,
            at: Vec3::new(x, ground.at(x, y), -y),
            yaw,
            scale,
        });
        let _ = index;
    }
    forest.kinds = names
        .iter()
        .filter_map(|name| manifest.model(&format!("tree.{name}")))
        .map(|m| Kind {
            gltf: server.load(m.path.clone()),
            tree: None,
            stump: None,
        })
        .collect();
    info!("forest: {} trees of {} models", forest.trees.len(), forest.kinds.len());
}

/// Takes each tree model's standing and stump meshes once its file loads.
fn read_kinds(
    mut forest: ResMut<Forest>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<bevy::gltf::GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for kind in forest.kinds.iter_mut().filter(|k| k.tree.is_none()) {
        if !server.is_loaded(&kind.gltf) {
            continue;
        }
        let Some(gltf) = gltfs.get(&kind.gltf) else {
            continue;
        };
        for (name, handle) in &gltf.named_meshes {
            let Some(mesh) = gltf_meshes.get(handle) else {
                continue;
            };
            let Some(primitive) = mesh.primitives.first() else {
                continue;
            };
            let Some(source) = primitive.material.as_ref().and_then(|m| gltf_materials.get(m)) else {
                continue;
            };
            let material = materials.add(StandardMaterial {
                base_color: source.base_color,
                base_color_texture: source.base_color_texture.clone(),
                normal_map_texture: source.normal_map_texture.clone(),
                alpha_mode: source.alpha_mode,
                double_sided: source.double_sided,
                cull_mode: source.cull_mode,
                perceptual_roughness: 0.9,
                metallic: 0.0,
                ..default()
            });
            let part = (primitive.mesh.clone(), material);
            if name.contains("stump") {
                kind.stump = Some(part);
            } else {
                kind.tree = Some(part);
            }
        }
    }
}

/// Builds the chunk meshes once the models are there, and rebuilds the
/// chunks of trees that fell or grew back since.
fn rebuild(
    mut commands: Commands,
    session: NonSend<Session>,
    mut forest: ResMut<Forest>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ready: ResMut<SceneryReady>,
    ground: Res<Ground>,
) {
    if forest.kinds.is_empty() && ground.known() && forest.trees.is_empty() && !ready.0 && session.0.view.is_some() {
        // Nothing to plant: the ground alone is the scenery.
        ready.0 = !session.0.trees.is_empty();
    }
    let Some(view) = &session.0.view else {
        return;
    };
    if forest.trees.is_empty() || forest.kinds.iter().any(|k| k.tree.is_none()) {
        return;
    }
    let mut felled = view.felled_trees.clone();
    felled.sort_unstable();
    let changed: Vec<u32> = if forest.built {
        felled
            .iter()
            .filter(|t| forest.felled.binary_search(t).is_err())
            .chain(forest.felled.iter().filter(|t| felled.binary_search(t).is_err()))
            .copied()
            .collect()
    } else {
        (0..forest.trees.len() as u32).collect()
    };
    if changed.is_empty() {
        return;
    }
    let chunk_of = |at: Vec3| ((at.x / CHUNK) as i32, (-at.z / CHUNK) as i32);
    let mut dirty: Vec<(i32, i32, usize)> = changed
        .iter()
        .filter_map(|t| forest.trees.get(*t as usize))
        .map(|t| {
            let (cx, cy) = chunk_of(t.at);
            (cx, cy, t.model)
        })
        .collect();
    dirty.sort_unstable();
    dirty.dedup();
    for key in dirty {
        if let Some(old) = forest.chunks.remove(&key) {
            for e in old {
                commands.entity(e).despawn();
            }
        }
        let kind = &forest.kinds[key.2];
        let mut spawned = Vec::new();
        for (stumps, part) in [(false, &kind.tree), (true, &kind.stump)] {
            let Some((mesh, material)) = part else {
                continue;
            };
            let Some(source) = meshes.get(mesh).cloned() else {
                continue;
            };
            let placed = forest.trees.iter().enumerate().filter(|(i, t)| {
                t.model == key.2 && chunk_of(t.at) == (key.0, key.1) && (felled.binary_search(&(*i as u32)).is_ok() == stumps)
            });
            let Some(merged) = merge(&source, placed.map(|(_, t)| t)) else {
                continue;
            };
            spawned.push(commands.spawn((Mesh3d(meshes.add(merged)), MeshMaterial3d(material.clone()))).id());
        }
        forest.chunks.insert(key, spawned);
    }
    forest.felled = felled;
    forest.built = true;
    if !ready.0 {
        ready.0 = true;
    }
}

/// Copies of a model mesh at every tree given, merged into one mesh.
fn merge<'a>(source: &Mesh, trees: impl Iterator<Item = &'a Planted>) -> Option<Mesh> {
    let mut out: Option<Mesh> = None;
    for t in trees {
        // The exported model is glTF: turned to face Bevy's forward, in metres.
        let transform = Transform::from_translation(t.at)
            .with_rotation(Quat::from_rotation_y(t.yaw + std::f32::consts::PI))
            .with_scale(Vec3::splat(UNITS_PER_METRE * t.scale));
        let mut copy = source.clone();
        if let Some(Indices::U16(i)) = copy.indices() {
            let wide: Vec<u32> = i.iter().map(|v| u32::from(*v)).collect();
            copy.insert_indices(Indices::U32(wide));
        }
        let copy = copy.transformed_by(transform);
        match &mut out {
            Some(m) => {
                if m.merge(&copy).is_err() {
                    return out;
                }
            }
            None => out = Some(copy),
        }
    }
    out
}
