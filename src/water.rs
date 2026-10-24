//! The river's flow: Dota's water scrolls its normal map; here the fog's
//! shader slides the water material's normal map along the current by the
//! frame's time (`fog.wgsl`, `flowing`), no reflection or refraction pass and
//! no write to the material: a changed material has its whole bindless slab
//! of materials bound anew. As in Dota the normal map is read by world
//! position, with one tangent frame for the whole river: the map's water is
//! hundreds of pieces whose own texture coordinates differ in scale and
//! direction from piece to piece, which drew every piece's glint apart.

use bevy::prelude::*;

/// Marks a mesh of the river.
#[derive(Component)]
pub struct Water;
