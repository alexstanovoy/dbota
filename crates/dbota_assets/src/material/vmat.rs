//! A material (`.vmat_c`, KV3 or NTRO): its shader and parameters.

use serde_json::{Map, Value as Json, json};

use crate::kv3::Value;

/// A material's shader and parameters, each list in file order with a name's last value at its
/// first place (as a dictionary filled in file order holds them).
pub struct Material {
    /// The material's path (`materials/.../x.vmat`).
    pub path: String,
    /// The file name without extension, as the glTF material's name.
    pub name: String,
    pub shader: String,
    pub ints: Vec<(String, i64)>,
    pub floats: Vec<(String, f32)>,
    pub vectors: Vec<(String, [f32; 4])>,
    /// Texture parameters and their textures (`materials/.../x.vtex`, without `_c`).
    pub textures: Vec<(String, String)>,
    /// The material's parameters as `extras.vmat`: Name, ShaderName, Int/Float/Vector/
    /// TextureParams.
    pub extras: Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alpha {
    Opaque,
    Mask,
    Blend,
}

impl Material {
    /// Reads the material `path` (`materials/.../x.vmat`) from its decoded `DATA`.
    pub fn from_vmat(path: &str, data: &Value) -> Self {
        let file = path.rsplit('/').next().unwrap_or(path);
        Self {
            path: path.to_owned(),
            name: file.trim_end_matches(".vmat").to_owned(),
            shader: data
                .get("m_shaderName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ints: dictionary(params(data, "m_intParams", "m_nValue"), |v| v.as_i64()),
            floats: dictionary(params(data, "m_floatParams", "m_flValue"), |v| {
                v.as_f64().map(|f| f as f32)
            }),
            vectors: dictionary(params(data, "m_vectorParams", "m_value"), |v| {
                let items = v.as_array()?;
                Some(std::array::from_fn(|i| {
                    items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32
                }))
            }),
            textures: dictionary(params(data, "m_textureParams", "m_pValue"), |v| {
                v.as_str().map(str::to_owned)
            }),
            extras: extras(path, data),
        }
    }

    pub fn int(&self, key: &str) -> i64 {
        find(&self.ints, key).copied().unwrap_or(0)
    }

    pub fn float(&self, key: &str) -> Option<f32> {
        find(&self.floats, key).copied()
    }

    pub fn vector(&self, key: &str) -> Option<[f32; 4]> {
        find(&self.vectors, key).copied()
    }
}

fn find<'a, T>(list: &'a [(String, T)], key: &str) -> Option<&'a T> {
    list.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `(name, value)` pairs, each name once: at its first place, with its last value.
fn dictionary<T>(
    pairs: Vec<(String, &Value)>,
    convert: impl Fn(&Value) -> Option<T>,
) -> Vec<(String, T)> {
    let mut out: Vec<(String, T)> = Vec::new();
    for (key, value) in pairs {
        let Some(value) = convert(value) else {
            continue;
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => out.push((key, value)),
        }
    }
    out
}

/// `(name, value)` pairs of one of the material's parameter lists, in file order.
fn params<'a>(data: &'a Value, list: &str, value: &str) -> Vec<(String, &'a Value)> {
    data.get(list)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let name = p.get("m_name")?.as_str()?.to_owned();
            Some((name, p.get(value)?))
        })
        .collect()
}

fn extras(path: &str, data: &Value) -> Json {
    let list = |name: &str, value: &str, convert: fn(&Value) -> Json| -> Json {
        let mut map = Map::new();
        for (key, v) in params(data, name, value) {
            map.insert(key, convert(v));
        }
        Json::Object(map)
    };
    json!({
        "Name": path,
        "ShaderName": data.get("m_shaderName").and_then(Value::as_str).unwrap_or_default(),
        "IntParams": list("m_intParams", "m_nValue", |v| json!(v.as_i64())),
        "FloatParams": list("m_floatParams", "m_flValue", |v| json!(v.as_f64())),
        "VectorParams": list("m_vectorParams", "m_value", |v| {
            json!(v.as_array().unwrap_or_default().iter().map(Value::as_f64).collect::<Vec<_>>())
        }),
        "TextureParams": list("m_textureParams", "m_pValue", |v| json!(v.as_str())),
    })
}
