//! Writes a [`Model`] as binary glTF (`.glb`), laid out as the asset cache's models are: one
//! node per render mesh (`<model>.<mesh>`); with a skeleton, a root node named after the model
//! holding the bones and one skin over all of them; textures as images next to the file.
//!
//! The model's types are public, so the writer checks what it is given before writing: a model
//! glTF cannot say (an index past its vertices, a bone parented after itself, a number that is
//! not finite, attributes of unequal lengths) or one past [`MAX_GLB`] is an error, never a
//! panic or a file the client's loader refuses.

use std::collections::HashMap;

use serde_json::{Value as Json, json};

use crate::anim::{Clip, Keys};
use crate::error::{Result, malformed};
use crate::export::ExportedTexture;
use crate::material::{Images, Material, convert, extensions_used};
use crate::model::{Mesh, Model, multiply, rotate};
use crate::vertices::{Attribute, Skin, Vertices};

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
/// Largest `.glb` written; the install's largest model makes ~10 MB.
const MAX_GLB: usize = 1 << 30;
/// JSON bytes counted per accessor, view, node or channel besides its names.
const JSON_ITEM: usize = 256;

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
    /// Animations of the skeleton (written only with it).
    pub clips: &'a [Clip],
}

#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Json>,
    accessors: Vec<Json>,
    nodes: Vec<Json>,
    meshes: Vec<Json>,
    materials: Vec<Json>,
    /// Materials by path and tint (its bits).
    material_index: HashMap<(String, [u32; 4]), usize>,
    images: Images,
    /// Keyframe time accessors by their bits, shared by the samplers with the same times.
    time_index: HashMap<Vec<u32>, usize>,
}

/// The model as a `.glb` file, and the images its materials use (to be written next to it).
pub fn write_glb(model: &Model, options: &GltfOptions) -> Result<(Vec<u8>, Vec<ExportedTexture>)> {
    check(model, options)?;
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
        let node = b.mesh_node(mesh, options, skin.is_some() && skinned(mesh))?;
        scene.push(node);
    }
    let animations = b.animations(options);
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
    if !animations.is_empty() {
        root["animations"] = json!(animations);
    }
    if let Some(ibm) = skin {
        let joints: Vec<usize> = (1..=model.bones.len()).collect();
        root["skins"] = json!([{ "joints": joints, "inverseBindMatrices": ibm }]);
    }
    if !b.images.list.is_empty() {
        root["textures"] = json!(textures(&b.images.list));
        root["images"] = json!(images(&b.images.list));
        root["samplers"] = json!([sampler()]);
    }
    let extensions = extensions_used(&b.materials);
    if !extensions.is_empty() {
        root["extensionsUsed"] = json!(extensions);
    }
    Ok((container(&root, &b.bin), b.images.list))
}

/// The one sampler: linear, mipmapped, repeating.
pub(crate) fn sampler() -> Json {
    json!({ "magFilter": 9729, "minFilter": 9987 })
}

/// glTF textures, one per image, in the same order.
pub(crate) fn textures(list: &[ExportedTexture]) -> Vec<Json> {
    (0..list.len())
        .map(|i| json!({ "sampler": 0, "source": i }))
        .collect()
}

/// glTF images: each named after its file.
pub(crate) fn images(list: &[ExportedTexture]) -> Vec<Json> {
    list.iter()
        .map(|t| json!({ "name": t.uri, "uri": t.uri }))
        .collect()
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

    /// `values` as an accessor of vertex attributes.
    fn floats<const N: usize>(&mut self, values: &[[f32; N]], kind: &str) -> usize {
        self.floats_for(values, kind, Some(ARRAY_BUFFER))
    }

    fn floats_for<const N: usize>(
        &mut self,
        values: &[[f32; N]],
        kind: &str,
        target: Option<u32>,
    ) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let view = self.view(&bytes, target);
        self.accessor(view, FLOAT, values.len(), kind)
    }

    /// `attribute` as an accessor of as many components as it has.
    fn attribute(&mut self, attribute: &Attribute) -> usize {
        let width = attribute.width.clamp(1, 4);
        let bytes: Vec<u8> = attribute
            .values
            .iter()
            .flat_map(|v| &v[..width])
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let kind = ["SCALAR", "VEC2", "VEC3", "VEC4"][width - 1];
        self.accessor(view, FLOAT, attribute.values.len(), kind)
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
        for extra in &v.extra {
            attributes[&extra.name] = json!(self.attribute(extra));
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

    /// A node with the render mesh `mesh` (skinned with skin 0 when `skinned`); its index.
    fn mesh_node(&mut self, mesh: &Mesh, options: &GltfOptions, skinned: bool) -> Result<usize> {
        let name = format!("{}.{}", options.name, mesh.name);
        let accessors: Vec<Json> = mesh.vertices.iter().map(|v| self.attributes(v)).collect();
        let mut primitives = Vec::new();
        for p in &mesh.primitives {
            let indices = self.indices(&p.indices);
            let mut primitive = json!({ "attributes": accessors[p.vertices], "indices": indices });
            if let Some(material) = self.material(&p.material, p.tint, options)? {
                primitive["material"] = json!(material);
            }
            primitives.push(primitive);
        }
        self.meshes
            .push(json!({ "name": name, "primitives": primitives }));
        let mut node = json!({ "name": name, "mesh": self.meshes.len() - 1 });
        if skinned {
            node["skin"] = json!(0);
        }
        self.nodes.push(node);
        Ok(self.nodes.len() - 1)
    }

    /// The clips, with the skeleton they animate.
    fn animations(&mut self, options: &GltfOptions) -> Vec<Json> {
        if !options.skeleton {
            return Vec::new();
        }
        options.clips.iter().map(|c| self.animation(c)).collect()
    }

    /// A clip as a glTF animation: per bone (node `bone + 1`), its rotation, translation and
    /// scale channels.
    fn animation(&mut self, clip: &Clip) -> Json {
        let mut channels = Vec::new();
        let mut samplers = Vec::new();
        for (bone, track) in clip.tracks.iter().enumerate() {
            let paths = [
                ("rotation", self.sampler(&track.rotation, "VEC4")),
                ("translation", self.sampler(&track.translation, "VEC3")),
                ("scale", self.sampler(&track.scale, "VEC3")),
            ];
            for (path, sampler) in paths {
                channels.push(json!({
                    "sampler": samplers.len(),
                    "target": { "node": bone + 1, "path": path },
                }));
                samplers.push(sampler);
            }
        }
        json!({ "name": clip.name, "channels": channels, "samplers": samplers })
    }

    fn sampler<const N: usize>(&mut self, keys: &Keys<N>, kind: &str) -> Json {
        let bits: Vec<u32> = keys.times.iter().map(|t| t.to_bits()).collect();
        let input = match self.time_index.get(&bits) {
            Some(&i) => i,
            None => {
                let times: Vec<[f32; 1]> = keys.times.iter().map(|&t| [t]).collect();
                let i = self.floats_for(&times, "SCALAR", None);
                let first = keys.times.first().copied().unwrap_or(0.0);
                self.accessors[i]["min"] = json!([first]);
                self.accessors[i]["max"] = json!([keys.times.last().copied().unwrap_or(0.0)]);
                self.time_index.insert(bits, i);
                i
            }
        };
        let output = self.floats_for(&keys.values, kind, None);
        json!({ "input": input, "output": output, "interpolation": "LINEAR" })
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

    /// The material `path` drawn with the draw call's `tint`, made on first use; none when
    /// the file is missing.
    fn material(
        &mut self,
        path: &str,
        tint: Option<[f32; 4]>,
        options: &GltfOptions,
    ) -> Result<Option<usize>> {
        let tint = tint.unwrap_or([1.0; 4]);
        let key = (path.to_owned(), tint.map(f32::to_bits));
        if let Some(&i) = self.material_index.get(&key) {
            return Ok(Some(i));
        }
        let Some(material) = options.materials.get(path) else {
            return Ok(None);
        };
        let converted = convert(material, tint, &mut self.images, options.texture_uri)?;
        self.materials.push(converted.to_json(|image| image));
        self.material_index.insert(key, self.materials.len() - 1);
        Ok(Some(self.materials.len() - 1))
    }
}

fn bad(detail: impl Into<String>) -> crate::Error {
    malformed("glTF", detail)
}

fn finite<const N: usize>(values: &[[f32; N]]) -> bool {
    values.iter().flatten().all(|v| v.is_finite())
}

/// Whether the model and options hold only what glTF can say, within [`MAX_GLB`].
fn check(model: &Model, options: &GltfOptions) -> Result<()> {
    let mut size = options.name.len() + JSON_ITEM;
    for (i, bone) in model.bones.iter().enumerate() {
        if bone.parent.is_some_and(|p| p >= i) {
            return Err(bad(format!("bone {i} has parent {:?}", bone.parent)));
        }
        if !finite(&[bone.translation]) || !finite(&[bone.rotation]) {
            return Err(bad(format!("bone {i} is not finite")));
        }
        size += bone.name.len() + 64 + JSON_ITEM;
    }
    let kept = model.meshes.iter().filter(|m| {
        options
            .meshes
            .is_none_or(|keep| keep.contains(&m.name.as_str()))
    });
    let mut made = std::collections::HashSet::new();
    for mesh in kept {
        size += options.name.len() + mesh.name.len() + JSON_ITEM;
        for v in &mesh.vertices {
            size += check_vertices(v, options.skeleton.then_some(model.bones.len()))?;
        }
        for (i, p) in mesh.primitives.iter().enumerate() {
            let count = mesh.vertices.get(p.vertices).map(Vertices::len);
            let count = count.ok_or_else(|| bad(format!("{}: draw call {i}", mesh.name)))?;
            if p.indices.iter().any(|&v| v as usize >= count) {
                return Err(bad(format!(
                    "{}: an index past {count} vertices",
                    mesh.name
                )));
            }
            if !p.tint.is_none_or(|t| finite(&[t])) {
                return Err(bad(format!("{}: draw call {i}'s tint", mesh.name)));
            }
            size += p.indices.len() * 4 + p.material.len() + 2 * JSON_ITEM;
            // A material is written once per tint it is drawn with.
            let key = (
                p.material.as_str(),
                p.tint.unwrap_or([1.0; 4]).map(f32::to_bits),
            );
            if let Some(m) = options.materials.get(&p.material)
                && made.insert(key)
            {
                size += m.json_size();
            }
        }
    }
    if options.skeleton {
        for clip in options.clips {
            size += check_clip(clip, model.bones.len())?;
        }
    }
    if size > MAX_GLB {
        return Err(bad(format!("more than {MAX_GLB} bytes")));
    }
    Ok(())
}

/// The bytes a vertex set takes, when its attributes are all as long as its positions and
/// finite, and its joints (when the skeleton is written, of `bones` bones) are bones.
fn check_vertices(v: &Vertices, bones: Option<usize>) -> Result<usize> {
    let n = v.len();
    let same = |len: usize| len == n;
    let lengths = v.normals.as_ref().is_none_or(|a| same(a.len()))
        && v.tangents.as_ref().is_none_or(|a| same(a.len()))
        && v.uvs.iter().all(|a| same(a.len()))
        && v.extra.iter().all(|a| same(a.values.len()))
        && v.skin
            .as_ref()
            .is_none_or(|s| same(s.joints.len()) && same(s.weights.len()));
    if !lengths {
        return Err(bad("attributes of unequal lengths"));
    }
    let numbers = finite(&v.positions)
        && v.normals.as_deref().is_none_or(finite)
        && v.tangents.as_deref().is_none_or(finite)
        && v.uvs.iter().all(|a| finite(a))
        && v.extra.iter().all(|a| finite(&a.values))
        && v.skin.as_ref().is_none_or(|s| finite(&s.weights));
    if !numbers {
        return Err(bad("an attribute that is not finite"));
    }
    let mut names = std::collections::HashSet::new();
    for a in &v.extra {
        if !(1..=4).contains(&a.width) || !extra_name(&a.name, a.width) || !names.insert(&a.name) {
            return Err(bad(format!("attribute {:?} of width {}", a.name, a.width)));
        }
    }
    if let (Some(bones), Some(skin)) = (bones, &v.skin)
        && skin
            .joints
            .iter()
            .flatten()
            .any(|&j| usize::from(j) >= bones.max(1))
    {
        return Err(bad(format!("a joint past {bones} bones")));
    }
    let extra: usize = v.extra.iter().map(|a| a.name.len() + JSON_ITEM).sum();
    Ok(n * (12 + 12 + 16 + 8 * v.uvs.len() + 16 * v.extra.len() + 24) + extra + 4 * JSON_ITEM)
}

/// Whether an attribute glTF does not define for the writer may be called `name`: one of an
/// application's own (`_NAME`), or a vertex colour of three or four components.
fn extra_name(name: &str, width: usize) -> bool {
    let color = name
        .strip_prefix("COLOR_")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    (name.len() > 1 && name.starts_with('_')) || (color && width >= 3)
}

/// The bytes a clip takes, when it animates at most `bones` bones with keyframes glTF can
/// hold: at least one, as many values as times, increasing finite times.
fn check_clip(clip: &crate::anim::Clip, bones: usize) -> Result<usize> {
    if clip.tracks.len() > bones {
        return Err(bad(format!(
            "clip {} of {} bones",
            clip.name,
            clip.tracks.len()
        )));
    }
    fn keys<const N: usize>(keys: &Keys<N>) -> Option<usize> {
        let times = &keys.times;
        let ok = !times.is_empty()
            && times.len() == keys.values.len()
            && times.iter().all(|t| t.is_finite() && *t >= 0.0)
            && times.windows(2).all(|w| w[0] < w[1])
            && finite(&keys.values);
        ok.then(|| times.len() * 4 * (N + 1) + 4 * JSON_ITEM)
    }
    let mut size = clip.name.len() + JSON_ITEM;
    for track in &clip.tracks {
        let sizes = [
            keys(&track.rotation),
            keys(&track.translation),
            keys(&track.scale),
        ];
        for s in sizes {
            size += s.ok_or_else(|| bad(format!("clip {}: its keyframes", clip.name)))?;
        }
    }
    Ok(size)
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
