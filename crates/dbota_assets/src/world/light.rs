//! The map's lighting by day and by night, from its `ent_dota_lightinfo` entities.

use crate::error::{Result, malformed};
use crate::kv3::Value;

/// The weather whose lights the map starts with.
const WEATHER: &str = "[PR#]default_";

/// `lighting.txt`: the sun's (`env_global_light`) and the ambient colour and scale by day and by
/// night, as the map's four default lights blend at its centre (each fades out between its
/// `inner_radius` and `outer_radius`), and the seconds the fog controller takes to turn.
/// One line per value: `sun day r g b scale`, `sun night ...`, `ambient day ...`,
/// `ambient night ...`, `turn seconds`. Colours are 0 to 255.
pub fn lighting_txt(entities: &[Value]) -> Result<String> {
    let lights: Vec<(&Value, f64)> = entities
        .iter()
        .filter(|e| class(e) == "ent_dota_lightinfo")
        .filter(|e| {
            e.get("targetname")
                .and_then(Value::as_str)
                .is_some_and(|n| n.starts_with(WEATHER))
        })
        .filter_map(|e| Some((e, weight(e)?)))
        .collect();
    let total: f64 = lights.iter().map(|l| l.1).sum();
    if lights.is_empty() || total <= 0.0 {
        return Err(malformed("lighting", "no default light reaches the centre"));
    }
    let blend = |key: &str, n: usize| -> Result<Vec<f64>> {
        let mut sum = vec![0.0; n];
        for (light, w) in &lights {
            let value = numbers(light, key).filter(|v| v.len() >= n);
            let value = value.ok_or_else(|| malformed("lighting", key))?;
            for (s, v) in sum.iter_mut().zip(&value) {
                *s += v * w / total;
            }
        }
        Ok(sum)
    };
    let turn = entities
        .iter()
        .find(|e| class(e) == "env_fog_controller")
        .and_then(|e| e.get("foglerptime")?.as_f64())
        .ok_or_else(|| malformed("lighting", "no fog controller"))?;
    let mut out = String::from(
        "# Dota's lighting at the map centre: colour 0-255, scale; turn = seconds day takes to become night\n",
    );
    for (name, color, scale) in [
        ("sun", "color", "light_scale"),
        ("ambient", "ambient_color", "ambient_scale"),
    ] {
        for time in ["day", "night"] {
            let c = blend(&format!("{color}_{time}"), 3)?;
            let s = blend(&format!("{scale}_{time}"), 1)?[0];
            out += &format!("{name} {time} {:.1} {:.1} {:.1} {s:.3}\n", c[0], c[1], c[2]);
        }
    }
    out += &format!("turn {turn:.1}\n");
    Ok(out)
}

/// How much `light` counts at the map's centre: 1 within its inner radius, falling to 0 at the
/// outer one.
fn weight(light: &Value) -> Option<f64> {
    let at = numbers(light, "origin")?;
    let distance = at.first()?.hypot(*at.get(1)?);
    let inner = light.get("inner_radius")?.as_f64()?;
    let outer = light.get("outer_radius")?.as_f64()?;
    Some(((outer - distance) / (outer - inner).max(f64::EPSILON)).clamp(0.0, 1.0))
}

/// The numbers of the array (or the one number) `key`.
fn numbers(entity: &Value, key: &str) -> Option<Vec<f64>> {
    let value = entity.get(key)?;
    match value.as_array() {
        Some(list) => list.iter().map(Value::as_f64).collect(),
        None => Some(vec![value.as_f64()?]),
    }
}

fn class(entity: &Value) -> &str {
    entity
        .get("classname")
        .and_then(Value::as_str)
        .unwrap_or("")
}
