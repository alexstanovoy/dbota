//! Terrain for the small sandbox, reconstructed from its transmitted grid.

use super::*;

const ELEVATION: f32 = 96.0;
const WALKABLE: u8 = 0x80;
const WATER: u8 = 0x40;
const TIER: u8 = 0x3f;

pub(super) fn ground(statics: &bota_client_core::MatchStatic) -> Ground {
    let n = statics.metadata().terrain_cells as usize;
    let heights = statics
        .terrain()
        .iter()
        .map(|tile| f32::from(tile & TIER) * ELEVATION)
        .collect();
    Ground {
        n,
        heights,
        surface: None,
    }
}

pub(super) fn mesh(statics: &bota_client_core::MatchStatic, ground: &Ground) -> Mesh {
    let terrain = statics.terrain();
    let n = ground.n;
    let stride = n + 1;
    let mut positions = Vec::with_capacity(stride * stride);
    let mut uv = Vec::with_capacity(stride * stride);
    let mut colors = Vec::with_capacity(stride * stride);
    let mut indices = Vec::new();
    let lane = bota_core::lane_polyline(
        bota_core::map_of(statics.metadata().map),
        bota_core::rules::LANE_MID,
    );
    let lane: Vec<_> = lane
        .iter()
        .map(|point| Vec2::new(point.x.to_f32(), point.y.to_f32()))
        .collect();
    for row in 0..=n {
        for column in 0..=n {
            let point = Vec2::new(column as f32, row as f32) * CELL;
            positions.push([point.x, ground.at(point.x, point.y), -point.y]);
            uv.push([point.x / CELL, point.y / CELL]);
            let tile = terrain[row.min(n - 1) * n + column.min(n - 1)];
            colors.push(color(tile, point, &lane));
        }
    }
    for (index, &tile) in terrain.iter().enumerate() {
        if tile == 0 {
            continue;
        }
        let corner = (index / n * stride + index % n) as u32;
        let north = corner + stride as u32;
        indices.extend_from_slice(&[corner, corner + 1, north + 1, corner, north + 1, north]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_normals();
    mesh
}

fn color(tile: u8, point: Vec2, lane: &[Vec2]) -> [f32; 4] {
    let grass = Color::srgb_u8(73, 92, 54);
    let road = Color::srgb_u8(132, 112, 83);
    let rock = Color::srgb_u8(58, 60, 56);
    let water = Color::srgb_u8(45, 83, 84);
    let distance = lane
        .windows(2)
        .map(|segment| {
            let from = segment[0];
            let along = segment[1] - from;
            let t = ((point - from).dot(along) / along.length_squared().max(1.0)).clamp(0.0, 1.0);
            point.distance(from + along * t)
        })
        .fold(f32::INFINITY, f32::min);
    let color = if tile & WATER != 0 {
        water
    } else if tile & WALKABLE == 0 {
        rock
    } else {
        road.mix(&grass, ((distance - 120.0) / 130.0).clamp(0.0, 1.0))
    };
    color.to_linear().to_f32_array()
}
