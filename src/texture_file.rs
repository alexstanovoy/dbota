//! What a PNG or KTX2 file of the asset cache decodes to, read from its
//! header before any decoder sees it. The cache is a folder on the user's
//! disk like any other, and Bevy's decoders take a header at its word: a PNG
//! of 256 x 4 billion pixels was a 4 TB allocation (an abort), and
//! supercompressed KTX2 levels inflate without a bound. So a texture file is
//! taken only within the caps, and a KTX2 file only as the client's own
//! export writes it.
//!
//! Plain std, no Bevy: the fuzz targets read this file as their own module.

/// Bytes of a file's start that [`decoded_size`] reads: a KTX2 header and
/// the index of its most levels.
pub const HEAD_BYTES: usize = KTX2_INDEX + 24 * MAX_LEVELS;
const KTX2_INDEX: usize = 80;
const MAX_LEVELS: usize = 32;
const PNG_MAGIC: [u8; 8] = *b"\x89PNG\r\n\x1a\n";
const KTX2_MAGIC: [u8; 12] = [
    0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
];
/// Longest side of a texture; the cache's longest is 4096.
const MAX_SIDE: u32 = 16_384;
/// Most memory one decoded texture may take; the cache's largest takes 64 MiB.
const MAX_DECODED: usize = 256 << 20;
/// The `vkFormat`s the export writes: the BC, ETC2 and EAC block formats,
/// and plain channels of 8, 16 and 32 bits.
const KTX2_FORMATS: [u32; 29] = [
    133, 134, 137, 138, 139, 141, 143, 145, 146, 147, 148, 151, 152, 153, 155, 9, 15, 16, 37, 43,
    70, 77, 91, 76, 83, 97, 100, 103, 109,
];
/// The block formats among them, 4 x 4 texels a block.
const KTX2_BLOCKS: std::ops::RangeInclusive<u32> = 131..=184;

/// The bytes a texture file holds once decoded, from its first
/// [`HEAD_BYTES`]: a PNG decodes to RGBA, a KTX2 file to all its mip levels
/// as they sit in memory (compressed blocks stay compressed). None for a
/// file of neither kind; an error for one past the caps, cut short, or a
/// KTX2 file not as the export writes it.
pub fn decoded_size(head: &[u8]) -> Option<Result<usize, String>> {
    if head.starts_with(&PNG_MAGIC) {
        Some(png_size(head))
    } else if head.starts_with(&KTX2_MAGIC) {
        Some(ktx2_size(head))
    } else {
        None
    }
}

fn png_size(head: &[u8]) -> Result<usize, String> {
    let ihdr = head
        .get(8..26)
        .filter(|ihdr| ihdr[4..8] == *b"IHDR")
        .ok_or("a PNG without its header")?;
    let side = |at: usize| u32::from_be_bytes([ihdr[at], ihdr[at + 1], ihdr[at + 2], ihdr[at + 3]]);
    let (width, height) = sides(side(8), side(12))?;
    // RGBA, of 16-bit channels where the file has them.
    let texel = if ihdr[16] == 16 { 8 } else { 4 };
    capped(width * height * texel)
}

fn ktx2_size(head: &[u8]) -> Result<usize, String> {
    let word = |at: usize| {
        head.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or("a KTX2 file cut short")
    };
    let format = word(12)?;
    let (depth, layers, faces, scheme) = (word(28)?, word(32)?, word(36)?, word(44)?);
    if !KTX2_FORMATS.contains(&format) || depth > 1 || layers > 1 || faces != 1 || scheme != 0 {
        return Err(format!(
            "a KTX2 file of format {format}, depth {depth}, {layers} layers, {faces} faces, \
             supercompression {scheme}: not as the export writes it"
        ));
    }
    let (width, height) = sides(word(20)?, word(24)?)?;
    if KTX2_BLOCKS.contains(&format) && (width % 4 != 0 || height % 4 != 0) {
        return Err(format!("{width} x {height} is not whole blocks"));
    }
    // Zero levels ask the loader to make the chain: the one level is there.
    let levels = (word(40)? as usize).max(1);
    if levels > MAX_LEVELS {
        return Err(format!("{levels} levels"));
    }
    let index = head
        .get(KTX2_INDEX..KTX2_INDEX + 24 * levels)
        .ok_or("a KTX2 level index cut short")?;
    let mut total = 0usize;
    for level in index.as_chunks::<24>().0 {
        let length = |at: usize| u64::from_le_bytes(std::array::from_fn(|i| level[at + i]));
        let (stored, decoded) = (length(8), length(16));
        if stored != decoded {
            return Err("a level stored at another size than it decodes to".into());
        }
        total = total.saturating_add(usize::try_from(decoded).unwrap_or(usize::MAX));
    }
    capped(total)
}

/// `width` and `height` as sizes, refused when empty or past [`MAX_SIDE`].
fn sides(width: u32, height: u32) -> Result<(usize, usize), String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("{width} x {height} texels"));
    }
    Ok((width as usize, height as usize))
}

fn capped(bytes: usize) -> Result<usize, String> {
    if bytes > MAX_DECODED {
        return Err(format!("decodes to {bytes} bytes, more than {MAX_DECODED}"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A KTX2 start: a 64 x 64 BC1 texture of `levels` in the header, and an
    /// index whose level `i` holds `sizes[i]` bytes.
    fn ktx2(levels: u32, sizes: &[u64]) -> Vec<u8> {
        let mut head = KTX2_MAGIC.to_vec();
        for word in [133, 1, 64, 64, 0, 0, 1, levels, 0] {
            head.extend_from_slice(&u32::to_le_bytes(word));
        }
        head.resize(KTX2_INDEX, 0);
        for &size in sizes {
            for field in [0, size, size] {
                head.extend_from_slice(&u64::to_le_bytes(field));
            }
        }
        head
    }

    fn png(width: u32, height: u32, depth: u8) -> Vec<u8> {
        let mut head = PNG_MAGIC.to_vec();
        head.extend_from_slice(b"\0\0\0\x0dIHDR");
        head.extend_from_slice(&width.to_be_bytes());
        head.extend_from_slice(&height.to_be_bytes());
        head.extend_from_slice(&[depth, 6]);
        head
    }

    #[test]
    fn texture_files_count_what_they_decode_to() {
        assert_eq!(decoded_size(&ktx2(3, &[2048, 512, 128])), Some(Ok(2688)));
        assert_eq!(decoded_size(&ktx2(0, &[2048])), Some(Ok(2048)));
        assert_eq!(decoded_size(&png(4096, 4096, 8)), Some(Ok(64 << 20)));
        assert_eq!(decoded_size(&png(16, 16, 16)), Some(Ok(2048)));
        assert_eq!(decoded_size(b"glTF"), None);
    }

    /// Each of these once reached Bevy's decoders, which allocate what the
    /// header says: 256 x 4 billion pixels aborted the client.
    #[test]
    fn texture_files_past_the_caps_or_not_as_exported_are_refused() {
        let refused = |head: &[u8]| matches!(decoded_size(head), Some(Err(_)));
        assert!(refused(&png(256, 0xFFFF_00C8, 8)));
        assert!(refused(&png(16_384, 16_384, 8)));
        assert!(refused(&png(0, 1, 8)));
        assert!(refused(&png(4, 4, 8)[..20]));
        assert!(refused(&ktx2(3, &[2048, 512])));
        assert!(refused(&ktx2(33, &[64; 33])));
        assert!(refused(&ktx2(2, &[u64::MAX, 1])));
        let mut zstd = ktx2(1, &[2048]);
        zstd[44] = 2;
        assert!(refused(&zstd));
        let mut odd = ktx2(1, &[2048]);
        odd[20] = 63;
        assert!(refused(&odd));
    }
}
