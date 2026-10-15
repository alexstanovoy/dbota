//! The ground: Dota's own mid-lane map from the cache, its terrain layers
//! baked into vertex colours, or a terrain built from the match's cells when
//! the cache has no map. Also the sun, and the height of the ground anywhere.

use std::f32::consts::FRAC_PI_2;

use bevy::{
    asset::RenderAssetUsages,
    gltf::GltfMaterialExtras,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    light::CascadeShadowConfigBuilder,
    math::Affine2,
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues, VertexFormat},
    prelude::*,
    world_serialization::WorldInstanceReady,
};

use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::terrain::{self, CELL};

/// How bright the baked terrain is against the lighting: Dota doubles its
/// terrain's light, the rest of the scene does not.
const TERRAIN_GAIN: f32 = 1.6;

/// Source 2 inches to the metres the exporter writes, inverted.
pub const UNITS_PER_METRE: f32 = 1.0 / 0.0254;
/// Where Dota's origin lies in bota's coordinates, on both axes.
pub const DOTA_ORIGIN: f32 = 9216.0;

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

    /// Whether the ground is known.
    pub fn known(&self) -> bool {
        self.n > 0
    }
}

/// The root of the map scene.
#[derive(Component)]
struct MapRoot;

/// A map scene that is spawned but not yet prepared.
#[derive(Component)]
struct MapPending {
    frames: u8,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Ground>()
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.72, 0.80, 1.0),
            brightness: crate::look("ambient", 330.0),
            ..default()
        })
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.04)))
        .add_systems(Startup, (spawn_sun, spawn_map))
        .add_systems(Update, (prepare_map, build_fallback_ground));
}

fn spawn_sun(mut commands: Commands, args: Res<crate::Args>) {
    commands.spawn((
        DirectionalLight {
            illuminance: crate::look("sun", 13_000.0),
            color: {
                let warm = crate::look("warm", 0.3);
                Color::srgb(1.0, 0.97 - 0.09 * warm, 0.92 - 0.22 * warm)
            },
            shadow_maps_enabled: !args.without.iter().any(|w| w == "shadows"),
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: 3_000.0,
            maximum_distance: 8_000.0,
            ..default()
        }
        .build(),
    ));
}

/// The transform that puts the exported map, glTF metres with `x` along
/// Dota's `y`, onto bota's coordinates.
fn map_transform() -> Transform {
    Transform::from_translation(Vec3::new(DOTA_ORIGIN, 0.0, -DOTA_ORIGIN))
        .with_rotation(Quat::from_rotation_y(-FRAC_PI_2))
        .with_scale(Vec3::splat(UNITS_PER_METRE))
}

fn spawn_map(
    mut commands: Commands,
    server: Res<AssetServer>,
    manifest: Res<Manifest>,
    args: Res<crate::Args>,
) {
    if args.without.iter().any(|w| w == "map") {
        return;
    }
    let Some(path) = &manifest.map else {
        return;
    };
    // The map's own meshes are read once, merged and let go: they need never
    // reach the GPU.
    let scene = server
        .load_builder()
        .with_settings(|s: &mut bevy::gltf::GltfLoaderSettings| {
            s.load_meshes = bevy::asset::RenderAssetUsages::MAIN_WORLD;
        })
        .load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(path.clone()));
    commands
        .spawn((MapRoot, WorldAssetRoot(scene), map_transform()))
        .observe(|ready: On<WorldInstanceReady>, mut commands: Commands| {
            info!("startup: map scene ready at {:.2} s", crate::since_start());
            commands
                .entity(ready.entity)
                .insert(MapPending { frames: 2 });
        });
}

/// Everything the map preparation reads and writes.
#[derive(bevy::ecs::system::SystemParam)]
struct MapAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    server: Res<'w, AssetServer>,
    manifest: Res<'w, Manifest>,
}

/// Once the map's transforms have settled: bakes the terrain layers into
/// vertex colours, takes the ground's height from the terrain, and merges
/// the whole map into one mesh per material, its transforms baked in.
fn prepare_map(
    mut commands: Commands,
    mut pending: Query<(Entity, &mut MapPending)>,
    children: Query<&Children>,
    parts: Query<(
        &Mesh3d,
        &GlobalTransform,
        &MeshMaterial3d<StandardMaterial>,
        Option<&GltfMaterialExtras>,
    )>,
    mut assets: MapAssets,
    mut ground: ResMut<Ground>,
    args: Res<crate::Args>,
) {
    let without = |what: &str| args.without.iter().any(|w| w == what);
    let Ok((root, mut wait)) = pending.single_mut() else {
        return;
    };
    if wait.frames > 0 {
        wait.frames -= 1;
        return;
    }
    let skipped: Vec<String> = std::env::var("DBOTA_SKIP")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let began = std::time::Instant::now();
    let mut means = LayerMeans::new(assets.manifest.root.clone());
    let detail_mean = means.detail(&assets.manifest);
    let terrain_material = assets
        .materials
        .add(terrain_material(&assets.server, &assets.manifest));
    let water_material = assets.materials.add(StandardMaterial {
        base_color: Color::srgba(0.10, 0.22, 0.26, 0.82),
        perceptual_roughness: 0.15,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut samples = Samples::new(terrain::MAP_CELLS);
    let mut batches: Vec<Batch> = Vec::new();
    // What each map part becomes, worked out on the main thread; the heavy
    // vertex work then runs on every core.
    struct Job {
        mesh: Handle<Mesh>,
        global: GlobalTransform,
        material: Handle<StandardMaterial>,
        /// The four terrain layers' colours, for a terrain part.
        layers: Option<[[f32; 3]; 4]>,
    }
    impl TerrainJob for Job {
        fn is_terrain(&self) -> bool {
            self.layers.is_some()
        }
    }
    let mut jobs: Vec<Job> = Vec::new();
    for entity in children.iter_descendants(root) {
        let Ok((mesh3d, global, material, extras)) = parts.get(entity) else {
            continue;
        };
        let vmat = extras.map(|e| Vmat::parse(&e.value)).unwrap_or_default();
        let (material, layers) = match vmat.shader.as_str() {
            "multiblend.vfx" => (terrain_material.clone(), Some(means.layers(&vmat))),
            "water_dota.vfx" => (water_material.clone(), None),
            // Glows, light shafts and fountain sprays are particles in all
            // but name: drawn as plain meshes they cover the ground.
            _ if vmat.additive => continue,
            _ if without("props") => continue,
            _ if skipped.iter().any(|s| vmat.name.contains(s.as_str())) => continue,
            _ if without("overlays") && vmat.translucent => continue,
            _ => {
                let Some(mut m) = assets.materials.get_mut(&material.0) else {
                    continue;
                };
                // Path borders, ground edges and wagon tracks are tinted
                // by what the export leaves out (per-vertex tints, or all
                // but black): drawn as they come they lay dark brown bands
                // along the lanes.
                let black = m
                    .base_color
                    .to_linear()
                    .to_f32_array_no_alpha()
                    .iter()
                    .all(|c| *c < 0.05);
                if vmat.overlay && (black || vmat.name.contains("tint")) {
                    continue;
                }
                dress_prop(&mut m, &vmat);
                (material.0.clone(), None)
            }
        };
        jobs.push(Job {
            mesh: mesh3d.0.clone(),
            global: *global,
            material,
            layers,
        });
    }
    let gain = crate::look("gain", TERRAIN_GAIN);
    let pool = bevy::tasks::ComputeTaskPool::get();
    // Each part cut down to its own vertices, the terrain's layers baked.
    let meshes = &assets.meshes;
    let local: Vec<Option<Mesh>> = pool.scope(|scope| {
        for job in &jobs {
            scope.spawn(async move {
                let source = meshes.get(&job.mesh)?;
                if source.primitive_topology() != PrimitiveTopology::TriangleList {
                    return None;
                }
                let mut mesh = compact(source)?;
                if let Some(layers) = &job.layers {
                    bake_terrain(&mut mesh, layers, detail_mean, gain);
                }
                for attr in [
                    &ATTRIBUTE_BLEND_WEIGHTS,
                    &ATTRIBUTE_BLEND_ALPHAS,
                    &ATTRIBUTE_PAINTED_TINT,
                ] {
                    mesh.remove_attribute(attr.id);
                }
                Some(mesh)
            });
        }
    });
    for (job, mesh) in jobs.iter().zip(&local) {
        if let (Some(_), Some(mesh)) = (&job.layers, mesh) {
            samples.add(mesh, &job.global);
        }
    }
    // Then placed in the world and measured, again on every core.
    let placed: Vec<Option<(Mesh, bevy::camera::primitives::Aabb)>> = pool.scope(|scope| {
        for (job, mesh) in jobs.iter().zip(local) {
            scope.spawn(async move {
                let mesh = bake_transform(mesh?, &job.global);
                let aabb = bevy::camera::primitives::MeshAabb::compute_aabb(&mesh)?;
                Some((mesh, aabb))
            });
        }
    });
    let mut placed = placed;
    let field = smooth_terrain(&jobs, &mut placed);
    for (job, placed) in jobs.iter().zip(placed) {
        if let Some((mesh, aabb)) = placed {
            add_to_batches(
                &mut batches,
                mesh,
                aabb,
                job.material.clone(),
                job.layers.is_some(),
            );
        }
    }
    commands.entity(root).despawn();
    let count = batches.len();
    for batch in batches {
        let mut entity = commands.spawn((
            Mesh3d(assets.meshes.add(batch.mesh)),
            MeshMaterial3d(batch.material),
            Transform::IDENTITY,
            crate::fog::Fogged,
        ));
        if batch.terrain {
            entity.insert(bevy::light::NotShadowCaster);
        }
    }
    *ground = samples.finish();
    // A floor under the map wherever the map has a hole.
    let mut floor_material = self::terrain_material(&assets.server, &assets.manifest);
    floor_material.base_color = Color::WHITE;
    commands.spawn((
        Mesh3d(assets.meshes.add(floor(&ground, 30.0, Some(&field)))),
        MeshMaterial3d(assets.materials.add(floor_material)),
        bevy::light::NotShadowCaster,
        crate::fog::Fogged,
    ));
    info!(
        "startup: map prepared at {:.2} s in {:.2} s, {:.2} s of it reading layer colours",
        crate::since_start(),
        began.elapsed().as_secs_f32(),
        means.spent.as_secs_f32()
    );
    info!(
        "map prepared: {count} merged meshes, ground known on {} cells",
        ground.n * ground.n
    );
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
fn smooth_terrain<A>(
    jobs: &[A],
    placed: &mut [Option<(Mesh, bevy::camera::primitives::Aabb)>],
) -> ColourField
where
    A: TerrainJob,
{
    let n = (terrain::MAP_CELLS as f32 * terrain::CELL / COLOUR_CELL) as usize + 2;
    let mut sum = vec![[0.0f32; 3]; n * n];
    let mut weight = vec![0.0f32; n * n];
    let cell = |x: f32, z: f32| -> Option<usize> {
        let (cx, cy) = (x / COLOUR_CELL, -z / COLOUR_CELL);
        (cx >= 0.0 && cy >= 0.0 && (cx as usize) < n && (cy as usize) < n)
            .then(|| cy as usize * n + cx as usize)
    };
    let terrain = |i: usize| jobs[i].is_terrain();
    for (i, entry) in placed.iter().enumerate() {
        let Some((mesh, _)) = entry else { continue };
        if !terrain(i) {
            continue;
        }
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
            if let Some(k) = cell(p[0], p[2]) {
                for ch in 0..3 {
                    sum[k][ch] += c[ch];
                }
                weight[k] += 1.0;
            }
        }
    }
    // A separable box blur, twice: near enough a gaussian, and cells no
    // vertex fell in take their neighbours' colour.
    let mut grid: Vec<[f32; 4]> = sum
        .iter()
        .zip(&weight)
        .map(|(s, w)| [s[0], s[1], s[2], *w])
        .collect();
    for _ in 0..2 {
        for axis in 0..2 {
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
            grid = out;
        }
    }
    // Cells far from any terrain take their neighbours' colour, spread out
    // until the grid is full: the floor under the map's holes is painted
    // with the ground it meets, no step at the edge.
    let mut filled: Vec<Option<[f32; 3]>> = grid
        .iter()
        .map(|g| (g[3] > 1e-3).then(|| [g[0] / g[3], g[1] / g[3], g[2] / g[3]]))
        .collect();
    for _ in 0..n {
        let snapshot = filled.clone();
        let mut changed = false;
        for y in 0..n {
            for x in 0..n {
                if snapshot[y * n + x].is_some() {
                    continue;
                }
                let mut acc = [0.0f32; 3];
                let mut k = 0.0;
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (sx, sy) = (x as i32 + dx, y as i32 + dy);
                    if sx < 0 || sy < 0 || sx as usize >= n || sy as usize >= n {
                        continue;
                    }
                    if let Some(c) = snapshot[sy as usize * n + sx as usize] {
                        for ch in 0..3 {
                            acc[ch] += c[ch];
                        }
                        k += 1.0;
                    }
                }
                if k > 0.0 {
                    filled[y * n + x] = Some(acc.map(|a| a / k));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let field = ColourField {
        n,
        cells: filled
            .into_iter()
            .map(|c| c.unwrap_or([0.3, 0.35, 0.2]))
            .collect(),
    };
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
    for (i, entry) in placed.iter_mut().enumerate() {
        let Some((mesh, _)) = entry else { continue };
        if !terrain(i) {
            continue;
        }
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let pos = pos.clone();
        let Some(VertexAttributeValues::Float32x4(col)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        else {
            continue;
        };
        for (p, c) in pos.iter().zip(col.iter_mut()) {
            if let Some(s) = sample(p[0], p[2]) {
                c[0] = s[0];
                c[1] = s[1];
                c[2] = s[2];
            }
        }
    }
    field
}

/// The terrain's colours over the whole map, a cell [`COLOUR_CELL`] wide,
/// for the floor under the map's holes.
pub struct ColourField {
    n: usize,
    cells: Vec<[f32; 3]>,
}

impl ColourField {
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

/// What the smoothing needs of a map part.
trait TerrainJob {
    fn is_terrain(&self) -> bool;
}

/// Sets a prop's material the way its Dota shader blends: additive glows add,
/// overlays blend, and nothing is metal.
fn dress_prop(m: &mut StandardMaterial, vmat: &Vmat) {
    m.metallic = 0.0;
    if vmat.additive {
        m.alpha_mode = AlphaMode::Add;
        m.unlit = true;
    } else if vmat.translucent && m.alpha_mode == AlphaMode::Opaque {
        m.alpha_mode = AlphaMode::Blend;
    }
}

/// Map meshes sharing a material and a vertex layout, merged in world space.
struct Batch {
    material: Handle<StandardMaterial>,
    /// The map chunk the batch's meshes are centred in.
    chunk: (i32, i32),
    layout: Vec<bevy::mesh::MeshVertexAttributeId>,
    terrain: bool,
    mesh: Mesh,
}

/// World units of a side of the map chunks meshes are merged within, so the
/// camera and the shadows leave out what they do not see.
const BATCH_CHUNK: f32 = 2048.0;

/// Vertices a merged map mesh holds at most before another one is begun.
const BATCH_VERTICES: usize = 1 << 20;

/// Bakes a mesh's transform into it and appends it to the batch of its
/// material and layout.
fn add_to_batches(
    batches: &mut Vec<Batch>,
    mesh: Mesh,
    aabb: bevy::camera::primitives::Aabb,
    material: Handle<StandardMaterial>,
    terrain: bool,
) {
    let mut layout: Vec<_> = mesh.attributes().map(|(a, _)| a.id).collect();
    layout.sort();
    let chunk = (
        (aabb.center.x / BATCH_CHUNK) as i32,
        (aabb.center.z / BATCH_CHUNK) as i32,
    );
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
        }
        None => batches.push(Batch {
            material,
            chunk,
            layout,
            terrain,
            mesh,
        }),
    }
}

/// A mesh cut down to the vertices its triangles use: the exporter gives a
/// world piece the whole vertex buffer of its mesh set.
fn compact(mesh: &Mesh) -> Option<Mesh> {
    let indices: Vec<u32> = mesh.indices()?.iter().map(|i| i as u32).collect();
    let count = mesh.count_vertices();
    let mut remap = vec![u32::MAX; count];
    let mut kept: Vec<u32> = Vec::new();
    let mut out_indices = Vec::with_capacity(indices.len());
    for i in indices {
        let slot = remap.get_mut(i as usize)?;
        if *slot == u32::MAX {
            *slot = kept.len() as u32;
            kept.push(i);
        }
        out_indices.push(*slot);
    }
    // The map's source meshes stay on the CPU; what is made of them is drawn.
    let mut out = Mesh::new(
        mesh.primitive_topology(),
        bevy::asset::RenderAssetUsages::default(),
    );
    for (attribute, values) in mesh.attributes() {
        let picked = pick(values, &kept)?;
        out.insert_attribute(*attribute, picked);
    }
    out.insert_indices(Indices::U32(out_indices));
    Some(out)
}

/// The values of an attribute at the given vertices, in that order.
fn pick(values: &VertexAttributeValues, kept: &[u32]) -> Option<VertexAttributeValues> {
    fn at<T: Copy>(v: &[T], kept: &[u32]) -> Vec<T> {
        kept.iter().map(|i| v[*i as usize]).collect()
    }
    Some(match values {
        VertexAttributeValues::Float32x2(v) => VertexAttributeValues::Float32x2(at(v, kept)),
        VertexAttributeValues::Float32x3(v) => VertexAttributeValues::Float32x3(at(v, kept)),
        VertexAttributeValues::Float32x4(v) => VertexAttributeValues::Float32x4(at(v, kept)),
        VertexAttributeValues::Uint16x4(v) => VertexAttributeValues::Uint16x4(at(v, kept)),
        VertexAttributeValues::Unorm8x4(v) => VertexAttributeValues::Unorm8x4(at(v, kept)),
        VertexAttributeValues::Uint8x4(v) => VertexAttributeValues::Uint8x4(at(v, kept)),
        _ => return None,
    })
}

/// A mesh with its transform applied to its positions, normals and tangents,
/// its indices widened to 32 bits.
fn bake_transform(mut mesh: Mesh, global: &GlobalTransform) -> Mesh {
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
struct LayerMeans {
    root: Option<std::path::PathBuf>,
    known: std::collections::HashMap<String, [f32; 3]>,
    /// Time spent decoding, for the startup log.
    spent: std::time::Duration,
}

impl LayerMeans {
    fn new(root: Option<std::path::PathBuf>) -> LayerMeans {
        LayerMeans {
            root,
            known: std::collections::HashMap::new(),
            spent: std::time::Duration::ZERO,
        }
    }

    /// The mean linear colour of a Dota texture path's exported PNG; white
    /// when it cannot be read.
    fn of(&mut self, vtex: &str) -> [f32; 3] {
        let name = vtex
            .rsplit('/')
            .next()
            .unwrap_or(vtex)
            .trim_end_matches(".vtex")
            .to_string();
        if let Some(mean) = self.known.get(&name) {
            return *mean;
        }
        let began = std::time::Instant::now();
        let mean = self
            .root
            .as_ref()
            .and_then(|root| {
                std::fs::read(root.join("textures/ground").join(format!("{name}.png"))).ok()
            })
            .and_then(|bytes| mean_color(&bytes))
            .unwrap_or([0.5; 3]);
        self.known.insert(name, mean);
        self.spent += began.elapsed();
        mean
    }

    /// The four layers of a terrain material, each its tint times its
    /// texture's mean colour.
    fn layers(&mut self, vmat: &Vmat) -> [[f32; 3]; 4] {
        [0, 1, 2, 3].map(|i| {
            let mean = self.of(&vmat.textures[i]);
            [0, 1, 2].map(|c| vmat.tints[i][c] * mean[c])
        })
    }

    /// The mean of the detail texture the terrain material tiles.
    fn detail(&mut self, manifest: &Manifest) -> f32 {
        let path = manifest
            .files
            .iter()
            .find(|(name, _)| name == "ground.grass")
            .map(|(_, path)| path.clone());
        let mean = path
            .and_then(|p| manifest.root.as_ref().map(|r| r.join(p)))
            .and_then(|p| std::fs::read(p).ok())
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

/// The material every baked terrain mesh is drawn with: a grey detail
/// texture tinted by the baked vertex colours.
fn terrain_material(server: &AssetServer, manifest: &Manifest) -> StandardMaterial {
    let detail = manifest.file("ground.grass").map(|path| {
        server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    anisotropy_clamp: 8,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path)
    });
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
struct Vmat {
    /// Dota's path of the material.
    name: String,
    shader: String,
    /// Whether the material adds its light rather than covering.
    additive: bool,
    /// Whether the material lets what is behind show through.
    translucent: bool,
    /// Whether the material is a decal laid on other surfaces.
    overlay: bool,
    /// Each layer's tint, the mean of its two tints, linear RGB.
    tints: [[f32; 3]; 4],
    /// Each layer's colour texture, as Dota names it.
    textures: [String; 4],
}

impl Vmat {
    fn parse(json: &str) -> Vmat {
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
                *tint.get_mut(c).expect("three channels") = srgb_to_linear((a[c] + b[c]) * 0.5);
            }
        }
        let textures = [0, 1, 2, 3].map(|layer| {
            vmat["TextureParams"][format!("g_tColor{layer}").as_str()]
                .as_str()
                .unwrap_or_default()
                .to_string()
        });
        let flag = |key: &str| vmat["IntParams"][key].as_i64().unwrap_or(0) != 0;
        Vmat {
            name: vmat["Name"].as_str().unwrap_or_default().to_string(),
            shader,
            additive: flag("F_ADDITIVE_BLEND"),
            translucent: flag("F_TRANSLUCENT") || flag("F_OVERLAY") || flag("F_BLEND_MODE"),
            overlay: flag("F_OVERLAY"),
            tints,
            textures,
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

/// Writes one terrain mesh's vertex colours: its four layers blended by the
/// vertex's weights, each in its layer's tint, under the painted tint.
fn bake_terrain(mesh: &mut Mesh, layers: &[[f32; 3]; 4], detail: f32, gain: f32) {
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
    let colors: Vec<[f32; 4]> = weights
        .iter()
        .zip(&alphas)
        .zip(&painted)
        .map(|((w, a), p)| {
            let w1 = modulate(w[0], a[0]);
            let w2 = modulate(w[1], a[1]).min(1.0 - w1);
            let w3 = modulate(w[2], a[2]).min(1.0 - w1 - w2);
            let w0 = (1.0 - w1 - w2 - w3).max(0.0);
            let mut c = [0.0, 0.0, 0.0, 1.0];
            for ch in 0..3 {
                let t = layers;
                let layered = w0 * t[0][ch] + w1 * t[1][ch] + w2 * t[2][ch] + w3 * t[3][ch];
                // Dota doubles its terrain's light; the detail texture's
                // own brightness is taken back out.
                c[ch] = (layered * srgb_to_linear(p[ch]) * gain / detail).min(1.5);
            }
            c
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
}

/// Heights gathered from terrain vertices, cell by cell.
struct Samples {
    n: usize,
    sum: Vec<f32>,
    count: Vec<u32>,
}

impl Samples {
    fn new(n: usize) -> Samples {
        Samples {
            n,
            sum: vec![0.0; n * n],
            count: vec![0; n * n],
        }
    }

    /// Adds every vertex of a mesh, placed by its transform.
    fn add(&mut self, mesh: &Mesh, global: &GlobalTransform) {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return;
        };
        let affine = global.affine();
        for p in positions {
            let w = affine.transform_point3(Vec3::from(*p));
            let (cx, cy) = ((w.x / CELL).floor(), (-w.z / CELL).floor());
            if cx < 0.0 || cy < 0.0 || cx >= self.n as f32 || cy >= self.n as f32 {
                continue;
            }
            let at = cy as usize * self.n + cx as usize;
            self.sum[at] += w.y;
            self.count[at] += 1;
        }
    }

    /// The ground, every cell without a vertex taking its neighbours'
    /// height.
    fn finish(self) -> Ground {
        let n = self.n;
        let mut heights: Vec<Option<f32>> = self
            .sum
            .iter()
            .zip(&self.count)
            .map(|(s, c)| (*c > 0).then(|| s / *c as f32))
            .collect();
        for _ in 0..n {
            let mut changed = false;
            let snapshot = heights.clone();
            for cy in 0..n {
                for cx in 0..n {
                    if snapshot[cy * n + cx].is_some() {
                        continue;
                    }
                    let near: Vec<f32> = [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)]
                        .iter()
                        .filter_map(|(dx, dy)| {
                            let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                            (x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n)
                                .then(|| snapshot[y as usize * n + x as usize])
                                .flatten()
                        })
                        .collect();
                    if !near.is_empty() {
                        heights[cy * n + cx] = Some(near.iter().sum::<f32>() / near.len() as f32);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        Ground {
            n,
            heights: heights.into_iter().map(|h| h.unwrap_or(128.0)).collect(),
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
    // Under a map from the cache it only shows where the map was cut off.
    let sink = if manifest.map.is_some() { 40.0 } else { 0.0 };
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            p[1] -= sink;
        }
    }
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
    if manifest.map.is_some() {
        return;
    }
    let heights = terrain::Heights::new(core.terrain(), n);
    let cells = (0..n * n)
        .map(|i| {
            let (cx, cy) = (i % n, i / n);
            heights.at((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL)
        })
        .collect();
    *ground = Ground { n, heights: cells };
}
