//! A material (`.vmat_c`, KV3 or NTRO) reduced to what a glTF material carries.

use serde_json::{Map, Value as Json, json};

use crate::kv3::Value;

/// A material as glTF describes it; textures are `.vtex` paths (without `_c`).
pub struct Material {
    /// The file name without extension, as the glTF material's name.
    pub name: String,
    pub base_color: Option<String>,
    pub normal: Option<String>,
    pub alpha: Alpha,
    pub double_sided: bool,
    pub specular_color: Option<[f32; 3]>,
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
        let ints = params(data, "m_intParams", "m_nValue");
        let int = |key: &str| {
            ints.iter()
                .rev()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.as_i64())
                .unwrap_or(0)
        };
        let textures = params(data, "m_textureParams", "m_pValue");
        let texture = |keys: &[&str]| {
            keys.iter().find_map(|key| {
                let found = textures.iter().rev().find(|(k, _)| k == key)?;
                found
                    .1
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
            })
        };
        let vectors = params(data, "m_vectorParams", "m_value");
        let specular_color = vectors
            .iter()
            .rev()
            .find(|(k, _)| k == "g_vSpecularColor")
            .and_then(|(_, v)| v.as_array())
            .filter(|v| v.len() >= 3)
            .map(|v| [0, 1, 2].map(|i| v[i].as_f64().unwrap_or(1.0) as f32));
        let alpha = if int("F_TRANSLUCENT") > 0 {
            Alpha::Blend
        } else if int("F_ALPHA_TEST") > 0 {
            Alpha::Mask
        } else {
            Alpha::Opaque
        };
        let file = path.rsplit('/').next().unwrap_or(path);
        Self {
            name: file.trim_end_matches(".vmat").to_owned(),
            base_color: texture(&["g_tColor", "g_tColor0", "g_tColorA"]),
            normal: texture(&["g_tNormal", "g_tNormal0", "g_tNormalA"]),
            alpha,
            double_sided: int("F_RENDER_BACKFACES") > 0,
            specular_color,
            extras: extras(path, data),
        }
    }
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
