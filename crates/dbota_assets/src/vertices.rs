//! One vertex buffer's attributes in glTF's frame (Y up, metres; Source is Z up, inches), named
//! as Source2Viewer names them: `POSITION`, `NORMAL`, `TANGENT`, `TEXCOORD_n` for the
//! two-component texture coordinates, `COLOR_0`, and `_<SEMANTIC>_n` for everything else (the
//! terrain's blend weights are `_TEXCOORD_1` to `_TEXCOORD_3`).

use std::collections::HashMap;

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::model::rotate;
use crate::vbib::{self, Buffer, Field};

/// Inches to metres.
pub(crate) const SCALE: f32 = 0.0254;
/// Source's Z-up frame to glTF's Y-up one: a rotation taking (x, y, z) to (y, z, x).
pub(crate) const TO_GLTF: [f32; 4] = [-0.5, -0.5, -0.5, 0.5];
/// How far a normal or tangent may be from unit length before it counts as broken (the glTF
/// validator's threshold).
const UNIT_LENGTH: f32 = 0.00674;

/// One vertex buffer, decoded and converted.
#[derive(Clone, Default)]
pub struct Vertices {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub tangents: Option<Vec<[f32; 4]>>,
    /// `TEXCOORD_0`, `TEXCOORD_1`, ...
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// The other attributes, in buffer order.
    pub extra: Vec<Attribute>,
    pub skin: Option<Skin>,
}

/// An attribute glTF does not define, or one it does beyond the above (`COLOR_0`).
#[derive(Clone)]
pub struct Attribute {
    pub name: String,
    /// Components per vertex, 1 to 4; the unused ones of `values` are 0.
    pub width: usize,
    pub values: Vec<[f32; 4]>,
}

/// Per vertex, four model skeleton bones and their weights (summing to 1).
#[derive(Clone)]
pub struct Skin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

impl Vertices {
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// A vertex buffer's attributes in glTF's frame; bone indices mapped to model bones through
/// `joints` when the mesh has `weight_count` weights per vertex. What each attribute takes
/// while it is read is charged to `budget` first: a small file may list one attribute many
/// times over a large buffer.
pub(crate) fn convert(
    buffer: &Buffer,
    joints: Option<&[u16]>,
    weight_count: usize,
    budget: &mut Budget,
) -> Result<Vertices> {
    let mut out = Vertices::default();
    let mut fields: Vec<&Field> = buffer.fields.iter().collect();
    fields.sort_by_key(|f| (f.index, f.offset));
    let mut counters: HashMap<String, u32> = HashMap::new();
    // Read as four floats a vertex, kept as up to four.
    let per_attribute = buffer.count.saturating_mul(2 * size_of::<[f32; 4]>());
    for field in fields {
        budget.bytes(per_attribute)?;
        if matches!(
            field.semantic.as_str(),
            "BLENDINDICES" | "BLENDWEIGHT" | "BLENDWEIGHTS"
        ) {
            continue;
        }
        if field.semantic == "NORMAL" {
            let (normals, tangents) = buffer.normals_tangents(field)?;
            out.normals = Some(normals.into_iter().map(direction).collect());
            if let Some(tangents) = tangents {
                out.tangents = Some(tangents.into_iter().map(tangent).collect());
            }
            continue;
        }
        let width = vbib::components(field.format).ok_or_else(|| {
            malformed(
                "mesh buffer",
                format!("{} format {}", field.semantic, field.format),
            )
        })?;
        let name = attribute_name(field, width, &mut counters)?;
        let values = buffer.floats(field)?.into_iter();
        match (name.as_str(), width) {
            ("POSITION", 3) => {
                out.positions = values
                    .map(|v| finite3(position([v[0], v[1], v[2]])))
                    .collect();
            }
            ("TANGENT", 4) => out.tangents = Some(values.map(tangent).collect()),
            (uv, 2) if uv.starts_with("TEXCOORD_") => {
                out.uvs
                    .push(values.map(|v| [v[0], v[1]].map(finite_or_zero)).collect());
            }
            ("POSITION" | "TANGENT", _) => {
                return Err(malformed(
                    "mesh buffer",
                    format!("{name} of {width} components"),
                ));
            }
            _ => out.extra.push(Attribute {
                name,
                width,
                values: values.map(|v| v.map(finite_or_zero)).collect(),
            }),
        }
    }
    if let (Some(table), Some(indices)) = (joints, buffer.field("BLENDINDICES", 0)) {
        budget.bytes(per_attribute.saturating_mul(2))?;
        out.skin = Some(skin(buffer, table, indices, weight_count)?);
    }
    Ok(out)
}

/// Source2Viewer's attribute name of `field`; `counters` counts the names given so far.
fn attribute_name(
    field: &Field,
    width: usize,
    counters: &mut HashMap<String, u32>,
) -> Result<String> {
    let semantic = field.semantic.as_str();
    let mut name = match semantic {
        "TEXCOORD" if width == 2 => "TEXCOORD".to_owned(),
        "COLOR" if field.index == 0 => "COLOR".to_owned(),
        "COLOR" => "_COLOR".to_owned(),
        "POSITION" | "TANGENT" => semantic.to_owned(),
        _ => format!("_{semantic}"),
    };
    // glTF has no scalar attributes of its own.
    if width == 1 && !name.starts_with('_') {
        name.insert(0, '_');
    }
    let counter = counters.entry(name.clone()).or_default();
    let count = *counter;
    *counter += 1;
    if field.index > 0 && name.starts_with('_') {
        Ok(format!("{name}_{}", field.index))
    } else if matches!(semantic, "TEXCOORD" | "COLOR") {
        Ok(format!("{name}_{count}"))
    } else if count > 0 {
        Err(malformed("mesh buffer", format!("{semantic} twice")))
    } else {
        Ok(name)
    }
}

/// A position in glTF's frame.
pub(crate) fn position(v: [f32; 3]) -> [f32; 3] {
    rotate(TO_GLTF, v.map(|c| c * SCALE))
}

/// A unit vector in glTF's frame; one that is not of unit length becomes straight down.
fn direction(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let unit = if (length - 1.0).abs() > UNIT_LENGTH {
        [0.0, 0.0, -1.0]
    } else {
        v
    };
    finite3(rotate(TO_GLTF, unit))
}

/// A tangent (w the bitangent's sign) in glTF's frame, fixed as [`direction`] fixes normals.
fn tangent(t: [f32; 4]) -> [f32; 4] {
    let [x, y, z] = direction([t[0], t[1], t[2]]);
    [x, y, z, finite_or_zero(t[3])]
}

/// NaN and infinities become 0, as glTF requires finite values.
fn finite_or_zero(c: f32) -> f32 {
    if c.is_finite() { c } else { 0.0 }
}

fn finite3(v: [f32; 3]) -> [f32; 3] {
    v.map(finite_or_zero)
}

fn skin(buffer: &Buffer, table: &[u16], indices: &Field, weight_count: usize) -> Result<Skin> {
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
    Ok(Skin { joints, weights })
}

fn equal_weights(count: usize) -> [f32; 4] {
    let share = 1.0 / count.clamp(1, 4) as f32;
    [0, 1, 2, 3].map(|i| if i < count { share } else { 0.0 })
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
