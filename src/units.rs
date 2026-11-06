//! Units on the map: one entity per unit the client sees, carrying Dota's
//! model of it, kept where the frame puts it, and kept a moment longer to fall when it dies.

use std::collections::HashMap;

use bevy::{
    gltf::Gltf, prelude::*, world_serialization::WorldAsset,
    world_serialization::WorldInstanceReady,
};
use bota_client_core::{ClientSession, bodies};
use bota_core::{MapDef, NEUTRALS, NeutralKind, barracks_ranged_at, kind_def};
use bota_proto::{
    EffectId, EntityId, LotusTier, ProjectileModel, Team, UnitKind, UnitView, WorldView,
};

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
    /// The ordinary model, whose manifest rows select state variants.
    base_model: Option<usize>,
    /// Elevation of the model actually drawn, not a pending replacement.
    elevation: f32,
    /// Forced elevation of the interpolated body.
    lift: f32,
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
    /// Every `attach_*` node of the model, by its name (case as the model has it).
    pub nodes: Vec<(String, Entity)>,
}

impl Attachments {
    /// The node of a name.
    pub fn node(&self, name: &str) -> Option<Entity> {
        self.nodes
            .iter()
            .find(|(node, _)| node == name)
            .map(|(_, entity)| *entity)
    }
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
    variants: Vec<ModelVariant>,
    elevation: f32,
}

struct ModelVariant {
    model: usize,
    when: ModelCondition,
}

#[derive(Clone, Copy)]
enum ModelCondition {
    Effect(EffectId),
    Side(Team),
    Pool { tier: LotusTier, map_skin: u8 },
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

/// The glow of a hero's own colours, in emissive units.
const HERO_LIFT: f32 = 1200.0;

/// Seconds a dead unit is shown falling at most.
const FALL_SECONDS: f32 = 1.6;

/// The frame's placing of every unit drawn (`Unit::at`, `Unit::facing` and the root's
/// transform): what is drawn at a unit (rings, ranges, the effects it wears) reads its place
/// after it, so it moves with the model in the same frame.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Placed;

pub fn plugin(app: &mut App) {
    app.init_resource::<UnitIndex>()
        .init_resource::<JumpedOver>()
        .init_resource::<ReplacedScenes>()
        .add_systems(
            PreStartup,
            |mut commands: Commands, manifest: Res<Manifest>| {
                commands.insert_resource(BuildingYaws::load(&manifest.root));
            },
        )
        .init_resource::<ModelBank>()
        .add_systems(Update, load_models.run_if(crate::loading::requested))
        .add_systems(PostUpdate, report.in_set(crate::loading::Report))
        .add_systems(
            Update,
            (
                build_graphs,
                count_replaced_scenes,
                sync_units.in_set(Placed),
                collapse,
                fall,
                bind_wearables,
            )
                .chain(),
        )
        .add_systems(PostUpdate, mark_figures);
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
    // A model the cache lacks is not drawn: loudly, not silently.
    for name in required_models(&manifest) {
        if manifest.model(&name).is_none() {
            error!(
                "model {name} is not in the asset cache: its units are not drawn; run dbota import"
            );
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
    load_model_variants(&mut bank, &manifest);
    info!("loading: {} unit models held resident", bank.0.len());
}

/// Resolve data rows once; selection in a frame compares only indexes and typed state.
fn load_model_variants(bank: &mut ModelBank, manifest: &Manifest) {
    use dbota_import::catalog::ModelCondition as Condition;
    for variant in &manifest.model_variants {
        let base = bank.0.iter().position(|m| m.model.logical == variant.base);
        let model = bank.0.iter().position(|m| m.model.logical == variant.model);
        let (Some(base), Some(model)) = (base, model) else {
            continue;
        };
        let when = match &variant.when {
            Condition::Modifier(name) => {
                let Some(effect) = bota_core::EFFECTS
                    .iter()
                    .find(|effect| !effect.retired && effect.key == name)
                else {
                    continue;
                };
                ModelCondition::Effect(effect.id)
            }
            Condition::Side(side) => ModelCondition::Side(*side),
            Condition::Pool { tier, map_skin } => ModelCondition::Pool {
                tier: *tier,
                map_skin: *map_skin,
            },
        };
        bank.0[model].elevation = variant.height;
        bank.0[base].variants.push(ModelVariant { model, when });
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
        variants: Vec::new(),
        elevation: 0.0,
    }
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
        // Its own file failed: its units are not drawn.
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
            error!("model {logical}: {why}; its units are not drawn");
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
        None if unit.kind == UnitKind::Ward => ward_unit(unit),
        None => NeutralKind::of(unit.neutral?)?.neutral().unit,
    };
    manifest.unit_art(npc)
}

/// The Dota unit a kind of the map's objectives is (npc_units.txt), whose
/// `unit` line in the manifest draws it.
fn objective_unit(kind: UnitKind) -> Option<&'static str> {
    kind_def(kind).npc
}

/// The side a unit is held for, as Dota colours a map objective by it: a side's own unit's (an
/// Outpost it holds), or the side a neutral one is lit for (a Watcher), which the wire tells
/// every view.
pub fn held_for(unit: &UnitView) -> Option<Team> {
    match unit.team {
        Team::Radiant | Team::Dire => Some(unit.team),
        Team::Neutral => unit.lit_for,
    }
}

/// The Dota unit (`npc_units.txt`) a ward, a map objective or a neutral creep is.
pub fn npc_unit(unit: &UnitView) -> Option<&'static str> {
    match objective_unit(unit.kind) {
        Some(npc) => Some(npc),
        None if unit.kind == UnitKind::Ward => Some(ward_unit(unit)),
        None => Some(NeutralKind::of(unit.neutral?)?.neutral().unit),
    }
}

/// The Dota unit a ward is: the one that gives true sight is a Sentry Ward.
fn ward_unit(ward: &UnitView) -> &'static str {
    if ward.true_sight_radius.to_f32() > 0.0 {
        "npc_dota_sentry_wards"
    } else {
        "npc_dota_observer_wards"
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

/// The ordinary model name, looked up once when a body appears. Manifest
/// rows select state variants from its cached index on subsequent frames.
fn base_model_name(unit: &UnitView, map: &MapDef, manifest: &Manifest) -> Option<String> {
    if kind_def(unit.kind).neutral
        || unit.kind == UnitKind::Ward
        || objective_unit(unit.kind).is_some()
    {
        let model = unit_art(unit, manifest)?.model.clone()?;
        return Some(model);
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

/// Elevation follows the model actually drawn, including while another model loads.
pub fn flight_height(unit: &Unit) -> f32 {
    unit.elevation + unit.lift
}

fn selected_model_index(
    base: Option<usize>,
    unit: &UnitView,
    view: &WorldView,
    map_skin: u8,
    bank: &ModelBank,
) -> Option<usize> {
    let base = base?;
    let variant = bank.0[base]
        .variants
        .iter()
        .find(|variant| match variant.when {
            ModelCondition::Effect(effect) => unit.effects.iter().any(|held| held.id == effect),
            ModelCondition::Side(side) => held_for(unit) == Some(side),
            ModelCondition::Pool {
                tier,
                map_skin: skin,
            } => {
                skin == map_skin
                    && view
                        .pools
                        .iter()
                        .any(|pool| pool.id == unit.id && pool.tier == tier)
            }
        });
    Some(variant.map_or(base, |variant| variant.model))
}

/// How every building stands on Dota's map, read from the map export.
#[derive(Resource, Default)]
pub struct BuildingYaws(pub Vec<MapBuilding>);

/// One building as Dota's map places it.
pub struct MapBuilding {
    /// Its model's name.
    pub model: String,
    /// Its bota position.
    pub at: Vec2,
    /// The yaw the map turns the model by (radians, as the trees are turned).
    pub yaw: f32,
    /// The model's material group the map draws it with.
    pub skin: u8,
    /// The map's scale of it on each of Dota's axes (x, y, up).
    pub scale: [f32; 3],
}

impl BuildingYaws {
    pub fn load(root: &std::path::Path) -> BuildingYaws {
        let Some(text) = crate::cachefs::read_small(root, "map/mid/buildings.txt")
            .and_then(|bytes| String::from_utf8(bytes).ok())
        else {
            return BuildingYaws::default();
        };
        BuildingYaws(
            crate::cache_text::buildings(&text, MAX_MAP_BUILDINGS)
                .into_iter()
                .map(|b| MapBuilding {
                    at: Vec2::new(b.x + DOTA_ORIGIN, b.y + DOTA_ORIGIN),
                    model: b.model,
                    yaw: b.yaw.to_radians(),
                    skin: b.skin,
                    scale: b.scale,
                })
                .collect(),
        )
    }

    /// The map's own building a building at a place is: the same side's
    /// building of the same kind nearest it, within 600 units; a map
    /// objective's, whichever side holds it, the objective standing there.
    pub fn placed(&self, unit: &UnitView, at: Vec2) -> Option<&MapBuilding> {
        let side = match (unit.kind, unit.team) {
            (UnitKind::Outpost | UnitKind::Watcher | UnitKind::Shrine | UnitKind::LotusPool, _) => {
                ""
            }
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
            UnitKind::LotusPool => "fountain_of_life",
            _ => return None,
        };
        self.0
            .iter()
            .filter(|b| {
                b.model.starts_with(side)
                    && b.model.contains(kind)
                    && b.at.distance(at) < MODEL_MATCH_RADIUS
            })
            .min_by(|a, b| a.at.distance(at).total_cmp(&b.at.distance(at)))
    }

    /// The facing a building at a place is drawn with: the map's yaw of its
    /// own building ([`BuildingYaws::placed`]).
    pub fn facing(&self, unit: &UnitView, at: Vec2) -> Option<f32> {
        // A building model's face is its +x, as a unit's is, for both sides:
        // the map's yaw is its facing.
        self.placed(unit, at).map(|b| b.yaw)
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
/// sides. One missing from the cache is not drawn, and said so.
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
    let objectives = bota_core::UNIT_KINDS
        .iter()
        .filter_map(|row| row.npc)
        .chain(["npc_dota_observer_wards", "npc_dota_sentry_wards"]);
    for unit in NEUTRALS.iter().map(|n| n.unit).chain(objectives) {
        match manifest.unit_art(unit) {
            Some(art) => names.extend(art.model.clone()),
            None => names.push(format!("{unit} (no unit line)")),
        }
    }
    names.extend(
        manifest
            .model_variants
            .iter()
            .map(|variant| variant.model.clone()),
    );
    names
}

/// How near the map's own model of a building must stand to a unit to give it its facing,
/// in world units.
const MODEL_MATCH_RADIUS: f32 = 600.0;
/// Buildings read from the cache at most (the map has 50); every structure
/// looks through them each frame.
const MAX_MAP_BUILDINGS: usize = 1024;
/// Heights of the buildings' picking volumes, in world units.
const TOWER_HEIGHT: f32 = 420.0;
const BARRACKS_HEIGHT: f32 = 220.0;
const ANCIENT_HEIGHT: f32 = 480.0;
const FOUNTAIN_HEIGHT: f32 = 120.0;

/// The radius and height of a unit's volume (what a click picks, what the clarity disc spreads
/// from), in world units.
pub fn unit_size(kind: UnitKind, bound: f32) -> (f32, f32) {
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
    ground: Res<'w, Ground>,
    fog: Res<'w, FogGrid>,
    time: Res<'w, Time>,
    jumped: Res<'w, JumpedOver>,
}

/// The ticks a replay was taken over without showing them (a seek: [`crate::capture`]): what fell
/// in them is shown as fallen, not falling. Live play never sets it, so a death the server tells
/// late (ticks coalesced) still falls.
#[derive(Resource, Default)]
pub struct JumpedOver(Option<(u32, u32)>);

impl JumpedOver {
    /// Notes the replay went from the view of tick `from` to that of tick `to` unseen.
    pub fn note(&mut self, from: u32, to: u32) {
        self.0 = match self.0 {
            Some((began, _)) => Some((began.min(from), to)),
            None => Some((from, to)),
        };
    }

    /// Whether something that died on `tick` died in the jump.
    pub fn over(&self, tick: u32) -> bool {
        self.0.is_some_and(|(from, to)| from < tick && tick <= to)
    }
}

/// New units of any kind spawned a frame at most (heroes have more room: [`spawn_cap`]), and
/// planted trees stood.
pub(crate) const SPAWNS_PER_FRAME: usize = 4;

/// New units spawned a frame at most once one of `kind` would be: the few of any kind, and a
/// hero of every seat besides.
fn spawn_cap(kind: UnitKind) -> usize {
    if kind == UnitKind::Hero {
        SPAWNS_PER_FRAME + bota_proto::limits::MAX_SEATS
    } else {
        SPAWNS_PER_FRAME
    }
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
    let Some(map) = core.session().map(ClientSession::map) else {
        return;
    };
    let shown = bodies(view, core.prev_view(), core.smooth.alpha());
    let mut spawned = 0;
    let mut seen = Vec::with_capacity(shown.len());
    for body in &shown {
        let u = body.unit;
        // Buildings are seen through the fog, as in Dota.
        if p.fog.hides(u.seen_by) && !p.fog.shows_fogged(u) {
            continue;
        }
        seen.push(u.id);
        // Bound scene creation per frame: a wave, a camp or a seek brings dozens, and each
        // model's instance is a world of its own cloned in; a match's heroes, one a seat, have
        // room besides ([`spawn_cap`]), and heroes past that wait like the rest.
        let can_spawn = spawned < spawn_cap(u.kind);
        spawned += usize::from(sync_body(&mut p, &mut units, body, map, view, can_spawn));
    }
    seen.sort_unstable();
    remove_missing_units(&mut p, &seen, core);
}

fn sync_body(
    p: &mut SyncParams,
    units: &mut Query<(&mut Unit, &mut Transform)>,
    body: &bota_client_core::Body<'_>,
    map: &MapDef,
    view: &WorldView,
    can_spawn: bool,
) -> bool {
    let live = body.unit;
    let at = Vec2::new(body.x, body.y);
    let drawn = p.index.0.get(&live.id).copied();
    let base = match drawn.and_then(|entity| units.get(entity).ok()) {
        Some((unit, _)) => unit.base_model,
        None => base_model_name(live, map, &p.manifest).and_then(|name| {
            p.bank
                .0
                .iter()
                .position(|model| model.model.logical == name)
        }),
    };
    let map_skin = p.yaws.placed(live, at).map_or(0, |building| building.skin);
    let selected = selected_model_index(base, live, view, map_skin, &p.bank);
    if let Some((entity, (mut unit, mut transform))) =
        drawn.and_then(|entity| Some((entity, units.get_mut(entity).ok()?)))
    {
        if model_changed(&p.bank, unit.model, selected) {
            p.commands.entity(entity).despawn();
            p.index.0.remove(&live.id);
        } else {
            move_drawn_unit(p, &mut unit, &mut transform, body);
        }
        return false;
    }
    if !can_spawn || selected.is_some_and(|index| !p.bank.0[index].ready()) {
        return false;
    }
    let place = Vec3::new(at.x, p.ground.at(at.x, at.y), -at.y);
    let facing = drawn_facing(live, body.facing, at, &p.yaws);
    let entity = spawn_unit(p, live, (at, facing, place), body.height, (base, selected));
    p.index.0.insert(live.id, entity);
    true
}

fn move_drawn_unit(
    p: &SyncParams,
    unit: &mut Unit,
    transform: &mut Transform,
    body: &bota_client_core::Body<'_>,
) {
    let at = Vec2::new(body.x, body.y);
    let dt = p.time.delta_secs().max(1e-4);
    let moved = at.distance(unit.at) / dt;
    unit.speed += (moved - unit.speed) * (dt * 10.0).min(1.0);
    unit.at = at;
    unit.lift = body.height;
    unit.facing = drawn_facing(body.unit, body.facing, at, &p.yaws);
    let place = Vec3::new(at.x, p.ground.at(at.x, at.y) + flight_height(unit), -at.y);
    // Avoid propagating an unchanged root through its whole skeleton.
    if transform.translation != place {
        transform.translation = place;
    }
}

/// Sight loss and replay jumps remove the old body; only a told death plays a fall.
fn remove_missing_units(p: &mut SyncParams, seen: &[EntityId], core: &bota_client_core::App) {
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
                    && track.last_death.is_some_and(|death| {
                        death.tick >= track.last_seen_tick && !p.jumped.over(death.tick)
                    })
                    && (!p.fog.on || p.fog.lit(u.pos.x.to_f32(), u.pos.y.to_f32()))
            });
        if died {
            p.commands.entity(entity).insert(Dying(0.0));
        } else {
            p.commands.entity(entity).despawn();
        }
    }
}

/// Whether the view selects another model for a drawn unit and that model
/// has loaded whole.
fn model_changed(bank: &ModelBank, drawn: Option<usize>, selected: Option<usize>) -> bool {
    drawn != selected
        && selected.is_some_and(|index| bank.0[index].ready() && bank.0[index].scenes.is_some())
}

/// Spawns one unit: a root at its place, and its model.
fn spawn_unit(
    p: &mut SyncParams,
    u: &UnitView,
    (at, facing, place): (Vec2, f32, Vec3),
    lift: f32,
    (base_model, model): (Option<usize>, Option<usize>),
) -> Entity {
    let model = model.filter(|index| p.bank.0[*index].scenes.is_some());
    let unit = Unit {
        id: u.id,
        kind: u.kind,
        team: u.team,
        model,
        base_model,
        elevation: model.map_or(0.0, |index| p.bank.0[index].elevation),
        lift,
        at,
        facing,
        speed: 0.0,
    };
    let place = place + Vec3::Y * flight_height(&unit);
    let root = p
        .commands
        .spawn((
            unit,
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
    spawn_model_scene(p, u, pivot, model, at);
    root
}

/// The scale of a model the map places with `scale` on each of Dota's axes (x, y, up), whose
/// own size is `size` (glTF's x, up and z are Dota's y, up and x).
pub(crate) fn map_scale([sx, sy, sz]: [f32; 3], size: f32) -> Transform {
    Transform::from_scale(Vec3::new(sy, sz, sx) * (UNITS_PER_METRE * size))
}

fn spawn_model_scene(
    p: &mut SyncParams,
    u: &UnitView,
    pivot: Entity,
    model: Option<usize>,
    at: Vec2,
) {
    // A unit the manifest draws as nothing (a bare effect in Dota), or whose model the cache lacks
    // (said at load), has no scene to spawn.
    if let Some((entry, scenes)) =
        model.and_then(|index| Some((&p.bank.0[index], p.bank.0[index].scenes.as_ref()?)))
    {
        let size = unit_art(u, &p.manifest)
            .and_then(|art| art.scale)
            .unwrap_or(entry.model.scale);
        let scale = map_scale(p.yaws.placed(u, at).map_or([1.0; 3], |b| b.scale), size);
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
    // Buildings stand in the fog too, and are drawn dulled there, as are the
    // neutrals a one-sided view of a full stream shows through it.
    if units
        .get(unit)
        .is_ok_and(|u| kind_def(u.kind).structure || kind_def(u.kind).neutral)
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
        let Ok(name) = names.get(part).map(Name::as_str) else {
            continue;
        };
        if name.starts_with("attach_") {
            found.nodes.push((name.to_owned(), part));
        }
        match name {
            "attach_attack1" => found.attack[0] = Some(part),
            "attach_attack2" => found.attack[1] = Some(part),
            "attach_hitloc" => found.hitloc = Some(part),
            _ => {}
        }
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

/// The model a fallen unit's model is swapped for, by the manifest's naming: `tower.dire.model`
/// falls apart as `tower.dire.destruction` (Dota's baked destruction models).
fn destruction_name(model: &str) -> Option<String> {
    Some(format!("{}.destruction", model.strip_suffix(".model")?))
}

/// Swaps a unit that has just died, whose model has a destruction model in the bank, for it: it
/// plays its death clip from the start, on the same ground at the same facing.
fn collapse(
    session: NonSend<Session>,
    mut p: SyncParams,
    mut fallen: Query<(Entity, &mut Unit, &ModelOf), Added<Dying>>,
) {
    for (entity, mut unit, pivot) in &mut fallen {
        let Some((name, replacement)) = unit
            .base_model
            .and_then(|base| destruction_name(&p.bank.0[base].model.logical))
            .and_then(|name| {
                let index = p.bank.0.iter().position(|m| m.model.logical == name)?;
                Some((name, index))
            })
        else {
            continue;
        };
        // Added<Dying> fires once: a model not ready now is not drawn later, so say so.
        if !p.bank.0[replacement].ready() || p.bank.0[replacement].scenes.is_none() {
            warn!(
                "{name} is not loaded: the fallen {:?} keeps its own model",
                unit.kind
            );
            continue;
        }
        let Some(last) = session.0.session().and_then(|s| s.entity(unit.id)) else {
            continue;
        };
        p.commands.entity(pivot.0).despawn_related::<Children>();
        p.commands
            .entity(entity)
            .remove::<Rig>()
            .insert(crate::anim::Pose::default());
        unit.model = Some(replacement);
        unit.elevation = p.bank.0[replacement].elevation;
        spawn_model_scene(&mut p, &last.unit, pivot.0, Some(replacement), unit.at);
    }
}

/// Ages the fallen and lets them go once they have fallen: after the death clip of the model
/// they are drawn with, and never before [`FALL_SECONDS`].
fn fall(
    mut commands: Commands,
    time: Res<Time>,
    bank: Res<ModelBank>,
    mut dying: Query<(Entity, &Unit, &mut Dying)>,
) {
    for (entity, unit, mut d) in &mut dying {
        d.0 += time.delta_secs();
        let death = unit
            .model
            .and_then(|model| bank.0[model].clips.iter().find(|c| c.0 == "death"))
            .map_or(0.0, |clip| clip.2);
        if d.0 > death.max(FALL_SECONDS) {
            commands.entity(entity).despawn();
        }
    }
}

/// The unit models' part of the loading.
fn report(bank: Res<ModelBank>, mut parts: ResMut<crate::loading::Parts>) {
    if bank.0.is_empty() {
        return;
    }
    let ready = bank.0.iter().filter(|m| m.ready()).count();
    parts.set("models", ready, bank.0.len());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A hero unit of no seat, as a server or a replay may send any number of.
    fn stray_hero(idx: u32) -> UnitView {
        use bota_proto::{Angle, Attributes, Fixed, HeroId, StatusFlags, Vec2};
        let fixed = |units: i32| Fixed { raw: units << 16 };
        UnitView {
            id: EntityId { idx, generation: 1 },
            kind: UnitKind::Hero,
            team: Team::Radiant,
            pos: Vec2::from_ints(0, 0),
            facing: Angle { brads: 0 },
            hp: 600,
            max_hp: 600,
            mana: 0,
            max_mana: 0,
            move_speed: fixed(300),
            attack_damage: 50,
            attack_damage_min: 50,
            attack_damage_max: 50,
            hp_regen: Fixed::ZERO,
            mana_regen: Fixed::ZERO,
            town_portal: None,
            neutral: None,
            cast: None,
            queue: Vec::new(),
            status_resist: Fixed::ZERO,
            evasion: Fixed::ZERO,
            hit_health: None,
            vision_grant: None,
            cast_rates: None,
            lit_for: None,
            gathering: None,
            forced: None,
            channel: None,
            seen_by: None,
            attack_range: fixed(150),
            attack_time: 1700,
            attack_point: 500,
            attack_speed: 100,
            attributes: Attributes::ZERO,
            primary: None,
            armor: Fixed::ZERO,
            magic_resist: Fixed::ZERO,
            collision: fixed(24),
            bound: fixed(24),
            vision_radius: fixed(1800),
            true_sight_radius: Fixed::ZERO,
            statuses: StatusFlags { bits: 0 },
            hero: Some(HeroId::SHADOW_FIEND),
            owner: None,
            level: 1,
            abilities: Vec::new(),
            items: Vec::new(),
            effects: Vec::new(),
        }
    }

    /// A replay of one snapshot that shows `heroes` heroes of no seat.
    pub(crate) fn replay_of_heroes(heroes: u32) -> Vec<u8> {
        replay_of_heroes_on_grid(heroes, 1)
    }

    pub(crate) fn replay_of_heroes_on_grid(heroes: u32, cells: u32) -> Vec<u8> {
        use bota_proto::{
            HeroId, MapId, MatchInfo, Pick, PlayerView, ReplayHeader, ReplayRecord, ServerMsg,
            SlotId, TickMode, Vec2, WorldView, encode_frame_to_vec,
        };
        let start = ServerMsg::MatchStart {
            info: MatchInfo {
                match_id: 1,
                map: MapId(0),
                tick_rate: 30,
                pregame_ticks: 900,
                trees: Vec::new(),
                terrain_cells: cells,
                terrain_rle: vec![(
                    u16::try_from(cells.checked_mul(cells).expect("fixture grid area"))
                        .expect("the fixture grid fits one terrain run"),
                    0x80,
                )],
                opaque_cells: Vec::new(),
                mode: TickMode::Lockstep,
                picks: vec![Pick {
                    slot: SlotId(0),
                    team: Team::Radiant,
                    hero: HeroId::SHADOW_FIEND,
                }],
                shop: Vec::new(),
                fountains: [Vec2::ZERO, Vec2::ZERO],
                shop_range: 0,
                secret_shops: Vec::new(),
                universal_shop: false,
                cheats: false,
                objectives: bota_proto::ObjectiveSet::NONE,
                runes: bota_proto::RuneSet::NONE,
                catalogue: bota_core::Catalogue::FULL.prefix(),
            },
        };
        let view = WorldView {
            tick: 900,
            viewer: None,
            units: (1..=heroes).map(stray_hero).collect(),
            players: vec![PlayerView {
                slot: SlotId(0),
                team: Team::Radiant,
                hero: HeroId::SHADOW_FIEND,
                unit: None,
                level: 1,
                xp: 0,
                gold: None,
                stash: None,
                kit: None,
                kills: 0,
                deaths: 0,
                assists: 0,
                last_hits: 0,
                denies: 0,
                respawn_left: 0,
                creep_score: None,
                seen_at: Some(900),
            }],
            projectiles: Vec::new(),
            loot: Vec::new(),
            aggro: Vec::new(),
            order_delays: Vec::new(),
            skipped_ticks: Vec::new(),
            stock: Vec::new(),
            talents: Vec::new(),
            teleports: Vec::new(),
            glyphs: Vec::new(),
            clock: None,
            felled_trees: Vec::new(),
            planted_trees: Vec::new(),
            runes: Vec::new(),
            pools: Vec::new(),
            lairs: Vec::new(),
        };
        [
            ReplayRecord::Header(ReplayHeader::new(bota_core::WIRE_FINGERPRINT)),
            ReplayRecord::Msg(start),
            ReplayRecord::Msg(ServerMsg::Snapshot { view }),
        ]
        .iter()
        .flat_map(|record| encode_frame_to_vec(record).expect("a record"))
        .collect()
    }

    pub(crate) fn owned_hero_scene() -> (bota_client_core::App, [EntityId; 2]) {
        use bota_client_core::{App as Core, Intent, ReplayPlayer, Source};
        use bota_proto::{ServerMsg, SlotId};

        let replay = replay_of_heroes_on_grid(2, 4);
        let player = ReplayPlayer::from_reader(std::io::Cursor::new(replay));
        let mut core = Core::new(Source::Replay(Box::new(player)));
        for _ in 0..100 {
            if core.lost || core.view().is_some() {
                break;
            }
            core.pump(0.1);
        }
        let mut view = core.view().expect("a replay scene").clone();
        let emitters = [view.units[0].id, view.units[1].id];
        view.tick += 1;
        view.units[0].owner = Some(SlotId(0));
        view.players[0].unit = Some(emitters[0]);
        view.units[1].pos.x = bota_proto::Fixed::from_int(110);
        core.handle(ServerMsg::Snapshot { view });
        let why = core.feed.iter().rev().find_map(|line| match &line.kind {
            bota_client_core::FeedKind::ProtocolBroken { why } => Some(why.as_str()),
            _ => None,
        });
        assert!(
            !core.lost,
            "the shared owned/stray hero fixture is valid: {why:?}"
        );
        core.act(Intent::NextEyes);
        assert_eq!(core.my_hero(), Some(emitters[0]));
        (core, emitters)
    }

    /// Each new unit's model is a world of its own cloned in, so new units are spawned a few a
    /// frame; heroes were let through all at once, as a match has a few. A replay or server
    /// showing thousands of heroes of no seat (new ones every snapshot) froze the client: a
    /// frame spawns no more heroes than a match has seats besides the few of any kind.
    #[test]
    fn a_view_of_thousands_of_heroes_spawns_a_frames_worth_at_a_time() {
        use bevy::ecs::system::RunSystemOnce;
        use bota_client_core::{App as Core, ReplayPlayer, Source};
        let player = ReplayPlayer::from_reader(std::io::Cursor::new(replay_of_heroes(4096)));
        let mut core = Core::new(Source::Replay(Box::new(player)));
        for _ in 0..100 {
            if core.lost || core.view().is_some() {
                break;
            }
            core.pump(0.1);
        }
        let shown = core.view().map_or(0, |v| v.units.len());
        let error = match &core.source {
            Source::Replay(player) => player.error().map(ToString::to_string),
            Source::Live(_) => None,
        };
        assert_eq!(shown, 4096, "the replay's view; its error: {error:?}");
        let mut app = App::new();
        app.insert_non_send(Session(core))
            .init_resource::<UnitIndex>()
            .init_resource::<JumpedOver>()
            .init_resource::<ModelBank>()
            .init_resource::<Manifest>()
            .init_resource::<BuildingYaws>()
            .init_resource::<Ground>()
            .init_resource::<FogGrid>()
            .init_resource::<Time>();

        app.world_mut().run_system_once(sync_units).unwrap();

        let spawned = app.world_mut().query::<&Unit>().iter(app.world()).count();
        assert_eq!(spawned, SPAWNS_PER_FRAME + bota_proto::limits::MAX_SEATS);
    }

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
