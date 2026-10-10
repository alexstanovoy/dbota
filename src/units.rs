//! Units on the map: one entity per unit the client sees, carrying Dota's
//! model of it or a stand-in, kept where the frame puts it, and kept a
//! moment longer to fall when it dies.

use std::collections::HashMap;

use bevy::{
    gltf::Gltf, light::NotShadowCaster, prelude::*, world_serialization::WorldAsset,
    world_serialization::WorldInstanceReady,
};
use bota_client_core::bodies;
use bota_proto::{EntityId, Team, UnitKind, UnitView};

use crate::coords::sim_facing_to_rotation;
use crate::fog::FogGrid;
use crate::manifest::{Manifest, Model};
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
struct Wearable {
    wearer: Entity,
    ready: bool,
}

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
        .init_resource::<ModelBank>()
        .add_systems(Startup, (load_models, make_stand_ins))
        .add_systems(
            Update,
            (build_graphs, sync_units, fall, bind_wearables).chain(),
        )
        .add_systems(PostUpdate, lift_heroes);
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
    for model in manifest.models.iter().filter(|m| m.attach.is_none()) {
        if model.logical.starts_with("tree.") {
            continue;
        }
        let wearables = manifest
            .models
            .iter()
            .filter(|w| w.attach.as_deref() == Some(model.logical.as_str()))
            .map(|w| server.load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(w.path.clone())))
            .collect();
        bank.0.push(BankModel {
            model: model.clone(),
            gltf: server.load(model.path.clone()),
            scene: server.load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(model.path.clone())),
            wearables,
            clips: Vec::new(),
            graph: None,
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
fn model_name(unit: &UnitView) -> Option<String> {
    let side = match unit.team {
        Team::Radiant => "radiant",
        Team::Dire => "dire",
        Team::Neutral => return None,
    };
    Some(match unit.kind {
        UnitKind::Hero => match unit.hero.map(|h| h.0) {
            Some(2) => "hero.nevermore.model".to_string(),
            _ => return None,
        },
        UnitKind::CreepMelee | UnitKind::CreepSiege => format!("creep.{side}.melee.model"),
        UnitKind::CreepFlagbearer => format!("creep.{side}.flagbearer.model"),
        UnitKind::CreepRanged => format!("creep.{side}.ranged.model"),
        UnitKind::Tower => format!("tower.{side}.model"),
        UnitKind::Ancient => format!("ancient.{side}.model"),
        UnitKind::Barracks => format!("barracks.{side}.melee.model"),
        UnitKind::Fountain => format!("fountain.{side}.model"),
        UnitKind::Courier => format!("courier.{side}.model"),
        _ => return None,
    })
}

/// The radius and height of a unit's stand-in, in world units.
fn stand_in_size(kind: UnitKind, bound: f32) -> (f32, f32) {
    match kind {
        UnitKind::Hero => (40.0, 180.0),
        UnitKind::Tower => (bound * 0.7, 420.0),
        UnitKind::Barracks => (bound * 0.8, 220.0),
        UnitKind::Ancient => (bound * 0.7, 480.0),
        UnitKind::Fountain => (bound * 0.8, 120.0),
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
    let Some(view) = &core.view else {
        return;
    };
    let shown = bodies(view, core.prev_view.as_ref(), core.smooth.alpha());
    let hidden = view.viewer.is_none() && p.fog.team.is_some();
    let mut seen = Vec::with_capacity(shown.len());
    let dt = p.time.delta_secs().max(1e-4);
    for body in &shown {
        let u = body.unit;
        if hidden && Some(u.team) != p.fog.team && !p.fog.lit(body.x, body.y) {
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
                unit.facing = body.facing;
                transform.translation = place;
            }
            None => {
                let entity = spawn_unit(&mut p, u, at, body.facing, place);
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
        let died = core.prev_view.as_ref().is_some_and(|prev| {
            prev.units.iter().any(|u| {
                u.id == id
                    && u.kind != UnitKind::Courier
                    && (p.fog.team.is_none() || p.fog.lit(u.pos.x.to_f32(), u.pos.y.to_f32()))
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
    let model =
        model_name(u).and_then(|name| p.bank.0.iter().position(|m| m.model.logical == name));
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
        UnitKind::Tower | UnitKind::Barracks | UnitKind::Ancient | UnitKind::Fountain
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
    // Buildings stand in the fog too, and are drawn dulled there.
    if units.get(unit).is_ok_and(|u| {
        matches!(
            u.kind,
            UnitKind::Tower | UnitKind::Barracks | UnitKind::Ancient | UnitKind::Fountain
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
