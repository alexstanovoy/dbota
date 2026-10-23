//! The ground's paths, drawn per pixel: Dota paints its terrain with four
//! layers a material, blended by weights on every vertex. The lanes are
//! layers of dirt and paving stones; baked into one vertex colour under one
//! grass texture they all but vanish. Each layer is sorted into a family
//! (ground, dirt or stone); every terrain vertex carries its dirt and stone
//! weights, the shader (`fog.wgsl`) sharpens them per pixel against the
//! layer's texture, as Dota's height blend does, and draws the paths with
//! their own texture, tinted from a map-wide field of the paths' colours.
//! The field's alpha is the shade the forest casts on the ground around it.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{MeshVertexAttribute, VertexAttributeValues, VertexFormat},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::session::Session;

/// What a terrain layer is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    /// Grass, rock and the rest: the baked colour under the grass texture.
    Ground = 0,
    /// Bare earth and sand: the lanes' borders, the open ground.
    Dirt = 1,
    /// Paving stones: the lanes themselves.
    Stone = 2,
}

impl Family {
    /// The family of a layer by its colour texture's name; Dota's files
    /// say nothing else of what a layer is.
    pub(crate) fn of(texture: &str) -> Family {
        let name = texture.rsplit(['/', '\\']).next().unwrap_or(texture);
        if name.starts_with("stone_path") {
            Family::Stone
        } else if ["sand_path", "sand_cracked", "dirt_"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            Family::Dirt
        } else {
            Family::Ground
        }
    }
}

/// A path family's colour on a terrain vertex while the map loads: linear
/// RGB and the family's weight there. Taken off the mesh into the field.
pub(crate) const ATTRIBUTE_DIRT: MeshVertexAttribute =
    MeshVertexAttribute::new("DbotaDirtTint", 0x0d07_a004, VertexFormat::Float32x4);
pub(crate) const ATTRIBUTE_STONE: MeshVertexAttribute =
    MeshVertexAttribute::new("DbotaStoneTint", 0x0d07_a005, VertexFormat::Float32x4);
/// Which of the two stone textures a terrain vertex's stone is drawn with,
/// 0 or 1, while the map loads; taken off the mesh into the field.
pub(crate) const ATTRIBUTE_STONE_SLOT: MeshVertexAttribute =
    MeshVertexAttribute::new("DbotaStoneSlot", 0x0d07_a006, VertexFormat::Float32);

/// World units a cell of the path colour field spans.
pub(crate) const FIELD_CELL: f32 = 32.0;
/// Cells of the field per axis: the whole map and a cell either side.
pub(crate) const FIELD_CELLS: usize = (bota_core::rules::MAP_SIZE as f32 / FIELD_CELL) as usize + 2;
/// The field holds colours up to this bright (the baked terrain's light is
/// doubled, as Dota's is); they are stored divided by it.
pub(crate) const FIELD_RANGE: f32 = 1.5;
/// Texels a side of the path textures as packed.
const DETAIL_SIZE: u32 = 512;

/// The textures the terrain's paths are drawn with: the detail of the dirt
/// and the two stones (red, green and blue, each its texture's brightness
/// against its mean, halved), and the field: dirt colours on its left half,
/// stone colours on its right, the forest's shade in the left half's alpha
/// and which stone in the right half's.
#[derive(Resource, Clone)]
pub struct GroundLayers {
    pub detail: Handle<Image>,
    pub field: Handle<Image>,
}

/// The dirt and the two stone textures the map uses most, packed into one
/// image with its mipmaps; `None` when the cache lacks any.
pub(crate) fn pack_detail(
    root: &std::path::Path,
    dirt: &str,
    stones: &[String; 2],
) -> Option<Image> {
    let read = |vtex: &str| -> Option<Vec<f32>> {
        let name = vtex
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(vtex)
            .trim_end_matches(".vtex");
        let bytes = crate::cachefs::read_small(root, &format!("textures/ground/{name}.png"))?;
        brightness(&bytes)
    };
    let (dirt, first, second) = (read(dirt)?, read(&stones[0])?, read(&stones[1])?);
    let n = DETAIL_SIZE as usize;
    let mut level: Vec<[u8; 4]> = (0..n * n)
        .map(|i| {
            [
                unit_byte(dirt[i]),
                unit_byte(first[i]),
                unit_byte(second[i]),
                255,
            ]
        })
        .collect();
    let mut data: Vec<u8> = level.iter().flatten().copied().collect();
    let mut side = n;
    let mut levels = 1;
    while side > 1 {
        level = halve(&level, side);
        side /= 2;
        levels += 1;
        data.extend(level.iter().flatten());
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: DETAIL_SIZE,
            height: DETAIL_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

/// A brightness from 0 to 1 as a byte.
fn unit_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The next mip level of a square image, a side of `side` texels.
fn halve(level: &[[u8; 4]], side: usize) -> Vec<[u8; 4]> {
    let half = side / 2;
    let mut out = Vec::with_capacity(half * half);
    for y in 0..half {
        for x in 0..half {
            let at = |dx: usize, dy: usize| level[(2 * y + dy) * side + 2 * x + dx];
            let quad = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
            out.push(std::array::from_fn(|c| {
                (quad.iter().map(|t| u32::from(t[c])).sum::<u32>() / 4) as u8
            }));
        }
    }
    out
}

/// A PNG's linear brightness over [`DETAIL_SIZE`] squared texels, each
/// against the mean and halved: 0.5 is the texture's mean.
fn brightness(bytes: &[u8]) -> Option<Vec<f32>> {
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
    if w < DETAIL_SIZE || h < DETAIL_SIZE {
        return None;
    }
    let n = DETAIL_SIZE;
    let (fx, fy) = (w / n, h / n);
    let mut out = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        for x in 0..n {
            let mut sum = 0.0;
            for dy in 0..fy {
                for dx in 0..fx {
                    let c = image.get_color_at(x * fx + dx, y * fy + dy).ok()?;
                    sum += c.luminance();
                }
            }
            out.push(sum / (fx * fy) as f32);
        }
    }
    let mean = (out.iter().sum::<f32>() / out.len() as f32).max(1e-3);
    Some(out.into_iter().map(|v| v / mean * 0.5).collect())
}

/// The path colour field of the terrain's meshes: each vertex's dirt and
/// stone colours, weighted by how much of each it shows, gathered cell by
/// cell, every cell no path reaches taking its neighbours' colour. The
/// colours are taken off the meshes.
pub(crate) fn path_field(meshes: &mut [&mut Mesh]) -> Image {
    let n = FIELD_CELLS;
    let mut halves = [vec![[0.0f32; 4]; n * n], vec![[0.0f32; 4]; n * n]];
    // The stone slot of each cell, weighted by the stone's weight.
    let mut slots = vec![[0.0f32; 2]; n * n];
    for mesh in meshes.iter_mut() {
        splat_slots(mesh, &mut slots, n);
        for (grid, attribute) in halves.iter_mut().zip([&ATTRIBUTE_DIRT, &ATTRIBUTE_STONE]) {
            splat(mesh, attribute, grid, n);
            mesh.remove_attribute(attribute.id);
        }
        mesh.remove_attribute(ATTRIBUTE_STONE_SLOT.id);
    }
    let mut data = vec![0u8; 2 * n * n * 4];
    for (half, grid) in halves.iter().enumerate() {
        let mut cells: Vec<Option<[f32; 3]>> = grid
            .iter()
            .map(|g| (g[3] > 1e-3).then(|| [g[0] / g[3], g[1] / g[3], g[2] / g[3]]))
            .collect();
        // Far enough for the path's edge to sample its own colour; a cell
        // further off shows no path.
        crate::map::fill_holes(&mut cells, n, [(1, 0), (-1, 0), (0, 1), (0, -1)], 6);
        for (i, cell) in cells.iter().enumerate() {
            let c = cell.unwrap_or([0.3; 3]);
            let texel = ((i / n) * 2 * n + half * n + i % n) * 4;
            let srgb =
                Color::linear_rgb(c[0] / FIELD_RANGE, c[1] / FIELD_RANGE, c[2] / FIELD_RANGE)
                    .to_srgba()
                    .to_u8_array();
            data[texel..texel + 3].copy_from_slice(&srgb[..3]);
        }
    }
    let mut slots: Vec<Option<[f32; 1]>> = slots
        .iter()
        .map(|[sum, weight]| (*weight > 1e-3).then(|| [sum / weight]))
        .collect();
    crate::map::fill_holes(&mut slots, n, [(1, 0), (-1, 0), (0, 1), (0, -1)], 6);
    for (i, slot) in slots.iter().enumerate() {
        data[((i / n) * 2 * n + n + i % n) * 4 + 3] = unit_byte(slot.map_or(0.0, |[k]| k));
    }
    let mut image = Image::new(
        Extent3d {
            width: 2 * n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Adds one family's colour of every vertex, weighted, into its grid cell.
fn splat(mesh: &Mesh, attribute: &MeshVertexAttribute, grid: &mut [[f32; 4]], n: usize) {
    let (Some(VertexAttributeValues::Float32x3(pos)), Some(VertexAttributeValues::Float32x4(tint))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(attribute.id),
    ) else {
        return;
    };
    for (p, t) in pos.iter().zip(tint) {
        let (cx, cy) = (p[0] / FIELD_CELL, -p[2] / FIELD_CELL);
        if cx < 0.0 || cy < 0.0 || cx as usize >= n || cy as usize >= n || t[3] <= 0.0 {
            continue;
        }
        let cell = &mut grid[cy as usize * n + cx as usize];
        for ch in 0..3 {
            cell[ch] += t[ch] * t[3];
        }
        cell[3] += t[3];
    }
}

/// Adds every vertex's stone slot, weighted by its stone, into its cell.
fn splat_slots(mesh: &Mesh, slots: &mut [[f32; 2]], n: usize) {
    let (
        Some(VertexAttributeValues::Float32x3(pos)),
        Some(VertexAttributeValues::Float32x4(stone)),
        Some(VertexAttributeValues::Float32(slot)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(ATTRIBUTE_STONE.id),
        mesh.attribute(ATTRIBUTE_STONE_SLOT.id),
    )
    else {
        return;
    };
    for ((p, s), k) in pos.iter().zip(stone).zip(slot) {
        let (cx, cy) = (p[0] / FIELD_CELL, -p[2] / FIELD_CELL);
        if cx < 0.0 || cy < 0.0 || cx as usize >= n || cy as usize >= n {
            continue;
        }
        let cell = &mut slots[cy as usize * n + cx as usize];
        cell[0] += k * s[3];
        cell[1] += s[3];
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, shade_forest);
}

/// World units around a tree its shade reaches.
const SHADE_REACH: f32 = 150.0;

/// Writes the forest's shade into the field's alpha whenever a tree falls or
/// grows back: every standing tree darkens the ground around it, most at its
/// foot.
fn shade_forest(
    session: NonSend<Session>,
    layers: Option<Res<GroundLayers>>,
    mut images: ResMut<Assets<Image>>,
    mut drawn: Local<Option<Vec<u32>>>,
) {
    let (Some(layers), Some(view)) = (layers, session.0.view()) else {
        return;
    };
    let trees = session.0.trees();
    if trees.is_empty() {
        return;
    }
    let mut felled = view.felled_trees.clone();
    felled.sort_unstable();
    if drawn.as_ref() == Some(&felled) {
        return;
    }
    let Some(mut image) = images.get_mut(&layers.field) else {
        return;
    };
    let n = FIELD_CELLS;
    let mut shade = vec![0.0f32; n * n];
    let reach = (SHADE_REACH / FIELD_CELL).ceil() as i32;
    for (i, tree) in trees.iter().enumerate() {
        if felled.binary_search(&(i as u32)).is_ok() {
            continue;
        }
        let (x, y) = (tree.x.to_f32() / FIELD_CELL, tree.y.to_f32() / FIELD_CELL);
        for cy in (y as i32 - reach).max(0)..=(y as i32 + reach).min(n as i32 - 1) {
            for cx in (x as i32 - reach).max(0)..=(x as i32 + reach).min(n as i32 - 1) {
                let d = (cx as f32 + 0.5 - x).hypot(cy as f32 + 0.5 - y) * FIELD_CELL;
                let k = (1.0 - d / SHADE_REACH).max(0.0);
                shade[cy as usize * n + cx as usize] += k * k;
            }
        }
    }
    let Some(data) = image.data.as_mut() else {
        return;
    };
    for (i, s) in shade.iter().enumerate() {
        data[((i / n) * 2 * n + i % n) * 4 + 3] = unit_byte(*s);
    }
    *drawn = Some(felled);
}
