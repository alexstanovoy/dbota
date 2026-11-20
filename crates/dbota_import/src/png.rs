//! The decoded PNG envelope shared by cache export and playback.

use std::collections::HashMap;

pub const HEADER_BYTES: usize = 26;
pub const MAGIC: [u8; 8] = *b"\x89PNG\r\n\x1a\n";
pub const MAX_IMAGE_BYTES: usize = 16 << 20;
pub const MAX_PATHS_PER_CLASS: usize = crate::index::MAX_FILES / 2;
/// One maximum-sized decode can overlap an admitted cache inventory.
pub const PLAYBACK_MARGIN: usize = MAX_IMAGE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    Interface,
    World,
}

impl Class {
    pub fn of(path: &str) -> Self {
        if path.starts_with("textures/ground/") || path.starts_with("map/") {
            Self::World
        } else {
            Self::Interface
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Interface => "interface",
            Self::World => "world",
        }
    }

    pub const fn import_bytes(self) -> usize {
        match self {
            Self::Interface => 192 << 20,
            Self::World => 128 << 20,
        }
    }

    pub const fn playback_bytes(self) -> usize {
        self.import_bytes() + PLAYBACK_MARGIN
    }
}

pub fn is_png_path(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("png"))
}

/// Upper bound for the decoded texels, before a decoder allocates.
pub fn decoded_bytes(head: &[u8]) -> Result<usize, String> {
    if !head.starts_with(&MAGIC) {
        return Err("no texture header for a PNG".into());
    }
    let header = head
        .get(8..HEADER_BYTES)
        .filter(|header| header[4..8] == *b"IHDR")
        .ok_or("a PNG without its header")?;
    let side = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let (width, height) = (side(8), side(12));
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return Err(format!("{width} x {height} texels"));
    }
    let texel = if header[16] == 16 { 8 } else { 4 };
    Ok(width as usize * height as usize * texel)
}

#[derive(Default)]
struct Total {
    bytes: usize,
    paths: HashMap<String, usize>,
}

/// Counts every PNG exported, including art that is loaded only outside matches.
#[derive(Default)]
pub struct Inventory([Total; 2]);

impl Inventory {
    pub fn record(&mut self, path: &str, bytes: usize) -> Result<(), String> {
        let class = Class::of(path);
        let name = class.name();
        if bytes > MAX_IMAGE_BYTES {
            return Err(format!(
                "{name} PNG {path} decodes to {bytes} bytes, above the {MAX_IMAGE_BYTES}-byte per-image bound"
            ));
        }
        let total = &mut self.0[class as usize];
        let previous = total.paths.get(path).copied();
        let updated = total.bytes - previous.unwrap_or(0) + bytes;
        let limit = class.import_bytes();
        if updated > limit {
            return Err(format!(
                "{name} PNGs total {updated} decoded bytes, above the {limit}-byte import bound"
            ));
        }
        if previous.is_none() && total.paths.len() == MAX_PATHS_PER_CLASS {
            return Err(format!(
                "{name} PNGs total {} paths, above the {MAX_PATHS_PER_CLASS}-path import bound",
                total.paths.len() + 1
            ));
        }
        total.bytes = updated;
        total.paths.insert(path.to_owned(), bytes);
        Ok(())
    }
}
