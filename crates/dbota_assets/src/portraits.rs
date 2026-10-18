//! The portrait framings of Dota's `scripts/npc/portraits.txt`: where the HUD portrait's camera
//! and light stand for each model, boiled down to one line per model of the manifest.

use crate::error::{Result, malformed};

/// Deepest nesting read; the file has four levels.
const DEPTH_LIMIT: usize = 8;
/// Values of a framing line, in order: camera position, camera angles, field of view, far plane,
/// light position, light angles, light field of view, light colour, ambient colour.
const VALUES: usize = 3 + 3 + 1 + 1 + 3 + 3 + 1 + 3 + 3;

/// A Valve KV1 value: a string, or a block of keyed values.
enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Self::Block(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Self::Text(_) => None,
        }
    }

    fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Self::Text(t) => Some(t),
            Self::Block(_) => None,
        }
    }
}

struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<Option<(bool, &'a str)>> {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(after) = self.rest.strip_prefix("//") {
                self.rest = after.split_once('\n').map_or("", |(_, tail)| tail);
            } else {
                break;
            }
        }
        let mut chars = self.rest.chars();
        match chars.next() {
            None => Ok(None),
            Some(c @ ('{' | '}')) => {
                let (head, tail) = self.rest.split_at(c.len_utf8());
                self.rest = tail;
                Ok(Some((false, head)))
            }
            Some('"') => {
                let body = &self.rest[1..];
                let end = body
                    .find('"')
                    .ok_or_else(|| malformed("portraits", "an unterminated string"))?;
                self.rest = &body[end + 1..];
                Ok(Some((true, &body[..end])))
            }
            Some(_) => Err(malformed("portraits", "an unquoted token")),
        }
    }
}

fn block(tokens: &mut Tokens<'_>, depth: usize) -> Result<Vec<(String, Kv)>> {
    if depth > DEPTH_LIMIT {
        return Err(malformed("portraits", "blocks nested too deep"));
    }
    let mut items = Vec::new();
    loop {
        let key = match tokens.next()? {
            None if depth == 0 => return Ok(items),
            Some((false, "}")) if depth > 0 => return Ok(items),
            Some((true, key)) => key.to_owned(),
            _ => return Err(malformed("portraits", "a key was expected")),
        };
        let value = match tokens.next()? {
            Some((true, text)) => Kv::Text(text.to_owned()),
            Some((false, "{")) => Kv::Block(block(tokens, depth + 1)?),
            _ => return Err(malformed("portraits", "a value was expected")),
        };
        items.push((key, value));
    }
}

/// The numbers of the entry's default camera and its light, or none when it lacks any.
fn values(entry: &Kv) -> Option<[f32; VALUES]> {
    let camera = entry.get("cameras")?.get("default")?;
    let triple = |from: &Kv, key: &str| -> Option<[f32; 3]> {
        let mut it = from.text(key)?.split_whitespace().map(str::parse::<f32>);
        Some([it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?])
    };
    let single = |from: &Kv, key: &str| from.text(key)?.trim().parse::<f32>().ok();
    let mut out = [0.0; VALUES];
    let parts = [
        triple(camera, "PortraitPosition")?,
        triple(camera, "PortraitAngles")?,
        triple(entry, "PortraitLightPosition")?,
        triple(entry, "PortraitLightAngles")?,
        triple(entry, "PortraitLightColor")?,
        triple(entry, "PortraitAmbientColor")?,
    ];
    let numbers = [
        parts[0].as_slice(),
        parts[1].as_slice(),
        &[single(camera, "PortraitFOV")?],
        // Entries without a far plane see as far as the biggest model needs.
        &[single(camera, "PortraitFar").unwrap_or(10_000.0)],
        parts[2].as_slice(),
        parts[3].as_slice(),
        &[single(entry, "PortraitLightFOV")?],
        parts[4].as_slice(),
        parts[5].as_slice(),
    ];
    for (slot, n) in out.iter_mut().zip(numbers.concat()) {
        if !n.is_finite() {
            return None;
        }
        *slot = n;
    }
    Some(out)
}

/// `models/portraits.txt`: a line per wanted model, `<logical> <numbers...>` (camera position
/// and angles in the model's own inches and degrees, horizontal field of view, far plane, the
/// light's position, angles and field of view, light colour and ambient colour as 0-255), in the order of `wanted`. A model Dota
/// has no framing for gets no line. `wanted` pairs a logical name with the model's `.vmdl` path.
pub fn framings(portraits: &[u8], wanted: &[(&str, &str)]) -> Result<String> {
    let text = std::str::from_utf8(portraits)
        .map_err(|_| malformed("portraits", "the file is not UTF-8"))?;
    let root = block(&mut Tokens { rest: text }, 0)?;
    let root = Kv::Block(root);
    let all = root
        .get("Portraits")
        .ok_or_else(|| malformed("portraits", "no Portraits block"))?;
    let mut out = String::from(
        "# logical, camera position (3), camera angles (3), fov, far, light position (3), light angles (3), light fov, light colour (3), ambient colour (3)\n",
    );
    for (logical, vmdl) in wanted {
        let Some(numbers) = all.get(vmdl).and_then(values) else {
            continue;
        };
        out.push_str(logical);
        for n in numbers {
            out.push_str(&format!(" {n}"));
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::framings;

    const FILE: &str = r#"
"Portraits" // comment
{
    "default" { "PortraitLightAngles" "1 2 3" }
    "models/a.vmdl"
    {
        "PortraitLightPosition" "1 2 3"
        "PortraitLightAngles" "49.61 263.39 0.00"
        "PortraitLightFOV" "90"
        "PortraitLightColor" "231 254 187"
        "PortraitAmbientColor" "97 114 0"
        "cameras" { "default" { "PortraitPosition" "162.31 63.73 144.54" "PortraitAngles" "2.75 199.94 0" "PortraitFOV" "25" } }
    }
}"#;

    #[test]
    fn a_model_gets_its_default_camera_and_light() {
        let out = framings(
            FILE.as_bytes(),
            &[("a", "models/a.vmdl"), ("b", "models/b.vmdl")],
        )
        .unwrap();
        let line = out.lines().find(|l| !l.starts_with('#')).unwrap();
        assert_eq!(
            line,
            "a 162.31 63.73 144.54 2.75 199.94 0 25 10000 1 2 3 49.61 263.39 0 90 231 254 187 97 114 0"
        );
        assert_eq!(
            out.lines().count(),
            2,
            "the model without a framing has no line"
        );
    }

    #[test]
    fn hostile_text_is_an_error() {
        let deep = "\"a\" {".repeat(100);
        assert!(framings(deep.as_bytes(), &[]).is_err());
        assert!(framings(b"\"Portraits\" { \"k\" ", &[]).is_err());
    }
}
