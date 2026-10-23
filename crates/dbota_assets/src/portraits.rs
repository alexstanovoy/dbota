//! The portrait framings of Dota's `scripts/npc/portraits.txt`: where the HUD portrait's camera
//! and light stand for each model, boiled down to one line per model of the manifest.

use crate::error::{Result, malformed};

/// Deepest nesting read; the file has four levels.
const DEPTH_LIMIT: usize = 8;
/// Largest file read; Dota's is ~1.4 MB.
const MAX_FILE: usize = 16 << 20;
/// Values of a framing line, in order: camera position, camera angles, field of view, far plane,
/// light position, light angles, light field of view, light colour, ambient colour, light scale,
/// ambient scale.
const VALUES: usize = 3 + 3 + 1 + 1 + 3 + 3 + 1 + 3 + 3 + 1 + 1;

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
        // Dota's brightness of the light and of the ambient colour; plain where unset.
        &[single(entry, "PortraitLightScale").unwrap_or(1.0)],
        &[single(entry, "PortraitAmbientScale").unwrap_or(1.0)],
    ];
    for (slot, n) in out.iter_mut().zip(numbers.concat()) {
        if !n.is_finite() {
            return None;
        }
        *slot = n;
    }
    Some(out)
}

/// The colours Dota tints a portrait's background with, one per corner (top left, top right,
/// bottom right, bottom left), each channel 0 to 1 (white leaves the image as it is).
const WHITE: [[f32; 3]; 4] = [[1.0; 3]; 4];

/// A wanted model's framing as Dota defines it.
pub struct Framed {
    pub logical: String,
    values: [f32; VALUES],
    /// The background's material (`.vmat`), when the entry names one, and its corner tints.
    pub background: Option<(String, [[f32; 3]; 4])>,
}

/// The background of an entry: its material and the four corner colours, white where unset.
fn background(entry: &Kv) -> Option<(String, [[f32; 3]; 4])> {
    let vmat = entry.text("PortraitBackgroundTexture")?.trim();
    if !vmat.ends_with(".vmat") {
        return None;
    }
    let mut colors = WHITE;
    for (i, slot) in colors.iter_mut().enumerate() {
        let Some(text) = entry.text(&format!("PortraitBackgroundColor{}", i + 1)) else {
            continue;
        };
        let n: Vec<f32> = text
            .split_whitespace()
            .filter_map(|w| w.parse::<f32>().ok())
            .collect();
        if let [r, g, b] = n[..]
            && [r, g, b].iter().all(|c| c.is_finite())
        {
            *slot = [r, g, b].map(|c| c.clamp(0.0, 1.0));
        }
    }
    Some((vmat.to_owned(), colors))
}

/// Dota's framings of the wanted models, in the order of `wanted` (pairs of a logical name and
/// the model's `.vmdl` path). A model Dota has no framing for is left out.
pub fn framings(portraits: &[u8], wanted: &[(&str, &str)]) -> Result<Vec<Framed>> {
    if portraits.len() > MAX_FILE {
        return Err(malformed("portraits", format!("{} bytes", portraits.len())));
    }
    let text = std::str::from_utf8(portraits)
        .map_err(|_| malformed("portraits", "the file is not UTF-8"))?;
    let root = Kv::Block(block(&mut Tokens { rest: text }, 0)?);
    let all = root
        .get("Portraits")
        .ok_or_else(|| malformed("portraits", "no Portraits block"))?;
    Ok(wanted
        .iter()
        .filter_map(|(logical, vmdl)| {
            let entry = all.get(vmdl)?;
            Some(Framed {
                logical: (*logical).to_owned(),
                values: values(entry)?,
                background: background(entry),
            })
        })
        .collect())
}

/// `models/portraits.txt`: a line per framing, `<logical> <numbers...> <background>`: camera
/// position and angles in the model's own inches and degrees, horizontal field of view, far
/// plane, the light's position, angles and field of view, light colour and ambient colour as
/// 0-255, the light's and the ambient colour's scales, the background's four corner tints as 0-1 (top left, top right, bottom right, bottom
/// left), and the background image's cache path, `-` where there is none. `image` names the
/// cache path a background material's image was written to, if it was.
pub fn portraits_txt(framed: &[Framed], image: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::from(
        "# logical, camera position (3), camera angles (3), fov, far, light position (3), light angles (3), light fov, light colour (3), ambient colour (3), light scale, ambient scale, background tints (4x3), background image\n",
    );
    for f in framed {
        out.push_str(&f.logical);
        let (colors, path) = match &f.background {
            Some((vmat, colors)) => (*colors, image(vmat)),
            None => (WHITE, None),
        };
        for n in f.values.iter().chain(colors.as_flattened()) {
            out.push_str(&format!(" {n}"));
        }
        out.push(' ');
        out.push_str(path.as_deref().unwrap_or("-"));
        out.push('\n');
    }
    out
}

/// Where the image of portrait background material `vmat` goes in the cache.
pub fn background_png(vmat: &str) -> String {
    let file = vmat.rsplit('/').next().unwrap_or(vmat);
    format!(
        "models/portrait_backgrounds/{}.png",
        file.trim_end_matches(".vmat")
    )
}

#[cfg(test)]
mod tests {
    use super::{background_png, framings, portraits_txt};

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
        "PortraitLightScale" "2.8"
        "PortraitAmbientScale" "5"
        "PortraitBackgroundTexture" "materials/vgui/hud/heroportraits/portraitbackground_desert.vmat"
        "PortraitBackgroundColor3" ".8 .2 .1"
        "cameras" { "default" { "PortraitPosition" "162.31 63.73 144.54" "PortraitAngles" "2.75 199.94 0" "PortraitFOV" "25" } }
    }
}"#;

    #[test]
    fn a_model_gets_its_default_camera_light_and_background() {
        let framed = framings(
            FILE.as_bytes(),
            &[("a", "models/a.vmdl"), ("b", "models/b.vmdl")],
        )
        .unwrap();
        assert_eq!(framed.len(), 1, "the model without a framing has no line");
        let out = portraits_txt(&framed, |vmat| Some(background_png(vmat)));
        let line = out.lines().find(|l| !l.starts_with('#')).unwrap();
        assert_eq!(
            line,
            "a 162.31 63.73 144.54 2.75 199.94 0 25 10000 1 2 3 49.61 263.39 0 90 231 254 187 97 114 0 \
             2.8 5 1 1 1 1 1 1 0.8 0.2 0.1 1 1 1 models/portrait_backgrounds/portraitbackground_desert.png"
        );
        let out = portraits_txt(&framed, |_| None);
        assert!(out.trim_end().ends_with(" -"), "no image, no path: {out}");
    }

    #[test]
    fn hostile_text_is_an_error() {
        let deep = "\"a\" {".repeat(100);
        assert!(framings(deep.as_bytes(), &[]).is_err());
        assert!(framings(b"\"Portraits\" { \"k\" ", &[]).is_err());
    }
}
