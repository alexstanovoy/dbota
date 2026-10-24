//! Images as RGBA8 for the golden tests: exported PNGs read, texture levels decoded.

use std::path::Path;

use dbota_assets::PixelFormat;

/// An exported PNG as RGBA8: width, height, pixels.
pub fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; a texture added after the reference was exported needs its image there, \
             made by the same Source2Viewer-CLI 20 (`-i pak01_dir.vpk -d -f <vtex_c> -o <dir>/`)",
            path.display()
        )
    });
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buf).unwrap();
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!(),
    };
    (frame.width, frame.height, rgba)
}

/// The RGBA8 pixels of a level, decoded as a GPU would: texels of an uncompressed format as
/// they are, blocks through an independent decoder.
pub fn to_rgba(format: PixelFormat, width: u32, height: u32, level: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut px = vec![0u32; w * h];
    let blocks = match format {
        PixelFormat::Rgba8 => return level.to_vec(),
        PixelFormat::Bc1 => texture2ddecoder::decode_bc1(level, w, h, &mut px),
        PixelFormat::Bc3 => texture2ddecoder::decode_bc3(level, w, h, &mut px),
        PixelFormat::Bc4 => texture2ddecoder::decode_bc4(level, w, h, &mut px),
        PixelFormat::Bc5 => texture2ddecoder::decode_bc5(level, w, h, &mut px),
        PixelFormat::Bc7 => texture2ddecoder::decode_bc7(level, w, h, &mut px),
        other => panic!("no reference decoder for {other:?}"),
    };
    blocks.unwrap();
    px.iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect()
}

/// How far two RGBA8 images are apart: the largest and the mean difference of any channel.
pub fn distance(a: &[u8], b: &[u8]) -> (u8, f64) {
    assert_eq!(a.len(), b.len());
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    (max, sum as f64 / a.len().max(1) as f64)
}
