//! The two DXT5 textures that are not colours as stored, which the resource says in its
//! "special dependencies" (what the texture compiler was asked to do): scaled YCoCg images and
//! tangent-space normals. Both are decoded to RGBA8, as the game's tools show them.

use crate::error::{Result, malformed};
use crate::resource::Resource;

const YCOCG: &str = "Texture Compiler Version Image YCoCg Conversion";
const NORMALS: &str = "Texture Compiler Version Image NormalizeNormals";

/// What a DXT5 texture holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Content {
    /// Red, green, blue and alpha.
    Colour,
    /// Colour as luma in alpha, chroma in red and green, a scale in blue.
    YCoCg,
    /// A normal's x in alpha and y in green.
    Normal,
}

impl Content {
    /// What the texture compiler was told to do with the image, read from the resource's edit
    /// info: the old `REDI` block lists its special dependencies as plain strings, the new
    /// `RED2` as a KV3 list.
    pub(super) fn of(resource: &Resource<'_>) -> Self {
        let says = |text: &str| {
            let in_redi = resource
                .block(b"REDI")
                .is_some_and(|redi| redi.windows(text.len()).any(|w| w == text.as_bytes()));
            in_redi
                || resource
                    .kv3(b"RED2")
                    .and_then(Result::ok)
                    .and_then(|doc| {
                        doc.root
                            .get("m_SpecialDependencies")?
                            .as_array()?
                            .iter()
                            .find_map(|d| (d.get("m_String")?.as_str()? == text).then_some(()))
                    })
                    .is_some()
        };
        if says(YCOCG) {
            Self::YCoCg
        } else if says(NORMALS) {
            Self::Normal
        } else {
            Self::Colour
        }
    }

    /// The RGBA8 pixels of a DXT5 level `width` x `height`, read as `self`.
    pub(super) fn decode_bc3(self, level: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let (w, h) = (width as usize, height as usize);
        let mut px = vec![0u32; w * h];
        texture2ddecoder::decode_bc3(level, w, h, &mut px)
            .map_err(|e| malformed("vtex", format!("DXT5 blocks: {e}")))?;
        let mut out = Vec::with_capacity(px.len() * 4);
        for p in px {
            let [b, g, r, a] = p.to_le_bytes();
            out.extend_from_slice(&match self {
                Self::Colour => [r, g, b, a],
                Self::YCoCg => ycocg(r, g, b, a),
                Self::Normal => normal(g, a),
            });
        }
        Ok(out)
    }
}

fn byte(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Scaled YCoCg-DXT5: luma in alpha, chroma in red and green around 128 divided by a scale kept
/// in blue's top five bits.
fn ycocg(r: u8, g: u8, b: u8, y: u8) -> [u8; 4] {
    let scale = f32::from((b >> 3) + 1);
    let co = (f32::from(r) - 128.0) / scale;
    let cg = (f32::from(g) - 128.0) / scale;
    let y = f32::from(y);
    [byte(y + co - cg), byte(y + cg), byte(y - co - cg), 255]
}

/// A DXT5nm normal: x in alpha, y in green, z from the unit length; a pair longer than one
/// (the blocks round) is scaled back to length one.
fn normal(g: u8, a: u8) -> [u8; 4] {
    let (mut x, mut y) = (
        f32::from(a) / 255.0 * 2.0 - 1.0,
        f32::from(g) / 255.0 * 2.0 - 1.0,
    );
    let flat = x * x + y * y;
    if flat > 1.0 {
        let len = flat.sqrt();
        (x, y) = (x / len, y / len);
    }
    let z = (1.0 - flat).max(0.0).sqrt();
    [
        byte((x * 0.5 + 0.5) * 255.0),
        byte((y * 0.5 + 0.5) * 255.0),
        byte((z * 0.5 + 0.5) * 255.0),
        255,
    ]
}
