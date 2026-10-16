//! Units on the map: one entity per unit the client sees, carrying Dota's
//! model of it or a stand-in, kept where the frame puts it, and kept a
//! moment longer to fall when it dies.

use std::collections::HashMap;

use bevy::{
    gltf::{Gltf, GltfAssetLabel},
    light::NotShadowCaster,
    prelude::*,
    world_serialization::WorldAsset,
    world_serialization::WorldInstanceReady,
};
use bota_client_core::bodies;
use bota_proto::{EntityId, Team, UnitKind, UnitView};

use crate::coords::sim_facing_to_rotation;
use crate::fog::FogGrid;
use crate::manifest::{Manifest, Model, load_model};
use crate::map::{Ground, UNITS_PER_METRE};
use crate::session::Session;

/// A unit on the map.
#[derive(Component, Clone, Debug)]
pub struct Unit {
    /// Its handle on the wire.
    pub id: EntityId,
    /// What kind of unit it is.
    pub kind: UnitKind,
    /// Its side.
    pub team: Team,
    /// Its place in the [`ModelBank`], when Dota's model of it is drawn.
    pub model: Option<usize>,
    /// Its bota position and facing as drawn this frame.
    pub at: Vec2,
    /// Facing, radians counter-clockwise from east.
    pub facing: f32,
    /// World units a second it moved over the last frames.
    pub speed: f32,
}

/// A unit that has died and is shown falling, with the seconds since.
#[derive(Component, Debug)]
pub struct Dying(pub f32);

/// The entity of every unit shown, by its wire handle.
#[derive(Resource, Default)]
pub struct UnitIndex(pub HashMap<EntityId, Entity>);

/// The pivot under a unit's root that turns its model to face its way.
#[derive(Component)]
pub struct ModelOf(pub Entity);

/// The pivot of a unit's model, and the unit's root above it.
#[derive(Component)]
pub struct Pivot(pub Entity);

/// A model scene whose instance is ready.
#[derive(Component)]
pub struct Posable;

/// The animation player of a unit's model, on the unit's root.
#[derive(Component)]
pub struct Rig(pub Entity);

/// A wearable scene waiting for its own instance and its wearer's.
#[derive(Component)]
pub struct Wearable {
    wearer: Entity,
    ready: bool,
}

/// Marks a wearable not yet bound to its wearer's skeleton, for the visual
/// check.
pub type WearableMarker = Wearable;

/// One model the units may be drawn with.
pub struct BankModel {
    /// What the manifest says of it.
    pub model: Model,
    /// The glTF file.
    pub gltf: Handle<Gltf>,
    /// Its first scene.
    pub scene: Handle<WorldAsset>,
    /// The scenes of what it wears.
    pub wearables: Vec<Handle<WorldAsset>>,
    /// Its clips once the file is loaded: logical name, graph node,
    /// duration and point in seconds.
    pub clips: Vec<(String, AnimationNodeIndex, f32, Option<f32>)>,
    /// The graph of its clips once the file is loaded.
    pub graph: Option<Handle<AnimationGraph>>,
    /// Whether the model, its wearables and everything they need are all
    /// loaded: no unit is drawn with it before, so no part goes missing.
    pub loaded: bool,
}

/// Every model the units may be drawn with.
#[derive(Resource, Default)]
pub struct ModelBank(pub Vec<BankModel>);

/// The meshes and materials of the stand-ins.
#[derive(Resource)]
struct StandIns {
    body: Handle<Mesh>,
    building: Handle<Mesh>,
    radiant: Handle<StandardMaterial>,
    dire: Handle<StandardMaterial>,
    neutral: Handle<StandardMaterial>,
}

/// Seconds a dead unit is shown falling at most.
const FALL_SECONDS: f32 = 1.6;

pub fn plugin(app: &mut App) {
    app.init_resource::<UnitIndex>()
        .add_systems(
            PreStartup,
            |mut commands: Commands, manifest: Res<Manifest>| {
                commands.insert_resource(BuildingYaws::load(manifest.root.as_deref()));
            },
        )
        .init_resource::<ModelBank>()
        .add_systems(Startup, (load_models, make_stand_ins))
        .add_systems(
            Update,
            (build_graphs, sync_units, fall, bind_wearables).chain(),
        )
        .add_systems(PostUpdate, lift_heroes)
        .add_systems(
            PostUpdate,
            hold_roots
                .after(bevy::app::AnimationSystems)
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

/// A root joint held in place across the ground, by its bind translation.
#[derive(Component)]
struct RootLock(Vec3);

/// Takes root motion out of the animations: the joint keeps its height.
fn hold_roots(mut roots: Query<(&RootLock, &mut Transform)>) {
    for (lock, mut t) in &mut roots {
        t.translation.x = lock.0.x;
        t.translation.z = lock.0.z;
    }
}

/// Lets every hero's own colours glow faintly, so a dark model on dark
/// ground still reads, as Dota's hero rim light does.
fn lift_heroes(
    added: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    parents: Query<&ChildOf>,
    units: Query<&Unit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let lift = crate::look("lift", 1200.0);
    for (entity, material) in &added {
        let hero = parents
            .iter_ancestors(entity)
            .any(|a| units.get(a).is_ok_and(|u| u.kind == UnitKind::Hero));
        if !hero {
            continue;
        }
        // Materials are shared by every instance of a model: lift once.
        if materials
            .get(&material.0)
            .is_none_or(|m| m.emissive_texture.is_some())
        {
            continue;
        }
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.emissive_texture = m.base_color_texture.clone();
            m.emissive = LinearRgba::rgb(lift, lift, lift);
        }
    }
}

fn load_models(mut bank: ResMut<ModelBank>, server: Res<AssetServer>, manifest: Res<Manifest>) {
    // A model the cache lacks is drawn as a stand-in: loudly, not silently.
    if manifest.root.is_some() {
        for name in required_models(&manifest) {
            if manifest.model(&name).is_none() {
                error!(
                    "model {name} is not in the asset cache: drawn as a stand-in; run tools/import_assets.sh"
                );
            }
        }
    }
    for model in manifest.models.iter().filter(|m| m.attach.is_none()) {
        if model.logical.starts_with("tree.") {
            continue;
        }
        let wearables = manifest
            .models
            .iter()
            .filter(|w| w.attach.as_deref() == Some(model.logical.as_str()))
            .map(|w| load_model(&server, GltfAssetLabel::Scene(0).from_asset(w.path.clone())))
            .collect();
        bank.0.push(BankModel {
            model: model.clone(),
            gltf: load_model(&server, model.path.clone()),
            scene: load_model(
                &server,
                GltfAssetLabel::Scene(0).from_asset(model.path.clone()),
            ),
            wearables,
            clips: Vec::new(),
            graph: None,
            loaded: false,
        });
    }
}

fn make_stand_ins(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mat = |c: Color| StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.8,
        ..default()
    };
    commands.insert_resource(StandIns {
        body: meshes.add(Capsule3d::new(0.5, 1.0)),
        building: meshes.add(Cylinder::new(0.5, 1.0)),
        radiant: materials.add(mat(Color::srgb(0.35, 0.75, 0.35))),
        dire: materials.add(mat(Color::srgb(0.8, 0.3, 0.25))),
        neutral: materials.add(mat(Color::srgb(0.75, 0.65, 0.4))),
    });
}

/// Builds each model's animation graph once its file has loaded.
fn build_graphs(
    mut bank: ResMut<ModelBank>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    use bevy::asset::RecursiveDependencyLoadState as State;
    for entry in bank.0.iter_mut().filter(|m| !m.loaded) {
        // The scenes and all they need: meshes, skins, materials, images.
        // (The file's own handle never settles as loaded with everything:
        // it lists sub-assets no scene uses. Its clips are waited for by the
        // graph.)
        let states: Vec<State> =
            std::iter::once(server.recursive_dependency_load_state(&entry.scene))
                .chain(
                    entry
                        .wearables
                        .iter()
                        .map(|w| server.recursive_dependency_load_state(w)),
                )
                .collect();
        if states.iter().any(|s| matches!(s, State::Failed(_))) {
            error!(
                "model {}: a part failed to load ({states:?}); its units are drawn as stand-ins",
                entry.model.logical
            );
            entry.loaded = true;
            continue;
        }
        entry.loaded = states.iter().all(|s| matches!(s, State::Loaded));
    }
    for entry in bank.0.iter_mut().filter(|m| m.graph.is_none()) {
        if !server.is_loaded(&entry.gltf) {
            continue;
        }
        let Some(gltf) = gltfs.get(&entry.gltf) else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        for clip in &entry.model.clips {
            if let Some(handle) = gltf.named_animations.get(clip.name.as_str()) {
                let node = graph.add_clip(handle.clone(), 1.0, root);
                entry
                    .clips
                    .push((clip.logical.clone(), node, clip.duration, clip.point));
            }
        }
        entry.graph = Some(graphs.add(graph));
    }
}

/// The logical name of the model a unit is drawn with.
fn model_name(unit: &UnitView, manifest: &Manifest) -> Option<String> {
    let side = match unit.team {
        Team::Radiant => "radiant",
        Team::Dire => "dire",
        Team::Neutral => return None,
    };
    Some(match unit.kind {
        UnitKind::Hero => hero_model(unit.hero?.0, manifest)?,
        UnitKind::CreepMelee => format!("creep.{side}.melee.model"),
        UnitKind::CreepSiege => format!("creep.{side}.siege.model"),
        UnitKind::CreepFlagbearer => format!("creep.{side}.flagbearer.model"),
        UnitKind::CreepRanged => format!("creep.{side}.ranged.model"),
        UnitKind::Tower => format!("tower.{side}.model"),
        UnitKind::Ancient => format!("ancient.{side}.model"),
        UnitKind::Barracks => format!("barracks.{side}.melee.model"),
        UnitKind::Fountain => format!("fountain.{side}.model"),
        UnitKind::Filler => format!("filler.{side}.model"),
        UnitKind::Courier => format!("courier.{side}.model"),
        _ => return None,
    })
}

/// Whether a unit is a building: always shown, whatever the fog.
pub fn is_building(kind: UnitKind) -> bool {
    matches!(
        kind,
        UnitKind::Tower
            | UnitKind::Barracks
            | UnitKind::Ancient
            | UnitKind::Fountain
            | UnitKind::Filler
    )
}

/// How every building stands on Dota's map: its model's name, its bota
/// position, and the yaw the map turns the model by (radians, as the trees
/// are turned), read from the map export.
#[derive(Resource, Default)]
pub struct BuildingYaws(pub Vec<(String, Vec2, f32)>);

impl BuildingYaws {
    pub fn load(root: Option<&std::path::Path>) -> BuildingYaws {
        let Some(text) =
            root.and_then(|r| std::fs::read_to_string(r.join("map/mid/buildings.txt")).ok())
        else {
            return BuildingYaws::default();
        };
        BuildingYaws(
            text.lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| {
                    let w: Vec<&str> = l.split_whitespace().collect();
                    let [model, x, y, yaw] = w.as_slice() else {
                        return None;
                    };
                    let at = Vec2::new(
                        x.parse::<f32>().ok()? + 9216.0,
                        y.parse::<f32>().ok()? + 9216.0,
                    );
                    Some((model.to_string(), at, yaw.parse::<f32>().ok()?.to_radians()))
                })
                .collect(),
        )
    }

    /// The facing a building at a place is drawn with: the map's yaw of the
    /// same side's building of the same kind nearest it, within 600 units.
    pub fn facing(&self, unit: &UnitView, at: Vec2) -> Option<f32> {
        let side = match unit.team {
            Team::Radiant => "radiant",
            Team::Dire => "dire",
            Team::Neutral => return None,
        };
        let kind = match unit.kind {
            UnitKind::Tower => "tower",
            UnitKind::Barracks => "barracks",
            UnitKind::Ancient => "ancient",
            UnitKind::Fountain => "fountain",
            // Radiant's fillers are statues, Dire's columns.
            UnitKind::Filler if unit.team == Team::Dire => "column",
            UnitKind::Filler => "statue",
            _ => return None,
        };
        let (_, _, yaw) = self
            .0
            .iter()
            .filter(|(m, p, _)| {
                m.starts_with(side) && m.contains(kind) && p.distance(at) < MODEL_MATCH_RADIUS
            })
            .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))?;
        // A building model's face is its +x, as a unit's is, for both sides:
        // the map's yaw is its facing.
        Some(*yaw)
    }
}

/// Which way a unit is drawn facing: a building never turns (the wire
/// turns a tower to its target for bota's attack rules) and stands as
/// Dota's map places it; everything else as the wire says.
fn drawn_facing(unit: &UnitView, facing: f32, at: Vec2, yaws: &BuildingYaws) -> f32 {
    if is_building(unit.kind) {
        return yaws.facing(unit, at).unwrap_or(std::f32::consts::FRAC_PI_4);
    }
    facing
}

/// The model of a hero, by the Dota name ids.txt gives its id.
fn hero_model(hero: u16, manifest: &Manifest) -> Option<String> {
    Some(format!("hero.{}.model", manifest.dota_name("hero", hero)?))
}

/// Every model a match can need: each hero's and each lane unit's of both
/// sides. One missing from the cache is drawn as a stand-in, and said so.
fn required_models(manifest: &Manifest) -> Vec<String> {
    let mut names: Vec<String> = (0..bota_core::HEROES.len() as u16)
        .map(|id| {
            hero_model(id, manifest)
                .unwrap_or_else(|| format!("hero #{id} (no Dota name in ids.txt)"))
        })
        .collect();
    for side in ["radiant", "dire"] {
        for kind in [
            "creep.{}.melee.model",
            "creep.{}.flagbearer.model",
            "creep.{}.ranged.model",
            "creep.{}.siege.model",
            "tower.{}.model",
            "ancient.{}.model",
            "barracks.{}.melee.model",
            "fountain.{}.model",
            "filler.{}.model",
            "courier.{}.model",
        ] {
            names.push(kind.replace("{}", side));
        }
    }
    names
}

/// How near the map's own model of a building must stand to a unit to give it its facing,
/// in world units.
const MODEL_MATCH_RADIUS: f32 = 600.0;
/// Heights of the stand-ins of buildings, in world units.
const TOWER_HEIGHT: f32 = 420.0;
const BARRACKS_HEIGHT: f32 = 220.0;
const ANCIENT_HEIGHT: f32 = 480.0;
const FOUNTAIN_HEIGHT: f32 = 120.0;

/// The radius and height of a unit's stand-in, in world units.
fn stand_in_size(kind: UnitKind, bound: f32) -> (f32, f32) {
    match kind {
        UnitKind::Hero => (40.0, 180.0),
        UnitKind::Tower => (bound * 0.7, TOWER_HEIGHT),
        UnitKind::Barracks => (bound * 0.8, BARRACKS_HEIGHT),
        UnitKind::Ancient => (bound * 0.7, ANCIENT_HEIGHT),
        UnitKind::Fountain => (bound * 0.8, FOUNTAIN_HEIGHT),
        UnitKind::Courier => (20.0, 60.0),
        UnitKind::Ward => (14.0, 70.0),
        UnitKind::CreepSiege => (40.0, 90.0),
        _ => (24.0, 90.0),
    }
}

/// Everything the unit sync reads besides the core.
#[derive(bevy::ecs::system::SystemParam)]
struct SyncParams<'w, 's> {
    commands: Commands<'w, 's>,
    index: ResMut<'w, UnitIndex>,
    bank: Res<'w, ModelBank>,
    manifest: Res<'w, Manifest>,
    yaws: Res<'w, BuildingYaws>,
    stand_ins: Option<Res<'w, StandIns>>,
    ground: Res<'w, Ground>,
    fog: Res<'w, FogGrid>,
    time: Res<'w, Time>,
}

/// Spawns what came into view, moves what stays, and lets go of what left:
/// a unit that left where its side could still see it has died, and falls.
fn sync_units(
    session: NonSend<Session>,
    mut p: SyncParams,
    mut units: Query<(&mut Unit, &mut Transform)>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let shown = bodies(view, core.prev_view(), core.smooth.alpha());
    let hidden = view.viewer.is_none() && p.fog.on;
    let mut seen = Vec::with_capacity(shown.len());
    let dt = p.time.delta_secs().max(1e-4);
    for body in &shown {
        let u = body.unit;
        // Buildings are seen through the fog, as in Dota.
        if hidden
            && Some(u.team) != p.fog.team
            && !is_building(u.kind)
            && !p.fog.lit(body.x, body.y)
        {
            continue;
        }
        seen.push(u.id);
        let at = Vec2::new(body.x, body.y);
        let place = Vec3::new(body.x, p.ground.at(body.x, body.y), -body.y);
        match p.index.0.get(&u.id).and_then(|e| units.get_mut(*e).ok()) {
            Some((mut unit, mut transform)) => {
                let moved = at.distance(unit.at) / dt;
                unit.speed += (moved - unit.speed) * (dt * 10.0).min(1.0);
                unit.at = at;
                unit.facing = drawn_facing(u, body.facing, at, &p.yaws);
                // Moved only when it moves: a changed root sends its whole
                // model, skeleton and all, through transform propagation.
                if transform.translation != place {
                    transform.translation = place;
                }
            }
            None => {
                // Not before its model has loaded whole, wearables and all.
                let waiting = model_name(u, &p.manifest)
                    .and_then(|name| p.bank.0.iter().find(|m| m.model.logical == name))
                    .is_some_and(|m| !m.loaded || m.graph.is_none());
                if waiting {
                    continue;
                }
                let facing = drawn_facing(u, body.facing, at, &p.yaws);
                let entity = spawn_unit(&mut p, u, at, facing, place);
                p.index.0.insert(u.id, entity);
            }
        }
    }
    let gone: Vec<EntityId> = p
        .index
        .0
        .keys()
        .filter(|id| !seen.contains(id))
        .copied()
        .collect();
    for id in gone {
        let Some(entity) = p.index.0.remove(&id) else {
            continue;
        };
        let died = core.prev_view().is_some_and(|prev| {
            prev.units.iter().any(|u| {
                u.id == id
                    && u.kind != UnitKind::Courier
                    && (!p.fog.on || p.fog.lit(u.pos.x.to_f32(), u.pos.y.to_f32()))
                    && u.hp * 2 < u.max_hp.max(1)
            })
        });
        if died {
            p.commands.entity(entity).insert(Dying(0.0));
        } else {
            p.commands.entity(entity).despawn();
        }
    }
}

/// Spawns one unit: a root at its place, and its model or stand-in.
fn spawn_unit(p: &mut SyncParams, u: &UnitView, at: Vec2, facing: f32, place: Vec3) -> Entity {
    let model = model_name(u, &p.manifest)
        .and_then(|name| p.bank.0.iter().position(|m| m.model.logical == name));
    let root = p
        .commands
        .spawn((
            Unit {
                id: u.id,
                kind: u.kind,
                team: u.team,
                model,
                at,
                facing,
                speed: 0.0,
            },
            crate::anim::Pose::default(),
            Transform::from_translation(place),
            Visibility::default(),
        ))
        .id();
    let pivot = p
        .commands
        .spawn((
            Pivot(root),
            Transform::from_rotation(sim_facing_to_rotation(facing)),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    p.commands.entity(root).insert(ModelOf(pivot));
    match model {
        Some(index) => {
            let entry = &p.bank.0[index];
            let scale = Transform::from_scale(Vec3::splat(UNITS_PER_METRE * entry.model.scale));
            let scene = p
                .commands
                .spawn((WorldAssetRoot(entry.scene.clone()), scale, ChildOf(pivot)))
                .observe(on_model_ready)
                .id();
            for wearable in &entry.wearables {
                p.commands
                    .spawn((
                        WorldAssetRoot(wearable.clone()),
                        scale,
                        ChildOf(pivot),
                        Wearable {
                            wearer: scene,
                            ready: false,
                        },
                    ))
                    .observe(
                        |ready: On<WorldInstanceReady>, mut wearables: Query<&mut Wearable>| {
                            if let Ok(mut w) = wearables.get_mut(ready.entity) {
                                w.ready = true;
                            }
                        },
                    );
            }
        }
        None => spawn_stand_in(p, u, pivot),
    }
    root
}

/// A coloured capsule or column in place of a model.
fn spawn_stand_in(p: &mut SyncParams, u: &UnitView, pivot: Entity) {
    let Some(art) = &p.stand_ins else {
        return;
    };
    let (r, h) = stand_in_size(u.kind, u.bound.to_f32());
    let building = matches!(
        u.kind,
        UnitKind::Tower
            | UnitKind::Barracks
            | UnitKind::Ancient
            | UnitKind::Fountain
            | UnitKind::Filler
    );
    let mesh = if building {
        art.building.clone()
    } else {
        art.body.clone()
    };
    let material = match u.team {
        Team::Radiant => art.radiant.clone(),
        Team::Dire => art.dire.clone(),
        Team::Neutral => art.neutral.clone(),
    };
    let scale = if building {
        Vec3::new(2.0 * r, h, 2.0 * r)
    } else {
        Vec3::new(2.0 * r, h / 2.0, 2.0 * r)
    };
    let model = p
        .commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, h / 2.0, 0.0).with_scale(scale),
            ChildOf(pivot),
        ))
        .id();
    if u.kind == UnitKind::Fountain {
        p.commands.entity(model).insert(NotShadowCaster);
    }
}

/// Finds the animation player of a model's instance and gives it the
/// model's graph.
fn on_model_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    parents: Query<&ChildOf>,
    pivots: Query<&Pivot>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    units: Query<&Unit>,
    bank: Res<ModelBank>,
    joints: Query<(&Name, &Transform)>,
) {
    let Some(unit) = parents
        .get(ready.entity)
        .ok()
        .and_then(|c| pivots.get(c.parent()).ok())
        .map(|pivot| pivot.0)
    else {
        return;
    };
    let player = children
        .iter_descendants(ready.entity)
        .find(|e| players.contains(*e));
    if let (Some(player), Some(graph)) = (
        player,
        units
            .get(unit)
            .ok()
            .and_then(|u| u.model)
            .and_then(|m| bank.0[m].graph.clone()),
    ) {
        commands.entity(player).insert(AnimationGraphHandle(graph));
        commands.entity(unit).insert(Rig(player));
    }
    commands.entity(ready.entity).insert(Posable);
    // Pudge's run carries him forward eight metres a cycle on his root
    // joint (Dota moves the unit, not the bone): the root is held where
    // its bind pose puts it, up and down aside.
    for part in children.iter_descendants(ready.entity) {
        if let Ok((name, t)) = joints.get(part)
            && name.as_str() == "root"
        {
            commands.entity(part).insert(RootLock(t.translation));
        }
    }
    // Buildings stand in the fog too, and are drawn dulled there.
    if units.get(unit).is_ok_and(|u| {
        matches!(
            u.kind,
            UnitKind::Tower
                | UnitKind::Barracks
                | UnitKind::Ancient
                | UnitKind::Fountain
                | UnitKind::Filler
        )
    }) {
        for part in children.iter_descendants(ready.entity) {
            commands.entity(part).insert(crate::fog::Fogged);
        }
    }
}

/// Points every ready wearable's skin at its wearer's joints of the same
/// name, case aside.
fn bind_wearables(
    mut commands: Commands,
    wearables: Query<(Entity, &Wearable)>,
    posable: Query<(), With<Posable>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut skins: Query<&mut bevy::mesh::skinning::SkinnedMesh>,
) {
    for (entity, wearable) in &wearables {
        if !wearable.ready || !posable.contains(wearable.wearer) {
            continue;
        }
        let mut joints: HashMap<String, Entity> = HashMap::new();
        for e in children.iter_descendants(wearable.wearer) {
            if let Ok(name) = names.get(e) {
                joints.insert(name.as_str().to_ascii_lowercase(), e);
            }
        }
        let parts: Vec<Entity> = children.iter_descendants(entity).collect();
        for part in parts {
            let Ok(mut skin) = skins.get_mut(part) else {
                continue;
            };
            let rebound: Vec<Entity> = skin
                .joints
                .iter()
                .map(|joint| {
                    names
                        .get(*joint)
                        .ok()
                        .and_then(|n| joints.get(&n.as_str().to_ascii_lowercase()))
                        .copied()
                        .unwrap_or(*joint)
                })
                .collect();
            skin.joints = rebound;
            // Its bounds were worked out on its own skeleton.
            commands
                .entity(part)
                .insert(bevy::camera::visibility::NoFrustumCulling);
        }
        commands.entity(entity).remove::<Wearable>();
    }
}

/// Ages the fallen and lets them go once they have fallen.
fn fall(mut commands: Commands, time: Res<Time>, mut dying: Query<(Entity, &mut Dying)>) {
    for (entity, mut d) in &mut dying {
        d.0 += time.delta_secs();
        if d.0 > FALL_SECONDS {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hero and lane unit has a model line in the manifest: what the
    /// importer exports and the client draws.
    #[test]
    fn every_unit_a_match_can_show_has_a_model_in_the_manifest() {
        let manifest = Manifest::load(std::path::Path::new("/nonexistent"));
        let listed: Vec<&str> = crate::manifest::MANIFEST
            .lines()
            .filter_map(|l| l.strip_prefix("model "))
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        for name in required_models(&manifest) {
            assert!(listed.contains(&name.as_str()), "no model line for {name}");
        }
    }
}
