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
use std::sync::OnceLock;

use heatel::Slice;

use crate::import::{
    IDS, INDEX, Item, MANIFEST, MAX_STAMP, PATH_LIMIT, Plan, READ_ONE_LIMIT, STAMP, WRITE_LIMIT,
    fingerprint, is_small, safe_path, stamped_here,
};

/// Most files a cache holds besides the stamp and the index (a full one has ~1500).
pub const MAX_FILES: usize = 8192;
/// Largest file (the importer reads no more at once).
pub const MAX_FILE: u64 = READ_ONE_LIMIT as u64;
/// Most bytes of files (the importer writes no more).
pub const MAX_TOTAL: u64 = WRITE_LIMIT;
/// Most bytes of the files a client keeps in memory (the `small` ones; a full cache has 41 MB).
const MAX_SMALL_TOTAL: u64 = 128 << 20;
/// Longest index: a line is at most ~230 bytes.
const MAX_INDEX: u64 = 2 << 20;

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
const LIMITS: [(&str, u64); 9] = [
    ("bin", MAX_FILE),
    ("glb", 64 << 20),
    ("gltf", 64 << 20),
    ("json", 16 << 20),
    ("ktx2", 64 << 20),
    ("mp3", 16 << 20),
    ("png", 64 << 20),
    ("txt", 16 << 20),
    ("wav", 64 << 20),
];

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
    index: String,
    /// The container's entries of the listed files, in the index's order.
    files: Vec<usize>,
    bytes: u64,
}

impl<C: Container> Pack<C> {
    /// Checks the container as a cache of this build and keeps it for [`read`](Self::read).
    pub fn open(container: C) -> Result<Self, PackError> {
        let entries = container.entries();
        if entries.len() > MAX_FILES + 2 {
            return Err(PackError::TooBig(format!(
                "it has {} files, a cache has at most {MAX_FILES}",
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
        let index = text(&container, &at, INDEX, MAX_INDEX)?;
        let (files, bytes) = list(entries, &at, &index)?;
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
        &self.index
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

/// A cache file that is not the stamp or the index: a plain path in one of the cache's folders,
/// with an extension the importer makes.
fn check_place(path: &str) -> Result<(), PackError> {
    if path == STAMP || path == INDEX {
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

/// One line of the index: a file the cache says it holds.
struct Listed<'a> {
    path: &'a str,
    size: u64,
    small: bool,
}

fn listed(line: &str) -> Option<Listed<'_>> {
    let mut words = line.split(' ');
    (words.next()? == "file").then_some(())?;
    let size = words.next()?;
    // A size is plain digits: no sign, no spaces, not longer than any file's.
    if size.is_empty() || size.len() > 10 || !size.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let path = words.next()?;
    let small = match words.next() {
        None => false,
        Some("small") => true,
        Some(_) => return None,
    };
    words.next().is_none().then_some(())?;
    Some(Listed {
        path,
        size: size.parse().ok()?,
        small,
    })
}

/// Checks `index` against the container's `entries`: the same files with the same sizes, nothing
/// else; the entries of the listed files in the index's order, and their bytes together.
fn list(
    entries: &[Entry],
    at: &HashMap<&str, usize>,
    index: &str,
) -> Result<(Vec<usize>, u64), PackError> {
    let header = format!("dbota-cache 1 rev {}", crate::import::IMPORT_REV);
    let mut lines = index.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.strip_suffix('\n') != Some(header.as_str()) {
        return Err(PackError::Invalid(format!(
            "index.txt starts with {:?}, not {header:?}",
            shown(first.trim_end())
        )));
    }
    let (mut files, mut seen) = (Vec::new(), HashSet::new());
    let (mut bytes, mut small_bytes) = (0u64, 0u64);
    for line in lines {
        let line = line.strip_suffix('\n').ok_or_else(|| {
            PackError::Invalid("index.txt does not end with a new line".to_owned())
        })?;
        let file = listed(line).ok_or_else(|| {
            PackError::Invalid(format!(
                "index.txt has a line it cannot read: {:?}",
                shown(line)
            ))
        })?;
        check_place(file.path)?;
        if file.path == STAMP || file.path == INDEX || !seen.insert(file.path) {
            return Err(PackError::Invalid(format!(
                "index.txt lists {} again",
                shown(file.path)
            )));
        }
        let n = *at.get(file.path).ok_or_else(|| {
            PackError::Invalid(format!(
                "index.txt lists {}, which is not in it",
                shown(file.path)
            ))
        })?;
        check_file(&entries[n], &file)?;
        bytes += file.size;
        if file.small {
            small_bytes += file.size;
        }
        files.push(n);
    }
    if let Some(extra) = entries
        .iter()
        .find(|e| e.path != STAMP && e.path != INDEX && !seen.contains(e.path.as_str()))
    {
        return Err(PackError::Invalid(format!(
            "{} is in it, but index.txt does not list it",
            shown(&extra.path)
        )));
    }
    if bytes > MAX_TOTAL || small_bytes > MAX_SMALL_TOTAL {
        return Err(PackError::TooBig(format!(
            "{bytes} bytes of files ({small_bytes} to keep in memory), at most {MAX_TOTAL} ({MAX_SMALL_TOTAL})"
        )));
    }
    assert_eq!(files.len() + 2, entries.len());
    Ok((files, bytes))
}

/// One listed file against the entry the container has: its size, its limit and its `small`
/// mark (which makes a client keep the file in memory, so it is the importer's rule only).
fn check_file(entry: &Entry, file: &Listed<'_>) -> Result<(), PackError> {
    if file.size != entry.size {
        return Err(PackError::Invalid(format!(
            "{} is {} bytes, but index.txt says {}",
            shown(file.path),
            entry.size,
            file.size
        )));
    }
    let extension = file.path.rsplit_once('.').map(|(_, e)| e);
    let limit = LIMITS
        .iter()
        .find(|(e, _)| Some(*e) == extension)
        .map_or(MAX_FILE, |(_, limit)| *limit);
    if file.size > limit {
        return Err(PackError::TooBig(format!(
            "{} is {} bytes, at most {limit}",
            shown(file.path),
            file.size
        )));
    }
    if file.small != is_small(file.path, file.size) {
        return Err(PackError::Invalid(format!(
            "index.txt marks {} wrongly as {}",
            shown(file.path),
            if file.small { "small" } else { "not small" }
        )));
    }
    Ok(())
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
        if files.len() > MAX_FILES + 2 {
            return Err(PackError::TooBig(format!(
                "it has {} files, a cache has at most {MAX_FILES}",
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
