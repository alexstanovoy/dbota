//! The ground as a mesh: the terrain cells of the match raised to their
//! tiers, with smooth slopes between tiers and the river sunk below them.

/// Terrain cells per axis of the map.
pub const MAP_CELLS: usize = bota_core::rules::GRID_CELLS;

/// World units of one terrain cell.
pub const CELL: f32 = bota_core::rules::GRID_CELL_SIZE as f32;
/// World units one elevation tier stands above the one below.
pub const TIER_HEIGHT: f32 = 128.0;
/// How far water lies below the ground of tier zero.
pub const WATER_DEPTH: f32 = 48.0;

/// What one terrain byte says of its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    /// Walkable ground of an elevation tier.
    Land(u8),
    /// River water.
    Water,
    /// Rock nothing walks on.
    Rock,
}

/// Reads one terrain byte: bit 7 walkable, bit 6 water, the low bits the
/// tier.
pub fn ground_of(byte: u8) -> Ground {
    if byte & 0x80 == 0 {
        Ground::Rock
    } else if byte & 0x40 != 0 {
        Ground::Water
    } else {
        Ground::Land(byte & 0x1f)
    }
}

/// The surface height of a cell, world units.
pub fn cell_height(cells: &[u8], n: usize, cx: usize, cy: usize) -> f32 {
    match ground_of(cells[cy * n + cx]) {
        Ground::Land(tier) => f32::from(tier) * TIER_HEIGHT,
        Ground::Water => -WATER_DEPTH,
        // Rock stands a little proud of the highest ground beside it.
        Ground::Rock => {
            let mut top = 0.0f32;
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x >= 0
                    && y >= 0
                    && (x as usize) < n
                    && (y as usize) < n
                    && let Ground::Land(t) = ground_of(cells[y as usize * n + x as usize])
                {
                    top = top.max(f32::from(t) * TIER_HEIGHT);
                }
            }
            top + TIER_HEIGHT * 0.6
        }
    }
}

/// The ground mesh in world space: X east, Y north, Z up.
pub struct GroundMesh {
    /// Corner positions.
    pub positions: Vec<[f32; 3]>,
    /// Corner normals.
    pub normals: Vec<[f32; 3]>,
    /// Corner colours, linear RGBA.
    pub colors: Vec<[f32; 4]>,
    /// Texture coordinates, one unit per `uv_span` world units.
    pub uvs: Vec<[f32; 2]>,
    /// Triangles, counter-clockwise seen from above.
    pub indices: Vec<u32>,
}

/// The height at every cell corner: the mean of the cells around it.
fn corner_heights(cells: &[u8], n: usize) -> Vec<f32> {
    let heights: Vec<f32> = (0..n * n)
        .map(|i| cell_height(cells, n, i % n, i / n))
        .collect();
    let m = n + 1;
    let mut corners = vec![0.0f32; m * m];
    for cy in 0..m {
        for cx in 0..m {
            let mut sum = 0.0;
            let mut count = 0.0;
            for (dx, dy) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
                if cx >= dx && cy >= dy && cx - dx < n && cy - dy < n {
                    sum += heights[(cy - dy) * n + (cx - dx)];
                    count += 1.0;
                }
            }
            corners[cy * m + cx] = sum / count;
        }
    }
    corners
}

/// The colour of a corner from the cells around it and the side of the map
/// it lies on, 0 at the Radiant corner and 1 at the Dire one.
fn corner_color(cells: &[u8], n: usize, cx: usize, cy: usize) -> [f32; 4] {
    let side = (cx + cy) as f32 / (2 * n) as f32;
    let (mut land, mut water, mut rock) = (0.0f32, 0.0f32, 0.0f32);
    for (dx, dy) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
        if cx >= dx && cy >= dy && cx - dx < n && cy - dy < n {
            match ground_of(cells[(cy - dy) * n + (cx - dx)]) {
                Ground::Land(_) => land += 1.0,
                Ground::Water => water += 1.0,
                Ground::Rock => rock += 1.0,
            }
        }
    }
    let total = (land + water + rock).max(1.0);
    let grass = lerp3([0.62, 0.78, 0.48], [0.66, 0.58, 0.50], side);
    let wet = [0.30, 0.46, 0.52];
    let stone = [0.42, 0.40, 0.38];
    let mut c = [0.0; 4];
    for i in 0..3 {
        c[i] = (grass[i] * land + wet[i] * water + stone[i] * rock) / total;
    }
    c[3] = 1.0;
    c
}

fn lerp3(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
    ]
}

/// Builds the ground mesh of `n` by `n` cells, row-major from the
/// south-west, one texture repeat every `uv_span` world units.
pub fn build(cells: &[u8], n: usize, uv_span: f32) -> GroundMesh {
    assert_eq!(cells.len(), n * n, "one byte a cell");
    assert!(n >= 2);
    let m = n + 1;
    let heights = corner_heights(cells, n);
    let h = |x: usize, y: usize| heights[y.min(n) * m + x.min(n)];
    let mut mesh = GroundMesh {
        positions: Vec::with_capacity(m * m),
        normals: Vec::with_capacity(m * m),
        colors: Vec::with_capacity(m * m),
        uvs: Vec::with_capacity(m * m),
        indices: Vec::with_capacity(n * n * 6),
    };
    for cy in 0..m {
        for cx in 0..m {
            let (x, y) = (cx as f32 * CELL, cy as f32 * CELL);
            mesh.positions.push([x, y, h(cx, cy)]);
            let dzdx = (h(cx + 1, cy) - h(cx.saturating_sub(1), cy)) / (2.0 * CELL);
            let dzdy = (h(cx, cy + 1) - h(cx, cy.saturating_sub(1))) / (2.0 * CELL);
            let len = (dzdx * dzdx + dzdy * dzdy + 1.0).sqrt();
            mesh.normals.push([-dzdx / len, -dzdy / len, 1.0 / len]);
            mesh.colors.push(corner_color(cells, n, cx, cy));
            mesh.uvs.push([x / uv_span, y / uv_span]);
        }
    }
    for cy in 0..n {
        for cx in 0..n {
            let a = (cy * m + cx) as u32;
            let b = a + 1;
            let c = a + m as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    mesh
}

/// The ground height anywhere on the map, sampled between cell corners.
#[derive(Clone, Debug, Default)]
pub struct Heights {
    n: usize,
    corners: Vec<f32>,
}

impl Heights {
    /// The heights of `n` by `n` cells.
    pub fn new(cells: &[u8], n: usize) -> Heights {
        if n < 2 || cells.len() != n * n {
            return Heights::default();
        }
        Heights {
            n,
            corners: corner_heights(cells, n),
        }
    }

    /// The ground height at world point `(x, y)`; zero off the map or before
    /// the terrain is known.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if self.n == 0 {
            return 0.0;
        }
        let m = self.n + 1;
        let fx = (x / CELL).clamp(0.0, self.n as f32);
        let fy = (y / CELL).clamp(0.0, self.n as f32);
        let (x0, y0) = ((fx as usize).min(self.n - 1), (fy as usize).min(self.n - 1));
        let (kx, ky) = (fx - x0 as f32, fy - y0 as f32);
        let h = |x: usize, y: usize| self.corners[y * m + x];
        let south = h(x0, y0) + (h(x0 + 1, y0) - h(x0, y0)) * kx;
        let north = h(x0, y0 + 1) + (h(x0 + 1, y0 + 1) - h(x0, y0 + 1)) * kx;
        south + (north - south) * ky
    }
}
