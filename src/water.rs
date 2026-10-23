//! The river's flow: Dota's water scrolls its normal map; here the one water
//! material's normal map is slid along the current, a single uniform write a
//! frame, no reflection or refraction pass.

use bevy::{math::Affine2, prelude::*};

use crate::fog::FoggedMaterial;

/// Marks a mesh of the river.
#[derive(Component)]
pub struct Water;

/// Normal-map repeats per unit of the river's texture coordinates: the
/// glint's size on the water.
const TILING: f32 = 6.0;
/// Texture coordinates the current slides a second.
const FLOW: Vec2 = Vec2::new(0.02, 0.03);
/// How far, in texture coordinates, and how fast the glint sways across the
/// current, so the flow does not read as a belt.
const SWAY: f32 = 0.005;
const SWAY_RATE: f32 = 0.5;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, flow);
}

fn flow(
    time: Res<Time>,
    water: Query<&MeshMaterial3d<FoggedMaterial>, With<Water>>,
    mut materials: ResMut<Assets<FoggedMaterial>>,
) {
    // Every water mesh shares one material (the fog's copy of it).
    let Some(handle) = water.iter().next() else {
        return;
    };
    let t = time.elapsed_secs();
    let sway = Vec2::new((t * SWAY_RATE).sin(), (t * SWAY_RATE * 0.7 + 1.3).cos()) * SWAY;
    if let Some(mut m) = materials.get_mut(handle.id()) {
        m.base.uv_transform = Affine2::from_scale_angle_translation(
            Vec2::splat(TILING),
            0.0,
            (FLOW * t + sway).rem_euclid(Vec2::ONE),
        );
    }
}
