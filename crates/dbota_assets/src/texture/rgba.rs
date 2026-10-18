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
