//! The cropped map read by dbota's own loader and merged as it is read: the
//! `.gltf`'s nodes and materials, the `.bin`'s vertices cut down to what each
//! piece uses, placed in the world and merged into a few meshes per material
//! and chunk. Bevy's glTF loader made a mesh asset, a node asset and their
//! handle infos of every one of the map's fourteen thousand pieces, and held
//! them all with the geometry file while the map was merged.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    camera::primitives::{Aabb, MeshAabb},
    image::{
        ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
        ImageSamplerDescriptor,
    },
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues, VertexFormat},
    prelude::*,
    render::render_resource::Face,
};
use serde_json::Value;

use crate::map::{
    ATTRIBUTE_BLEND_ALPHAS, ATTRIBUTE_BLEND_WEIGHTS, ATTRIBUTE_PAINTED_TINT, Batch, ColourField,
    Ground, LayerMeans, Samples, TerrainJob, Vmat, add_to_batches, bake_terrain, bake_transform,
    chunk_of, detail_settings, dress_prop, map_transform, smooth_terrain, terrain_material_with,
};

/// The map, merged: what [`MapLoader`] makes of the map's file.
#[derive(Asset, TypePath)]
pub struct MapGeometry {
    /// Merged meshes in the world, their materials, and whether terrain.
    pub batches: Vec<Batch>,
    /// The ground's height, from the terrain.
    pub ground: Ground,
    /// The terrain's colours over the whole map, for the floor under it.
    pub field: ColourField,
    /// The terrain's material, for the floor.
    pub terrain: Handle<StandardMaterial>,
}

/// Reads the map's `.gltf` and `.bin` into a [`MapGeometry`].
#[derive(TypePath)]
pub struct MapLoader {
    /// The cache folder, for the ground layers' colours.
    pub root: Option<PathBuf>,
    /// The terrain's detail texture, as an asset path and as a file of the
    /// cache.
    pub detail: Option<String>,
    pub detail_file: Option<String>,
    /// Whether props are kept.
    pub props: bool,
    /// Whether translucent decals are kept.
    pub overlays: bool,
    /// Material names left out (debug).
    pub skipped: Vec<String>,
    /// How bright the baked terrain is.
    pub gain: f32,
    /// Where finished chunks go.
    pub stream: MapStream,
}

/// What a material of the map draws as.
#[derive(Clone)]
enum Kind {
    /// Terrain, its four layers' colours baked into the vertices.
    Terrain([[f32; 3]; 4]),
    /// The water's own material.
    Water,
    /// A prop with its material.
    Prop(Handle<StandardMaterial>),
    /// Nothing drawn.
    Skip,
}

/// The map's materials as the loader draws them.
struct Materials {
    kinds: Vec<Kind>,
    terrain: Handle<StandardMaterial>,
    water: Handle<StandardMaterial>,
    /// The detail texture's mean brightness, taken back out of the terrain.
    detail_mean: f32,
}

/// Where an accessor's elements lie in the geometry file.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    count: usize,
    component: u64,
    width: usize,
}

/// One triangle list to read: where its indices and attributes lie, where
/// it stands, what it draws as, and the chunk it is merged in.
struct Job {
    indices: Span,
    attributes: Vec<(MeshVertexAttribute, Span)>,
    global: GlobalTransform,
    kind: Kind,
    chunk: (i32, i32),
}

/// One piece of the map read, cut and placed in the world.
struct Piece {
    mesh: Mesh,
    aabb: Aabb,
    material: Handle<StandardMaterial>,
    terrain: bool,
    chunk: (i32, i32),
}

/// Merged meshes handed to the main thread as each chunk of the map is
/// done, so they reach the GPU and leave memory while the rest is read.
#[derive(Resource, Clone, Default)]
pub struct MapStream(pub Arc<Mutex<Vec<Batch>>>);

impl AssetLoader for MapLoader {
    type Asset = MapGeometry;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        cx: &mut LoadContext<'_>,
    ) -> Result<MapGeometry, std::io::Error> {
        let mut json = Vec::new();
        reader.read_to_end(&mut json).await?;
        let doc: Value = serde_json::from_slice(&json).map_err(std::io::Error::other)?;
        drop(json);
        let uri = doc["buffers"][0]["uri"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("the map has no external buffer"))?;
        let bin_path = cx
            .path()
            .resolve_embed_str(uri)
            .map_err(std::io::Error::other)?;
        let materials = self.materials(&doc, cx);
        let jobs = jobs(&doc, &materials);
        // The document's tree is let go before the geometry is read: the
        // two together were the load's peak.
        drop(doc);
        let bin = cx
            .read_asset_bytes(bin_path)
            .await
            .map_err(std::io::Error::other)?;
        let terrain = self.stream(jobs, &bin, &materials).await;
        drop(bin);
        Ok(finish(terrain, materials.terrain))
    }

    fn extensions(&self) -> &[&str] {
        &["gltf"]
    }
}

impl MapLoader {
    /// Every material of the map as it draws: the terrain's layers, water,
    /// a prop's material (a labeled asset), or nothing.
    fn materials(&self, doc: &Value, cx: &mut LoadContext) -> Materials {
        let mut means = LayerMeans::new(self.root.clone());
        let detail_mean = means.detail(self.detail_file.as_deref());
        let detail = self.detail.as_ref().map(|path| {
            cx.load_builder()
                .with_settings(detail_settings)
                .load(path.clone())
        });
        let terrain = cx.add_labeled_asset("terrain".to_string(), terrain_material_with(detail));
        let water = cx.add_labeled_asset(
            "water".to_string(),
            StandardMaterial {
                base_color: Color::srgba(0.10, 0.22, 0.26, 0.82),
                perceptual_roughness: 0.15,
                alpha_mode: AlphaMode::Blend,
                ..default()
            },
        );
        let linear = linear_textures(doc);
        let empty = Vec::new();
        let list = doc["materials"].as_array().unwrap_or(&empty);
        let mut kinds = Vec::with_capacity(list.len());
        for (index, material) in list.iter().enumerate() {
            let vmat = Vmat::parse(&material["extras"].to_string());
            kinds.push(match vmat.shader.as_str() {
                "multiblend.vfx" => Kind::Terrain(means.layers(&vmat)),
                "water_dota.vfx" => Kind::Water,
                // Glows, light shafts and fountain sprays are particles in all
                // but name: drawn as plain meshes they cover the ground.
                _ if vmat.additive => Kind::Skip,
                _ if !self.props => Kind::Skip,
                _ if self.skipped.iter().any(|s| vmat.name.contains(s.as_str())) => Kind::Skip,
                _ if !self.overlays && vmat.translucent => Kind::Skip,
                _ => {
                    let mut m = standard_material(doc, material, &linear, cx);
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
                        Kind::Skip
                    } else {
                        dress_prop(&mut m, &vmat);
                        Kind::Prop(cx.add_labeled_asset(format!("material{index}"), m))
                    }
                }
            });
        }
        Materials {
            kinds,
            terrain,
            water,
            detail_mean,
        }
    }

    /// Reads the pieces chunk by chunk, on every core, merging each chunk's
    /// into batches handed to the main thread as soon as the chunk is done.
    /// The terrain is kept back: its colours are made continuous over the
    /// whole map once all of it is read.
    async fn stream(&self, mut jobs: Vec<Job>, bin: &[u8], materials: &Materials) -> Vec<Piece> {
        jobs.sort_by_key(|job| job.chunk);
        let mut terrain = Vec::new();
        let mut rest = &jobs[..];
        while let Some(first) = rest.first() {
            let end = rest
                .iter()
                .position(|j| j.chunk != first.chunk)
                .unwrap_or(rest.len());
            let (chunk, next) = rest.split_at(end);
            rest = next;
            let mut batches = Vec::new();
            for piece in pieces(chunk, bin, materials, self.gain) {
                if piece.terrain {
                    terrain.push(piece);
                } else {
                    add_to_batches(
                        &mut batches,
                        piece.mesh,
                        piece.aabb,
                        piece.material,
                        false,
                        piece.chunk,
                    );
                }
            }
            if let Ok(mut stream) = self.stream.0.lock() {
                stream.append(&mut batches);
            }
            // In a browser this runs on the frame's own thread: the frame
            // must come round to upload the chunk before the next is read.
            crate::cachefs::next_frame().await;
        }
        terrain
    }
}

/// Every triangle list the map draws, as where to read it; the document can
/// go once these are taken.
fn jobs(doc: &Value, materials: &Materials) -> Vec<Job> {
    // The scene entity's turn that glTF's coordinate conversion puts over
    // the nodes, under the map's own placement.
    let root = map_transform().compute_affine()
        * Transform::from_rotation(Quat::from_xyzw(0.0, 1.0, 0.0, 0.0)).compute_affine();
    let empty = Vec::new();
    let names: [(&str, MeshVertexAttribute); 7] = [
        ("POSITION", Mesh::ATTRIBUTE_POSITION),
        ("NORMAL", Mesh::ATTRIBUTE_NORMAL),
        ("TANGENT", Mesh::ATTRIBUTE_TANGENT),
        ("TEXCOORD_0", Mesh::ATTRIBUTE_UV_0),
        ("_TEXCOORD_1", ATTRIBUTE_BLEND_WEIGHTS),
        ("_TEXCOORD_2", ATTRIBUTE_BLEND_ALPHAS),
        ("_TEXCOORD_3", ATTRIBUTE_PAINTED_TINT),
    ];
    let mut jobs = Vec::new();
    for node in doc["nodes"].as_array().unwrap_or(&empty) {
        let Some(mesh) = node["mesh"].as_u64().map(|m| &doc["meshes"][m as usize]) else {
            continue;
        };
        let Some(matrix) = node_matrix(node) else {
            continue;
        };
        let affine = root * bevy::math::Affine3A::from_mat4(matrix);
        let chunk = chunk_of(Vec3::from(affine.translation));
        for primitive in mesh["primitives"].as_array().unwrap_or(&empty) {
            let kind = primitive["material"]
                .as_u64()
                .and_then(|m| materials.kinds.get(m as usize))
                .cloned()
                .unwrap_or(Kind::Skip);
            if matches!(kind, Kind::Skip) || primitive["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let Some(indices) = span(doc, &primitive["indices"]) else {
                continue;
            };
            let attributes = names
                .iter()
                .filter_map(|(name, attribute)| {
                    let span = span(doc, &primitive["attributes"][*name])?;
                    (span.component == 5126).then_some((*attribute, span))
                })
                .collect();
            jobs.push(Job {
                indices,
                attributes,
                global: GlobalTransform::from(affine),
                kind,
                chunk,
            });
        }
    }
    jobs
}

/// The pieces of a run of jobs, read on every core.
fn pieces(jobs: &[Job], bin: &[u8], materials: &Materials, gain: f32) -> Vec<Piece> {
    let detail = materials.detail_mean;
    let pool = bevy::tasks::ComputeTaskPool::get();
    let pieces: Vec<Option<Piece>> = pool.scope(|scope| {
        for job in jobs {
            scope.spawn(async move {
                let mut mesh = read_primitive(job, bin)?;
                let (material, terrain) = match &job.kind {
                    Kind::Terrain(layers) => {
                        bake_terrain(&mut mesh, layers, detail, gain);
                        (materials.terrain.clone(), true)
                    }
                    Kind::Water => (materials.water.clone(), false),
                    Kind::Prop(material) => (material.clone(), false),
                    Kind::Skip => return None,
                };
                for attr in [
                    &ATTRIBUTE_BLEND_WEIGHTS,
                    &ATTRIBUTE_BLEND_ALPHAS,
                    &ATTRIBUTE_PAINTED_TINT,
                ] {
                    mesh.remove_attribute(attr.id);
                }
                let mesh = bake_transform(mesh, &job.global);
                let aabb = mesh.compute_aabb()?;
                Some(Piece {
                    mesh,
                    aabb,
                    material,
                    terrain,
                    chunk: job.chunk,
                })
            });
        }
    });
    pieces.into_iter().flatten().collect()
}

/// The terrain: its heights taken for the ground, its colours made
/// continuous, merged by chunk.
fn finish(terrain: Vec<Piece>, material: Handle<StandardMaterial>) -> MapGeometry {
    let mut samples = Samples::new(crate::terrain::MAP_CELLS);
    for piece in &terrain {
        samples.add(&piece.mesh, &GlobalTransform::IDENTITY);
    }
    let mut placed: Vec<Option<(Mesh, Aabb)>> = Vec::with_capacity(terrain.len());
    let mut marks = Vec::with_capacity(terrain.len());
    for piece in terrain {
        marks.push(piece.chunk);
        placed.push(Some((piece.mesh, piece.aabb)));
    }
    struct Terrain;
    impl TerrainJob for Terrain {
        fn is_terrain(&self) -> bool {
            true
        }
    }
    let all: Vec<Terrain> = marks.iter().map(|_| Terrain).collect();
    let field = smooth_terrain(&all, &mut placed);
    let mut batches = Vec::new();
    for (chunk, piece) in marks.into_iter().zip(placed) {
        if let Some((mesh, aabb)) = piece {
            add_to_batches(&mut batches, mesh, aabb, material.clone(), true, chunk);
        }
    }
    MapGeometry {
        batches,
        ground: samples.finish(),
        field,
        terrain: material,
    }
}

/// A node's matrix: its `matrix`, or its translation, rotation and scale.
fn node_matrix(node: &Value) -> Option<Mat4> {
    let numbers = |key: &str| -> Option<Vec<f32>> {
        node[key].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect()
        })
    };
    if let Some(m) = numbers("matrix") {
        return (m.len() == 16).then(|| Mat4::from_cols_slice(&m));
    }
    let t = numbers("translation").unwrap_or_else(|| vec![0.0; 3]);
    let r = numbers("rotation").unwrap_or_else(|| vec![0.0, 0.0, 0.0, 1.0]);
    let s = numbers("scale").unwrap_or_else(|| vec![1.0; 3]);
    (t.len() == 3 && r.len() == 4 && s.len() == 3).then(|| {
        Mat4::from_scale_rotation_translation(
            Vec3::from_slice(&s),
            Quat::from_xyzw(r[0], r[1], r[2], r[3]),
            Vec3::from_slice(&t),
        )
    })
}

/// Where an accessor's elements lie: float or unsigned index components,
/// tightly packed.
fn span(doc: &Value, index: &Value) -> Option<Span> {
    let acc = &doc["accessors"][index.as_u64()? as usize];
    let view = &doc["bufferViews"][acc["bufferView"].as_u64()? as usize];
    let component = acc["componentType"].as_u64()?;
    let width = match acc["type"].as_str()? {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => return None,
    } * match component {
        5121 => 1,
        5123 => 2,
        5125 | 5126 => 4,
        _ => return None,
    };
    if view["byteStride"]
        .as_u64()
        .is_some_and(|s| s as usize != width)
    {
        return None;
    }
    Some(Span {
        start: view["byteOffset"].as_u64().unwrap_or(0) as usize
            + acc["byteOffset"].as_u64().unwrap_or(0) as usize,
        count: acc["count"].as_u64()? as usize,
        component,
        width,
    })
}

/// A triangle list read from the geometry, cut down to the vertices its
/// indices use, in the order they are first used.
fn read_primitive(job: &Job, bin: &[u8]) -> Option<Mesh> {
    let at = |span: &Span| bin.get(span.start..span.start + span.count * span.width);
    let raw = at(&job.indices)?;
    let index = |i: usize| -> u32 {
        match job.indices.component {
            5121 => u32::from(raw[i]),
            5123 => u32::from(u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]])),
            _ => u32::from_le_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]),
        }
    };
    let vertices = job
        .attributes
        .iter()
        .find(|(a, _)| a.id == Mesh::ATTRIBUTE_POSITION.id)?
        .1
        .count;
    let mut remap = vec![u32::MAX; vertices];
    let mut kept: Vec<u32> = Vec::new();
    let mut indices = Vec::with_capacity(job.indices.count);
    for i in 0..job.indices.count {
        let slot = remap.get_mut(index(i) as usize)?;
        if *slot == u32::MAX {
            *slot = kept.len() as u32;
            kept.push(index(i));
        }
        indices.push(*slot);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    for (attribute, span) in &job.attributes {
        let bytes = at(span)?;
        let values = match attribute.format {
            VertexFormat::Float32x2 => VertexAttributeValues::Float32x2(gather(bytes, &kept)?),
            VertexFormat::Float32x3 => VertexAttributeValues::Float32x3(gather(bytes, &kept)?),
            VertexFormat::Float32x4 => VertexAttributeValues::Float32x4(gather(bytes, &kept)?),
            _ => return None,
        };
        mesh.insert_attribute(*attribute, values);
    }
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// The float vectors of the vertices kept, from tightly packed little-endian
/// floats.
fn gather<const N: usize>(bytes: &[u8], kept: &[u32]) -> Option<Vec<[f32; N]>> {
    kept.iter()
        .map(|&i| {
            let at = i as usize * N * 4;
            let b = bytes.get(at..at + N * 4)?;
            Some(std::array::from_fn(|k| {
                f32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])
            }))
        })
        .collect()
}

/// The textures used as data rather than colour (normal, metallic-roughness
/// and occlusion maps), read without the sRGB curve, as Bevy's loader does.
fn linear_textures(doc: &Value) -> Vec<u64> {
    let empty = Vec::new();
    let mut linear = Vec::new();
    for m in doc["materials"].as_array().unwrap_or(&empty) {
        for t in [
            &m["normalTexture"]["index"],
            &m["occlusionTexture"]["index"],
            &m["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"],
        ] {
            if let Some(t) = t.as_u64() {
                linear.push(t);
            }
        }
    }
    linear
}

/// A texture of the map, loaded with its sampler, on the GPU alone.
fn texture(
    doc: &Value,
    info: &Value,
    linear: &[u64],
    cx: &mut LoadContext,
) -> Option<Handle<Image>> {
    let index = info["index"].as_u64()?;
    let texture = &doc["textures"][index as usize];
    let uri = doc["images"][texture["source"].as_u64()? as usize]["uri"].as_str()?;
    let path = cx.path().resolve_embed_str(uri).ok()?;
    let sampler = &doc["samplers"][texture["sampler"].as_u64().unwrap_or(u64::MAX) as usize];
    let wrap = |key: &str| match sampler[key].as_u64() {
        Some(33071) => ImageAddressMode::ClampToEdge,
        Some(33648) => ImageAddressMode::MirrorRepeat,
        _ => ImageAddressMode::Repeat,
    };
    let filter = |code: Option<u64>, nearest: &[u64]| {
        if code.is_some_and(|c| nearest.contains(&c)) {
            ImageFilterMode::Nearest
        } else {
            ImageFilterMode::Linear
        }
    };
    let min = sampler["minFilter"].as_u64();
    let descriptor = ImageSamplerDescriptor {
        address_mode_u: wrap("wrapS"),
        address_mode_v: wrap("wrapT"),
        mag_filter: filter(sampler["magFilter"].as_u64(), &[9728]),
        min_filter: filter(min, &[9728, 9984, 9986]),
        mipmap_filter: if min.is_none_or(|m| m == 9986 || m == 9987) {
            ImageFilterMode::Linear
        } else {
            ImageFilterMode::Nearest
        },
        ..ImageSamplerDescriptor::linear()
    };
    let srgb = !linear.contains(&index);
    Some(
        cx.load_builder()
            .with_settings(move |s: &mut ImageLoaderSettings| {
                s.is_srgb = srgb;
                s.sampler = ImageSampler::Descriptor(descriptor.clone());
                s.asset_usage = RenderAssetUsages::RENDER_WORLD;
            })
            .load(path),
    )
}

/// A glTF material as Bevy's loader would make its StandardMaterial, for
/// what the map's materials use.
fn standard_material(
    doc: &Value,
    m: &Value,
    linear: &[u64],
    cx: &mut LoadContext,
) -> StandardMaterial {
    let pbr = &m["pbrMetallicRoughness"];
    let factor = |v: &Value, n: usize, default: f32| -> Vec<f32> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|x| x.as_f64().unwrap_or(default as f64) as f32)
                    .collect()
            })
            .filter(|a: &Vec<f32>| a.len() == n)
            .unwrap_or_else(|| vec![default; n])
    };
    let color = factor(&pbr["baseColorFactor"], 4, 1.0);
    let emissive = factor(&m["emissiveFactor"], 3, 0.0);
    let double_sided = m["doubleSided"].as_bool().unwrap_or(false);
    let specular = &m["extensions"]["KHR_materials_specular"];
    let tint = specular["specularColorFactor"]
        .as_array()
        .map(|_| factor(&specular["specularColorFactor"], 3, 1.0));
    StandardMaterial {
        base_color: Color::linear_rgba(color[0], color[1], color[2], color[3]),
        base_color_texture: texture(doc, &pbr["baseColorTexture"], linear, cx),
        perceptual_roughness: pbr["roughnessFactor"].as_f64().unwrap_or(1.0) as f32,
        metallic: pbr["metallicFactor"].as_f64().unwrap_or(1.0) as f32,
        metallic_roughness_texture: texture(doc, &pbr["metallicRoughnessTexture"], linear, cx),
        normal_map_texture: texture(doc, &m["normalTexture"], linear, cx),
        occlusion_texture: texture(doc, &m["occlusionTexture"], linear, cx),
        emissive: LinearRgba::rgb(emissive[0], emissive[1], emissive[2]),
        emissive_texture: texture(doc, &m["emissiveTexture"], linear, cx),
        double_sided,
        cull_mode: if double_sided { None } else { Some(Face::Back) },
        alpha_mode: match m["alphaMode"].as_str() {
            Some("MASK") => AlphaMode::Mask(m["alphaCutoff"].as_f64().unwrap_or(0.5) as f32),
            Some("BLEND") => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        },
        reflectance: specular["specularFactor"].as_f64().unwrap_or(1.0) as f32 * 0.5,
        specular_tint: tint.map_or(Color::WHITE, |t| Color::linear_rgb(t[0], t[1], t[2])),
        ..default()
    }
}
