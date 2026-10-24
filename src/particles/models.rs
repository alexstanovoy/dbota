//! Particles drawn as models (`C_OP_RenderModels`): each model the effects draw has a pool of
//! its scene spawned once, hidden, with the model's own materials (shared by every instance);
//! each frame the particles that draw it take the pool's entities in turn, and only their
//! transforms and visibility change. A model whose pool is spent draws no more that frame.

use bevy::camera::visibility::VisibilitySystems;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::transform::systems::{
    mark_dirty_trees, propagate_parent_transforms, sync_simple_transforms,
};
use bevy::world_serialization::WorldAssetRoot;
use heatel::particle::{Models, Renderer};

use super::eval::{Scene, draw as random, float, vector};
use super::library::{Library, ModelDraw};
use super::plugin::{Playing, tick};
use super::sim::{Instance, Node};
use super::store;
use crate::map::UNITS_PER_METRE;
use crate::warmup::Warmup;

/// Entities in each model's pool: the particles drawn as one model at once.
const POOL: usize = 32;
/// Most models placed in a frame, all models together.
const MAX_PLACED: usize = 1024;

/// A particle drawn as a model this frame: the library's model and where it stands (Bevy's
/// frame, the model's scene at the origin unrotated).
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub model: u16,
    pub transform: Transform,
}

/// Each model's pool, once its file has loaded.
#[derive(Resource, Default)]
struct Pool(Vec<Option<Vec<Entity>>>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Pool>()
        .add_systems(Update, spawn)
        .add_systems(
            PostUpdate,
            (
                place,
                // The pools' scenes moved after the frame's transforms were propagated: their
                // trees are propagated again, so they stand where their particles are this
                // frame, not the next.
                (
                    mark_dirty_trees,
                    propagate_parent_transforms,
                    sync_simple_transforms,
                )
                    .chain(),
            )
                .chain()
                .after(tick)
                .before(VisibilitySystems::VisibilityPropagate)
                .before(VisibilitySystems::CheckVisibility),
        );
}

/// Spawns the pool of each model whose file has loaded: its scene, hidden, many times, and once
/// more as a warm-up piece, so its pipelines compile while the game loads.
fn spawn(
    mut commands: Commands,
    library: Option<Res<Library>>,
    gltfs: Res<Assets<Gltf>>,
    mut pool: ResMut<Pool>,
) {
    let Some(library) = library else {
        return;
    };
    pool.0.resize_with(library.models.len(), || None);
    for (model, entry) in library.models.iter().zip(&mut pool.0) {
        if entry.is_some() {
            continue;
        }
        let Some(scene) = gltfs.get(&model.gltf).and_then(|g| g.scenes.first()) else {
            continue;
        };
        commands.spawn((WorldAssetRoot(scene.clone()), Warmup));
        let entities = (0..POOL)
            .map(|_| {
                commands
                    .spawn((
                        WorldAssetRoot(scene.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                    ))
                    .id()
            })
            .collect();
        *entry = Some(entities);
    }
}

/// Gives each pool the frame's placements of its model; the rest of the pool hides.
fn place(
    playing: Res<Playing>,
    pool: Res<Pool>,
    mut roots: Query<(&mut Transform, &mut Visibility), With<WorldAssetRoot>>,
) {
    for (model, entities) in pool.0.iter().enumerate() {
        let Some(entities) = entities else {
            continue;
        };
        let mut placed = playing
            .placed
            .iter()
            .filter(|p| usize::from(p.model) == model);
        for &entity in entities {
            let Ok((mut transform, mut visibility)) = roots.get_mut(entity) else {
                continue;
            };
            match placed.next() {
                Some(p) => {
                    *transform = p.transform;
                    visibility.set_if_neq(Visibility::Inherited);
                }
                None => {
                    visibility.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
}

/// Appends the models every drawn model renderer of `instance` places this frame.
pub fn gather(instance: &Instance, out: &mut Vec<Placed>) {
    for node in &instance.nodes {
        if node.parts.is_empty() {
            continue;
        }
        let def = &instance.effect.defs[node.def];
        for d in &def.model_draws {
            let renderer = &def.system.renderers[d.renderer];
            let Renderer::Models(models) = &renderer.class else {
                continue;
            };
            if node.strength_now(&renderer.timing) > 0.0 {
                place_particles(models, d, node, out);
            }
        }
    }
}

/// The models of one renderer's particles: each particle's model picked once for its life
/// (by the manual selection, or by chance), placed, turned and scaled by it.
fn place_particles(models: &Models, d: &ModelDraw, node: &Node, out: &mut Vec<Placed>) {
    let (scene, parts) = (node.scene_now(), &node.parts);
    let total: f32 = d
        .models
        .iter()
        .filter(|(m, w)| m.is_some() && *w > 0.0)
        .map(|(_, w)| w)
        .sum();
    let radius_scale = float(&models.radius_scale, &scene, parts, None, 1);
    let roll_scale = float(&models.roll_scale, &scene, parts, None, 2);
    for i in 0..parts.len() {
        if out.len() == MAX_PLACED {
            return;
        }
        let manual = float(&models.manual_selection, &scene, parts, Some(i), 3);
        let picked = if manual >= 0.0 && (manual as usize) < d.models.len() {
            d.models[manual as usize].0
        } else {
            pick(&d.models, total, random(parts.seed[i], 0x30DE))
        };
        let Some(model) = picked else {
            continue;
        };
        let matrix = source_matrix(models, &scene, node, i, (radius_scale, roll_scale));
        out.push(Placed {
            model,
            transform: Transform::from_matrix(to_bevy(matrix)),
        });
    }
}

/// The model a draw of 0..1 picks by the models' chances.
fn pick(models: &[(Option<u16>, f32)], total: f32, draw: f32) -> Option<u16> {
    if total <= 0.0 {
        return None;
    }
    let mut left = draw * total;
    let mut picked = None;
    for (model, weight) in models {
        if model.is_none() || *weight <= 0.0 {
            continue;
        }
        picked = *model;
        left -= weight;
        if left <= 0.0 {
            break;
        }
    }
    picked
}

/// Where particle `i` puts its model, in Source's frame: offset in the model's frame, turned by
/// the local rotation and then the particle's yaw, pitch and roll, scaled (before the turn when
/// local), laid along the particle's normal (the model's X, or its Z), at the particle.
fn source_matrix(
    models: &Models,
    scene: &Scene<'_>,
    node: &Node,
    i: usize,
    (radius_scale, roll_scale): (f32, f32),
) -> Mat4 {
    let parts = &node.parts;
    let mut scale = vector(&models.component_scale, scene, parts, Some(i), 4);
    if let Some(cp) = models.scale_control_point {
        scale *= scene.control_points[cp.index()].position;
    }
    if !models.ignore_radius {
        scale *= (parts.scalars[store::RADIUS][i] * radius_scale).max(f32::EPSILON);
    }
    let degrees = vector(&models.local_rotation, scene, parts, Some(i), 7);
    let turn = angles(
        parts.scalars[store::YAW][i],
        parts.scalar(store::slot(heatel::particle::Field::PITCH), i),
        parts.scalars[store::ROLL][i] * roll_scale,
    ) * angles(
        degrees.y.to_radians(),
        degrees.x.to_radians(),
        degrees.z.to_radians(),
    );
    let turn = Mat4::from_quat(turn);
    let scaling = Mat4::from_scale(scale);
    let basis = if models.ignore_normal {
        Mat4::IDENTITY
    } else {
        normal_basis(parts.vectors[store::NORMAL][i], models.orient_z)
    };
    let model = if models.local_scale {
        basis * turn * scaling
    } else {
        basis * scaling * turn
    };
    let offset = vector(&models.local_offset, scene, parts, Some(i), 10);
    Mat4::from_translation(parts.vectors[store::POSITION][i])
        * model
        * Mat4::from_translation(offset)
}

/// A turn of yaw, pitch and roll in radians, as Source turns a QAngle.
fn angles(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll)
}

/// The frame that lays a model's X axis (or its Z, `orient_z`) along `normal`; none for a
/// normal of no length.
fn normal_basis(normal: Vec3, orient_z: bool) -> Mat4 {
    let length = normal.length();
    if length == 0.0 || !length.is_finite() {
        return Mat4::IDENTITY;
    }
    let d = normal / length;
    let side = Vec3::new((1.0 - d.z) * d.y * d.y + d.z, 0.0, -d.x).normalize_or_zero();
    let side = (side - d * side.dot(d)).normalize_or_zero();
    let third = d.cross(side);
    let (x, y, z) = if orient_z {
        (third, -side, d)
    } else {
        (d, side, third)
    };
    Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), Vec4::W)
}

/// A placement in Source's frame as the transform of a model's scene in Bevy's: the world's
/// axes to Bevy's (`draw::to_world`), and the scene's own, which holds the model in metres
/// with its front (Source's X) along Bevy's -Z and its up along Y.
fn to_bevy(source: Mat4) -> Mat4 {
    let world = Mat4::from_cols(Vec4::X, Vec4::NEG_Z, Vec4::Y, Vec4::W);
    let scene = Mat4::from_cols(Vec4::NEG_Y, Vec4::Z, Vec4::NEG_X, Vec4::W);
    world * source * scene * Mat4::from_scale(Vec3::splat(UNITS_PER_METRE))
}
