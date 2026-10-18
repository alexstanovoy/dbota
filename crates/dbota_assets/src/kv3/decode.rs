use super::value::Document;
use super::{binary, legacy};
use crate::budget::Budget;
use crate::error::{Result, malformed};

/// `VKV\x03`: the first binary KV3 header, with an encoding GUID.
pub(crate) const MAGIC_LEGACY: u32 = 0x0356_4B56;
/// `\x0n3VK` for versions 1..=5.
const MAGIC_VERSIONED: u32 = 0x4B56_3300;

/// Node types of binary KV3.
pub(crate) mod node {
    pub const NULL: u8 = 1;
    pub const BOOLEAN: u8 = 2;
    pub const INT64: u8 = 3;
    pub const UINT64: u8 = 4;
    pub const DOUBLE: u8 = 5;
    pub const STRING: u8 = 6;
    pub const BINARY_BLOB: u8 = 7;
    pub const ARRAY: u8 = 8;
    pub const OBJECT: u8 = 9;
    pub const ARRAY_TYPED: u8 = 10;
    pub const INT32: u8 = 11;
    pub const UINT32: u8 = 12;
    pub const BOOLEAN_TRUE: u8 = 13;
    pub const BOOLEAN_FALSE: u8 = 14;
    pub const INT64_ZERO: u8 = 15;
    pub const INT64_ONE: u8 = 16;
    pub const DOUBLE_ZERO: u8 = 17;
    pub const DOUBLE_ONE: u8 = 18;
    pub const FLOAT: u8 = 19;
    pub const INT16: u8 = 20;
    pub const UINT16: u8 = 21;
    pub const INT8: u8 = 22;
    pub const UINT8: u8 = 23;
    pub const ARRAY_TYPE_BYTE_LENGTH: u8 = 24;
    pub const ARRAY_TYPE_AUXILIARY_BUFFER: u8 = 25;
    pub const UINT32_ZERO: u8 = 26;
    pub const UINT32_ONE: u8 = 27;
}

/// Largest decompressed buffer one document may claim; real ones are a few MB at most.
const MAX_BUFFER: usize = 256 << 20;
/// LZ4 expands a byte at most 255 times.
pub(crate) const LZ4_RATIO: usize = 255;

/// Whether `bytes` start with a binary KV3 signature this crate reads.
pub fn is_kv3(bytes: &[u8]) -> bool {
    let Some(&magic) = bytes.first_chunk::<4>() else {
        return false;
    };
    let magic = u32::from_le_bytes(magic);
    magic == MAGIC_LEGACY
        || (magic & 0xFFFF_FF00 == MAGIC_VERSIONED && (1..=5).contains(&(magic & 0xFF)))
}

/// Decodes a binary KV3 document (a resource block, or a whole file).
pub fn decode(bytes: &[u8]) -> Result<Document> {
    if !is_kv3(bytes) {
        return Err(malformed("KV3", "no KV3 signature"));
    }
    if bytes[..4] == MAGIC_LEGACY.to_le_bytes() {
        legacy::decode(bytes)
    } else {
        binary::decode(bytes)
    }
}

/// A zeroed buffer of `len` bytes, refused past [`MAX_BUFFER`].
pub(crate) fn buffer(len: usize) -> Result<Vec<u8>> {
    if len > MAX_BUFFER {
        return Err(malformed("KV3", format!("a {len}-byte buffer")));
    }
    Ok(vec![0; len])
}

/// One LZ4 block that decompresses to exactly `len` bytes; a length the input cannot reach is
/// refused before anything is allocated.
pub(crate) fn lz4(input: &[u8], len: usize) -> Result<Vec<u8>> {
    if len / LZ4_RATIO > input.len() {
        return Err(malformed(
            "KV3 LZ4",
            format!("{} bytes cannot expand to {len}", input.len()),
        ));
    }
    let mut out = buffer(len)?;
    match lz4_flex::block::decompress_into(input, &mut out) {
        Ok(n) if n == out.len() => Ok(out),
        Ok(n) => Err(malformed(
            "KV3 LZ4",
            format!("{n} bytes, expected {}", out.len()),
        )),
        Err(e) => Err(malformed("KV3 LZ4", e.to_string())),
    }
}

/// Decompresses zstd frames that must fill `out` exactly.
pub(crate) fn zstd(input: &[u8], out: &mut [u8]) -> Result<()> {
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    match decoder.decode_all(input, out) {
        Ok(n) if n == out.len() => Ok(()),
        Ok(n) => Err(malformed(
            "KV3 zstd",
            format!("{n} bytes, expected {}", out.len()),
        )),
        Err(e) => Err(malformed("KV3 zstd", e.to_string())),
    }
}

/// Decompresses zstd frames of unknown decoded size, at most `max` bytes.
pub(crate) fn zstd_to_vec(input: &[u8], max: usize) -> Result<Vec<u8>> {
    let mut out = buffer(max)?;
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    let len = decoder
        .decode_all(input, &mut out)
        .map_err(|e| malformed("zstd", e.to_string()))?;
    out.truncate(len);
    Ok(out)
}

/// A string-table entry, charged to `budget`; ids out of range (-1 is the empty string) read
/// as empty.
pub(crate) fn string(strings: &[String], id: i32, budget: &mut Budget) -> Result<String> {
    let text = usize::try_from(id).ok().and_then(|i| strings.get(i));
    budget.copy(text.map_or("", String::as_str))
}
