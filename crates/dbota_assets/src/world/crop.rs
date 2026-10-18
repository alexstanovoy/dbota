//! The mid-lane band of a [`Scene`], as `tools/crop_world.py` cut the exported world: the nodes
//! whose mesh bounds touch the band, less the ones the client spawns itself, flattened to world
//! matrices, without skins; a triangle list that uses less than half of its vertex buffer gets
//! its own vertices.

use std::collections::HashMap;
use std::f64::consts::SQRT_2;

use serde_json::{Value as Json, json};

use super::scene::{Bounds, Scene, SceneMesh, overlay_positions};
use crate::error::{Result, malformed};
use crate::gltf::{images, sampler, textures};
use crate::material::extensions_used;
use crate::model::{Mesh, Model};
use crate::vertices::{SCALE, Vertices};

/// Node names left out: trees, towers, barracks, ancients and fountains (the client spawns
/// those as units), the end-game cameras.
const EXCLUDED: [&str; 16] = [
    "tree_",
    "dire_tree",
    "radiant_tower",
    "dire_tower",
    "radiant_ancient001",
    "dire_ancient_base001",
    "radiant_melee_barracks001",
    "radiant_ranged_barracks001",
    "dire_barracks_melee001",
    "dire_barracks_ranged001",
    "radiant_fountain002",
    "dire_fountain002",
    "radiant_statue001",
    "dire_column001",
    "bad_ancient_destruction_camera",
    "radiant_endcam",
];
/// Most bytes of geometry written.
const MAX_BIN: usize = 1 << 30;
/// Most indices and vertices visited while writing (the band of Dota's map takes ~2e7): many
/// pieces over one large buffer must not cost without bound before the output fills.
const MAX_WORK: usize = 1 << 31;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// The band around the mid lane, in Dota units: `|x - y| / √2 <= half_width` across it and
/// `v_min <= (x + y) / √2 <= v_max` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub half_width: f64,
    pub v_min: f64,
    pub v_max: f64,
}

/// Whether a node is left out by name: the client's own units and the world's lower-detail
/// layers (`n0_lr1_...` and up).
fn excluded(name: &str) -> bool {
    let layer = name
        .strip_prefix("n0_lr")
        .and_then(|rest| rest.bytes().next())
        .is_some_and(|d| (b'1'..=b'9').contains(&d));
    layer || EXCLUDED.iter().any(|p| name.starts_with(p))
}

/// Whether `bounds` placed by `matrix` (glTF, column major) touch `band`.
fn touches(matrix: &[f32; 16], bounds: &Bounds, band: &Band) -> bool {
    let m = matrix.map(f64::from);
    let (mut u, mut v) = (
        [f64::INFINITY, f64::NEG_INFINITY],
        [f64::INFINITY, f64::NEG_INFINITY],
    );
    for corner in 0..8 {
        let [x, y, z] = [0, 1, 2].map(|i| f64::from(bounds[(corner >> (2 - i)) & 1][i]));
        let wx = m[0] * x + m[4] * y + m[8] * z + m[12];
        let wz = m[2] * x + m[6] * y + m[10] * z + m[14];
        let (dota_x, dota_y) = (wz / f64::from(SCALE), wx / f64::from(SCALE));
        let (cu, cv) = ((dota_x - dota_y) / SQRT_2, (dota_x + dota_y) / SQRT_2);
        u = [u[0].min(cu), u[1].max(cu)];
        v = [v[0].min(cv), v[1].max(cv)];
    }
    u[0] <= band.half_width && u[1] >= -band.half_width && v[0] <= band.v_max && v[1] >= band.v_min
}

/// The cropped world: the `.gltf` document (its buffer named `bin_uri`) and its geometry.
pub(super) struct Cropped {
    pub doc: Json,
    pub bin: Vec<u8>,
    /// Scene material indices, in the document's order.
    pub materials: Vec<usize>,
    /// Scene image indices, in the document's order.
    pub images: Vec<usize>,
}

/// Writes the nodes of `scene` that `band` keeps; `load` gives a scene model's decoded model.
pub(super) fn crop(
    scene: &Scene,
    band: &Band,
    bin_uri: &str,
    load: &mut dyn FnMut(usize) -> Result<std::rc::Rc<Model>>,
) -> Result<Cropped> {
    let mut w = Writer::default();
    let mut nodes = Vec::new();
    let mut mesh_index: HashMap<usize, usize> = HashMap::new();
    for node in &scene.nodes {
        let mesh = &scene.meshes[node.mesh];
        let Some(bounds) = mesh.bounds else {
            continue;
        };
        if excluded(&node.name) || !touches(&node.matrix, &bounds, band) {
            continue;
        }
        let index = match mesh_index.get(&node.mesh) {
            Some(&i) => i,
            None => {
                let model = load(mesh.model)?;
                let source = model
                    .meshes
                    .get(mesh.mesh)
                    .ok_or_else(|| malformed("world", format!("{}: no mesh", mesh.name)))?;
                let json = w.mesh(mesh, source)?;
                w.meshes.push(json);
                mesh_index.insert(node.mesh, w.meshes.len() - 1);
                w.meshes.len() - 1
            }
        };
        nodes.push(json!({ "name": node.name, "matrix": node.matrix, "mesh": index }));
    }
    let (materials, image_order) = w.materials(scene);
    let list: Vec<_> = image_order
        .iter()
        .map(|&i| scene.images.list[i].clone())
        .collect();
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "dbota_assets" },
        "scene": 0,
        "scenes": [{ "name": "mid", "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": w.meshes,
        "accessors": w.accessors,
        "bufferViews": w.views,
        "buffers": [{ "byteLength": w.bin.len().next_multiple_of(4), "uri": bin_uri }],
        "materials": materials,
    });
    if !list.is_empty() {
        doc["textures"] = json!(textures(&list));
        doc["images"] = json!(images(&list));
        doc["samplers"] = json!([sampler()]);
    }
    let used = extensions_used(doc["materials"].as_array().map_or(&[], Vec::as_slice));
    if !used.is_empty() {
        doc["extensionsUsed"] = json!(used);
    }
    let mut bin = w.bin;
    bin.resize(bin.len().next_multiple_of(4), 0);
    Ok(Cropped {
        doc,
        bin,
        materials: w.material_order,
        images: image_order,
    })
}

#[derive(Default)]
struct Writer {
    bin: Vec<u8>,
    views: Vec<Json>,
    accessors: Vec<Json>,
    meshes: Vec<Json>,
    /// Accessors of whole vertex buffers by (accessor set, buffer), shared by the primitives
    /// that use most of their buffer: all but the positions, and the positions (an overlay's
    /// lifted positions are its own).
    shared: HashMap<(usize, usize), Json>,
    shared_positions: HashMap<(usize, usize), usize>,
    material_order: Vec<usize>,
    material_index: HashMap<usize, usize>,
    work: usize,
}

impl Writer {
    fn mesh(&mut self, mesh: &SceneMesh, source: &Mesh) -> Result<Json> {
        let mut primitives = Vec::new();
        for p in &mesh.primitives {
            let call = source
                .primitives
                .get(p.draw_call)
                .ok_or_else(|| malformed("world", format!("{}: draw call", mesh.name)))?;
            let vertices = source
                .vertices
                .get(call.vertices)
                .ok_or_else(|| malformed("world", format!("{}: vertex buffer", mesh.name)))?;
            self.work += call.indices.len() + if p.overlay { vertices.len() } else { 0 };
            if self.work > MAX_WORK {
                return Err(malformed("world", "too much geometry to write"));
            }
            let lifted = p.overlay.then(|| overlay_positions(vertices));
            let set = (mesh.set, call.vertices);
            let mut out = self.primitive(vertices, lifted, &call.indices, set)?;
            if let Some(material) = p.material {
                let next = self.material_order.len();
                let index = *self.material_index.entry(material).or_insert(next);
                if index == next {
                    self.material_order.push(material);
                }
                out["material"] = json!(index);
            }
            primitives.push(out);
        }
        Ok(json!({ "name": mesh.name, "primitives": primitives }))
    }

    /// A triangle list over `vertices` (positions replaced by `lifted` when given): its own
    /// vertices, in the order its indices first use them, unless it uses at least half of them.
    fn primitive(
        &mut self,
        vertices: &Vertices,
        lifted: Option<Vec<[f32; 3]>>,
        indices: &[u32],
        set: (usize, usize),
    ) -> Result<Json> {
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut kept: Vec<usize> = Vec::new();
        let local: Vec<u32> = indices
            .iter()
            .map(|&i| {
                *remap.entry(i).or_insert_with(|| {
                    kept.push(i as usize);
                    kept.len() as u32 - 1
                })
            })
            .collect();
        let positions = lifted.as_deref().unwrap_or(&vertices.positions);
        if kept.len() * 2 < vertices.len() {
            let indices = self.indices(&local, kept.len() < 1 << 16)?;
            let mut attributes = self.attributes(vertices, &kept)?;
            attributes["POSITION"] = json!(self.positions(positions, &kept)?);
            return Ok(json!({ "attributes": attributes, "mode": 4, "indices": indices }));
        }
        let all: Vec<usize> = (0..vertices.len()).collect();
        let mut attributes = match self.shared.get(&set) {
            Some(shared) => shared.clone(),
            None => {
                let shared = self.attributes(vertices, &all)?;
                self.shared.insert(set, shared.clone());
                shared
            }
        };
        attributes["POSITION"] = json!(match (&lifted, self.shared_positions.get(&set)) {
            (None, Some(&accessor)) => accessor,
            _ => {
                let accessor = self.positions(positions, &all)?;
                if lifted.is_none() {
                    self.shared_positions.insert(set, accessor);
                }
                accessor
            }
        });
        let indices = self.indices(indices, false)?;
        Ok(json!({ "attributes": attributes, "mode": 4, "indices": indices }))
    }

    /// The attributes but the positions of vertices `kept`.
    fn attributes(&mut self, v: &Vertices, kept: &[usize]) -> Result<Json> {
        let mut out = json!({});
        if let Some(normals) = &v.normals {
            out["NORMAL"] = json!(self.floats(normals, kept, "VEC3")?);
        }
        if let Some(tangents) = &v.tangents {
            out["TANGENT"] = json!(self.floats(tangents, kept, "VEC4")?);
        }
        for (i, uv) in v.uvs.iter().enumerate() {
            out[format!("TEXCOORD_{i}")] = json!(self.floats(uv, kept, "VEC2")?);
        }
        for extra in &v.extra {
            let width = extra.width.clamp(1, 4);
            let picked: Vec<f32> = kept
                .iter()
                .filter_map(|&i| extra.values.get(i))
                .flat_map(|value| value[..width].iter().copied())
                .collect();
            if picked.len() != kept.len() * width {
                return Err(malformed(
                    "world",
                    "an attribute shorter than its positions",
                ));
            }
            let kind = ["SCALAR", "VEC2", "VEC3", "VEC4"][width - 1];
            let view = self.view(&floats_bytes(&picked), ARRAY_BUFFER)?;
            out[&extra.name] = json!(self.accessor(view, FLOAT, kept.len(), kind));
        }
        Ok(out)
    }

    fn positions(&mut self, positions: &[[f32; 3]], kept: &[usize]) -> Result<usize> {
        let accessor = self.floats(positions, kept, "VEC3")?;
        let picked: Vec<[f32; 3]> = kept
            .iter()
            .filter_map(|&i| positions.get(i).copied())
            .collect();
        if let Some([min, max]) = super::scene::bounds_of(&picked) {
            self.accessors[accessor]["min"] = json!(min);
            self.accessors[accessor]["max"] = json!(max);
        }
        Ok(accessor)
    }

    fn floats<const N: usize>(
        &mut self,
        values: &[[f32; N]],
        kept: &[usize],
        kind: &str,
    ) -> Result<usize> {
        let picked: Vec<f32> = kept
            .iter()
            .filter_map(|&i| values.get(i))
            .flatten()
            .copied()
            .collect();
        if picked.len() != kept.len() * N {
            return Err(malformed(
                "world",
                "an attribute shorter than its positions",
            ));
        }
        let view = self.view(&floats_bytes(&picked), ARRAY_BUFFER)?;
        Ok(self.accessor(view, FLOAT, kept.len(), kind))
    }

    /// Indices as 16-bit numbers when `short`, else 32-bit.
    fn indices(&mut self, indices: &[u32], short: bool) -> Result<usize> {
        let bytes: Vec<u8> = if short {
            indices
                .iter()
                .flat_map(|&i| (i as u16).to_le_bytes())
                .collect()
        } else {
            indices.iter().flat_map(|i| i.to_le_bytes()).collect()
        };
        let view = self.view(&bytes, ELEMENT_ARRAY_BUFFER)?;
        let component = if short { UNSIGNED_SHORT } else { UNSIGNED_INT };
        Ok(self.accessor(view, component, indices.len(), "SCALAR"))
    }

    fn view(&mut self, bytes: &[u8], target: u32) -> Result<usize> {
        let at = self.bin.len().next_multiple_of(4);
        if at + bytes.len() > MAX_BIN {
            return Err(malformed(
                "world",
                format!("more than {MAX_BIN} bytes of geometry"),
            ));
        }
        self.bin.resize(at, 0);
        self.bin.extend_from_slice(bytes);
        self.views.push(json!({
            "buffer": 0, "byteOffset": at, "byteLength": bytes.len(), "target": target,
        }));
        Ok(self.views.len() - 1)
    }

    fn accessor(&mut self, view: usize, component: u32, count: usize, kind: &str) -> usize {
        self.accessors.push(json!({
            "bufferView": view, "componentType": component, "count": count, "type": kind,
        }));
        self.accessors.len() - 1
    }

    /// The kept materials as glTF, and the scene images they use, in first-use order.
    fn materials(&self, scene: &Scene) -> (Vec<Json>, Vec<usize>) {
        let mut order: Vec<usize> = Vec::new();
        let mut index: HashMap<usize, usize> = HashMap::new();
        let mut list = Vec::new();
        for &m in &self.material_order {
            let gltf = &scene.materials[m].gltf;
            let slots = [
                gltf.base_color_texture,
                gltf.metallic_roughness_texture,
                gltf.normal_texture,
                gltf.occlusion_texture,
                gltf.emissive_texture,
                gltf.specular_texture,
            ];
            for image in slots.into_iter().flatten() {
                index.entry(image).or_insert_with(|| {
                    order.push(image);
                    order.len() - 1
                });
            }
            list.push(gltf.to_json(|image| index[&image]));
        }
        (list, order)
    }
}

fn floats_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|f| f.to_le_bytes()).collect()
}
