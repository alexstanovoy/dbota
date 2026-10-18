use crate::error::{Result, malformed};

const WHAT: &str = "vsnd mp3";
/// How far into the stream the first frame may start, past a tag.
const SYNC_SEARCH: usize = 4096;

/// The sample rate and channel count of the first frame of an MP3 stream, after an ID3v2 tag.
pub(super) fn first_frame(data: &[u8]) -> Result<(u32, u16)> {
    let mut at = 0;
    if data.starts_with(b"ID3") && data.len() >= 10 {
        // Four 7-bit bytes of size after the ten-byte header.
        let size = data[6..10]
            .iter()
            .fold(0usize, |n, &b| n << 7 | usize::from(b & 0x7F));
        at = 10 + size;
    }
    let end = data.len().min(at.saturating_add(SYNC_SEARCH));
    let window = data.get(at..end).unwrap_or_default();
    window
        .windows(4)
        .find_map(|h| header(h.try_into().ok()?))
        .ok_or_else(|| malformed(WHAT, "no frame header"))
}

/// `(rate, channels)` of a frame header, if it is one.
fn header(h: [u8; 4]) -> Option<(u32, u16)> {
    let version = (h[1] >> 3) & 3;
    let layer = (h[1] >> 1) & 3;
    let rate_index = usize::from((h[2] >> 2) & 3);
    let bitrate_index = h[2] >> 4;
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 || version == 1 || layer == 0 || rate_index == 3 {
        return None;
    }
    if bitrate_index == 0 || bitrate_index == 15 {
        return None;
    }
    let base = [44100, 48000, 32000][rate_index];
    let rate = match version {
        3 => base,
        2 => base / 2,
        _ => base / 4,
    };
    Some((rate, if h[3] >> 6 == 3 { 1 } else { 2 }))
}
