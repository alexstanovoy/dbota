//! The river's flow: Dota's water scrolls its normal map; here the fog's
//! shader slides the water material's normal map along the current by the
//! frame's time (`fog.wgsl`, `flow`), no reflection or refraction pass and no
//! write to the material: a changed material has its whole bindless slab of
//! materials bound anew.

use bevy::prelude::*;

/// Marks a mesh of the river.
#[derive(Component)]
pub struct Water;
