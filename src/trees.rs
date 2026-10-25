//! The forest: one Dota tree at every tree of the match, the model, turn
//! and size taken from where the real map puts its tree there, and a stump
//! where one has been cut down. Trees are merged into one mesh per model and
//! map chunk, rebuilt when a tree in the chunk falls or grows back. Planted
//! trees (an Iron Branch's) stand as Dota's temporary tree, one entity each.
//! Both are the watching side's forest: what it saw fall, grow and be
//! planted, as the view tells it.

use std::collections::{HashMap, HashSet};

use bota_proto::Vec2;

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
/// World units of one bucket of the lookup that pairs trees with models.
const HASH_CELL: f32 = 64.0;
/// World units of a side of a forest chunk.
const CHUNK: f32 = 2304.0;
/// Dota's temporary tree, which a planting raises (the engine's model for
/// every tree an ability or item plants).
const PLANTED_MODEL: &str = "tree.planted";
/// Trees read from the cache at most (the map has 2300): each tree of the
/// match is paired with the ones near it, and a file of a million in one
/// spot made that a billion pairs.
const MAX_MAP_TREES: usize = 16_384;

/// Seconds a felled tree takes to go down, and a regrown one to grow up.
const FALL_SECS: f32 = 1.0;
const GROW_SECS: f32 = 1.6;
/// Trees that fall or grow in one frame at most to be animated: a seek in a
/// replay may change thousands, which simply change.
const MAX_FX: usize = 24;
/// How far a tree's trunk reaches up for a cursor to point at it, and how
/// wide it is, in world units.
const TREE_HEIGHT: f32 = 520.0;
pub const TREE_RADIUS: f32 = 64.0;
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
    /// Which of the match's trees it is, as the view counts them.
    index: u32,
    model: usize,
    at: Vec3,
    yaw: f32,
    scale: f32,
}

/// The parts of one tree model once its file has loaded.
struct Kind {
    /// The standing tree's mesh without morph targets, for one drawn alone.
    plain: Option<(Handle<Mesh>, Option<bevy::camera::primitives::Aabb>)>,
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
    /// The felled trees the view told last frame, to see which fell and grew.
    told: Vec<u32>,
    /// Trees grown back in the view that are still growing on screen: their
    /// chunks show the stump until they have grown.
    growing: Vec<(u32, f32)>,
    /// Whether every model has loaded and the chunks are built.
    built: bool,
    /// The planted tree's model, among `kinds`.
    planted_kind: Option<usize>,
    /// The planted tree's mesh, without the morph targets an entity with no
    /// weights cannot be drawn with, its material, and its bounds (the mesh
    /// leaves the CPU once on the GPU).
    planted_mesh: Option<(
        Handle<Mesh>,
        Handle<StandardMaterial>,
        Option<bevy::camera::primitives::Aabb>,
    )>,
    /// The planted trees drawn, by where they stand.
    planted: HashMap<Vec2, Entity>,
}

impl Forest {
    /// The tree of the match with this index in the view's counting, if it is drawn.
    fn planted(&self, index: u32) -> Option<&Planted> {
        let at = self.trees.binary_search_by_key(&index, |t| t.index).ok()?;
        self.trees.get(at)
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Forest>()
        .init_resource::<SceneryReady>()
        .init_resource::<TreeTarget>()
        .add_systems(Update, target_tree.after(crate::pick::update))
        .add_systems(
            Update,
            (
                plant,
                read_kinds,
                rebuild,
                animate,
                ready_planted,
                plant_planted,
            )
                .chain(),
        );
}

/// Reads the real map's trees, if the cache has them.
fn map_trees(manifest: &Manifest) -> Vec<MapTree> {
    let Some(root) = &manifest.root else {
        return Vec::new();
    };
    let Some(text) = crate::cachefs::read_small(root, "map/mid/trees.txt")
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return Vec::new();
    };
    crate::cache_text::trees(&text, MAX_MAP_TREES)
        .into_iter()
        .map(|t| MapTree {
            model: t.model,
            x: t.x + DOTA_ORIGIN,
            y: t.y + DOTA_ORIGIN,
            yaw: t.yaw.to_radians(),
            scale: t.scale,
        })
        .collect()
}

/// Places one tree at every tree of the match once the match and the ground
/// are known, and starts loading the models they need.
fn plant(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    ground: Res<Ground>,
    server: Res<AssetServer>,
    mut forest: ResMut<Forest>,
    mut done: Local<bool>,
) {
    let core = &session.0;
    if *done || core.trees().is_empty() || !ground.known() {
        return;
    }
    *done = true;
    let known = map_trees(&manifest);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, t) in known.iter().enumerate() {
        grid.entry(((t.x / HASH_CELL) as i32, (t.y / HASH_CELL) as i32))
            .or_default()
            .push(i);
    }
    let mut names: Vec<String> = Vec::new();
    for (index, tree) in core.trees().iter().enumerate() {
        let (x, y) = (tree.x.to_f32(), tree.y.to_f32());
        let (cx, cy) = ((x / HASH_CELL) as i32, (y / HASH_CELL) as i32);
        let near = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .filter_map(|key| grid.get(&key))
            .flatten()
            .map(|i| &known[*i])
            .filter(|t| (t.x - x).hypot(t.y - y) <= MATCH_REACH)
            .min_by(|a, b| {
                (a.x - x)
                    .hypot(a.y - y)
                    .total_cmp(&(b.x - x).hypot(b.y - y))
            });
        let fallback = if x + y < bota_core::rules::MAP_SIZE as f32 {
            "tree_oak_01"
        } else {
            "dire_tree004"
        };
        let (name, yaw, scale) = near.map_or(
            (fallback, (index as f32 * 2.4) % std::f32::consts::TAU, 1.0),
            |t| (t.model.as_str(), t.yaw, t.scale),
        );
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
            index: index as u32,
            model,
            at: Vec3::new(x, ground.at(x, y), -y),
            yaw,
            scale,
        });
    }
    let models: Vec<_> = names
        .iter()
        .filter_map(|name| manifest.model(&format!("tree.{name}")))
        .collect();
    forest.planted_kind = manifest.model(PLANTED_MODEL).map(|_| models.len());
    forest.kinds = models
        .into_iter()
        .chain(manifest.model(PLANTED_MODEL))
        .map(|m| Kind {
            plain: None,
            gltf: crate::manifest::load_model(&server, m.path.clone()),
            tree: None,
            stump: None,
        })
        .collect();
    info!(
        "forest: {} trees of {} models",
        forest.trees.len(),
        forest.kinds.len()
    );
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
            let Some(source) = primitive
                .material
                .as_ref()
                .and_then(|m| gltf_materials.get(m))
            else {
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
    time: Res<Time>,
) {
    if forest.kinds.is_empty()
        && ground.known()
        && forest.trees.is_empty()
        && !ready.0
        && session.0.view().is_some()
    {
        // Nothing to plant: the ground alone is the scenery.
        ready.0 = !session.0.trees().is_empty();
    }
    let Some(view) = session.0.view() else {
        return;
    };
    if forest.trees.is_empty() || forest.kinds.iter().any(|k| k.tree.is_none()) {
        return;
    }
    let now = time.elapsed_secs();
    let mut told = view.felled_trees.clone();
    told.sort_unstable();
    if forest.built {
        let fell = difference(&told, &forest.told);
        let grew = difference(&forest.told, &told);
        if fell.len() + grew.len() <= MAX_FX {
            for tree in fell {
                animate_tree(&mut commands, &mut forest, &mut meshes, tree, Fx::Fall, now);
            }
            for tree in grew {
                forest.growing.push((tree, now + GROW_SECS));
                animate_tree(&mut commands, &mut forest, &mut meshes, tree, Fx::Grow, now);
            }
        }
    }
    forest.told = told.clone();
    forest
        .growing
        .retain(|(t, end)| *end > now && told.binary_search(t).is_err());
    // A tree still growing is drawn by its animation over a stump.
    let mut felled = told;
    felled.extend(forest.growing.iter().map(|(t, _)| *t));
    felled.sort_unstable();
    felled.dedup();
    let changed: Vec<u32> = if forest.built {
        felled
            .iter()
            .filter(|t| forest.felled.binary_search(t).is_err())
            .chain(
                forest
                    .felled
                    .iter()
                    .filter(|t| felled.binary_search(t).is_err()),
            )
            .copied()
            .collect()
    } else {
        forest.trees.iter().map(|t| t.index).collect()
    };
    if changed.is_empty() {
        return;
    }
    let chunk_of = |at: Vec3| ((at.x / CHUNK) as i32, (-at.z / CHUNK) as i32);
    let mut dirty: Vec<(i32, i32, usize)> = changed
        .iter()
        .filter_map(|t| forest.planted(*t))
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
            let Some(source) = meshes.get(mesh) else {
                continue;
            };
            let placed = forest.trees.iter().filter(|t| {
                t.model == key.2
                    && chunk_of(t.at) == (key.0, key.1)
                    && (felled.binary_search(&t.index).is_ok() == stumps)
            });
            let Some(mut merged) = merge(source, placed) else {
                continue;
            };
            // Drawn only: its vertices leave the CPU once on the GPU, its
            // bounds taken first.
            let aabb = bevy::camera::primitives::MeshAabb::compute_aabb(&merged);
            merged.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
            let mut chunk = commands.spawn((
                Mesh3d(meshes.add(merged)),
                MeshMaterial3d(material.clone()),
                crate::fog::Fogged,
            ));
            if let Some(aabb) = aabb {
                chunk.insert(aabb);
            }
            spawned.push(chunk.id());
        }
        forest.chunks.insert(key, spawned);
    }
    forest.felled = felled;
    forest.built = true;
    if !ready.0 {
        ready.0 = true;
        info!("startup: forest built at {:.2} s", crate::since_start());
    }
}

/// The trees of `a` that `b` lacks, both sorted.
fn difference(a: &[u32], b: &[u32]) -> Vec<u32> {
    a.iter()
        .filter(|t| b.binary_search(t).is_err())
        .copied()
        .collect()
}

/// What an animated tree does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fx {
    Fall,
    Grow,
}

/// A tree drawn alone while it falls or grows: where it stands, how it is
/// turned and sized, and when its animation began.
#[derive(Component)]
struct TreeFx {
    kind: Fx,
    born: f32,
    at: Vec3,
    turn: Quat,
    scale: f32,
    /// The axis a falling tree tips about.
    axis: Vec3,
}

/// Starts the falling or growing of a tree, drawn alone over its chunk's stump.
fn animate_tree(
    commands: &mut Commands,
    forest: &mut Forest,
    meshes: &mut Assets<Mesh>,
    tree: u32,
    kind: Fx,
    now: f32,
) {
    let Some(planted) = forest.planted(tree) else {
        return;
    };
    let (model, at, yaw, scale) = (planted.model, planted.at, planted.yaw, planted.scale);
    let Some(entry) = forest.kinds.get_mut(model) else {
        return;
    };
    let Some((mesh, material)) = entry.tree.clone() else {
        return;
    };
    if entry.plain.is_none() {
        let Some(source) = meshes.get(&mesh) else {
            return;
        };
        let mut plain = plain(source);
        let aabb = bevy::camera::primitives::MeshAabb::compute_aabb(&plain);
        plain.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
        entry.plain = Some((meshes.add(plain), aabb));
    }
    let Some((plain, aabb)) = entry.plain.clone() else {
        return;
    };
    // Each tree falls its own way, as no one fells it from one side.
    let way = (tree as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU);
    let direction = Vec3::new(way.cos(), 0.0, way.sin());
    let turn = Quat::from_rotation_y(yaw + std::f32::consts::PI);
    let mut drawn = commands.spawn((
        Mesh3d(plain),
        MeshMaterial3d(material),
        Transform::from_translation(at)
            .with_rotation(turn)
            .with_scale(if kind == Fx::Grow {
                Vec3::ZERO
            } else {
                Vec3::splat(UNITS_PER_METRE * scale)
            }),
        TreeFx {
            kind,
            born: now,
            at,
            turn,
            scale: UNITS_PER_METRE * scale,
            axis: Vec3::Y.cross(direction),
        },
        crate::fog::Fogged,
    ));
    if let Some(aabb) = aabb {
        drawn.insert(aabb);
    }
}

/// Tips the falling trees over, grows the growing ones up, and takes each
/// away when it is done.
fn animate(
    mut commands: Commands,
    time: Res<Time>,
    mut trees: Query<(Entity, &mut Transform, &TreeFx)>,
) {
    for (entity, mut transform, fx) in &mut trees {
        let life = if fx.kind == Fx::Fall {
            FALL_SECS
        } else {
            GROW_SECS
        };
        let p = ((time.elapsed_secs() - fx.born) / life).clamp(0.0, 1.0);
        match fx.kind {
            Fx::Fall => {
                // Gathers speed as it goes over, and shrinks away on the ground.
                let angle = std::f32::consts::FRAC_PI_2 * p * p;
                let gone = ((p - 0.8) / 0.2).clamp(0.0, 1.0);
                transform.rotation = Quat::from_axis_angle(fx.axis, angle) * fx.turn;
                transform.scale = Vec3::splat(fx.scale * (1.0 - gone * gone));
            }
            Fx::Grow => {
                transform.rotation = fx.turn;
                transform.scale = Vec3::splat(fx.scale * (1.0 - (1.0 - p).powi(3)));
            }
        }
        transform.translation = fx.at;
        if p >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// The tree the cursor points at while a Tango (or any slot that aims at a
/// tree) is taken up: its index among the match's trees and where it stands
/// in bota's coordinates, for the click to name.
#[derive(Resource, Default)]
pub struct TreeTarget(pub Option<(u32, Vec2)>);

/// Finds the standing tree under the cursor (the overlay rings it).
fn target_tree(
    session: NonSend<Session>,
    forest: Res<Forest>,
    over: Res<crate::input::OverHud>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>,
    mut target: ResMut<TreeTarget>,
) {
    let core = &session.0;
    let aiming_at_a_tree = core
        .aiming
        .and_then(|slot| core.aim_in(slot))
        .is_some_and(|aim| aim == bota_proto::Aim::Tree);
    let ray = window
        .cursor_position()
        .filter(|_| aiming_at_a_tree && !over.0)
        .and_then(|cursor| camera.0.viewport_to_world(camera.1, cursor).ok());
    let Some(ray) = ray else {
        if target.0.is_some() {
            target.0 = None;
        }
        return;
    };
    let mut best: Option<(f32, u32)> = None;
    for tree in &forest.trees {
        if forest.felled.binary_search(&tree.index).is_ok() {
            continue;
        }
        if let Some(along) = reach(ray, tree.at, TREE_HEIGHT, TREE_RADIUS)
            && best.is_none_or(|(b, _)| along < b)
        {
            best = Some((along, tree.index));
        }
    }
    let hit = best.and_then(|(_, i)| Some((i, *core.trees().get(i as usize)?)));
    if target.0 != hit {
        target.0 = hit;
    }
}

/// How far along the ray it comes within `radius` of the upright trunk
/// from `base` up `height`; none if it never does.
fn reach(ray: Ray3d, base: Vec3, height: f32, radius: f32) -> Option<f32> {
    // The ray's closest approach to the trunk's line, then the trunk's part
    // within height.
    let d = ray.direction.as_vec3();
    let w = ray.origin - base;
    let (a, b, c) = (d.dot(d), d.dot(Vec3::Y), 1.0);
    let (e, f) = (d.dot(w), Vec3::Y.dot(w));
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    let along = (b * f - c * e) / denom;
    let up = ((a * f - b * e) / denom).clamp(0.0, height);
    let on_ray = ray.origin + d * along.max(0.0);
    let on_trunk = base + Vec3::Y * up;
    (on_ray.distance(on_trunk) <= radius).then_some(along.max(0.0))
}

/// Makes the planted tree's mesh once its model has loaded, and one tree of
/// it as a warm-up piece: the model's pipelines compile while the game
/// loads, not in the frame the first tree is planted.
fn ready_planted(
    mut commands: Commands,
    mut forest: ResMut<Forest>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if forest.planted_mesh.is_some() {
        return;
    }
    let Some((source, material)) = forest
        .planted_kind
        .and_then(|k| forest.kinds.get(k))
        .and_then(|k| k.tree.clone())
    else {
        return;
    };
    let Some(source) = meshes.get(&source) else {
        return;
    };
    let mut mesh = plain(source);
    let aabb = bevy::camera::primitives::MeshAabb::compute_aabb(&mesh);
    mesh.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    let mesh = meshes.add(mesh);
    commands.spawn((
        crate::warmup::Warmup,
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        crate::fog::Fogged,
    ));
    forest.planted_mesh = Some((mesh, material, aabb));
}

/// Stands the planted trees the view tells of and takes down the ones it no
/// longer does: those the side saw planted and has not seen gone. A few new
/// ones a frame, as new units: a seek may bring them all at once.
fn plant_planted(
    mut commands: Commands,
    session: NonSend<Session>,
    mut forest: ResMut<Forest>,
    ground: Res<Ground>,
) {
    let (Some(view), Some((mesh, material, aabb))) =
        (session.0.view(), forest.planted_mesh.clone())
    else {
        return;
    };
    let told: HashSet<Vec2> = view.planted_trees.iter().copied().collect();
    forest.planted.retain(|at, entity| {
        let standing = told.contains(at);
        if !standing {
            commands.entity(*entity).despawn();
        }
        standing
    });
    let new: Vec<Vec2> = told
        .into_iter()
        .filter(|at| !forest.planted.contains_key(at))
        .take(crate::units::SPAWNS_PER_FRAME)
        .collect();
    for at in new {
        let (x, y) = (at.x.to_f32(), at.y.to_f32());
        // As a map tree's copy: glTF turned to face Bevy's forward, in metres.
        let transform = Transform::from_xyz(x, ground.at(x, y), -y)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
            .with_scale(Vec3::splat(UNITS_PER_METRE));
        let mut tree = commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            transform,
            crate::fog::Fogged,
        ));
        if let Some(aabb) = aabb {
            tree.insert(aabb);
        }
        let tree = tree.id();
        forest.planted.insert(at, tree);
    }
}

/// A mesh's attributes and triangles alone, its indices 32-bit: the Radiant
/// trees carry wind morph targets, which a merged chunk with no morph
/// weights cannot be drawn with.
fn plain(source: &Mesh) -> Mesh {
    let mut mesh = Mesh::new(source.primitive_topology(), source.asset_usage);
    for (attribute, values) in source.attributes() {
        mesh.insert_attribute(*attribute, values.clone());
    }
    match source.indices() {
        Some(Indices::U16(i)) => {
            mesh.insert_indices(Indices::U32(i.iter().map(|v| u32::from(*v)).collect()));
        }
        Some(indices) => mesh.insert_indices(indices.clone()),
        None => {}
    }
    mesh
}

/// Copies of a model mesh at every tree given, merged into one mesh.
fn merge<'a>(source: &Mesh, trees: impl Iterator<Item = &'a Planted>) -> Option<Mesh> {
    let source = plain(source);
    let mut out: Option<Mesh> = None;
    for t in trees {
        // The exported model is glTF: turned to face Bevy's forward, in metres.
        let transform = Transform::from_translation(t.at)
            .with_rotation(Quat::from_rotation_y(t.yaw + std::f32::consts::PI))
            .with_scale(Vec3::splat(UNITS_PER_METRE * t.scale));
        let copy = source.clone().transformed_by(transform);
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
