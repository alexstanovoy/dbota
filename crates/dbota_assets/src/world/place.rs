//! Where the world puts things: Source's placements (a 3 x 4 rotation-and-translation, or an
//! entity's origin, angles and scales) as glTF node matrices, and entities' tints.

use crate::kv3::Value;
use crate::material::srgb_to_linear;
use crate::vertices::SCALE;

/// A placement in Source's frame: rotation-scale rows with the translation as fourth column
/// (a point goes to `R p + t`).
pub(super) type Affine = [[f32; 4]; 3];

pub(super) const IDENTITY: Affine = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// glTF's axis for each of Source's: glTF x is Source y, y is z, z is x.
const AXIS: [usize; 3] = [1, 2, 0];

/// `affine` as a glTF node matrix (column major) placing geometry already in glTF's frame.
pub(super) fn node_matrix(affine: &Affine) -> [f32; 16] {
    let mut m = [0.0; 16];
    for row in 0..3 {
        for col in 0..3 {
            m[col * 4 + row] = affine[AXIS[row]][AXIS[col]];
        }
        m[12 + row] = affine[AXIS[row]][3] * SCALE;
    }
    m[15] = 1.0;
    m
}

/// A placement stored as 3 rows of 4 numbers, nested or flat.
pub(super) fn affine(value: Option<&Value>) -> Option<Affine> {
    let items = value?.as_array()?;
    let numbers: Vec<f32> = match items.first()?.as_array() {
        Some(_) => items
            .iter()
            .flat_map(|row| row.as_array().unwrap_or_default())
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect(),
        None => items
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect(),
    };
    if numbers.len() < 12 {
        return None;
    }
    Some(std::array::from_fn(|r| {
        std::array::from_fn(|c| numbers[r * 4 + c])
    }))
}

/// Whether `affine` collapses space (its rotation-scale part has determinant 0).
pub(super) fn is_flat(a: &Affine) -> bool {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    det == 0.0
}

/// An entity's placement: scaled by `scales`, turned by `angles` (pitch about y, yaw about z,
/// roll about x, in degrees; roll first, yaw last), moved to `origin`.
pub(super) fn entity_affine(entity: &Value) -> Affine {
    let scales = vector3(entity, "scales", [1.0; 3]);
    let [pitch, yaw, roll] = vector3(entity, "angles", [0.0; 3]).map(f32::to_radians);
    let origin = vector3(entity, "origin", [0.0; 3]);
    let rz = rotation(yaw, 0, 1);
    let ry = rotation(pitch, 2, 0);
    let rx = rotation(roll, 1, 2);
    let r = product(&product(&rz, &ry), &rx);
    std::array::from_fn(|i| {
        let mut row = [0.0; 4];
        for (j, cell) in row.iter_mut().take(3).enumerate() {
            *cell = r[i][j] * scales[j];
        }
        row[3] = origin[i];
        row
    })
}

/// The rotation by `angle` that turns axis `a` towards axis `b`.
fn rotation(angle: f32, a: usize, b: usize) -> [[f32; 3]; 3] {
    let (s, c) = angle.sin_cos();
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m[a][a] = c;
    m[b][b] = c;
    m[b][a] = s;
    m[a][b] = -s;
    m
}

fn product(x: &[[f32; 3]; 3], y: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| x[i][k] * y[k][j]).sum()))
}

/// An entity's tint: `rendercolor` (sRGB bytes, white when absent) made linear, with
/// `renderamt` (0 to 1, or to 255) as its opacity.
pub(super) fn entity_tint(entity: &Value) -> [f32; 4] {
    let color = vector3(entity, "rendercolor", [255.0; 3]).map(|c| c / 255.0);
    let mut amount = number(entity.get("renderamt")).unwrap_or(1.0);
    if amount > 1.0 {
        amount /= 255.0;
    }
    let [r, g, b] = srgb_to_linear(color);
    [r, g, b, amount]
}

/// Three numbers of key `key`: an array, or a string of three numbers separated by spaces.
fn vector3(entity: &Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    match entity.get(key) {
        Some(Value::Array(items)) => std::array::from_fn(|i| number(items.get(i)).unwrap_or(0.0)),
        Some(Value::String(text)) => {
            let parts: Vec<f32> = text
                .split(' ')
                .map_while(|p| p.parse::<f32>().ok())
                .collect();
            match parts[..] {
                [x, y, z] if text.split(' ').count() == 3 => [x, y, z],
                _ => default,
            }
        }
        _ => default,
    }
}

fn number(value: Option<&Value>) -> Option<f32> {
    match value? {
        Value::String(text) => text.trim().parse().ok(),
        other => other.as_f64().map(|v| v as f32),
    }
}
