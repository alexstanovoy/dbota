//! Units on the map: one entity per unit the client sees, carrying Dota's
//! model of it or a stand-in, kept where the frame puts it, and kept a
//! moment longer to fall when it dies.

use std::collections::HashMap;

use bevy::{
    gltf::Gltf, light::NotShadowCaster, prelude::*, world_serialization::WorldAsset,
    world_serialization::WorldInstanceReady,
};
use bota_client_core::{ClientSession, bodies};
use bota_core::{MapDef, NEUTRALS, NeutralKind, barracks_ranged_at, kind_def};
use bota_proto::{EntityId, ProjectileModel, Team, UnitKind, UnitView};

use crate::coords::sim_facing_to_rotation;
use crate::fog::FogGrid;
use crate::manifest::{Manifest, Model, UnitArt, load_unit_model};
use crate::map::{DOTA_ORIGIN, Ground, UNITS_PER_METRE};
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

/// Where a unit's model carries what its attacks need, as its model names them: the hands an
/// attack's missile leaves from (`attach_attack1`, `attach_attack2`) and the place one hits
/// (`attach_hitloc`).
#[derive(Component, Debug, Default)]
pub struct Attachments {
    pub attack: [Option<Entity>; 2],
    pub hitloc: Option<Entity>,
}

/// The animation player of a unit's model, on the unit's root.
#[derive(Component)]
pub struct Rig(pub Entity);

/// A wearable scene, drawn on its wearer's skeleton: its skins are bound to
/// the wearer's joints once both instances are ready, and bound again
/// whenever either instance is spawned anew (the scene spawner replaces an
/// instance whose scene changes, and the joints bound before go with it).
#[derive(Component)]
pub struct Wearable {
    wearer: Entity,
    ready: bool,
    bound: bool,
}

impl Wearable {
    /// A wearable of the model scene `wearer`, its own instance not ready yet.
    pub fn on(wearer: Entity) -> Self {
        Self {
            wearer,
            ready: false,
            bound: false,
        }
    }

    /// Its own instance is ready: its skins are on its own skeleton.
    pub fn mark_ready(&mut self) {
        self.ready = true;
        self.bound = false;
    }

    /// The model scene it is worn on.
    pub fn wearer(&self) -> Entity {
        self.wearer
    }

    /// Whether its skins are on its wearer's current skeleton.
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// The instance of `wearer` is ready, anew or for the first time: what
    /// it wears is bound to its joints again.
    pub fn wearer_ready(wearer: Entity, wearables: &mut Query<&mut Wearable>) {
        for mut wearable in wearables.iter_mut().filter(|w| w.wearer == wearer) {
            wearable.bound = false;
        }
    }
}

/// Scenes replaced after they loaded, all told: each replacement spawns
/// every unit drawn with the scene anew.
#[derive(Resource, Default)]
pub struct ReplacedScenes(pub u32);

/// Counts, and tells, a scene replaced once loaded: its file was loaded
/// twice.
fn count_replaced_scenes(
    mut events: MessageReader<AssetEvent<WorldAsset>>,
    server: Res<AssetServer>,
    mut replaced: ResMut<ReplacedScenes>,
) {
    for event in events.read() {
        if let AssetEvent::Modified { id } = event {
            warn!("scene {:?} replaced once loaded", server.get_path(*id));
            replaced.0 += 1;
        }
    }
}

/// The scenes a model is drawn with: its own first scene and that of each
/// thing it wears.
pub struct Scenes {
    pub body: Handle<WorldAsset>,
    pub wearables: Vec<Handle<WorldAsset>>,
}

/// One model the units may be drawn with.
pub struct BankModel {
    /// What the manifest says of it.
    pub model: Model,
    /// The glTF file, loaded once and whole: its scenes and clips are taken
    /// from it. (A labelled part asked of the asset server loads its file
    /// again, and that second load replaces the scenes already drawn.)
    pub gltf: Handle<Gltf>,
    /// The glTF files of what it wears.
    wearing: Vec<Handle<Gltf>>,
    /// Its scenes, once every file has loaded; none when its own failed.
    pub scenes: Option<Scenes>,
    /// Its clips once the file is loaded: logical name, graph node,
    /// duration and point in seconds.
    pub clips: Vec<(String, AnimationNodeIndex, f32, Option<f32>)>,
    /// The graph of its clips once the file is loaded.
    pub graph: Option<Handle<AnimationGraph>>,
    /// Whether the model, its wearables and everything they need are all
    /// loaded: no unit is drawn with it before, so no part goes missing.
    pub loaded: bool,
}

impl BankModel {
    /// Whether units may be drawn with it: every part loaded (or failed,
    /// said so), and its clips' graph built when it is drawn as a model.
    pub fn ready(&self) -> bool {
        self.loaded && (self.scenes.is_none() || self.graph.is_some())
    }
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

/// The glow of a hero's own colours, in emissive units.
const HERO_LIFT: f32 = 1200.0;

/// Seconds a dead unit is shown falling at most.
const FALL_SECONDS: f32 = 1.6;

pub fn plugin(app: &mut App) {
    app.init_resource::<UnitIndex>()
        .init_resource::<ReplacedScenes>()
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
            (
                build_graphs,
                count_replaced_scenes,
                sync_units,
                fall,
                bind_wearables,
            )
                .chain(),
        )
        .add_systems(PostUpdate, mark_figures)
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

/// Marks every mesh of a unit's model but a building's a
/// [`crate::fog::Figure`] (not what else hangs on the unit), for
/// the visual clarity aid; first lets every hero's own colours glow
/// faintly, so a dark model on dark ground still reads, as Dota's hero rim
/// light does (the mark has the material copied, glow and all).
pub(crate) fn mark_figures(
    mut commands: Commands,
    added: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    parents: Query<&ChildOf>,
    pivots: Query<&Pivot>,
    units: Query<&Unit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, material) in &added {
        let Some(kind) = parents
            .iter_ancestors(entity)
            .find_map(|a| pivots.get(a).ok())
            .and_then(|pivot| units.get(pivot.0).ok())
            .map(|u| u.kind)
        else {
            continue;
        };
        if kind_def(kind).structure {
            continue;
        }
        commands.entity(entity).insert(crate::fog::Figure);
        if kind == UnitKind::Hero
            && let Some(unit) = parents
                .iter_ancestors(entity)
                .find_map(|a| pivots.get(a).ok())
        {
            commands.entity(entity).insert(crate::fog::HeroPart(unit.0));
        }
        // Materials are shared by every instance of a model: lift once.
        if kind != UnitKind::Hero
            || materials
                .get(&material.0)
                .is_none_or(|m| m.emissive_texture.is_some())
        {
            continue;
        }
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.emissive_texture = m.base_color_texture.clone();
            m.emissive = LinearRgba::rgb(HERO_LIFT, HERO_LIFT, HERO_LIFT);
        }
    }
}

fn load_models(mut bank: ResMut<ModelBank>, server: Res<AssetServer>, manifest: Res<Manifest>) {
    // A model the cache lacks is drawn as a stand-in: loudly, not silently.
    if manifest.root.is_some() {
        for name in required_models(&manifest) {
            if manifest.model(&name).is_none() {
                error!(
                    "model {name} is not in the asset cache: drawn as a stand-in; run dbota import"
                );
            }
        }
    }
    // Every unit model loads while the game does, the neutrals' too: with
    // their textures block-compressed they take some 60 MiB of video memory,
    // and loaded at a camp's first sight their uploads held that frame.
    for model in manifest
        .models
        .iter()
        .filter(|m| m.attach.is_none() && !m.logical.starts_with(TREE_MODELS))
    {
        bank.0.push(bank_entry(&server, &manifest, model));
    }
}

/// The logical-name prefix of tree models: trees are not units and load
/// elsewhere (`trees.rs`).
const TREE_MODELS: &str = "tree.";

/// A model of the manifest, its files asked of the asset server: each
/// file once, as a whole.
fn bank_entry(server: &AssetServer, manifest: &Manifest, model: &Model) -> BankModel {
    let wearing = manifest
        .models
        .iter()
        .filter(|w| w.attach.as_deref() == Some(model.logical.as_str()))
        .map(|w| load_unit_model(server, w.path.clone()))
        .collect();
    BankModel {
        model: model.clone(),
        gltf: load_unit_model(server, model.path.clone()),
        wearing,
        scenes: None,
        clips: Vec::new(),
        graph: None,
        loaded: false,
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

/// Takes each model's scenes from its files, waits for all they need, and
/// builds its animation graph once its file has loaded.
fn build_graphs(
    mut bank: ResMut<ModelBank>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    use bevy::asset::RecursiveDependencyLoadState as State;
    for entry in bank.0.iter_mut().filter(|m| !m.loaded) {
        if entry.scenes.is_none() && !take_scenes(entry, &server, &gltfs) {
            continue;
        }
        // Its own file failed: its units are drawn as stand-ins.
        let Some(scenes) = &entry.scenes else {
            entry.loaded = true;
            continue;
        };
        // The scenes and all they need: meshes, skins, materials, images.
        // (The file's own handle never settles as loaded with everything:
        // it lists sub-assets no scene uses. Its clips are waited for by the
        // graph.)
        let states: Vec<State> = std::iter::once(&scenes.body)
            .chain(&scenes.wearables)
            .map(|scene| server.recursive_dependency_load_state(scene))
            .collect();
        if states.iter().any(|s| matches!(s, State::Failed(_))) {
            error!(
                "model {}: a part failed to load ({states:?}); its units are drawn without it",
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

/// Takes a model's first scene and each wearable's from their files once
/// every file has loaded or failed; whether it has. A wearable whose file
/// failed is left off, loudly.
fn take_scenes(entry: &mut BankModel, server: &AssetServer, gltfs: &Assets<Gltf>) -> bool {
    use bevy::asset::LoadState;
    let scene = |file: &Handle<Gltf>| match server.load_state(file) {
        LoadState::Loaded => Some(
            gltfs
                .get(file)
                .and_then(|gltf| gltf.scenes.first().cloned())
                .ok_or_else(|| "it has no scene".to_owned()),
        ),
        LoadState::Failed(err) => Some(Err(err.to_string())),
        LoadState::NotLoaded | LoadState::Loading => None,
    };
    let Some(body) = scene(&entry.gltf) else {
        return false;
    };
    let Some(wearing) = entry.wearing.iter().map(scene).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let logical = &entry.model.logical;
    let body = match body {
        Ok(body) => body,
        Err(why) => {
            error!("model {logical}: {why}; its units are drawn as stand-ins");
            return true;
        }
    };
    let mut wearables = Vec::with_capacity(wearing.len());
    for (file, scene) in entry.wearing.iter().zip(wearing) {
        match scene {
            Ok(scene) => wearables.push(scene),
            Err(why) => error!(
                "model {logical}: wearable {:?}: {why}; drawn without it",
                file.path()
            ),
        }
    }
    entry.scenes = Some(Scenes { body, wearables });
    true
}

/// How the manifest says a neutral creep or a map objective is drawn: by
/// the Dota unit it is.
fn unit_art<'a>(unit: &UnitView, manifest: &'a Manifest) -> Option<&'a UnitArt> {
    let npc = match objective_unit(unit.kind) {
        Some(npc) => npc,
        None => NeutralKind::of(unit.neutral?)?.neutral().unit,
    };
    manifest.unit_art(npc)
}

/// The Dota unit a kind of the map's objectives is (npc_units.txt), whose
/// `unit` line in the manifest draws it.
fn objective_unit(kind: UnitKind) -> Option<&'static str> {
    match kind {
        UnitKind::Outpost => Some("npc_dota_watch_tower"),
        UnitKind::Watcher => Some("npc_dota_lantern"),
        UnitKind::Shrine => Some("npc_dota_xp_fountain"),
        _ => None,
    }
}

/// What a unit is called: a neutral creep by its own name, anything else by
/// its kind's, a barracks by the one the map puts where it stands.
pub fn unit_name(map: &MapDef, unit: &UnitView) -> &'static str {
    match unit.neutral.and_then(NeutralKind::of) {
        Some(kind) => kind.neutral().name,
        None => kind_name(map, unit.kind, unit.team, unit.pos),
    }
}

/// What the game calls one unit on a map, from its kind, its side and where
/// it stands: the barracks that feeds the ranged creeps is the one the map
/// puts there, and each Ancient is its side's own.
fn kind_name(map: &MapDef, kind: UnitKind, team: Team, pos: bota_proto::Vec2) -> &'static str {
    match (kind, team) {
        (UnitKind::Ancient, Team::Radiant) => "Radiant's Ancient",
        (UnitKind::Ancient, Team::Dire) => "Dire's Ancient",
        (UnitKind::Barracks, _) if barracks_ranged_at(map, team, pos) == Some(true) => {
            "Ranged Barracks"
        }
        (UnitKind::Barracks, _) => "Melee Barracks",
        _ => kind_def(kind).name,
    }
}

/// The logical name of the model a unit is drawn with.
fn model_name(unit: &UnitView, map: &MapDef, manifest: &Manifest) -> Option<String> {
    if unit.kind == UnitKind::CreepNeutral || objective_unit(unit.kind).is_some() {
        return unit_art(unit, manifest)?.model.clone();
    }
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
        UnitKind::Barracks => {
            let reach = if bota_core::barracks_ranged_at(map, unit.team, unit.pos) == Some(true) {
                "ranged"
            } else {
                "melee"
            };
            format!("barracks.{side}.{reach}.model")
        }
        UnitKind::Fountain => format!("fountain.{side}.model"),
        UnitKind::Filler => format!("filler.{side}.model"),
        UnitKind::Courier => format!("courier.{side}.model"),
        _ => return None,
    })
}

/// How every building stands on Dota's map: its model's name, its bota
/// position, and the yaw the map turns the model by (radians, as the trees
/// are turned), read from the map export.
#[derive(Resource, Default)]
pub struct BuildingYaws(pub Vec<(String, Vec2, f32)>);

impl BuildingYaws {
    pub fn load(root: Option<&std::path::Path>) -> BuildingYaws {
        let Some(text) = root
            .and_then(|r| crate::cachefs::read_small(r, "map/mid/buildings.txt"))
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            return BuildingYaws::default();
        };
        BuildingYaws(
            crate::cache_text::buildings(&text, MAX_MAP_BUILDINGS)
                .into_iter()
                .map(|b| {
                    let at = Vec2::new(b.x + DOTA_ORIGIN, b.y + DOTA_ORIGIN);
                    (b.model, at, b.yaw.to_radians())
                })
                .collect(),
        )
    }

    /// The facing a building at a place is drawn with: the map's yaw of the
    /// same side's building of the same kind nearest it, within 600 units; a
    /// map objective's, whichever side holds it, of the objective standing
    /// there.
    pub fn facing(&self, unit: &UnitView, at: Vec2) -> Option<f32> {
        let side = match (unit.kind, unit.team) {
            (UnitKind::Outpost | UnitKind::Watcher | UnitKind::Shrine, _) => "",
            (_, Team::Radiant) => "radiant",
            (_, Team::Dire) => "dire",
            (_, Team::Neutral) => return None,
        };
        let kind = match unit.kind {
            UnitKind::Tower => "tower",
            UnitKind::Barracks => "barracks",
            UnitKind::Ancient => "ancient",
            UnitKind::Fountain => "fountain",
            // Radiant's fillers are statues, Dire's columns.
            UnitKind::Filler if unit.team == Team::Dire => "column",
            UnitKind::Filler => "statue",
            UnitKind::Outpost => "outpost",
            UnitKind::Watcher => "lantern_of_sight",
            UnitKind::Shrine => "xp_shrine",
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
    if kind_def(unit.kind).structure {
        return yaws.facing(unit, at).unwrap_or(std::f32::consts::FRAC_PI_4);
    }
    facing
}

/// The model of a hero, by the Dota name ids.txt gives its id.
fn hero_model(hero: u16, manifest: &Manifest) -> Option<String> {
    Some(format!("hero.{}.model", manifest.dota_name("hero", hero)?))
}

/// The logical name of the particle an attack's missile of look `model`
/// wears, thrown by side `team`: its thrower's kind's `<model>.attack`, a
/// neutral's as its unit line names it.
pub fn missile_effect(model: ProjectileModel, team: Team, manifest: &Manifest) -> Option<String> {
    let side = match team {
        Team::Radiant => "radiant",
        Team::Dire => "dire",
        Team::Neutral => "",
    };
    let lane = |kind: &str| (!side.is_empty()).then(|| format!("{kind}.{side}"));
    let model = match model {
        ProjectileModel::RangedCreep => lane("creep")? + ".ranged.model",
        ProjectileModel::SiegeCreep => lane("creep")? + ".siege.model",
        ProjectileModel::Tower => lane("tower")? + ".model",
        ProjectileModel::Fountain => lane("fountain")? + ".model",
        ProjectileModel::Hero(hero) => hero_model(hero.0, manifest)?,
        ProjectileModel::Neutral(id) => {
            let kind = NeutralKind::of(id)?;
            return manifest.unit_art(kind.neutral().unit)?.attack.clone();
        }
    };
    Some(format!("{}.attack", model.strip_suffix(".model")?))
}

/// The logical name of hero `hero`'s effect `what` (`hero.<name>.<what>`, as Dota names its
/// files after the hero's).
pub fn hero_effect(hero: bota_proto::HeroId, what: &str, manifest: &Manifest) -> Option<String> {
    let model = hero_model(hero.0, manifest)?;
    Some(format!("{}.{what}", model.strip_suffix(".model")?))
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
            "barracks.{}.ranged.model",
            "fountain.{}.model",
            "filler.{}.model",
            "courier.{}.model",
        ] {
            names.push(kind.replace("{}", side));
        }
    }
    let objectives = [UnitKind::Outpost, UnitKind::Watcher, UnitKind::Shrine]
        .into_iter()
        .filter_map(objective_unit);
    for unit in NEUTRALS.iter().map(|n| n.unit).chain(objectives) {
        match manifest.unit_art(unit) {
            Some(art) => names.extend(art.model.clone()),
            None => names.push(format!("{unit} (no unit line)")),
        }
    }
    names
}

/// How near the map's own model of a building must stand to a unit to give it its facing,
/// in world units.
const MODEL_MATCH_RADIUS: f32 = 600.0;
/// Buildings read from the cache at most (the map has 50); every structure
/// looks through them each frame.
const MAX_MAP_BUILDINGS: usize = 1024;
/// Heights of the stand-ins of buildings, in world units.
const TOWER_HEIGHT: f32 = 420.0;
const BARRACKS_HEIGHT: f32 = 220.0;
const ANCIENT_HEIGHT: f32 = 480.0;
const FOUNTAIN_HEIGHT: f32 = 120.0;

/// The radius and height of a unit's stand-in, in world units.
pub fn stand_in_size(kind: UnitKind, bound: f32) -> (f32, f32) {
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
    bank: ResMut<'w, ModelBank>,
    manifest: Res<'w, Manifest>,
    yaws: Res<'w, BuildingYaws>,
    stand_ins: Option<Res<'w, StandIns>>,
    ground: Res<'w, Ground>,
    fog: Res<'w, FogGrid>,
    time: Res<'w, Time>,
}

/// New units spawned a frame at most, and planted trees stood.
pub(crate) const SPAWNS_PER_FRAME: usize = 4;

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
    let Some(map) = core.session().map(ClientSession::map) else {
        return;
    };
    let shown = bodies(view, core.prev_view(), core.smooth.alpha());
    let mut spawned = 0;
    let mut seen = Vec::with_capacity(shown.len());
    let dt = p.time.delta_secs().max(1e-4);
    for body in &shown {
        let u = body.unit;
        // Buildings are seen through the fog, as in Dota.
        if p.fog.hides(u.seen_by) && !p.fog.shows_fogged(u) {
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
            // A few new units a frame: a wave, a camp or a seek brings
            // dozens, and each model's instance is a world of its own
            // cloned in (some 0.3 ms); the rest come the next frames.
            None if spawned >= SPAWNS_PER_FRAME => continue,
            None => {
                // Not before its model has loaded whole, wearables and all.
                let waiting = model_name(u, map, &p.manifest)
                    .and_then(|name| p.bank.0.iter().find(|m| m.model.logical == name))
                    .is_some_and(|m| !m.ready());
                if waiting {
                    continue;
                }
                let facing = drawn_facing(u, body.facing, at, &p.yaws);
                let entity = spawn_unit(&mut p, u, map, at, facing, place);
                spawned += 1;
                p.index.0.insert(u.id, entity);
            }
        }
    }
    seen.sort_unstable();
    let gone: Vec<EntityId> = p
        .index
        .0
        .keys()
        .filter(|id| seen.binary_search(id).is_err())
        .copied()
        .collect();
    for id in gone {
        let Some(entity) = p.index.0.remove(&id) else {
            continue;
        };
        // It falls when its side was told it died since it was last seen;
        // one that only left the sight goes.
        let died = core
            .session()
            .and_then(|session| session.entity(id))
            .is_some_and(|track| {
                let u = &track.unit;
                u.kind != UnitKind::Courier
                    && track
                        .last_death
                        .is_some_and(|death| death.tick >= track.last_seen_tick)
                    && (!p.fog.on || p.fog.lit(u.pos.x.to_f32(), u.pos.y.to_f32()))
            });
        if died {
            p.commands.entity(entity).insert(Dying(0.0));
        } else {
            p.commands.entity(entity).despawn();
        }
    }
}

/// Spawns one unit: a root at its place, and its model or stand-in.
fn spawn_unit(
    p: &mut SyncParams,
    u: &UnitView,
    map: &MapDef,
    at: Vec2,
    facing: f32,
    place: Vec3,
) -> Entity {
    let model = model_name(u, map, &p.manifest).and_then(|name| {
        p.bank
            .0
            .iter()
            .position(|m| m.model.logical == name && m.scenes.is_some())
    });
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
    match model.and_then(|index| Some((&p.bank.0[index], p.bank.0[index].scenes.as_ref()?))) {
        Some((entry, scenes)) => {
            let size = unit_art(u, &p.manifest)
                .and_then(|art| art.scale)
                .unwrap_or(entry.model.scale);
            let scale = Transform::from_scale(Vec3::splat(UNITS_PER_METRE * size));
            let scene = p
                .commands
                .spawn((WorldAssetRoot(scenes.body.clone()), scale, ChildOf(pivot)))
                .observe(on_model_ready)
                .id();
            for wearable in &scenes.wearables {
                p.commands
                    .spawn((
                        WorldAssetRoot(wearable.clone()),
                        scale,
                        ChildOf(pivot),
                        Wearable::on(scene),
                    ))
                    .observe(
                        |ready: On<WorldInstanceReady>, mut wearables: Query<&mut Wearable>| {
                            if let Ok(mut w) = wearables.get_mut(ready.entity) {
                                w.mark_ready();
                            }
                        },
                    );
            }
        }
        // A unit the manifest draws as nothing (a bare effect in Dota).
        None if unit_art(u, &p.manifest).is_some_and(|art| art.model.is_none()) => {}
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
    let building = kind_def(u.kind).structure;
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
    names: Query<&Name>,
    mut wearables: Query<&mut Wearable>,
) {
    // Its joints are new: what it wears follows them, not the ones before.
    Wearable::wearer_ready(ready.entity, &mut wearables);
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
    // The body's own attachments, which its clips move: a wearable carries copies on joints of
    // its own that stay at rest.
    if player.is_some() {
        commands
            .entity(unit)
            .insert(attachments(ready.entity, &children, &names));
    }
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
    // Buildings stand in the fog too, and are drawn dulled there, as are the
    // neutrals a one-sided view of a full stream shows through it.
    if units
        .get(unit)
        .is_ok_and(|u| kind_def(u.kind).structure || u.kind == UnitKind::CreepNeutral)
    {
        for part in children.iter_descendants(ready.entity) {
            commands.entity(part).insert(crate::fog::Fogged);
        }
    }
}

/// The attachments the model scene `scene` carries.
fn attachments(scene: Entity, children: &Query<&Children>, names: &Query<&Name>) -> Attachments {
    let mut found = Attachments::default();
    for part in children.iter_descendants(scene) {
        let slot = match names.get(part).map(Name::as_str) {
            Ok("attach_attack1") => &mut found.attack[0],
            Ok("attach_attack2") => &mut found.attack[1],
            Ok("attach_hitloc") => &mut found.hitloc,
            _ => continue,
        };
        *slot = Some(part);
    }
    found
}

/// The names of a worn skin's joints on its own skeleton, in its order and
/// lower case: what it is bound to its wearer's joints by, the first time and
/// whenever the wearer's instance is spawned anew.
#[derive(Component)]
struct WornJoints(Vec<String>);

/// Points the skins of every ready wearable not yet on its wearer's current
/// skeleton at the wearer's joints of the same name, case aside.
fn bind_wearables(
    mut commands: Commands,
    mut wearables: Query<(Entity, &mut Wearable)>,
    posable: Query<(), With<Posable>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut skins: Query<(&mut bevy::mesh::skinning::SkinnedMesh, Option<&WornJoints>)>,
) {
    for (entity, mut wearable) in &mut wearables {
        if wearable.bound || !wearable.ready || !posable.contains(wearable.wearer) {
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
            let Ok((mut skin, worn)) = skins.get_mut(part) else {
                continue;
            };
            // Named on its own skeleton while it is still on it.
            let own = match worn {
                Some(worn) => worn.0.clone(),
                None => {
                    let own: Vec<String> = skin
                        .joints
                        .iter()
                        .map(|joint| {
                            names
                                .get(*joint)
                                .map_or_else(|_| String::new(), |n| n.as_str().to_ascii_lowercase())
                        })
                        .collect();
                    commands.entity(part).insert(WornJoints(own.clone()));
                    own
                }
            };
            // A joint the wearer lacks stays on the wearable's own skeleton.
            let rebound: Vec<Entity> = own
                .iter()
                .zip(&skin.joints)
                .map(|(name, joint)| joints.get(name).copied().unwrap_or(*joint))
                .collect();
            skin.joints = rebound;
            // Its bounds were worked out on its own skeleton.
            commands
                .entity(part)
                .insert(bevy::camera::visibility::NoFrustumCulling);
        }
        wearable.bound = true;
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
