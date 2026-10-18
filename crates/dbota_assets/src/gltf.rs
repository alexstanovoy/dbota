//! Writes a [`Model`] as binary glTF (`.glb`), laid out as the asset cache's models are: one
//! node per render mesh (`<model>.<mesh>`); with a skeleton, a root node named after the model
//! holding the bones and one skin over all of them; textures as images next to the file.

use std::collections::HashMap;

use serde_json::{Value as Json, json};

use crate::material::{Alpha, Material, orm_uri};
use crate::model::{Mesh, Model, Skin, Vertices, multiply, rotate};

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// What to write besides the model itself.
pub struct GltfOptions<'a> {
    /// The model's path (`models/heroes/pudge/pudge.vmdl_c`): the root node's name and the
    /// prefix of the mesh names.
    pub name: &'a str,
    /// Only these render meshes, when given (the manifest's `meshes=`).
    pub meshes: Option<&'a [&'a str]>,
    /// Whether to write the skeleton and skin: models that animate, or wear onto one that does.
    /// Without it, the meshes stand alone and keep only their vertex bone weights.
    pub skeleton: bool,
    /// Materials by path (`materials/.../x.vmat`).
    pub materials: &'a HashMap<String, Material>,
    /// The image file a texture (`.vtex` path) is written to, relative to the `.glb`.
    pub texture_uri: &'a dyn Fn(&str) -> String,
}

#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Json>,
    accessors: Vec<Json>,
    nodes: Vec<Json>,
    meshes: Vec<Json>,
    materials: Vec<Json>,
    material_index: HashMap<String, usize>,
    textures: Vec<Json>,
    images: Vec<Json>,
    texture_index: HashMap<(String, bool), usize>,
}

/// The model as a `.glb` file.
pub fn write_glb(model: &Model, options: &GltfOptions) -> Vec<u8> {
    let mut b = Builder::default();
    let mut scene = Vec::new();
    if options.skeleton {
        b.skeleton(model, options.name);
        scene.push(0);
    }
    let meshes: Vec<&Mesh> = model
        .meshes
        .iter()
        .filter(|m| {
            options
                .meshes
                .is_none_or(|keep| keep.contains(&m.name.as_str()))
        })
        .collect();
    let skinned = |mesh: &Mesh| mesh.vertices.iter().all(|v| v.skin.is_some());
    let any_skinned = meshes.iter().any(|m| skinned(m));
    let skin = (options.skeleton && any_skinned && !model.bones.is_empty())
        .then(|| b.inverse_binds(model));
    for mesh in meshes {
        let name = format!("{}.{}", options.name, mesh.name);
        let accessors: Vec<Json> = mesh.vertices.iter().map(|v| b.attributes(v)).collect();
        let mut primitives = Vec::new();
        for p in &mesh.primitives {
            let indices = b.indices(&p.indices);
            let material = b.material(&p.material, options);
            primitives.push(json!({
                "attributes": accessors[p.vertices],
                "indices": indices,
                "material": material,
            }));
        }
        b.meshes
            .push(json!({ "name": name, "primitives": primitives }));
        let mut node = json!({ "name": name, "mesh": b.meshes.len() - 1 });
        if skin.is_some() && skinned(mesh) {
            node["skin"] = json!(0);
        }
        b.nodes.push(node);
        scene.push(b.nodes.len() - 1);
    }
    let mut root = json!({
        "asset": { "version": "2.0", "generator": "dbota_assets" },
        "scene": 0,
        "scenes": [{ "nodes": scene }],
        "nodes": b.nodes,
        "meshes": b.meshes,
        "materials": b.materials,
        "accessors": b.accessors,
        "bufferViews": b.views,
        "buffers": [{ "byteLength": b.bin.len() }],
    });
    if let Some(ibm) = skin {
        let joints: Vec<usize> = (1..=model.bones.len()).collect();
        root["skins"] = json!([{ "joints": joints, "inverseBindMatrices": ibm }]);
    }
    if !b.textures.is_empty() {
        root["textures"] = json!(b.textures);
        root["images"] = json!(b.images);
        root["samplers"] = json!([{ "magFilter": 9729, "minFilter": 9987 }]);
    }
    if b.materials.iter().any(|m| m.get("extensions").is_some()) {
        root["extensionsUsed"] = json!(["KHR_materials_specular"]);
    }
    container(&root, &b.bin)
}

fn push_child(node: &mut Json, child: usize) {
    match node.get_mut("children").and_then(Json::as_array_mut) {
        Some(children) => children.push(json!(child)),
        None => node["children"] = json!([child]),
    }
}

/// The `.glb` container: a header, the JSON chunk (space padded) and the binary chunk.
fn container(root: &Json, bin: &[u8]) -> Vec<u8> {
    let mut text = root.to_string().into_bytes();
    text.resize(text.len().next_multiple_of(4), b' ');
    let mut data = bin.to_vec();
    data.resize(data.len().next_multiple_of(4), 0);
    let total = 12 + 8 + text.len() + 8 + data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    for (chunk, kind) in [(&text, b"JSON"), (&data, b"BIN\0")] {
        out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(chunk);
    }
    out
}

impl Builder {
    /// Node 0, named after the model, holding the bones (nodes 1..) in skeleton order.
    fn skeleton(&mut self, model: &Model, name: &str) {
        self.nodes.push(json!({ "name": name }));
        for bone in &model.bones {
            self.nodes.push(json!({
                "name": bone.name,
                "translation": bone.translation,
                "rotation": bone.rotation,
            }));
        }
        for (i, bone) in model.bones.iter().enumerate() {
            let parent = bone.parent.map_or(0, |p| p + 1);
            push_child(&mut self.nodes[parent], i + 1);
        }
    }

    /// Appends `bytes` as a buffer view (4-byte aligned).
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.bin.resize(self.bin.len().next_multiple_of(4), 0);
        let mut view =
            json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    fn accessor(&mut self, view: usize, component: u32, count: usize, kind: &str) -> usize {
        self.accessors.push(json!({
            "bufferView": view, "componentType": component, "count": count, "type": kind,
        }));
        self.accessors.len() - 1
    }

    fn floats<const N: usize>(&mut self, values: &[[f32; N]], kind: &str) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        self.accessor(view, FLOAT, values.len(), kind)
    }

    fn attributes(&mut self, v: &Vertices) -> Json {
        let position = self.floats(&v.positions, "VEC3");
        let (min, max) = bounds(&v.positions);
        self.accessors[position]["min"] = json!(min);
        self.accessors[position]["max"] = json!(max);
        let mut attributes = json!({ "POSITION": position });
        if let Some(normals) = &v.normals {
            attributes["NORMAL"] = json!(self.floats(normals, "VEC3"));
        }
        if let Some(tangents) = &v.tangents {
            attributes["TANGENT"] = json!(self.floats(tangents, "VEC4"));
        }
        for (i, uv) in v.uvs.iter().enumerate() {
            attributes[format!("TEXCOORD_{i}")] = json!(self.floats(uv, "VEC2"));
        }
        if let Some(Skin { joints, weights }) = &v.skin {
            let bytes: Vec<u8> = joints
                .iter()
                .flatten()
                .flat_map(|j| j.to_le_bytes())
                .collect();
            let view = self.view(&bytes, Some(ARRAY_BUFFER));
            attributes["JOINTS_0"] =
                json!(self.accessor(view, UNSIGNED_SHORT, joints.len(), "VEC4"));
            attributes["WEIGHTS_0"] = json!(self.floats(weights, "VEC4"));
        }
        attributes
    }

    fn indices(&mut self, indices: &[u32]) -> usize {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessor(view, UNSIGNED_INT, indices.len(), "SCALAR")
    }

    /// Each bone's inverse world transform at rest, column major.
    fn inverse_binds(&mut self, model: &Model) -> usize {
        let mut world: Vec<([f32; 4], [f32; 3])> = Vec::with_capacity(model.bones.len());
        for bone in &model.bones {
            let pose = match bone.parent.and_then(|p| world.get(p)) {
                Some(&(rotation, translation)) => {
                    let moved = rotate(rotation, bone.translation);
                    let t = [0, 1, 2].map(|i| translation[i] + moved[i]);
                    (multiply(rotation, bone.rotation), t)
                }
                None => (bone.rotation, bone.translation),
            };
            world.push(pose);
        }
        let matrices: Vec<[f32; 16]> = world.iter().map(|&(r, t)| inverse_rigid(r, t)).collect();
        let view = self.view(
            &matrices
                .iter()
                .flatten()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<_>>(),
            None,
        );
        self.accessor(view, FLOAT, matrices.len(), "MAT4")
    }

    fn material(&mut self, path: &str, options: &GltfOptions) -> usize {
        if let Some(&i) = self.material_index.get(path) {
            return i;
        }
        let json = match options.materials.get(path) {
            Some(m) => self.material_json(m, options),
            None => {
                json!({ "name": path.rsplit('/').next().unwrap_or(path).trim_end_matches(".vmat") })
            }
        };
        self.materials.push(json);
        self.material_index
            .insert(path.to_owned(), self.materials.len() - 1);
        self.materials.len() - 1
    }

    fn material_json(&mut self, m: &Material, options: &GltfOptions) -> Json {
        // The image carries the metalness, so the factors stay 1.
        let metal = if m.masks.is_some() { 1.0 } else { 0.0 };
        let mut pbr = json!({ "metallicFactor": metal, "roughnessFactor": 1.0 });
        if let Some(color) = &m.base_color {
            pbr["baseColorTexture"] = json!({ "index": self.texture(color, false, options) });
        }
        let mut out =
            json!({ "name": m.name, "pbrMetallicRoughness": pbr, "extras": { "vmat": m.extras } });
        if let Some(normal) = &m.normal {
            out["normalTexture"] = json!({ "index": self.texture(normal, false, options) });
        }
        if let Some(masks) = &m.masks {
            let orm = self.texture(masks, true, options);
            out["pbrMetallicRoughness"]["metallicRoughnessTexture"] = json!({ "index": orm });
            out["occlusionTexture"] = json!({ "index": orm });
        }
        match m.alpha {
            Alpha::Opaque => {}
            Alpha::Mask => out["alphaMode"] = json!("MASK"),
            Alpha::Blend => out["alphaMode"] = json!("BLEND"),
        }
        if m.double_sided {
            out["doubleSided"] = json!(true);
        }
        if let Some(color) = m.specular_color {
            out["extensions"] =
                json!({ "KHR_materials_specular": { "specularColorFactor": color } });
        }
        out
    }

    /// The texture of `vtex`, or with `orm` the occlusion/roughness/metallic image made from it.
    fn texture(&mut self, vtex: &str, orm: bool, options: &GltfOptions) -> usize {
        let key = (vtex.to_owned(), orm);
        if let Some(&i) = self.texture_index.get(&key) {
            return i;
        }
        let name = vtex.rsplit('/').next().unwrap_or(vtex);
        let uri = (options.texture_uri)(vtex);
        let (name, uri) = if orm {
            (format!("{name}_orm"), orm_uri(&uri))
        } else {
            (name.to_owned(), uri)
        };
        self.images.push(json!({ "name": name, "uri": uri }));
        self.textures
            .push(json!({ "name": name, "sampler": 0, "source": self.images.len() - 1 }));
        self.texture_index.insert(key, self.textures.len() - 1);
        self.textures.len() - 1
    }
}

fn bounds(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in points {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    if points.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

/// The inverse of rotation `r` then translation `t`, as a column-major 4×4 matrix.
fn inverse_rigid(r: [f32; 4], t: [f32; 3]) -> [f32; 16] {
    let inverse = [-r[0], -r[1], -r[2], r[3]];
    let columns =
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|axis| rotate(inverse, axis));
    let moved = rotate(inverse, t);
    let mut m = [0.0; 16];
    for (c, column) in columns.iter().enumerate() {
        m[c * 4..c * 4 + 3].copy_from_slice(column);
    }
    m[12..15].copy_from_slice(&moved.map(|v| -v));
    m[15] = 1.0;
    m
}
