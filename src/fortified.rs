//! What the Glyph of Fortification looks like: while it holds, a side's
//! structures wear a tube of blue rings rising around them (7 s) and its lane
//! creeps a smaller one (3 s), as in Dota's `glyph.vpcf` and
//! `glyph_creeps.vpcf`. The rings are Dota's own textures; the particle
//! systems themselves are not played, so the smoke spirals, the sparkles and
//! the vertical trails of the original are left out and the rings only rise,
//! fade and pulse. Without the textures the rings are plain tinted bands.
//!
//! A unit under a Glyph carries `StatusFlags::FORTIFIED`, whichever side's it
//! is, when the viewer is told of it at all: the enemy's Glyph shows on what
//! one sees, as in Dota.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use bota_core::{kind_def, rules};
use bota_proto::{EntityId, StatusFlags};

use crate::manifest::Manifest;
use crate::session::Session;
use crate::units::{UnitIndex, stand_in_size};

/// Rings rising in a structure's tube, and in a creep's.
const BUILDING_RINGS: usize = 4;
const CREEP_RINGS: usize = 3;
/// Seconds a ring takes to rise from the ground to the top of its tube.
const RISE: f32 = 2.0;
/// Seconds before the cover ends in which the rings fade out.
const FADE_OUT: f32 = 0.5;
/// Dota's tint of the structures' tube and of the creeps' (`m_ConstantColor`).
const BUILDING_TINT: Color = Color::srgb(0.47, 0.74, 0.95);
const CREEP_TINT: Color = Color::srgb(0.43, 0.74, 1.0);
/// How tall a creep's tube is, in world units.
const CREEP_TUBE: f32 = 150.0;

/// The textures of the rings, when the cache has them.
#[derive(Resource)]
struct Art {
    /// A structure's rising rings, a creep's rising rings, and the ring on
    /// the ground under either.
    tube: Option<Handle<Image>>,
    creep_tube: Option<Handle<Image>>,
    base: Option<Handle<Image>>,
}

/// A fortified unit's rings, a child of the unit's entity.
#[derive(Component)]
struct Wear {
    /// 0 to 1: the cover running out fades the rings away.
    fade: f32,
}

/// One ring of a [`Wear`].
#[derive(Component)]
struct Ring {
    wear: Entity,
    material: Handle<StandardMaterial>,
    /// Where in its rise the ring starts, 0 to 1; `None` for the ground ring.
    phase: Option<f32>,
    height: f32,
    radius: f32,
}

/// The wears by the unit they are on.
#[derive(Resource, Default)]
struct Worn(HashMap<EntityId, Entity>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Worn>()
        .add_systems(Startup, make_art)
        .add_systems(Update, (wear, animate).chain());
}

fn make_art(mut commands: Commands, manifest: Res<Manifest>, server: Res<AssetServer>) {
    let load = |name: &str| manifest.file(name).map(|path| server.load(path));
    commands.insert_resource(Art {
        tube: load("particle.glyph.tube"),
        creep_tube: load("particle.glyph.creep_tube"),
        base: load("particle.glyph.base"),
    });
}

/// The units under a Glyph, whichever side's, and the seconds a structure's
/// cover has left; a creep's own end is not told, so it keeps its rings while
/// it is flagged.
fn covered(view: &bota_proto::WorldView) -> Vec<(&bota_proto::UnitView, f32)> {
    let rate = rules::TICKS_PER_SECOND as f32;
    view.units
        .iter()
        .filter(|u| u.statuses.bits & StatusFlags::FORTIFIED != 0)
        .map(|u| {
            let left = view
                .glyphs
                .iter()
                .find(|g| g.team == u.team)
                .map_or(f32::MAX, |g| g.active_for as f32 / rate);
            (
                u,
                if kind_def(u.kind).structure {
                    left
                } else {
                    f32::MAX
                },
            )
        })
        .collect()
}

/// Puts rings on the units under a Glyph and takes them off the rest.
fn wear(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    index: Res<UnitIndex>,
    mut worn: ResMut<Worn>,
    mut wears: Query<&mut Wear>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    rings: Query<(&Ring, &ChildOf)>,
) {
    let (Some(view), Some(art)) = (session.0.view(), art) else {
        return;
    };
    let mut keep = Vec::new();
    for (unit, left) in covered(view) {
        let Some(root) = index.0.get(&unit.id).copied() else {
            continue;
        };
        keep.push(unit.id);
        let fade = (left / FADE_OUT).min(1.0);
        if let Some(entity) = worn.0.get(&unit.id)
            && let Ok(mut wear) = wears.get_mut(*entity)
        {
            wear.fade = fade;
            continue;
        }
        let entity = put_on(
            &mut commands,
            &art,
            &mut materials,
            &mut meshes,
            root,
            unit,
            fade,
        );
        worn.0.insert(unit.id, entity);
    }
    worn.0.retain(|id, entity| {
        // A wear spawned this frame is not in the query yet.
        if keep.contains(id) {
            return true;
        }
        for (ring, parent) in &rings {
            if parent.parent() == *entity {
                materials.remove(&ring.material);
            }
        }
        commands.entity(*entity).despawn();
        false
    });
}

/// A unit's rings: one on the ground and the ones that rise.
fn put_on(
    commands: &mut Commands,
    art: &Art,
    materials: &mut Assets<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
    root: Entity,
    unit: &bota_proto::UnitView,
    fade: f32,
) -> Entity {
    let structure = kind_def(unit.kind).structure;
    let bound = unit.bound.to_f32();
    let (tint, rising, height, radius, texture) = if structure {
        let (_, height) = stand_in_size(unit.kind, bound);
        (
            BUILDING_TINT,
            BUILDING_RINGS,
            height,
            bound * 1.2,
            &art.tube,
        )
    } else {
        (
            CREEP_TINT,
            CREEP_RINGS,
            CREEP_TUBE,
            (bound * 2.5).max(50.0),
            &art.creep_tube,
        )
    };
    let wear = commands
        .spawn((
            Wear { fade },
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    let flat = Quat::from_rotation_x(-FRAC_PI_2);
    let mut ring = |texture: &Option<Handle<Image>>, phase: Option<f32>| {
        // The plain band stands in for a texture the cache lacks.
        let mesh = match texture {
            Some(_) => meshes.add(Rectangle::new(2.0, 2.0)),
            None => meshes.add(Annulus::new(0.8, 1.0)),
        };
        let material = materials.add(StandardMaterial {
            base_color: tint.with_alpha(0.0),
            base_color_texture: texture.clone(),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        commands.spawn((
            Ring {
                wear,
                material: material.clone(),
                phase,
                height,
                radius,
            },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_rotation(flat),
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(wear),
        ));
    };
    ring(&art.base, None);
    for i in 0..rising {
        ring(texture, Some(i as f32 / rising as f32));
    }
    wear
}

/// Raises the rings along their tubes and lets them swell and fade.
fn animate(
    time: Res<Time>,
    wears: Query<&Wear>,
    mut rings: Query<(&Ring, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    for (ring, mut transform) in &mut rings {
        let Ok(wear) = wears.get(ring.wear) else {
            continue;
        };
        let (rise, alpha, size) = match ring.phase {
            Some(phase) => {
                let p = (t / RISE + phase).fract();
                // Born small at the foot, widest at the top, gone at the end.
                (p, (p * std::f32::consts::PI).sin() * 0.9, 0.85 + 0.3 * p)
            }
            None => (0.0, 0.55 + 0.15 * (t * 3.0).sin(), 1.0),
        };
        let r = ring.radius * size;
        transform.translation = Vec3::new(0.0, 6.0 + rise * ring.height, 0.0);
        transform.scale = Vec3::new(r, r, 1.0);
        if let Some(mut material) = materials.get_mut(&ring.material) {
            material.base_color.set_alpha(alpha * wear.fade);
        }
    }
}
