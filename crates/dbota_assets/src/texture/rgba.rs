//! Textures as plain RGBA8 pixels: decoding any level to them and writing them as a PNG.

use super::format::PixelFormat;
use crate::error::{Result, malformed};

const WHAT: &str = "texture";

/// The RGBA8 pixels of a level `width` x `height` in `format`: plain RGBA8 as it is, the colour
/// block formats (BC1, BC3, BC4, BC5, BC7) decoded. Other formats are errors.
pub(super) fn decode(
    format: PixelFormat,
    level: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let decoder = match format {
        PixelFormat::Rgba8 => return Ok(level.to_vec()),
        PixelFormat::Bc1 => texture2ddecoder::decode_bc1,
        PixelFormat::Bc3 => texture2ddecoder::decode_bc3,
        PixelFormat::Bc4 => texture2ddecoder::decode_bc4,
        PixelFormat::Bc5 => texture2ddecoder::decode_bc5,
        PixelFormat::Bc7 => texture2ddecoder::decode_bc7,
        other => return Err(malformed(WHAT, format!("no RGBA8 decoding of {other:?}"))),
    };
    let mut px = vec![0u32; w * h];
    decoder(level, w, h, &mut px)
        .map_err(|e| malformed(WHAT, format!("{format:?} blocks: {e}")))?;
    Ok(px
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect())
}

/// A PNG file of RGBA8 `pixels`.
pub(super) fn png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let err = |e: png::EncodingError| malformed(WHAT, format!("png: {e}"));
    let mut writer = encoder.write_header().map_err(err)?;
    writer.write_image_data(pixels).map_err(err)?;
    writer.finish().map_err(err)?;
    Ok(out)
}

/// RGBA8 `pixels` of `width` x `height` halved with a 2 x 2 box (a side of 1 stays 1); with
/// `srgb` the colour channels are averaged as light, alpha as it is.
pub(super) fn halve(pixels: &[u8], width: u32, height: u32, srgb: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
    let to_light = |v: u8| {
        let c = f32::from(v) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let from_light = |l: f32| {
        let c = if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut out = vec![0u8; hw * hh * 4];
    for y in 0..hh {
        for x in 0..hw {
            let rows = [(2 * y).min(h - 1), (2 * y + 1).min(h - 1)];
            let cols = [(2 * x).min(w - 1), (2 * x + 1).min(w - 1)];
            for c in 0..4 {
                let texels = rows
                    .iter()
                    .flat_map(|r| cols.iter().map(move |col| pixels[(r * w + col) * 4 + c]));
                out[(y * hw + x) * 4 + c] = if srgb && c < 3 {
                    from_light(texels.map(to_light).sum::<f32>() / 4.0)
                } else {
                    (texels.map(u32::from).sum::<u32>() as f32 / 4.0).round() as u8
                };
            }
        }
    }
    out
}
