//! KTX2 files (Khronos texture container 2.0) holding one 2D texture with its mip levels,
//! uncompressed in the container (no supercompression).

use super::format::PixelFormat;
use crate::error::{Result, malformed};

const WHAT: &str = "ktx2";
/// Largest side of a texture the importer writes.
const MAX_SIDE: u32 = 16384;
/// Most memory the decoded levels of one texture may take.
const MAX_DECODED: usize = 256 << 20;

/// How the colours of a texture are meant to be read: the sRGB curve of colour textures, or
/// the plain numbers of normals, masks and data. A `vtex_c` does not say, the material using
/// it does. Formats that have no sRGB variant (one- and two-channel block formats, 16- and
/// 32-bit channels, floats) are written linear whatever is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Srgb,
    Linear,
}

const IDENTIFIER: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];
const HEADER: usize = 80;
/// Where each level starts, a multiple of the block size (at most 16 bytes, a power of two).
const LEVEL_ALIGN: usize = 16;

/// One channel of a data format descriptor sample.
struct Sample {
    offset: u32,
    bits: u32,
    channel: u32,
    /// The top four bits of the sample's word: linear, exponent, signed, float.
    qualifiers: u32,
    lower: u32,
    upper: u32,
}

const LINEAR: u32 = 0x1;
const FLOAT: u32 = 0x8;
const RED: u32 = 0;
const GREEN: u32 = 1;
const BLUE: u32 = 2;
const ALPHA: u32 = 15;

/// `vkFormat`, and the colour model of the descriptor with its samples.
struct Layout {
    vk: u32,
    model: u32,
    samples: Vec<Sample>,
}

/// A whole-block sample of `bits` bits at bit `offset`.
fn block_sample(offset: u32, bits: u32, channel: u32) -> Sample {
    Sample {
        offset,
        bits,
        channel,
        qualifiers: 0,
        lower: 0,
        upper: u32::MAX,
    }
}

/// The `bits`-wide unsigned normalised channels `channels` in a row, lowest bit first.
fn unorm(bits: u32, channels: &[u32], srgb: bool) -> Vec<Sample> {
    let upper = if bits == 32 {
        u32::MAX
    } else {
        (1 << bits) - 1
    };
    channels
        .iter()
        .zip(0..)
        .map(|(&channel, i)| Sample {
            offset: i * bits,
            bits,
            channel,
            // Alpha stays linear under the sRGB curve.
            qualifiers: if srgb && channel == ALPHA { LINEAR } else { 0 },
            lower: 0,
            upper,
        })
        .collect()
}

fn float(bits: u32, channels: &[u32]) -> Vec<Sample> {
    let one = if bits == 32 { 1.0f32.to_bits() } else { 0x3C00 };
    channels
        .iter()
        .zip(0..)
        .map(|(&channel, i)| Sample {
            offset: i * bits,
            bits,
            channel,
            qualifiers: FLOAT | 0x4,
            lower: if bits == 32 {
                (-1.0f32).to_bits()
            } else {
                0xBC00
            },
            upper: one,
        })
        .collect()
}

const RGBA: [u32; 4] = [RED, GREEN, BLUE, ALPHA];

fn layout(format: PixelFormat, srgb: bool) -> Layout {
    use PixelFormat as P;
    // Colour models of the descriptor: raw channels, the BC family from 128, ETC2 and EAC 161.
    const RGBSDA: u32 = 1;
    let pick = |linear: u32, srgb_vk: u32| if srgb { srgb_vk } else { linear };
    let (vk, model, samples) = match format {
        P::Bc1 => (pick(133, 134), 128, vec![block_sample(0, 64, 1)]),
        P::Bc3 => (
            pick(137, 138),
            130,
            vec![block_sample(0, 64, ALPHA), block_sample(64, 64, RED)],
        ),
        P::Bc4 => (139, 131, vec![block_sample(0, 64, RED)]),
        P::Bc5 => (
            141,
            132,
            vec![block_sample(0, 64, RED), block_sample(64, 64, GREEN)],
        ),
        P::Bc6h => (
            143,
            133,
            vec![Sample {
                qualifiers: FLOAT,
                lower: (-1.0f32).to_bits(),
                upper: 1.0f32.to_bits(),
                ..block_sample(0, 128, RED)
            }],
        ),
        P::Bc7 => (pick(145, 146), 134, vec![block_sample(0, 128, RED)]),
        P::Etc2Rgb => (pick(147, 148), 161, vec![block_sample(0, 64, RED)]),
        P::Etc2Rgba => (
            pick(151, 152),
            161,
            vec![block_sample(0, 64, ALPHA), block_sample(64, 64, RED)],
        ),
        P::EacR11 => (153, 161, vec![block_sample(0, 64, RED)]),
        P::EacRg11 => (
            155,
            161,
            vec![block_sample(0, 64, RED), block_sample(64, 64, GREEN)],
        ),
        P::R8 => (pick(9, 15), RGBSDA, unorm(8, &[RED], srgb)),
        P::Rg8 => (16, RGBSDA, unorm(8, &RGBA[..2], false)),
        P::Rgba8 => (pick(37, 43), RGBSDA, unorm(8, &RGBA, srgb)),
        P::R16 => (70, RGBSDA, unorm(16, &[RED], false)),
        P::Rg16 => (77, RGBSDA, unorm(16, &RGBA[..2], false)),
        P::Rgba16 => (91, RGBSDA, unorm(16, &RGBA, false)),
        P::R16f => (76, RGBSDA, float(16, &[RED])),
        P::Rg16f => (83, RGBSDA, float(16, &RGBA[..2])),
        P::Rgba16f => (97, RGBSDA, float(16, &RGBA)),
        P::R32f => (100, RGBSDA, float(32, &[RED])),
        P::Rg32f => (103, RGBSDA, float(32, &RGBA[..2])),
        P::Rgba32f => (109, RGBSDA, float(32, &RGBA)),
    };
    Layout { vk, model, samples }
}

/// Whether the format has an sRGB variant to ask for.
fn has_srgb(format: PixelFormat) -> bool {
    use PixelFormat as P;
    matches!(
        format,
        P::Bc1 | P::Bc3 | P::Bc7 | P::Etc2Rgb | P::Etc2Rgba | P::R8 | P::Rgba8
    )
}

/// The basic data format descriptor, with its leading total size.
fn descriptor(format: PixelFormat, layout: &Layout, srgb: bool) -> Vec<u8> {
    let (bw, bh, bytes) = format.block();
    let size = 24 + 16 * layout.samples.len() as u32;
    let mut out = Vec::new();
    let mut word = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    word(4 + size);
    word(0); // vendor Khronos, type basic
    word(2 | size << 16); // descriptor version 2 (KTX2's), block size
    let transfer = if srgb { 2 } else { 1 };
    word(layout.model | 1 << 8 | transfer << 16); // primaries BT.709
    word((bw - 1) | (bh - 1) << 8);
    word(bytes as u32);
    word(0);
    for s in &layout.samples {
        word(s.offset | (s.bits - 1) << 16 | (s.channel | s.qualifiers << 4) << 24);
        word(0); // sample position
        word(s.lower);
        word(s.upper);
    }
    out
}

/// Bytes of one channel of a plain format (what a machine of the other endianness swaps), 1
/// for the 8-bit ones and the blocks.
fn type_size(format: PixelFormat) -> u32 {
    use PixelFormat as P;
    match format {
        P::R16 | P::Rg16 | P::Rgba16 | P::R16f | P::Rg16f | P::Rgba16f => 2,
        P::R32f | P::Rg32f | P::Rgba32f => 4,
        _ => 1,
    }
}

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

/// The KTX2 file of a `width` x `height` texture whose mip levels `mips` (largest first,
/// `max(1, size >> i)` texels square, each exactly [`PixelFormat::level_size`] bytes) are given.
pub(super) fn write(
    width: u32,
    height: u32,
    format: PixelFormat,
    encoding: Encoding,
    mips: &[Vec<u8>],
) -> Vec<u8> {
    let srgb = encoding == Encoding::Srgb && has_srgb(format);
    let layout = layout(format, srgb);
    let dfd = descriptor(format, &layout, srgb);
    let levels = mips.len();
    let index = HEADER + 24 * levels;
    let dfd_at = index;
    // The smallest level goes first in the file, as the specification recommends.
    let mut offsets = vec![0; levels];
    let mut at = align(dfd_at + dfd.len(), LEVEL_ALIGN);
    for (offset, mip) in offsets.iter_mut().zip(mips).rev() {
        *offset = at;
        at = align(at + mip.len(), LEVEL_ALIGN);
    }
    let mut out = Vec::with_capacity(at);
    out.extend_from_slice(&IDENTIFIER);
    let word = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    word(&mut out, layout.vk);
    word(&mut out, type_size(format));
    word(&mut out, width);
    word(&mut out, height);
    word(&mut out, 0); // depth
    word(&mut out, 0); // layers
    word(&mut out, 1); // faces
    word(&mut out, levels as u32);
    word(&mut out, 0); // no supercompression
    word(&mut out, dfd_at as u32);
    word(&mut out, dfd.len() as u32);
    word(&mut out, 0); // no key/value data
    word(&mut out, 0);
    out.extend_from_slice(&0u64.to_le_bytes()); // no global data
    out.extend_from_slice(&0u64.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER);
    for (offset, mip) in offsets.iter().zip(mips) {
        for v in [*offset, mip.len(), mip.len()] {
            out.extend_from_slice(&(v as u64).to_le_bytes());
        }
    }
    out.extend_from_slice(&dfd);
    for (offset, mip) in offsets.iter().zip(mips).rev() {
        out.resize(*offset, 0);
        out.extend_from_slice(mip);
    }
    out.resize(at, 0);
    out
}

/// The KTX2 file of plain channels the importer's KTX2 file `bytes` decodes to, for a GPU that
/// samples no block format: every level decoded (BC1, BC3, BC7 and RGBA8 to RGBA8; BC4 to its
/// one channel, BC5 to its two, which a normal map is read as), sRGB where the file is. Any
/// other file is an error.
pub fn to_plain(bytes: &[u8]) -> Result<Vec<u8>> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| malformed(WHAT, "cut short"))
    };
    let long = |at: usize| {
        bytes
            .get(at..at + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| malformed(WHAT, "cut short"))
    };
    if !bytes.starts_with(&IDENTIFIER) {
        return Err(malformed(WHAT, "not a KTX2 file"));
    }
    let (vk, width, height, levels) = (word(12)?, word(20)?, word(24)?, word(40)? as usize);
    let (format, srgb) = format_of(vk).ok_or_else(|| malformed(WHAT, format!("format {vk}")))?;
    let plain = match format {
        PixelFormat::Bc4 => PixelFormat::R8,
        PixelFormat::Bc5 => PixelFormat::Rg8,
        _ => PixelFormat::Rgba8,
    };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || levels > 32 {
        return Err(malformed(
            WHAT,
            format!("{width} x {height}, {levels} levels"),
        ));
    }
    let decoded: usize = (0..levels.max(1))
        .filter_map(|l| PixelFormat::Rgba8.level_size((width >> l).max(1), (height >> l).max(1)))
        .sum();
    if decoded > MAX_DECODED {
        return Err(malformed(WHAT, format!("decodes to {decoded} bytes")));
    }
    let mut mips = Vec::with_capacity(levels.max(1));
    for level in 0..levels.max(1) {
        let at = HEADER + 24 * level;
        let (offset, length) = (long(at)?, long(at + 8)?);
        let data = offset
            .checked_add(length)
            .and_then(|end| bytes.get(offset..end))
            .ok_or_else(|| malformed(WHAT, format!("level {level} out of the file")))?;
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        if format.level_size(w, h) != Some(length) {
            return Err(malformed(WHAT, format!("level {level} is not {w} x {h}")));
        }
        let rgba = super::rgba::decode(format, data, w, h)?;
        mips.push(match plain {
            PixelFormat::R8 => rgba.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
            PixelFormat::Rg8 => rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1]])
                .collect(),
            _ => rgba,
        });
    }
    let encoding = if srgb {
        Encoding::Srgb
    } else {
        Encoding::Linear
    };
    Ok(write(width, height, plain, encoding, &mips))
}

/// The pixel format, and whether sRGB, of a `vkFormat` the importer writes.
fn format_of(vk: u32) -> Option<(PixelFormat, bool)> {
    use PixelFormat as P;
    Some(match vk {
        133 => (P::Bc1, false),
        134 => (P::Bc1, true),
        137 => (P::Bc3, false),
        138 => (P::Bc3, true),
        139 => (P::Bc4, false),
        141 => (P::Bc5, false),
        145 => (P::Bc7, false),
        146 => (P::Bc7, true),
        37 => (P::Rgba8, false),
        43 => (P::Rgba8, true),
        _ => return None,
    })
}
