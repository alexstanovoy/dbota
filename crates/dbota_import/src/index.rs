//! The validated, bounded asset-cache index used by every cache reader.

use crate::import::{
    IDS, INDEX, Item, LAST_USE_MARKER, LAST_USE_TEXT, MANIFEST, OWNER_MARKER, PATH_LIMIT, Plan,
    READ_ONE_LIMIT, STAMP, WRITE_LIMIT, is_small, safe_path,
};
use crate::pack::{PackError, shown};
use std::collections::HashSet;
use std::sync::OnceLock;

/// Most files a cache holds besides the stamp and the index (a full one has ~1500).
pub const MAX_FILES: usize = 8192;
/// Largest file (the importer reads no more at once).
pub const MAX_FILE: u64 = READ_ONE_LIMIT as u64;
/// Most bytes of files (the importer writes no more).
pub const MAX_TOTAL: u64 = WRITE_LIMIT;
/// Most bytes of the files a client keeps in memory (the `small` ones; a full cache has 41 MB).
const MAX_SMALL_TOTAL: u64 = 128 << 20;
/// Longest index: a line is at most ~230 bytes.
pub const MAX_INDEX: u64 = 2 << 20;

/// The folders at the top of a cache: those the manifest's outputs go in, so a folder the
/// manifest adds (the loading screen's, the faces') is one a prepared cache may hold at once.
fn folders() -> &'static HashSet<String> {
    static FOLDERS: OnceLock<HashSet<String>> = OnceLock::new();
    FOLDERS.get_or_init(|| {
        // A manifest that cannot be read has no folders: nothing is accepted (the client does not
        // start on it either).
        Plan::parse(MANIFEST, IDS).map_or_else(
            |_| HashSet::new(),
            |plan| {
                let tops = plan.items.iter().flat_map(Item::outputs);
                tops.filter_map(|out| out.split_once('/').map(|(top, _)| top.to_owned()))
                    .collect()
            },
        )
    })
}

/// The most bytes a file of an extension may have: well above the largest the importer makes
/// (the map's geometry is 209 MB, its glTF 15 MB, a texture 22 MB, a particle file 0.2 MB) and
/// well below what a parser holds in memory several times over (a JSON file is a tree of
/// allocations ten times its size).
const LIMITS: [(&str, u64); 10] = [
    ("bin", MAX_FILE),
    ("glb", 64 << 20),
    ("gltf", 64 << 20),
    ("json", 16 << 20),
    ("ktx2", 64 << 20),
    ("mp3", 16 << 20),
    ("otf", 8 << 20),
    ("png", 64 << 20),
    ("txt", 16 << 20),
    ("wav", 64 << 20),
];

/// The most bytes a cache file at `path` may have: by its extension, else the most any file may.
pub fn size_limit(path: &str) -> u64 {
    let extension = path.rsplit_once('.').map(|(_, e)| e);
    LIMITS
        .iter()
        .find(|(e, _)| Some(*e) == extension)
        .map_or(MAX_FILE, |(_, limit)| *limit)
}

/// A permitted container path: top-level metadata or a plain asset path with an imported extension.
pub(crate) fn check_place(path: &str) -> Result<(), PackError> {
    if is_metadata(path) {
        return Ok(());
    }
    let unsafe_path = || PackError::Unsafe(shown(path));
    if !safe_path(path) || path.len() > PATH_LIMIT {
        return Err(unsafe_path());
    }
    let (top, rest) = path.split_once('/').ok_or_else(unsafe_path)?;
    let extension = rest.rsplit_once('.').map(|(_, e)| e);
    if folders().contains(top) && extension.is_some_and(|e| LIMITS.iter().any(|(l, _)| *l == e)) {
        Ok(())
    } else {
        Err(unsafe_path())
    }
}

/// A file whose path, size and memory-residency flag passed index validation.
#[derive(Clone, Debug)]
pub struct IndexFile {
    pub path: String,
    pub size: u64,
    pub small: bool,
}

/// Constructed only after the complete index passes its byte, entry and aggregate limits.
#[derive(Clone, Debug)]
pub struct CacheIndex {
    text: String,
    files: Vec<IndexFile>,
    bytes: u64,
}

impl CacheIndex {
    pub fn header() -> String {
        format!("dbota-cache 1 rev {}", crate::import::IMPORT_REV)
    }

    /// Checked before native reads or JS-to-WASM copies allocate from an input's length.
    pub fn check_length(size: u64) -> Result<(), PackError> {
        if size > MAX_INDEX {
            return Err(PackError::TooBig(format!(
                "index.txt is {size} bytes, at most {MAX_INDEX}"
            )));
        }
        Ok(())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, PackError> {
        Self::check_length(bytes.len() as u64)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|_| PackError::Invalid("index.txt is not text".to_owned()))?;
        let mut lines = text.split_inclusive('\n');
        let first = lines.next().unwrap_or_default();
        if first.strip_suffix('\n') != Some(Self::header().as_str()) {
            return Err(PackError::Invalid(format!(
                "index.txt starts with {:?}, not {:?}",
                shown(first.trim_end()),
                Self::header()
            )));
        }
        let (mut files, mut seen) = (Vec::new(), HashSet::new());
        let (mut total, mut small_total) = (0u64, 0u64);
        for line in lines {
            if files.len() == MAX_FILES {
                return Err(PackError::TooBig(format!(
                    "index.txt has more than {MAX_FILES} files"
                )));
            }
            let file = checked_line(line)?;
            if is_metadata(&file.path) || !seen.insert(file.path.clone()) {
                return Err(PackError::Invalid(format!(
                    "index.txt lists {} again",
                    shown(&file.path)
                )));
            }
            check_file(&file)?;
            total += file.size;
            if file.small {
                small_total += file.size;
            }
            if total > MAX_TOTAL || small_total > MAX_SMALL_TOTAL {
                return Err(PackError::TooBig(format!(
                    "{total} bytes of files ({small_total} to keep in memory), at most {MAX_TOTAL} ({MAX_SMALL_TOTAL})"
                )));
            }
            files.push(file);
        }
        Ok(Self {
            text: text.to_owned(),
            files,
            bytes: total,
        })
    }

    pub fn files(&self) -> &[IndexFile] {
        &self.files
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    #[cfg(any(unix, windows))]
    pub fn read(root: &std::path::Path) -> Result<Self, PackError> {
        use std::io::Read;
        let path = root.join(INDEX);
        let io = |e| PackError::Io(format!("cannot read {}: {e}", path.display()));
        let meta = std::fs::symlink_metadata(&path).map_err(io)?;
        if !meta.is_file() {
            return Err(PackError::Invalid(
                "index.txt is not a regular file".to_owned(),
            ));
        }
        Self::check_length(meta.len())?;
        let mut bytes = Vec::new();
        // Metadata inspection and this bounded open are separate. A same-user path swap
        // can follow a link or block on a FIFO; cache parents must not be replaced concurrently.
        std::fs::File::open(&path)
            .map_err(io)?
            .take(MAX_INDEX + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        Self::parse(&bytes)
    }
}

fn checked_line(line: &str) -> Result<IndexFile, PackError> {
    let line = line
        .strip_suffix('\n')
        .ok_or_else(|| PackError::Invalid("index.txt does not end with a new line".to_owned()))?;
    let invalid = || {
        PackError::Invalid(format!(
            "index.txt has a line it cannot read: {:?}",
            shown(line)
        ))
    };
    let mut words = line.split(' ');
    if words.next() != Some("file") {
        return Err(invalid());
    }
    let size = words.next().ok_or_else(invalid)?;
    if size.is_empty() || size.len() > 10 || !size.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let path = words.next().ok_or_else(invalid)?;
    check_place(path)?;
    let small = match words.next() {
        None => false,
        Some("small") => true,
        Some(_) => return Err(invalid()),
    };
    if words.next().is_some() {
        return Err(invalid());
    }
    Ok(IndexFile {
        path: path.to_owned(),
        size: size.parse().map_err(|_| invalid())?,
        small,
    })
}

fn check_file(file: &IndexFile) -> Result<(), PackError> {
    let limit = size_limit(&file.path);
    if file.size > limit {
        return Err(PackError::TooBig(format!(
            "{} is {} bytes, at most {limit}",
            shown(&file.path),
            file.size
        )));
    }
    if file.small != is_small(&file.path, file.size) {
        return Err(PackError::Invalid(format!(
            "index.txt marks {} wrongly as {}",
            shown(&file.path),
            if file.small { "small" } else { "not small" }
        )));
    }
    Ok(())
}

pub(crate) fn is_metadata(path: &str) -> bool {
    matches!(path, STAMP | INDEX | OWNER_MARKER | LAST_USE_MARKER)
}

pub(crate) fn last_use_seconds(text: &str) -> Option<u64> {
    let seconds = text.strip_prefix(LAST_USE_TEXT)?.strip_suffix('\n')?;
    if seconds.is_empty() || seconds.len() > 20 || !seconds.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    seconds.parse().ok()
}
