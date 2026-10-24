//! The ground: Dota's own map, the band around the mid lane, from the cache, its terrain layers
//! baked into vertex colours, or a terrain built from the match's cells when
//! the cache has no map. Also the sun, and the height of the ground anywhere.

use std::f32::consts::FRAC_PI_2;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues, VertexFormat},
    prelude::*,
};

use crate::layers::Family;
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::terrain::{self, CELL};

/// How bright the baked terrain is against the lighting: Dota doubles its
/// terrain's light, the rest of the scene does not.
pub(crate) const TERRAIN_GAIN: f32 = 1.6;

/// Source 2 inches to the metres the exporter writes, inverted.
pub const UNITS_PER_METRE: f32 = 1.0 / 0.0254;
/// Where Dota's origin, the middle of the map, lies in bota's coordinates,
/// on both axes.
pub const DOTA_ORIGIN: f32 = bota_core::rules::MAP_SIZE as f32 / 2.0;

/// The terrain blend weights of a Dota terrain vertex.
pub const ATTRIBUTE_BLEND_WEIGHTS: MeshVertexAttribute =
    MeshVertexAttribute::new("DotaBlendWeights", 0x0d07_a001, VertexFormat::Float32x4);
/// The terrain blend softness of a Dota terrain vertex.
pub const ATTRIBUTE_BLEND_ALPHAS: MeshVertexAttribute =
    MeshVertexAttribute::new("DotaBlendAlphas", 0x0d07_a002, VertexFormat::Float32x4);
/// The painted tint of a Dota terrain vertex.
pub const ATTRIBUTE_PAINTED_TINT: MeshVertexAttribute =
    MeshVertexAttribute::new("DotaPaintedTint", 0x0d07_a003, VertexFormat::Float32x4);

/// The ground height anywhere, once the ground is known.
#[derive(Resource, Clone, Debug, Default)]
pub struct Ground {
    /// Cells per axis; zero before the ground is known.
    n: usize,
    /// Height of every cell, row-major from the south-west.
    heights: Vec<f32>,
    /// The terrain's own triangles, for what must lie on the drawn ground exactly.
    surface: Option<std::sync::Arc<TerrainSurface>>,
}

/// The terrain's triangles by the cells they cover: a height read from them is the drawn
/// ground's, where the cell means of [`Ground::at`] may sit under it on a bump.
#[derive(Debug, Default)]
struct TerrainSurface {
    /// Vertices, bota x and y and height.
    points: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    /// Per cell (as [`Ground`]'s), the triangles over it.
    cells: Vec<Vec<u32>>,
}

impl Ground {
    /// The ground height at a bota world point.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if self.n == 0 {
            return 0.0;
        }
        let fx = (x / CELL - 0.5).clamp(0.0, (self.n - 1) as f32);
        let fy = (y / CELL - 0.5).clamp(0.0, (self.n - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(self.n - 1), (y0 + 1).min(self.n - 1));
        let (kx, ky) = (fx - x0 as f32, fy - y0 as f32);
        let h = |x: usize, y: usize| self.heights[y * self.n + x];
        let south = h(x0, y0) + (h(x1, y0) - h(x0, y0)) * kx;
        let north = h(x0, y1) + (h(x1, y1) - h(x0, y1)) * kx;
        south + (north - south) * ky
    }

    /// The height of the drawn terrain at a bota world point (its highest triangle there):
    /// where Source's traces would find it, for what is laid on the ground; [`Self::at`]
    /// where the terrain's triangles are not known.
    pub fn surface_at(&self, x: f32, y: f32) -> f32 {
        let found = self.surface.as_ref().and_then(|surface| {
            let (cx, cy) = ((x / CELL).floor(), (y / CELL).floor());
            if cx < 0.0 || cy < 0.0 || cx >= self.n as f32 || cy >= self.n as f32 {
                return None;
            }
            let cell = &surface.cells[cy as usize * self.n + cx as usize];
            cell.iter()
                .filter_map(|&t| height_in(&surface.points, surface.triangles[t as usize], x, y))
                .reduce(f32::max)
        });
        found.unwrap_or_else(|| self.at(x, y))
    }

    /// Whether the ground is known.
    pub fn known(&self) -> bool {
        self.n > 0
    }
}

/// The height of triangle `t` at `(x, y)`, if the point is over it.
fn height_in(points: &[[f32; 3]], t: [u32; 3], x: f32, y: f32) -> Option<f32> {
    let [a, b, c] = t.map(|i| points[i as usize]);
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < 1e-6 {
        return None;
    }
    let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
    let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
    let w = 1.0 - u - v;
    const EDGE: f32 = -1e-4;
    (u >= EDGE && v >= EDGE && w >= EDGE).then(|| u * a[2] + v * b[2] + w * c[2])
}

/// The material the terrain and the floor under it are drawn with.
#[derive(Resource)]
pub struct TerrainMaterial(pub AssetId<StandardMaterial>);

/// The map while it loads, read and merged by `mapfile`; let go once spawned.
#[derive(Resource)]
struct MapFile(Handle<crate::mapfile::MapGeometry>);

pub fn plugin(app: &mut App) {
    let stream = crate::mapfile::MapStream::default();
    let loader = map_loader(app.world(), stream.clone());
    app.insert_resource(stream)
        .init_asset::<crate::mapfile::MapGeometry>()
        .register_asset_loader(loader)
        .init_resource::<Ground>()
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.72, 0.80, 1.0),
            brightness: 330.0,
            ..default()
        })
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.04)))
        .add_systems(Startup, (spawn_sun, spawn_map))
        .add_systems(Update, (prepare_map, build_fallback_ground));
}

/// The map's loader, as the manifest, the arguments and the settings set it.
fn map_loader(world: &World, stream: crate::mapfile::MapStream) -> crate::mapfile::MapLoader {
    let manifest = world.resource::<Manifest>();
    let settings = world.resource::<crate::config::Settings>();
    crate::mapfile::MapLoader {
        root: manifest.root.clone(),
        detail: manifest.path("ground.grass").map(str::to_string),
        props: settings.quality.props,
        stream,
    }
}

fn spawn_sun(mut commands: Commands, settings: Res<crate::config::Settings>) {
    let shadows = settings.quality.shadows;
    commands.spawn((
        DirectionalLight {
            illuminance: 13_000.0,
            // Slightly warm.
            color: Color::srgb(1.0, 0.97 - 0.09 * 0.3, 0.92 - 0.22 * 0.3),
            shadow_maps_enabled: shadows > 0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
        crate::quality::cascades(shadows),
    ));
}

/// The transform that puts the exported map, glTF metres with `x` along
/// Dota's `y`, onto bota's coordinates.
pub(crate) fn map_transform() -> Transform {
    Transform::from_translation(Vec3::new(DOTA_ORIGIN, 0.0, -DOTA_ORIGIN))
        .with_rotation(Quat::from_rotation_y(-FRAC_PI_2))
        .with_scale(Vec3::splat(UNITS_PER_METRE))
}

fn spawn_map(mut commands: Commands, server: Res<AssetServer>, manifest: Res<Manifest>) {
    let Some(path) = &manifest.map else {
        return;
    };
    // Read and merged by dbota's own loader: no glTF scene, no mesh or
    // node asset of each of its pieces.
    let file = server.load::<crate::mapfile::MapGeometry>(path.clone());
    commands.insert_resource(MapFile(file));
}

/// Spawns the map's merged meshes as its loader finishes each chunk; once
/// the whole map is read, the terrain, the floor under it, and the ground's
/// height.
fn prepare_map(
    mut commands: Commands,
    file: Option<Res<MapFile>>,
    stream: Res<crate::mapfile::MapStream>,
    mut geometries: ResMut<Assets<crate::mapfile::MapGeometry>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ground: ResMut<Ground>,
    mut count: Local<usize>,
) {
    let done: Vec<Batch> = stream
        .0
        .lock()
        .map(|mut s| s.drain(..).collect())
        .unwrap_or_default();
    *count += done.len();
    for batch in done {
        spawn_batch(&mut commands, &mut meshes, batch);
    }
    let Some(file) = file else {
        return;
    };
    let Some(map) = geometries.remove(&file.0) else {
        return;
    };
    commands.remove_resource::<MapFile>();
    *count += map.batches.len();
    for batch in map.batches {
        spawn_batch(&mut commands, &mut meshes, batch);
    }
    *ground = map.ground;
    commands.insert_resource(TerrainMaterial(map.terrain.id()));
    if let Some(layers) = map.layers {
        commands.insert_resource(layers);
    }
    // A floor under the map wherever the map has a hole.
    commands.spawn((
        Mesh3d(meshes.add(floor(&ground, 30.0, Some(&map.field)))),
        MeshMaterial3d(map.terrain),
        bevy::light::NotShadowCaster,
        crate::fog::Fogged,
    ));
    info!("startup: map prepared at {:.2} s", crate::since_start());
    info!(
        "map prepared: {} merged meshes, ground known on {} cells",
        *count,
        ground.n * ground.n
    );
}

/// One merged mesh of the map, drawn only: its vertices leave the CPU once
/// on the GPU.
fn spawn_batch(commands: &mut Commands, meshes: &mut Assets<Mesh>, mut batch: Batch) {
    batch.mesh.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let mut entity = commands.spawn((
        Mesh3d(meshes.add(batch.mesh)),
        MeshMaterial3d(batch.material),
        Transform::IDENTITY,
        batch.aabb,
        crate::fog::Fogged,
    ));
    if batch.surface != Surface::Prop {
        entity.insert(bevy::light::NotShadowCaster);
    }
    if batch.surface == Surface::Water {
        entity.insert(crate::water::Water);
    }
}

/// World units a cell of the terrain colour grid spans.
const COLOUR_CELL: f32 = 32.0;
/// Cells either side the terrain colours are blurred over.
const COLOUR_BLUR: i32 = 3;

/// Makes the terrain's baked colours continuous across its chunks: every
/// terrain vertex's colour is splatted into one map-wide grid, the grid is
/// blurred a little, and each vertex takes the grid's colour where it
/// stands. Neighbouring chunks of the map are painted with different layer
/// textures and tints, and their baked colours step where they meet; Dota's
/// own shader blends them per pixel. This is the cheap way to the same
/// smooth ground.
pub(crate) fn smooth_terrain(meshes: &mut [&mut Mesh]) -> ColourField {
    let n = (terrain::MAP_CELLS as f32 * terrain::CELL / COLOUR_CELL) as usize + 2;
    let mut grid = splat_colours(meshes, n);
    // A separable box blur, twice: near enough a gaussian, and cells no
    // vertex fell in take their neighbours' colour.
    for _ in 0..2 {
        for axis in 0..2 {
            grid = box_blur(&grid, n, axis);
        }
    }
    let field = ColourField::spread(&grid, n);
    for mesh in meshes.iter_mut() {
        recolour(mesh, &grid, n);
    }
    field
}

/// Every vertex colour summed into the grid cell it stands in: RGB sums and
/// the count of vertices.
fn splat_colours(meshes: &[&mut Mesh], n: usize) -> Vec<[f32; 4]> {
    let mut grid = vec![[0.0f32; 4]; n * n];
    for mesh in meshes {
        let (
            Some(VertexAttributeValues::Float32x3(pos)),
            Some(VertexAttributeValues::Float32x4(col)),
        ) = (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_COLOR),
        )
        else {
            continue;
        };
        for (p, c) in pos.iter().zip(col) {
            let (cx, cy) = (p[0] / COLOUR_CELL, -p[2] / COLOUR_CELL);
            if cx >= 0.0 && cy >= 0.0 && (cx as usize) < n && (cy as usize) < n {
                let cell = &mut grid[cy as usize * n + cx as usize];
                for ch in 0..3 {
                    cell[ch] += c[ch];
                }
                cell[3] += 1.0;
            }
        }
    }
    grid
}

/// The grid summed over [`COLOUR_BLUR`] cells either side along one axis.
fn box_blur(grid: &[[f32; 4]], n: usize, axis: usize) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; n * n];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0.0f32; 4];
            for d in -COLOUR_BLUR..=COLOUR_BLUR {
                let (sx, sy) = if axis == 0 {
                    (x as i32 + d, y as i32)
                } else {
                    (x as i32, y as i32 + d)
                };
                if sx < 0 || sy < 0 || sx as usize >= n || sy as usize >= n {
                    continue;
                }
                let v = grid[sy as usize * n + sx as usize];
                for k in 0..4 {
                    acc[k] += v[k];
                }
            }
            out[y * n + x] = acc;
        }
    }
    out
}

/// Each vertex of a terrain mesh takes the blurred grid's colour where it
/// stands, bilinear.
fn recolour(mesh: &mut Mesh, grid: &[[f32; 4]], n: usize) {
    let sample = |x: f32, z: f32| -> Option<[f32; 3]> {
        let (fx, fy) = (x / COLOUR_CELL - 0.5, -z / COLOUR_CELL - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let mut acc = [0.0f32; 4];
        for (dx, dy, w) in [
            (0, 0, (1.0 - tx) * (1.0 - ty)),
            (1, 0, tx * (1.0 - ty)),
            (0, 1, (1.0 - tx) * ty),
            (1, 1, tx * ty),
        ] {
            let (cx, cy) = (x0 as i32 + dx, y0 as i32 + dy);
            if cx < 0 || cy < 0 || cx as usize >= n || cy as usize >= n {
                continue;
            }
            let v = grid[cy as usize * n + cx as usize];
            for k in 0..4 {
                acc[k] += v[k] * w;
            }
        }
        (acc[3] > 1e-3).then(|| [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3]])
    };
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let pos = pos.clone();
    let Some(VertexAttributeValues::Float32x4(col)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
    else {
        return;
    };
    for (p, c) in pos.iter().zip(col.iter_mut()) {
        if let Some(s) = sample(p[0], p[2]) {
            c[..3].copy_from_slice(&s);
        }
    }
}

/// The terrain's colours over the whole map, a cell [`COLOUR_CELL`] wide,
/// for the floor under the map's holes.
pub struct ColourField {
    n: usize,
    cells: Vec<[f32; 3]>,
}

impl ColourField {
    /// The blurred grid's colours, cells far from any terrain taking their
    /// neighbours' colour, spread out until the grid is full: the floor under
    /// the map's holes is painted with the ground it meets, no step at the
    /// edge.
    fn spread(grid: &[[f32; 4]], n: usize) -> ColourField {
        let mut filled: Vec<Option<[f32; 3]>> = grid
            .iter()
            .map(|g| (g[3] > 1e-3).then(|| [g[0] / g[3], g[1] / g[3], g[2] / g[3]]))
            .collect();
        fill_holes(&mut filled, n, [(1, 0), (-1, 0), (0, 1), (0, -1)], n);
        ColourField {
            n,
            cells: filled
                .into_iter()
                .map(|c| c.unwrap_or([0.3, 0.35, 0.2]))
                .collect(),
        }
    }

    /// The colour at a Bevy ground point, bilinear.
    pub fn at(&self, x: f32, z: f32) -> [f32; 3] {
        let n = self.n;
        let (fx, fy) = (x / COLOUR_CELL - 0.5, -z / COLOUR_CELL - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |cx: f32, cy: f32| {
            let (cx, cy) = (
                (cx.max(0.0) as usize).min(n - 1),
                (cy.max(0.0) as usize).min(n - 1),
            );
            self.cells[cy * n + cx]
        };
        let (a, b, c, d) = (
            at(x0, y0),
            at(x0 + 1.0, y0),
            at(x0, y0 + 1.0),
            at(x0 + 1.0, y0 + 1.0),
        );
        [0, 1, 2].map(|k| {
            (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty
        })
    }
}

/// Sets a prop's material the way its Dota shader blends: additive glows add,
/// overlays blend, and nothing is metal.
pub(crate) fn dress_prop(m: &mut StandardMaterial, vmat: &Vmat) {
    m.metallic = 0.0;
    if vmat.additive {
        m.alpha_mode = AlphaMode::Add;
        m.unlit = true;
    } else if vmat.translucent && m.alpha_mode == AlphaMode::Opaque {
        m.alpha_mode = AlphaMode::Blend;
    }
}

/// What a map mesh is, which decides how it is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    Prop,
    Terrain,
    /// The river: casts no shadow and flows (see `water.rs`).
    Water,
}

/// Map meshes sharing a material and a vertex layout, merged in world space.
pub(crate) struct Batch {
    pub material: Handle<StandardMaterial>,
    /// The map chunk the batch's meshes are centred in.
    chunk: (i32, i32),
    layout: Vec<bevy::mesh::MeshVertexAttributeId>,
    pub surface: Surface,
    pub mesh: Mesh,
    /// The bounds of all its meshes.
    pub aabb: bevy::camera::primitives::Aabb,
}

/// World units of a side of the map chunks meshes are merged within, so the
/// camera and the shadows leave out what they do not see.
const BATCH_CHUNK: f32 = 2048.0;

/// Vertices a merged map mesh holds at most before another one is begun.
const BATCH_VERTICES: usize = 1 << 20;

/// The map chunk a world point lies in, for batching.
pub(crate) fn chunk_of(p: Vec3) -> (i32, i32) {
    ((p.x / BATCH_CHUNK) as i32, (p.z / BATCH_CHUNK) as i32)
}

/// Appends a mesh placed in the world to the batch of its material, layout
/// and chunk.
pub(crate) fn add_to_batches(
    batches: &mut Vec<Batch>,
    mesh: Mesh,
    aabb: bevy::camera::primitives::Aabb,
    material: Handle<StandardMaterial>,
    surface: Surface,
    chunk: (i32, i32),
) {
    let mut layout: Vec<_> = mesh.attributes().map(|(a, _)| a.id).collect();
    layout.sort();
    let found = batches.iter_mut().find(|b| {
        b.material == material
            && b.chunk == chunk
            && b.layout == layout
            && b.mesh.count_vertices() + mesh.count_vertices() < BATCH_VERTICES
    });
    match found {
        Some(batch) => {
            if batch.mesh.merge(&mesh).is_err() {
                warn!("a map mesh could not be merged");
            }
            let (lo, hi) = (
                Vec3::from(batch.aabb.min()).min(Vec3::from(aabb.min())),
                Vec3::from(batch.aabb.max()).max(Vec3::from(aabb.max())),
            );
            batch.aabb = bevy::camera::primitives::Aabb::from_min_max(lo, hi);
        }
        None => batches.push(Batch {
            material,
            chunk,
            layout,
            surface,
            mesh,
            aabb,
        }),
    }
}

/// A mesh with its transform applied to its positions, normals and tangents,
/// its indices widened to 32 bits.
pub(crate) fn bake_transform(mut mesh: Mesh, global: &GlobalTransform) -> Mesh {
    let affine = global.affine();
    let normal_matrix = Mat3::from(affine.matrix3).inverse().transpose();
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            *p = affine.transform_point3(Vec3::from(*p)).into();
        }
    }
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        for n in normals.iter_mut() {
            *n = (normal_matrix * Vec3::from(*n)).normalize_or_zero().into();
        }
    }
    if let Some(VertexAttributeValues::Float32x4(tangents)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT)
    {
        for t in tangents.iter_mut() {
            let v = (Mat3::from(affine.matrix3) * Vec3::new(t[0], t[1], t[2])).normalize_or_zero();
            *t = [v.x, v.y, v.z, t[3]];
        }
    }
    if affine.matrix3.determinant() < 0.0
        && let Some(Indices::U32(indices)) = mesh.indices_mut()
    {
        for tri in indices.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
    }
    let indices: Option<Vec<u32>> = mesh.indices().map(|i| i.iter().map(|v| v as u32).collect());
    if let Some(indices) = indices {
        mesh.insert_indices(Indices::U32(indices));
    }
    mesh
}

/// The mean colour of the ground layer textures, read from the cache once
/// each.
pub(crate) struct LayerMeans {
    pub(crate) root: Option<std::path::PathBuf>,
    pub(crate) known: std::collections::HashMap<String, [f32; 3]>,
}

impl LayerMeans {
    /// The mean linear colour of a Dota texture path's exported PNG; a
    /// middling grey when it cannot be read.
    fn of(&mut self, vtex: &str) -> [f32; 3] {
        // A name of the map file must not climb out of the folder, on
        // Windows either.
        let name = vtex
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(vtex)
            .trim_end_matches(".vtex");
        let root = &self.root;
        *self.known.entry(name.to_string()).or_insert_with(|| {
            root.as_ref()
                .and_then(|root| {
                    crate::cachefs::read_small(root, &format!("textures/ground/{name}.png"))
                })
                .and_then(|bytes| mean_color(&bytes))
                .unwrap_or([0.5; 3])
        })
    }

    /// The four layers of a terrain material, each its tint times its
    /// texture's mean colour, and its family.
    pub(crate) fn layers(&mut self, vmat: &Vmat) -> TerrainLayers {
        TerrainLayers {
            colours: [0, 1, 2, 3].map(|i| {
                let mean = self.of(&vmat.textures[i]);
                [0, 1, 2].map(|c| vmat.tints[i][c] * mean[c])
            }),
            families: [0, 1, 2, 3].map(|i| Family::of(&vmat.textures[i])),
            stone_slot: 0.0,
        }
    }

    /// The mean of the detail texture the terrain material tiles, by its
    /// file in the cache.
    pub(crate) fn detail(&mut self, file: Option<&str>) -> f32 {
        let mean = file
            .zip(self.root.as_ref())
            .and_then(|(p, root)| crate::cachefs::read_small(root, p))
            .and_then(|bytes| mean_color(&bytes))
            .unwrap_or([0.5; 3]);
        ((mean[0] + mean[1] + mean[2]) / 3.0).max(0.05)
    }
}

/// The mean linear RGB of a PNG, sampled sparsely.
fn mean_color(bytes: &[u8]) -> Option<[f32; 3]> {
    let image = Image::from_buffer(
        bytes,
        bevy::image::ImageType::Extension("png"),
        bevy::image::CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .ok()?;
    let (w, h) = (image.width(), image.height());
    let mut sum = [0.0f32; 3];
    let mut n = 0.0;
    for y in (0..h).step_by(7) {
        for x in (0..w).step_by(7) {
            let c = image.get_color_at(x, y).ok()?.to_linear();
            sum[0] += c.red;
            sum[1] += c.green;
            sum[2] += c.blue;
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum.map(|v| v / n))
}

/// How the terrain's detail texture is sampled: tiled, sharp at a slant.
pub(crate) fn detail_settings(s: &mut ImageLoaderSettings) {
    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
}

/// The material every baked terrain mesh is drawn with: a grey detail
/// texture tinted by the baked vertex colours.
pub(crate) fn terrain_material_with(detail: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: detail,
        uv_transform: Affine2::from_scale(Vec2::splat(0.25)),
        perceptual_roughness: 0.92,
        reflectance: 0.2,
        ..default()
    }
}

/// What the baking needs of a Dota terrain material: its shader and the
/// tints of its four layers.
#[derive(Clone, Debug, Default)]
pub(crate) struct Vmat {
    /// Dota's path of the material.
    pub name: String,
    pub shader: String,
    /// Whether the material adds its light rather than covering.
    pub additive: bool,
    /// Whether the material lets what is behind show through.
    pub translucent: bool,
    /// Whether the material is a decal laid on other surfaces.
    pub overlay: bool,
    /// Each layer's tint, the mean of its two tints, linear RGB.
    pub tints: [[f32; 3]; 4],
    /// Each layer's colour texture, as Dota names it.
    pub textures: [String; 4],
    /// What a water material's file says of how it looks.
    pub water: WaterLook,
}

/// The water shader's parameters that make its colour: Dota's cheap
/// ("low end") look, its surface colour and opacity, and the tint of what
/// is seen through it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaterLook {
    /// sRGB.
    pub surface: [f32; 3],
    pub opacity: f32,
    /// sRGB.
    pub refraction_tint: [f32; 3],
    /// World units one repeat of the normal map spans: Dota's water reads
    /// its normals by world position, not by the pieces' own texture
    /// coordinates, so the river is one surface across them.
    pub normal_uv_scale: f32,
}

impl Default for WaterLook {
    fn default() -> WaterLook {
        WaterLook {
            surface: [0.18, 0.27, 0.24],
            opacity: 0.875,
            refraction_tint: [0.35, 0.66, 0.78],
            normal_uv_scale: 500.0,
        }
    }
}

impl Vmat {
    pub(crate) fn parse(json: &str) -> Vmat {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            return Vmat::default();
        };
        let vmat = &value["vmat"];
        let shader = vmat["ShaderName"].as_str().unwrap_or_default().to_string();
        let mut tints = [[1.0; 3]; 4];
        for (layer, tint) in tints.iter_mut().enumerate() {
            let read = |key: String| -> Option<[f32; 3]> {
                let v = vmat["VectorParams"][key.as_str()].as_array()?;
                Some([
                    v.first()?.as_f64()? as f32,
                    v.get(1)?.as_f64()? as f32,
                    v.get(2)?.as_f64()? as f32,
                ])
            };
            let a = read(format!("g_vColorTint{layer}")).unwrap_or([1.0; 3]);
            // A white second tint is no second tint at all.
            let b = read(format!("g_vColorTintB{layer}"))
                .filter(|b| b.iter().any(|c| *c < 0.99))
                .unwrap_or(a);
            for c in 0..3 {
                tint[c] = srgb_to_linear((a[c] + b[c]) * 0.5);
            }
        }
        let textures = [0, 1, 2, 3].map(|layer| {
            vmat["TextureParams"][format!("g_tColor{layer}").as_str()]
                .as_str()
                .unwrap_or_default()
                .to_string()
        });
        let flag = |key: &str| vmat["IntParams"][key].as_i64().unwrap_or(0) != 0;
        let colour = |key: &str, default: [f32; 3]| -> [f32; 3] {
            let v = &vmat["VectorParams"][key];
            std::array::from_fn(|i| v[i].as_f64().map_or(default[i], |c| c as f32))
        };
        let look = WaterLook::default();
        Vmat {
            name: vmat["Name"].as_str().unwrap_or_default().to_string(),
            shader,
            additive: flag("F_ADDITIVE_BLEND"),
            translucent: flag("F_TRANSLUCENT") || flag("F_OVERLAY") || flag("F_BLEND_MODE"),
            overlay: flag("F_OVERLAY"),
            tints,
            textures,
            water: WaterLook {
                surface: colour("g_vLowEndSurfaceColor", look.surface),
                opacity: vmat["FloatParams"]["g_flLowEndSurfaceOpacity"]
                    .as_f64()
                    .map_or(look.opacity, |o| o as f32),
                refraction_tint: colour("g_vRefractionTint", look.refraction_tint),
                normal_uv_scale: vmat["FloatParams"]["g_flNormalUvScale"]
                    .as_f64()
                    .map(|s| s as f32)
                    .filter(|s| s.is_finite() && *s > 1.0)
                    .unwrap_or(look.normal_uv_scale),
            },
        }
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// How much of a layer shows: Dota's blend modulation against a texture of
/// middling height.
fn modulate(weight: f32, softness: f32) -> f32 {
    let soft = softness.max(0.02);
    let (lo, hi) = ((0.5 - soft).max(0.0), (0.5 + soft).min(1.0));
    let t = ((weight - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A terrain material's four layers: each one's colour (its tint times its
/// texture's mean) and family.
#[derive(Clone, Debug)]
pub(crate) struct TerrainLayers {
    pub colours: [[f32; 3]; 4],
    pub families: [Family; 4],
    /// Which of the two stone textures its stone layers are drawn with.
    pub stone_slot: f32,
}

/// Writes one terrain mesh's vertex colours: its ground layers blended by
/// the vertex's weights, each in its layer's tint, under the painted tint
/// (all four layers where it shows no ground). Its path layers go into
/// their own colours, taken into the path field ([`crate::layers`]), and
/// their weights into the second texture coordinates, dirt then stone.
pub(crate) fn bake_terrain(mesh: &mut Mesh, layers: &TerrainLayers, detail: f32, gain: f32) {
    let read = |mesh: &Mesh, attr: &MeshVertexAttribute| -> Option<Vec<[f32; 4]>> {
        match mesh.attribute(attr.id)? {
            VertexAttributeValues::Float32x4(v) => Some(v.clone()),
            _ => None,
        }
    };
    let (Some(weights), Some(alphas), Some(painted)) = (
        read(mesh, &ATTRIBUTE_BLEND_WEIGHTS),
        read(mesh, &ATTRIBUTE_BLEND_ALPHAS),
        read(mesh, &ATTRIBUTE_PAINTED_TINT),
    ) else {
        return;
    };
    let count = weights.len();
    let mut colors = Vec::with_capacity(count);
    let mut paths = Vec::with_capacity(count);
    let mut tints = [Vec::with_capacity(count), Vec::with_capacity(count)];
    for ((w, a), p) in weights.iter().zip(&alphas).zip(&painted) {
        let w1 = modulate(w[0], a[0]);
        let w2 = modulate(w[1], a[1]).min(1.0 - w1);
        let w3 = modulate(w[2], a[2]).min(1.0 - w1 - w2);
        let w0 = (1.0 - w1 - w2 - w3).max(0.0);
        // Per family: the weighted sum of its layers' colours, and its weight.
        let mut sums = [[0.0f32; 4]; 3];
        for (layer, weight) in [w0, w1, w2, w3].into_iter().enumerate() {
            let sum = &mut sums[layers.families[layer] as usize];
            for (total, colour) in sum.iter_mut().zip(layers.colours[layer]) {
                *total += weight * colour;
            }
            sum[3] += weight;
        }
        let all: [f32; 4] = std::array::from_fn(|ch| sums.iter().map(|s| s[ch]).sum());
        let lit = |sum: &[f32; 4], scale: f32| -> [f32; 4] {
            let w = sum[3].max(1e-4);
            // Dota doubles its terrain's light.
            std::array::from_fn(|ch| {
                if ch == 3 {
                    sum[3]
                } else {
                    (sum[ch] / w * srgb_to_linear(p[ch]) * gain * scale).min(1.5)
                }
            })
        };
        let ground = if sums[0][3] > 1e-3 { &sums[0] } else { &all };
        // The grass texture's own brightness is taken back out.
        let mut c = lit(ground, 1.0 / detail);
        c[3] = 1.0;
        colors.push(c);
        paths.push([sums[1][3], sums[2][3]]);
        tints[0].push(lit(&sums[1], 1.0));
        tints[1].push(lit(&sums[2], 1.0));
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, paths);
    let [dirt, stone] = tints;
    mesh.insert_attribute(crate::layers::ATTRIBUTE_DIRT, dirt);
    mesh.insert_attribute(crate::layers::ATTRIBUTE_STONE, stone);
    mesh.insert_attribute(
        crate::layers::ATTRIBUTE_STONE_SLOT,
        vec![layers.stone_slot; count],
    );
}

/// Heights gathered from terrain vertices, cell by cell.
pub(crate) struct Samples {
    n: usize,
    sum: Vec<f32>,
    count: Vec<u32>,
    surface: TerrainSurface,
}

impl Samples {
    pub(crate) fn new(n: usize) -> Samples {
        Samples {
            n,
            sum: vec![0.0; n * n],
            count: vec![0; n * n],
            surface: TerrainSurface {
                cells: vec![Vec::new(); n * n],
                ..TerrainSurface::default()
            },
        }
    }

    /// Adds every vertex of a mesh placed in the world.
    pub(crate) fn add(&mut self, mesh: &Mesh) {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return;
        };
        for &[x, y, z] in positions {
            let (cx, cy) = ((x / CELL).floor(), (-z / CELL).floor());
            if cx < 0.0 || cy < 0.0 || cx >= self.n as f32 || cy >= self.n as f32 {
                continue;
            }
            let at = cy as usize * self.n + cx as usize;
            self.sum[at] += y;
            self.count[at] += 1;
        }
        self.add_triangles(mesh, positions);
    }

    /// Keeps the mesh's triangles, each listed in the cells its bounds cover.
    fn add_triangles(&mut self, mesh: &Mesh, positions: &[[f32; 3]]) {
        let Some(indices) = mesh.indices() else {
            return;
        };
        let base = self.surface.points.len() as u32;
        let surface = &mut self.surface;
        surface
            .points
            .extend(positions.iter().map(|&[x, y, z]| [x, -z, y]));
        let indices: Vec<u32> = indices.iter().map(|i| i as u32).collect();
        for t in indices.as_chunks::<3>().0 {
            let t = [t[0] + base, t[1] + base, t[2] + base];
            let corners = t.map(|i| surface.points[i as usize]);
            let (x0, x1) = (
                corners.iter().map(|p| p[0]).fold(f32::MAX, f32::min),
                corners.iter().map(|p| p[0]).fold(f32::MIN, f32::max),
            );
            let (y0, y1) = (
                corners.iter().map(|p| p[1]).fold(f32::MAX, f32::min),
                corners.iter().map(|p| p[1]).fold(f32::MIN, f32::max),
            );
            let cell = |v: f32| ((v / CELL).floor().max(0.0) as usize).min(self.n - 1);
            let index = surface.triangles.len() as u32;
            surface.triangles.push(t);
            for cy in cell(y0)..=cell(y1) {
                for cx in cell(x0)..=cell(x1) {
                    surface.cells[cy * self.n + cx].push(index);
                }
            }
        }
    }

    /// The ground, every cell without a vertex taking its neighbours'
    /// height.
    pub(crate) fn finish(self) -> Ground {
        let n = self.n;
        let mut heights: Vec<Option<[f32; 1]>> = self
            .sum
            .iter()
            .zip(&self.count)
            .map(|(s, c)| (*c > 0).then(|| [s / *c as f32]))
            .collect();
        fill_holes(&mut heights, n, [(0, 1), (0, -1), (1, 0), (-1, 0)], n);
        let surface =
            (!self.surface.triangles.is_empty()).then(|| std::sync::Arc::new(self.surface));
        Ground {
            n,
            surface,
            heights: heights
                .into_iter()
                .map(|h| h.map_or(128.0, |[h]| h))
                .collect(),
        }
    }
}

/// Fills every empty cell of an `n` by `n` grid with the mean of its filled
/// neighbours, taken in `order`, a ring at a time until none is left that
/// borders a filled one, or `rings` rings are laid.
pub(crate) fn fill_holes<const K: usize>(
    cells: &mut [Option<[f32; K]>],
    n: usize,
    order: [(i32, i32); 4],
    rings: usize,
) {
    for _ in 0..rings.min(n) {
        let snapshot = cells.to_vec();
        let mut changed = false;
        for y in 0..n {
            for x in 0..n {
                if snapshot[y * n + x].is_some() {
                    continue;
                }
                let mut acc = [0.0f32; K];
                let mut k = 0.0;
                for (dx, dy) in order {
                    let (sx, sy) = (x as i32 + dx, y as i32 + dy);
                    if sx < 0 || sy < 0 || sx as usize >= n || sy as usize >= n {
                        continue;
                    }
                    if let Some(c) = snapshot[sy as usize * n + sx as usize] {
                        for ch in 0..K {
                            acc[ch] += c[ch];
                        }
                        k += 1.0;
                    }
                }
                if k > 0.0 {
                    cells[y * n + x] = Some(acc.map(|a| a / k));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// A grid over the whole map one cell apart, `below` world units under the
/// ground.
pub fn floor(ground: &Ground, below: f32, colours: Option<&ColourField>) -> Mesh {
    let n = ground.n;
    let m = n + 1;
    let mut positions = Vec::with_capacity(m * m);
    let mut uvs = Vec::with_capacity(m * m);
    for cy in 0..m {
        for cx in 0..m {
            let (x, y) = (cx as f32 * CELL, cy as f32 * CELL);
            positions.push([x, ground.at(x, y) - below, -y]);
            uvs.push([x / CELL, y / CELL]);
        }
    }
    let mut indices = Vec::with_capacity(n * n * 6);
    for cy in 0..n {
        for cx in 0..n {
            let a = (cy * m + cx) as u32;
            let (b, c) = (a + 1, a + m as u32);
            let d = c + 1;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    // Painted with the ground's own colours where it shows through a hole.
    if let Some(field) = colours {
        let colors: Vec<[f32; 4]> = (0..m * m)
            .map(|i| {
                let (x, y) = ((i % m) as f32 * CELL, (i / m) as f32 * CELL);
                let c = field.at(x, -y);
                [c[0], c[1], c[2], 1.0]
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_normals();
    mesh
}

/// Builds the ground from the match's terrain cells when no map came from
/// the cache.
fn build_fallback_ground(
    mut commands: Commands,
    session: NonSend<Session>,
    known: Res<MatchKnown>,
    manifest: Res<Manifest>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut done: Local<bool>,
) {
    if *done || !known.0 || manifest.map.is_some() {
        return;
    }
    *done = true;
    let core = &session.0;
    let n = core.terrain_cells();
    // The wire allows a map of one cell, which has no slopes to build.
    if n < 2 {
        return;
    }
    let built = terrain::build(core.terrain(), n, 512.0);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    let to_bevy = |p: &[f32; 3]| [p[0], p[2], -p[1]];
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        built.positions.iter().map(to_bevy).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        built.normals.iter().map(to_bevy).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, built.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, built.colors);
    mesh.insert_indices(Indices::U32(built.indices));
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    let heights = terrain::Heights::new(core.terrain(), n);
    let cells = (0..n * n)
        .map(|i| {
            let (cx, cy) = (i % n, i / n);
            heights.at((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL)
        })
        .collect();
    *ground = Ground {
        n,
        heights: cells,
        surface: None,
    };
}
