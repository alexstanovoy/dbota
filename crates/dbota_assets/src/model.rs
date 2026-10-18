//! A compiled model (`.vmdl_c`): its skeleton and the render meshes of its most detailed level
//! of detail, in glTF's frame (Y up, metres; Source is Z up, inches).

use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::resource::Resource;
use crate::vbib::{self, Buffer};

/// Inches to metres.
const SCALE: f32 = 0.0254;
/// Source's Z-up frame to glTF's Y-up one: a rotation taking (x, y, z) to (y, z, x).
const TO_GLTF: [f32; 4] = [-0.5, -0.5, -0.5, 0.5];

pub struct Model {
    pub bones: Vec<Bone>,
    pub meshes: Vec<Mesh>,
}

/// A bone's rest pose relative to its parent, in glTF's frame.
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

pub struct Mesh {
    /// The embedded mesh's name (`shadow_fiend_base_lod0`).
    pub name: String,
    pub vertices: Vec<Vertices>,
    pub primitives: Vec<Primitive>,
}

/// One vertex buffer, decoded and converted.
#[derive(Default)]
pub struct Vertices {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub tangents: Option<Vec<[f32; 4]>>,
    pub uvs: Vec<Vec<[f32; 2]>>,
    pub skin: Option<Skin>,
}

/// Per vertex, four model skeleton bones and their weights (summing to 1).
pub struct Skin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// One draw call: triangles over one of the mesh's vertex buffers with one material.
pub struct Primitive {
    pub vertices: usize,
    pub indices: Vec<u32>,
    /// The material's path (`materials/.../x.vmat`, without `_c`).
    pub material: String,
}

impl Model {
    pub fn read(resource: &Resource) -> Result<Self> {
        let data = resource
            .data()
            .ok_or_else(|| malformed("model", "no DATA"))??;
        let ctrl = resource
            .kv3(b"CTRL")
            .ok_or_else(|| malformed("model", "no CTRL"))??
            .root;
        let bones = skeleton(&data)?;
        let masks = integers(data.get("m_refLODGroupMasks"));
        let lowest = masks
            .iter()
            .fold(0, |all, m| all | m)
            .trailing_zeros()
            .min(63);
        let remap = Remap {
            starts: integers(data.get("m_remappingTableStarts")),
            table: integers(data.get("m_remappingTable")),
        };
        let mut meshes = Vec::new();
        for (i, embedded) in array(ctrl.get("embedded_meshes")).iter().enumerate() {
            let index = number(embedded, &["m_nMeshIndex", "mesh_index"]).unwrap_or(i as i64);
            let in_lod = usize::try_from(index)
                .ok()
                .and_then(|i| masks.get(i))
                .is_none_or(|mask| mask >> lowest & 1 != 0);
            if in_lod {
                let joints = remap.of(index, bones.len());
                meshes.push(read_mesh(resource, embedded, joints.as_deref())?);
            }
        }
        Ok(Self { bones, meshes })
    }
}

pub(crate) fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).unwrap_or_default()
}

fn integers(value: Option<&Value>) -> Vec<i64> {
    array(value).iter().filter_map(Value::as_i64).collect()
}

/// The first of `keys` present as an integer (newer and older models name fields differently).
fn number(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|k| value.get(k).and_then(Value::as_i64))
}

/// Per mesh, the model bones its vertex bone indices stand for.
struct Remap {
    starts: Vec<i64>,
    table: Vec<i64>,
}

impl Remap {
    fn of(&self, mesh: i64, bones: usize) -> Option<Vec<u16>> {
        let i = usize::try_from(mesh).ok()?;
        let start = usize::try_from(*self.starts.get(i)?).ok()?;
        let end = self
            .starts
            .get(i + 1)
            .and_then(|&e| usize::try_from(e).ok())
            .unwrap_or(self.table.len());
        let slice = self.table.get(start..end)?;
        // An index the table cannot map clamps to bone 0, as in Valve's reader.
        Some(
            slice
                .iter()
                .map(|&b| {
                    u16::try_from(b)
                        .ok()
                        .filter(|&b| usize::from(b) < bones)
                        .unwrap_or(0)
                })
                .collect(),
        )
    }
}

/// A bone's rest pose relative to its parent as the model stores it (Source's frame, inches).
pub(crate) struct SourceBone {
    pub name: String,
    pub parent: Option<usize>,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// The model skeleton of DATA's `m_modelSkeleton`, in bone order. A parent index that does not
/// come before its bone makes the bone a root.
pub(crate) fn source_skeleton(data: &Value) -> Result<Vec<SourceBone>> {
    let skeleton = data.get("m_modelSkeleton");
    let get = |key| array(skeleton.and_then(|s| s.get(key)));
    let (names, parents) = (get("m_boneName"), get("m_nParent"));
    let (positions, rotations) = (get("m_bonePosParent"), get("m_boneRotParent"));
    let mut bones = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let parent = parents
            .get(i)
            .and_then(Value::as_i64)
            .and_then(|p| usize::try_from(p).ok())
            .filter(|&p| p < i);
        bones.push(SourceBone {
            name: name.as_str().unwrap_or_default().to_owned(),
            parent,
            position: floats::<3>(positions.get(i))?,
            rotation: floats::<4>(rotations.get(i))?,
        });
    }
    Ok(bones)
}

/// A bone-local transform in glTF's frame: the translation scaled to metres and, on a root
/// bone, the frame change baked in (children inherit it through the hierarchy).
pub(crate) fn to_gltf(position: [f32; 3], rotation: [f32; 4], root: bool) -> ([f32; 3], [f32; 4]) {
    let translation = position.map(|v| v * SCALE);
    if root {
        (rotate(TO_GLTF, translation), multiply(TO_GLTF, rotation))
    } else {
        (translation, rotation)
    }
}

fn skeleton(data: &Value) -> Result<Vec<Bone>> {
    Ok(source_skeleton(data)?
        .into_iter()
        .map(|bone| {
            let (translation, rotation) =
                to_gltf(bone.position, bone.rotation, bone.parent.is_none());
            Bone {
                name: bone.name,
                parent: bone.parent,
                translation,
                rotation,
            }
        })
        .collect())
}

pub(crate) fn floats<const N: usize>(value: Option<&Value>) -> Result<[f32; N]> {
    let items = array(value);
    let mut out = [0.0; N];
    if items.len() != N {
        return Err(malformed(
            "model",
            format!("a bone vector of {}", items.len()),
        ));
    }
    for (o, v) in out.iter_mut().zip(items) {
        *o = v.as_f64().unwrap_or(0.0) as f32;
    }
    Ok(out)
}

/// Rotates `v` by the unit quaternion `q` (x, y, z, w).
pub(crate) fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// The quaternion product `a * b` (apply `b`, then `a`).
pub(crate) fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// One embedded mesh: its buffers (from `CTRL`, or an older binary `VBIB` block) and the draw
/// calls of its `MDAT`/`DATA` block.
fn read_mesh(resource: &Resource, embedded: &Value, joints: Option<&[u16]>) -> Result<Mesh> {
    let name = ["m_Name", "name"]
        .iter()
        .find_map(|k| embedded.get(k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let block = |keys: &[&str]| {
        number(embedded, keys)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| resource.blocks().get(i))
    };
    let (vertex_buffers, index_buffers) = match block(&["m_nVBIBBlock", "vbib_block"]) {
        Some(vbib) => vbib::from_vbib(resource.bytes(vbib))?,
        None => (
            buffers(resource, embedded.get("m_vertexBuffers"), true)?,
            buffers(resource, embedded.get("m_indexBuffers"), false)?,
        ),
    };
    let data = block(&["m_nDataBlock", "data_block"])
        .ok_or_else(|| malformed("model", format!("mesh {name} has no data block")))?;
    let data = kv3::decode(resource.bytes(data))?.root;
    let weight_count = data
        .get("m_skeleton")
        .and_then(|s| s.get("m_nBoneWeightCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let vertices = vertex_buffers
        .iter()
        .map(|b| convert(b, joints.filter(|_| weight_count > 0), weight_count))
        .collect::<Result<_>>()?;
    let mut primitives = Vec::new();
    for object in array(data.get("m_sceneObjects")) {
        for call in array(object.get("m_drawCalls")) {
            primitives.push(draw_call(call, &index_buffers, &vertex_buffers)?);
        }
    }
    Ok(Mesh {
        name,
        vertices,
        primitives,
    })
}

fn buffers(resource: &Resource, list: Option<&Value>, vertex: bool) -> Result<Vec<Buffer>> {
    array(list)
        .iter()
        .map(|desc| vbib::from_ctrl(resource, desc, vertex))
        .collect()
}

fn draw_call(
    call: &Value,
    index_buffers: &[Buffer],
    vertex_buffers: &[Buffer],
) -> Result<Primitive> {
    let int = |v: Option<&Value>| {
        v.and_then(Value::as_i64)
            .and_then(|v| usize::try_from(v).ok())
    };
    let field = |key| int(call.get(key)).ok_or_else(|| malformed("draw call", format!("no {key}")));
    let handle = |key: &str| {
        let slot = call.get(key);
        let slot = slot
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .or(slot);
        int(slot.and_then(|s| s.get("m_hBuffer"))).unwrap_or(0)
    };
    if let Some(kind) = call.get("m_nPrimitiveType").and_then(Value::as_str)
        && kind != "RENDER_PRIM_TRIANGLES"
    {
        return Err(malformed("draw call", format!("primitive {kind}")));
    }
    let (start, count, base) = (
        field("m_nStartIndex")?,
        field("m_nIndexCount")?,
        field("m_nBaseVertex")?,
    );
    let vertices = handle("m_vertexBuffers");
    let vertex_count = vertex_buffers.get(vertices).map_or(0, |b| b.count);
    let all = index_buffers
        .get(handle("m_indexBuffer"))
        .ok_or_else(|| malformed("draw call", "no index buffer"))?
        .indices()?;
    let range = all
        .get(start..start.saturating_add(count))
        .ok_or_else(|| malformed("draw call", "indices past the buffer"))?;
    let mut indices = Vec::with_capacity(range.len());
    for &i in range {
        let i = i as usize + base;
        if i >= vertex_count {
            return Err(malformed(
                "draw call",
                format!("vertex {i} of {vertex_count}"),
            ));
        }
        indices.push(i as u32);
    }
    let material = call
        .get("m_material")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(Primitive {
        vertices,
        indices,
        material: material.to_owned(),
    })
}

/// A vertex buffer's attributes in glTF's frame; bone indices mapped to model bones.
fn convert(buffer: &Buffer, joints: Option<&[u16]>, weight_count: usize) -> Result<Vertices> {
    let mut out = Vertices::default();
    let position = buffer
        .field("POSITION", 0)
        .ok_or_else(|| malformed("mesh buffer", "no POSITION"))?;
    out.positions = buffer
        .floats(position)?
        .into_iter()
        .map(|v| rotate(TO_GLTF, [v[0], v[1], v[2]].map(|c| c * SCALE)))
        .collect();
    if let Some(normal) = buffer.field("NORMAL", 0) {
        let (normals, tangents) = buffer.normals_tangents(normal)?;
        out.normals = Some(
            normals
                .into_iter()
                .map(|n| rotate(TO_GLTF, fix_zero(n)))
                .collect(),
        );
        let tangents = match (tangents, buffer.field("TANGENT", 0)) {
            (Some(t), _) => Some(t),
            (None, Some(field)) => Some(buffer.floats(field)?),
            (None, None) => None,
        };
        out.tangents = tangents.map(|ts| {
            ts.into_iter()
                .map(|t| {
                    let [x, y, z] = rotate(TO_GLTF, fix_zero([t[0], t[1], t[2]]));
                    [x, y, z, if t[3] < 0.0 { -1.0 } else { 1.0 }]
                })
                .collect()
        });
    }
    for index in 0..8 {
        if let Some(uv) = buffer
            .field("TEXCOORD", index)
            .filter(|f| is_two_component(f.format))
        {
            out.uvs.push(
                buffer
                    .floats(uv)?
                    .into_iter()
                    .map(|v| [v[0], v[1]])
                    .collect(),
            );
        }
    }
    if let (Some(table), Some(indices)) = (joints, buffer.field("BLENDINDICES", 0)) {
        let weights = match buffer
            .field("BLENDWEIGHT", 0)
            .or(buffer.field("BLENDWEIGHTS", 0))
        {
            Some(field) => buffer.floats(field)?,
            None => vec![equal_weights(weight_count); buffer.count],
        };
        let (joints, weights) = buffer
            .joints(indices)?
            .into_iter()
            .zip(weights)
            .map(|(j, w)| {
                merge_duplicates(
                    j.map(|b| table.get(usize::from(b)).copied().unwrap_or(0)),
                    w,
                )
            })
            .unzip();
        out.skin = Some(Skin { joints, weights });
    }
    Ok(out)
}

fn is_two_component(format: u32) -> bool {
    matches!(
        format,
        vbib::R32G32_FLOAT | vbib::R16G16_FLOAT | vbib::R16G16_UNORM | vbib::R16G16_SNORM
    )
}

fn equal_weights(count: usize) -> [f32; 4] {
    let share = 1.0 / count.clamp(1, 4) as f32;
    [0, 1, 2, 3].map(|i| if i < count { share } else { 0.0 })
}

/// A zero vector would make a degenerate frame; glTF wants unit vectors.
fn fix_zero(v: [f32; 3]) -> [f32; 3] {
    if v.iter().all(|c| c.abs() < 1e-6) {
        [0.0, 0.0, 1.0]
    } else {
        vbib::normalize(v)
    }
}

/// Folds repeated bones into their first slot and normalizes the weights to sum to 1, as glTF
/// requires; unused slots get bone 0 and weight 0.
fn merge_duplicates(mut joints: [u16; 4], mut weights: [f32; 4]) -> ([u16; 4], [f32; 4]) {
    for i in 0..4 {
        for j in 0..i {
            if joints[j] == joints[i] && weights[j] > 0.0 && weights[i] > 0.0 {
                weights[j] += weights[i];
                weights[i] = 0.0;
            }
        }
    }
    for i in 0..4 {
        if weights[i] <= 0.0 {
            joints[i] = 0;
            weights[i] = 0.0;
        }
    }
    let sum: f32 = weights.iter().sum();
    if sum > 0.0 {
        weights = weights.map(|w| w / sum);
    } else {
        weights = [1.0, 0.0, 0.0, 0.0];
    }
    (joints, weights)
}
