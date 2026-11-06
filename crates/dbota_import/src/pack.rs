//! A prepared asset cache: the folder `dbota import` writes, or a zip of it, that its owner hands
//! to a player without Dota 2. The same checks run natively (`dbota import-cache`, the menu) and
//! in the browser worker before a single file is copied into the client's cache.
//!
//! Everything in a prepared cache is untrusted: its stamp must be this build's asset
//! fingerprint, `index.txt` must list exactly the files present (sizes included), every path must
//! be a plain one in a folder and with an extension the importer makes, and every size is
//! bounded, before anything is read but the stamp and the index. Where the files come from is a
//! [`Container`]: a zip ([`crate::zip`]), a folder native ([`crate::pack_dir`]) or the files a
//! browser's folder picker lists ([`SliceFolder`]).

use std::collections::{HashMap, HashSet};
use std::fmt;

use heatel::Slice;

use crate::import::{
    INDEX, LAST_USE_MARKER, MAX_STAMP, OWNER_MARKER, OWNER_TEXT, STAMP, fingerprint, stamped_here,
};
use crate::index::{CacheIndex, MAX_INDEX, check_place, is_metadata, last_use_seconds};
pub use crate::index::{MAX_FILE, MAX_FILES, MAX_TOTAL, size_limit};
/// Asset files plus the index, stamp and optional local ownership/usage metadata.
pub const MAX_ENTRIES: usize = MAX_FILES + 4;

/// Why a prepared cache is not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum PackError {
    /// The stamp names assets of another build (`found`), not this one's (`needed`).
    OtherBuild { found: String, needed: String },
    /// A file's name that must not be in a cache (shown made printable).
    Unsafe(String),
    /// It is not a cache, or a broken one.
    Invalid(String),
    /// More than a cache is.
    TooBig(String),
    /// A file could not be read or written.
    Io(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherBuild { found, needed } => write!(
                f,
                "This asset cache is for assets {found}, but this dbota needs assets {needed}. \
                 Ask for a cache prepared by this version of dbota, or import from your own \
                 Dota 2 install."
            ),
            Self::Unsafe(path) => write!(f, "The asset cache holds a file it must not: {path}"),
            Self::Invalid(why) => write!(f, "This is not a usable asset cache: {why}"),
            Self::TooBig(what) => write!(f, "This is too big for an asset cache: {what}"),
            Self::Io(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for PackError {}

/// `text` as printable ASCII, cut: it comes from a cache that may be anything.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 64;
    let mut out: String = text
        .chars()
        .take(SHOWN)
        .map(|c| if c.is_ascii_graphic() { c } else { '?' })
        .collect();
    if text.chars().count() > SHOWN {
        out.push_str("...");
    }
    out
}

/// One file of a container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the cache's top, `/`-separated, as the container names it.
    pub path: String,
    pub size: u64,
}

/// A place the files of a prepared cache are read from. Its names are the cache's own, without
/// a folder around them ([`without_root`] cuts one).
pub trait Container {
    fn entries(&self) -> &[Entry];

    /// The bytes of entry `at`: exactly its size, or why not.
    fn read(&self, at: usize) -> Result<Vec<u8>, PackError>;
}

impl Container for Box<dyn Container> {
    fn entries(&self) -> &[Entry] {
        (**self).entries()
    }

    fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
        (**self).read(at)
    }
}

/// How many bytes of every name to cut when the cache was packed with its folder around it
/// (`v17/stamp.txt`, what a file manager's "compress" makes, or what a browser lists for a
/// picked folder): the folder and its `/`; none when the stamp is at the top.
pub fn without_root(names: &[&str]) -> usize {
    if names.contains(&STAMP) {
        return 0;
    }
    let Some(root) = names
        .iter()
        .find_map(|n| n.strip_suffix(STAMP)?.strip_suffix('/'))
        .filter(|root| !root.is_empty() && !root.contains('/'))
    else {
        return 0;
    };
    let prefix = root.len() + 1;
    if names
        .iter()
        .all(|n| n.starts_with(root) && n[root.len()..].starts_with('/'))
    {
        prefix
    } else {
        0
    }
}

/// A prepared cache that passed every check: its stamp, its index and the files the index lists.
pub struct Pack<C> {
    container: C,
    stamp: String,
    index: CacheIndex,
    /// The container's entries of the listed files, in the index's order.
    files: Vec<usize>,
    bytes: u64,
}

impl<C: Container> Pack<C> {
    /// Checks the container as a cache of this build and keeps it for [`read`](Self::read).
    pub fn open(container: C) -> Result<Self, PackError> {
        let entries = container.entries();
        if entries.len() > MAX_ENTRIES {
            return Err(PackError::TooBig(format!(
                "it has {} files, a cache has at most {MAX_ENTRIES} entries including metadata",
                entries.len()
            )));
        }
        let mut at = HashMap::with_capacity(entries.len());
        for (n, entry) in entries.iter().enumerate() {
            if at.insert(entry.path.as_str(), n).is_some() {
                return Err(PackError::Invalid(format!(
                    "{} is in it twice",
                    shown(&entry.path)
                )));
            }
        }
        // The stamp first: a cache of another build may well have other folders and files.
        let stamp = text(&container, &at, STAMP, MAX_STAMP)?;
        let stamp = stamp.trim();
        if !stamped_here(stamp) {
            return Err(PackError::OtherBuild {
                found: fingerprint_of_stamp(stamp),
                needed: fingerprint(),
            });
        }
        entries
            .iter()
            .try_for_each(|entry| check_place(&entry.path))?;
        if at.contains_key(OWNER_MARKER)
            && text(&container, &at, OWNER_MARKER, OWNER_TEXT.len() as u64)? != OWNER_TEXT
        {
            return Err(PackError::Invalid(
                "invalid local ownership marker".to_owned(),
            ));
        }
        if at.contains_key(LAST_USE_MARKER)
            && last_use_seconds(&text(&container, &at, LAST_USE_MARKER, 96)?).is_none()
        {
            return Err(PackError::Invalid(
                "invalid local last-use stamp".to_owned(),
            ));
        }
        let index = text(&container, &at, INDEX, MAX_INDEX)?;
        let index = CacheIndex::parse(index.as_bytes())?;
        let files = list(entries, &at, &index)?;
        let bytes = index.bytes();
        let stamp = stamp.to_owned();
        Ok(Self {
            container,
            stamp,
            index,
            files,
            bytes,
        })
    }

    /// What the cache was made from; this build's fingerprint starts it.
    pub fn stamp(&self) -> &str {
        &self.stamp
    }

    /// The index file's text, as the cache has it.
    pub fn index(&self) -> &str {
        self.index.text()
    }

    /// How many files the index lists.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Bytes of the listed files together.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The `n`th listed file's path and size.
    pub fn entry(&self, n: usize) -> Option<&Entry> {
        self.container.entries().get(*self.files.get(n)?)
    }

    /// The `n`th listed file's bytes, exactly as long as the index says.
    pub fn read(&self, n: usize) -> Result<Vec<u8>, PackError> {
        let entry = self
            .entry(n)
            .ok_or_else(|| PackError::Invalid(format!("there is no file {n}")))?;
        let bytes = self.container.read(self.files[n])?;
        if bytes.len() as u64 != entry.size {
            return Err(PackError::Invalid(format!(
                "{} is {} bytes, not the {} its index says",
                shown(&entry.path),
                bytes.len(),
                entry.size
            )));
        }
        Ok(bytes)
    }
}

/// The part of a stamp that names the assets (`rev13.1/<plan>`), made printable.
fn fingerprint_of_stamp(stamp: &str) -> String {
    let mut parts = stamp.splitn(3, '/');
    let named: Vec<&str> = parts.by_ref().take(2).collect();
    shown(&named.join("/"))
}

/// The text file `name` of the container, at most `max` bytes.
fn text(
    container: &impl Container,
    at: &HashMap<&str, usize>,
    name: &str,
    max: u64,
) -> Result<String, PackError> {
    let missing = || PackError::Invalid(format!("it has no {name} at its top"));
    let n = *at.get(name).ok_or_else(missing)?;
    let entry = &container.entries()[n];
    if entry.size > max {
        return Err(PackError::TooBig(format!(
            "{name} is {} bytes, at most {max}",
            entry.size
        )));
    }
    let bytes = container.read(n)?;
    if bytes.len() as u64 != entry.size {
        return Err(PackError::Invalid(format!(
            "{name} is not as long as it says"
        )));
    }
    String::from_utf8(bytes).map_err(|_| PackError::Invalid(format!("{name} is not text")))
}

/// Checks a validated index against its container: identical paths and sizes, nothing extra.
fn list(
    entries: &[Entry],
    at: &HashMap<&str, usize>,
    index: &CacheIndex,
) -> Result<Vec<usize>, PackError> {
    let mut files = Vec::new();
    for file in index.files() {
        let n = *at.get(file.path.as_str()).ok_or_else(|| {
            PackError::Invalid(format!(
                "index.txt lists {}, which is not in it",
                shown(&file.path)
            ))
        })?;
        if file.size != entries[n].size {
            return Err(PackError::Invalid(format!(
                "{} is {} bytes, but index.txt says {}",
                shown(&file.path),
                entries[n].size,
                file.size
            )));
        }
        files.push(n);
    }
    let seen: HashSet<&str> = index
        .files()
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    if let Some(extra) = entries
        .iter()
        .find(|e| !is_metadata(&e.path) && !seen.contains(e.path.as_str()))
    {
        return Err(PackError::Invalid(format!(
            "{} is in it, but index.txt does not list it",
            shown(&extra.path)
        )));
    }
    Ok(files)
}

/// The files a browser's folder picker lists, read by slices (`File`s in a worker, byte vectors
/// in tests and the fuzzer).
pub struct SliceFolder<S> {
    entries: Vec<Entry>,
    slices: Vec<S>,
}

impl<S: Slice> SliceFolder<S> {
    /// The files by their paths as the picker names them, with the folder picked around them.
    pub fn new(files: Vec<(String, S)>) -> Result<Self, PackError> {
        if files.len() > MAX_ENTRIES {
            return Err(PackError::TooBig(format!(
                "it has {} files, a cache has at most {MAX_ENTRIES} entries including metadata",
                files.len()
            )));
        }
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        let cut = without_root(&names);
        let (mut entries, mut slices) = (Vec::new(), Vec::new());
        for (name, slice) in files {
            entries.push(Entry {
                path: name[cut..].to_owned(),
                size: slice.size(),
            });
            slices.push(slice);
        }
        Ok(Self { entries, slices })
    }
}

impl<S: Slice> Container for SliceFolder<S> {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
        let (entry, slice) = (&self.entries[at], &self.slices[at]);
        let len = usize::try_from(entry.size).map_err(|_| PackError::TooBig(shown(&entry.path)))?;
        slice
            .read_at(0, len)
            .map_err(|e| PackError::Io(format!("cannot read {}: {e}", shown(&entry.path))))
    }
}
