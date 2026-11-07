//! Soft dark discs under mobile units keep their silhouettes distinct from
//! the ground, independent of scenery colors and the sun's shadows.

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use bota_core::kind_def;

use crate::units::{Unit, unit_size};

/// The disc's darkness at its middle at full strength.
const DISC_ALPHA: f32 = 0.6;
/// The disc's radius against the unit's body.
const DISC_SPREAD: f32 = 1.6;
/// World units the disc floats over the ground at the unit's feet.
const DISC_LIFT: f32 = 15.0;
/// Corners round the disc's rings.
const DISC_SIDES: usize = 24;
/// The disc's rings out from its middle: radius against the rim, and the
/// darkness there, a smooth fall to nothing at the rim.
const DISC_RINGS: [(f32, f32); 4] = [(0.0, 1.0), (0.45, 0.66), (0.75, 0.25), (1.0, 0.0)];

/// The mesh and material every disc shares.
#[derive(Resource)]
struct Disc {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, make_disc)
        .add_systems(Update, lay_discs);
}

fn make_disc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::BLACK.with_alpha(DISC_ALPHA),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.insert_resource(Disc {
        mesh: meshes.add(disc_mesh()),
        material,
    });
}

/// A flat disc of unit diameter facing up, its darkness in its vertex
/// colours' alpha: rings of [`DISC_SIDES`] corners round one middle vertex.
fn disc_mesh() -> Mesh {
    let mut positions = vec![[0.0f32; 3]];
    let mut colors = vec![[1.0, 1.0, 1.0, DISC_RINGS[0].1]];
    for (radius, alpha) in &DISC_RINGS[1..] {
        for side in 0..DISC_SIDES {
            let a = side as f32 / DISC_SIDES as f32 * TAU;
            positions.push([0.5 * radius * a.cos(), 0.0, 0.5 * radius * a.sin()]);
            colors.push([1.0, 1.0, 1.0, *alpha]);
        }
    }
    let n = DISC_SIDES as u32;
    let mut indices = Vec::new();
    for side in 0..n {
        let next = (side + 1) % n;
        indices.extend_from_slice(&[0, 1 + next, 1 + side]);
    }
    for ring in 0..DISC_RINGS.len() as u32 - 2 {
        let (inner, outer) = (1 + ring * n, 1 + (ring + 1) * n);
        for side in 0..n {
            let next = (side + 1) % n;
            indices.extend_from_slice(&[inner + side, inner + next, outer + side]);
            indices.extend_from_slice(&[inner + next, outer + next, outer + side]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Lays a disc under every new unit but the buildings.
fn lay_discs(mut commands: Commands, disc: Res<Disc>, units: Query<(Entity, &Unit), Added<Unit>>) {
    for (entity, unit) in &units {
        if kind_def(unit.kind).structure {
            continue;
        }
        let size = 2.0 * DISC_SPREAD * unit_size(unit.kind, 0.0).0;
        commands.spawn((
            Mesh3d(disc.mesh.clone()),
            MeshMaterial3d(disc.material.clone()),
            Transform::from_xyz(0.0, DISC_LIFT, 0.0).with_scale(Vec3::new(size, 1.0, size)),
            Visibility::Inherited,
            NotShadowCaster,
            NotShadowReceiver,
            // Left out of the minimap's photograph, as the other ground aids.
            RenderLayers::layer(1),
            ChildOf(entity),
        ));
    }
}
