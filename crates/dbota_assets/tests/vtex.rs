//! Textures built here and read back: the `vtex_c` layout, the mip levels, sprite sheet cuts and
//! KTX2 output, without a Dota install. The golden tests compare with the real files.

use bevy_image::{CompressedImageFormats, ktx2_buffer_to_image};
use dbota_assets::{Encoding, PixelFormat, Texture};

/// A `vtex_c` as the game's compiler lays it out.
struct Vtex {
    flags: u16,
    width: u16,
    height: u16,
    format: u8,
    mips: u8,
    /// Extra data entries: type and payload.
    extras: Vec<(u32, Vec<u8>)>,
    /// The levels as they sit after the resource, smallest first.
    levels: Vec<Vec<u8>>,
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl Vtex {
    fn new(width: u16, height: u16, format: u8, levels: Vec<Vec<u8>>) -> Self {
        Self {
            flags: 0,
            width,
            height,
            format,
            mips: levels.len() as u8,
            extras: Vec::new(),
            levels,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&self.flags.to_le_bytes());
        data.extend_from_slice(&[0; 16]);
        for v in [self.width, self.height, 1] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&[self.format, self.mips]);
        data.extend_from_slice(&words(&[0, 8, self.extras.len() as u32]));
        // Each entry: type, offset to its payload (from the offset field), size.
        let mut payload_at = data.len() + 12 * self.extras.len();
        let mut payloads = Vec::new();
        for (kind, payload) in &self.extras {
            let field = data.len() + 4;
            data.extend_from_slice(&words(&[
                *kind,
                (payload_at - field) as u32,
                payload.len() as u32,
            ]));
            payload_at += payload.len();
            payloads.extend_from_slice(payload);
        }
        data.extend_from_slice(&payloads);
        // The resource: file size, header version 12, version, table offset, one block "DATA".
        let file_size = 28 + data.len();
        let mut out = words(&[file_size as u32]);
        out.extend_from_slice(&12u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&words(&[8, 1]));
        out.extend_from_slice(b"DATA");
        out.extend_from_slice(&words(&[8, data.len() as u32]));
        out.extend_from_slice(&data);
        for level in &self.levels {
            out.extend_from_slice(level);
        }
        out
    }
}

/// `n` bytes counting up from `seed`.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| seed.wrapping_add(i as u8).wrapping_mul(7))
        .collect()
}

fn bevy_format(ktx2: &[u8], srgb: bool) -> (String, u32, Vec<u8>) {
    let image = ktx2_buffer_to_image(ktx2, CompressedImageFormats::all(), srgb).unwrap();
    (
        format!("{:?}", image.texture_descriptor.format),
        image.texture_descriptor.mip_level_count,
        image.data.unwrap(),
    )
}

#[test]
fn block_formats_and_their_mips_pass_through_into_ktx2_that_bevy_loads() {
    let (small, large) = (pattern(8, 1), pattern(32, 2));
    let file = Vtex::new(8, 8, 1, vec![small.clone(), large.clone()]).bytes();
    let texture = Texture::parse(&file).unwrap();
    assert_eq!(texture.format(), PixelFormat::Bc1);
    assert_eq!((texture.width(), texture.height()), (8, 8));
    assert_eq!(texture.mips(), [large.clone(), small.clone()]);

    let reader_view = |encoding| {
        let ktx2 = texture.to_ktx2(encoding);
        let reader = ktx2::Reader::new(&ktx2[..]).unwrap();
        assert_eq!(reader.header().level_count, 2);
        let levels: Vec<_> = reader.levels().map(|l| l.data.to_vec()).collect();
        assert_eq!(levels, [large.clone(), small.clone()]);
        assert!(!reader.dfd_blocks().is_empty());
        ktx2
    };
    let srgb = reader_view(Encoding::Srgb);
    let linear = reader_view(Encoding::Linear);
    let expected = [large, small].concat();
    assert_eq!(
        bevy_format(&srgb, true),
        ("Bc1RgbaUnormSrgb".into(), 2, expected.clone())
    );
    assert_eq!(
        bevy_format(&linear, false),
        ("Bc1RgbaUnorm".into(), 2, expected)
    );
}

#[test]
fn plain_channels_load_in_bevy_whatever_their_width() {
    // 1, 2 and 4 byte texels, 4 x 4, with the last two levels of the chain left out.
    for (id, format, name) in [
        (3, PixelFormat::R8, "R8Unorm"),
        (22, PixelFormat::Rg8, "Rg8Unorm"),
        (4, PixelFormat::Rgba8, "Rgba8Unorm"),
        (9, PixelFormat::Rg16f, "Rg16Float"),
        (14, PixelFormat::Rgba32f, "Rgba32Float"),
    ] {
        let size = |side: u32| format.level_size(side, side).unwrap();
        let file = Vtex::new(4, 4, id, vec![pattern(size(2), 5), pattern(size(4), 6)]).bytes();
        let texture = Texture::parse(&file).unwrap();
        assert_eq!(texture.format(), format);
        let (got, levels, _) = bevy_format(&texture.to_ktx2(Encoding::Linear), false);
        assert_eq!((got.as_str(), levels), (name, 2), "{format:?}");
    }
}

#[test]
fn bgra_texels_come_out_as_rgba() {
    let file = Vtex::new(1, 1, 28, vec![vec![10, 20, 30, 40]]).bytes();
    let texture = Texture::parse(&file).unwrap();
    assert_eq!(texture.format(), PixelFormat::Rgba8);
    assert_eq!(texture.mips(), [vec![30, 20, 10, 40]]);
}

#[test]
fn lz4_compressed_levels_are_expanded_and_incompressible_ones_stay_raw() {
    let (l0, l1, l2) = (vec![9u8; 64], pattern(16, 3), pattern(4, 4));
    let packed = lz4_flex::block::compress(&l0);
    assert!(packed.len() < l0.len());
    // Level sizes as stored, level 0 first: the flag that they are compressed, the sizes' offset
    // (always 8) and count, then the sizes.
    let sizes = words(&[1, 8, 3, packed.len() as u32, 16, 4]);
    let mut file = Vtex::new(4, 4, 4, vec![l2.clone(), l1.clone(), packed]);
    file.extras.push((4, sizes));
    let texture = Texture::parse(&file.bytes()).unwrap();
    assert_eq!(texture.mips(), [l0, l1, l2]);
}

#[test]
fn a_level_shorter_than_its_size_without_lz4_is_an_error() {
    let sizes = words(&[0, 8, 1, 10]);
    let mut file = Vtex::new(4, 4, 4, vec![pattern(10, 1)]);
    file.mips = 1;
    file.extras.push((4, sizes));
    assert!(Texture::parse(&file.bytes()).is_err());
}

/// The data of a sprite sheet extra entry holding one sequence of one frame cropped to `rect`
/// (texture coordinates).
fn sheet(rect: [f32; 4]) -> Vec<u8> {
    let mut out = words(&[8, 1]);
    // The sequence: id, flags, offset to the frames (from this field), frame count, frame rate,
    // offset to the name, and two zero words.
    out.extend_from_slice(&words(&[0, 0, 24, 1, 1.0f32.to_bits(), 0, 0, 0]));
    // The frame: display time, offset to its images (from this field), image count, then the
    // image: the cropped rectangle and the whole cell.
    out.extend_from_slice(&words(&[1.0f32.to_bits(), 8, 1]));
    for v in rect.into_iter().chain([0.0, 0.0, 1.0, 1.0]) {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    out
}

fn texel(level: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    level[at..at + 4].try_into().unwrap()
}

#[test]
fn the_first_frame_of_a_sprite_sheet_is_cut_from_every_level_it_lines_up_with() {
    // 16 x 16 RGBA8 with all five levels, the frame at texels 4..12.
    let levels: Vec<Vec<u8>> = (0..5)
        .map(|level| pattern(4 * (16usize >> level).pow(2), level as u8))
        .collect();
    let mut file = Vtex::new(16, 16, 4, levels.iter().rev().cloned().collect());
    file.extras.push((2, sheet([0.25, 0.25, 0.75, 0.75])));
    let texture = Texture::parse(&file.bytes()).unwrap();
    assert!(texture.is_sheet());
    assert_eq!(texture.first_frame_rect(), Some([4, 4, 12, 12]));

    let frame = texture.first_frame();
    assert_eq!((frame.width(), frame.height()), (8, 8));
    // Level 3 puts the frame at texel 0.5, and the levels after it are gone.
    assert_eq!(frame.mips().len(), 3);
    for (level, mip) in frame.mips().iter().enumerate() {
        let (side, from) = (8usize >> level, 4usize >> level);
        for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
            let source = texel(&levels[level], 16 >> level, from + x, from + y);
            assert_eq!(texel(mip, side, x, y), source, "level {level} ({x}, {y})");
        }
    }
    check_ktx2_levels(&frame);
}

fn check_ktx2_levels(texture: &Texture) {
    let ktx2 = texture.to_ktx2(Encoding::Linear);
    let reader = ktx2::Reader::new(&ktx2[..]).unwrap();
    assert_eq!(reader.header().level_count as usize, texture.mips().len());
    for (level, mip) in reader.levels().zip(texture.mips()) {
        assert_eq!(level.data, &mip[..]);
    }
}

#[test]
fn a_sprite_frame_off_the_block_grid_grows_to_it() {
    // 16 x 16 DXT1: texels 5..13 start inside the second block column.
    let mut file = Vtex::new(16, 16, 1, vec![pattern(128, 1)]);
    file.extras
        .push((2, sheet([5.0 / 16.0, 5.0 / 16.0, 13.0 / 16.0, 13.0 / 16.0])));
    let texture = Texture::parse(&file.bytes()).unwrap();
    assert_eq!(texture.first_frame_rect(), Some([5, 5, 13, 13]));
    let frame = texture.first_frame();
    assert_eq!((frame.width(), frame.height()), (9, 9));
    assert_eq!(frame.mips().len(), 1);
    assert_eq!(
        frame.mips()[0].len(),
        PixelFormat::Bc1.level_size(9, 9).unwrap()
    );
    // The blocks are the ones of block columns and rows 1 to 3.
    let first = &texture.mips()[0][(4 + 1) * 8..(4 + 2) * 8];
    assert_eq!(&frame.mips()[0][..8], first);
    check_ktx2_levels(&frame);
}

#[test]
fn a_texture_that_is_no_sheet_is_its_own_first_frame() {
    let file = Vtex::new(4, 4, 4, vec![pattern(64, 1)]).bytes();
    let texture = Texture::parse(&file).unwrap();
    assert!(!texture.is_sheet());
    assert_eq!(texture.first_frame().mips(), texture.mips());
}

#[test]
fn damaged_textures_are_errors_not_panics() {
    let good = Vtex::new(8, 8, 1, vec![pattern(8, 1), pattern(32, 2)]).bytes();
    assert!(Texture::parse(&good).is_ok());
    for len in 0..good.len() {
        assert!(Texture::parse(&good[..len]).is_err(), "cut at {len}");
    }
    let mut cube = Vtex::new(8, 8, 1, vec![pattern(32, 2)]);
    cube.flags = 0x10;
    let mut jpeg = Vtex::new(8, 8, 15, vec![pattern(32, 2)]);
    jpeg.mips = 1;
    let mut chain = Vtex::new(8, 8, 1, vec![pattern(32, 2)]);
    chain.mips = 5;
    let empty = Vtex::new(0, 8, 1, vec![]);
    let big = Vtex::new(u16::MAX, u16::MAX, 4, vec![pattern(64, 1)]);
    for (what, bad) in [
        ("cube", cube),
        ("jpeg", jpeg),
        ("chain", chain),
        ("empty", empty),
        ("big", big),
    ] {
        assert!(Texture::parse(&bad.bytes()).is_err(), "{what}");
    }
    // Flip every byte of the header and the extra data once: any answer but a panic.
    let mut with_sheet = Vtex::new(8, 8, 4, vec![pattern(256, 1)]);
    with_sheet.mips = 1;
    with_sheet.extras.push((2, sheet([0.0, 0.0, 0.5, 0.5])));
    let bytes = with_sheet.bytes();
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0xFF;
        if let Ok(texture) = Texture::parse(&flipped) {
            let _ = texture.first_frame().to_ktx2(Encoding::Srgb);
        }
    }
}
