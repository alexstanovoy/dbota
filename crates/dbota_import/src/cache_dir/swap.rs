//! Ownership of the temporary directories in a cache swap. The cache lock covers every step.

use super::{
    INDEX, OWNER_MARKER, OWNER_TEXT, STAMP, ensure_marker, managed_io, marker_matches, read_stamp,
    remove, sibling,
};
use crate::index::{MAX_INDEX, is_metadata};
use heatel::Result;
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Role {
    Previous,
    Staging,
}

impl Role {
    fn marker(self) -> &'static str {
        match self {
            Self::Previous => ".dbota-import-previous",
            Self::Staging => ".dbota-import-staging",
        }
    }

    fn identity(self, cache: &Path, (volume, index): (u64, u64)) -> Vec<u8> {
        let mut bytes = b"dbota import swap v1\0".to_vec();
        bytes.extend_from_slice(self.marker().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(cache.as_os_str().as_encoded_bytes());
        bytes.extend_from_slice(&volume.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes
    }

    fn mark(self, path: &Path, cache: &Path, directory: (u64, u64)) -> Result<()> {
        ensure_marker(&path.join(self.marker()), &self.identity(cache, directory))
    }

    fn owns(self, path: &Path, cache: &Path) -> Result<bool> {
        match entry(path)? {
            Entry::Directory(directory) => {
                marker_matches(&path.join(self.marker()), &self.identity(cache, directory))
            }
            Entry::Missing | Entry::Other => Ok(false),
        }
    }

    fn clear(self, cache: &Path) -> Result<()> {
        if self.owns(cache, cache)? {
            let marker = cache.join(self.marker());
            fs::remove_file(&marker).map_err(|why| managed_io(&marker, why))?;
        }
        Ok(())
    }
}

enum Entry {
    Missing,
    Directory((u64, u64)),
    Other,
}

fn entry(path: &Path) -> Result<Entry> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if plain_dir(&metadata) => directory_identity(path, &metadata)
            .map(Entry::Directory)
            .map_err(|why| managed_io(path, why)),
        Ok(_) => Ok(Entry::Other),
        Err(why) if why.kind() == io::ErrorKind::NotFound => Ok(Entry::Missing),
        Err(why) => Err(managed_io(path, why)),
    }
}

#[cfg(unix)]
fn directory_identity(_: &Path, metadata: &Metadata) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn directory_identity(path: &Path, _: &Metadata) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    const OPEN_DIRECTORY: u32 = 0x0200_0000;
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(OPEN_DIRECTORY | OPEN_REPARSE_POINT)
        .open(path)?;
    if !plain_dir(&file.metadata()?) {
        return Err(io::Error::other(
            "cache path ceased to be a plain directory",
        ));
    }
    let info = winapi_util::file::information(&file)?;
    let index = info.file_index();
    if index == 0 {
        return Err(io::Error::other(
            "cache filesystem supplied no directory identity",
        ));
    }
    Ok((info.volume_serial_number(), index))
}

fn plain_dir(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & REPARSE_POINT != 0 {
            return false;
        }
    }
    metadata.is_dir()
}

pub(super) fn canonical_cache(cache: &Path) -> Result<PathBuf> {
    let name = cache
        .file_name()
        .ok_or_else(|| managed_io(cache, io::Error::other("the cache folder has no name")))?;
    let parent = cache.parent().filter(|path| !path.as_os_str().is_empty());
    fs::canonicalize(parent.unwrap_or(Path::new(".")))
        .map(|parent| parent.join(name))
        .map_err(|why| managed_io(cache, why))
}

fn occupied(path: &Path) -> heatel::Error {
    managed_io(
        path,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the import cannot use this path because it is occupied by an unrelated entry. \
             Move or delete it, then try again",
        ),
    )
}

/// The first line of every index a dbota importer wrote (any build's).
const INDEX_HEADER: &str = "dbota-cache ";
/// The most entries and the deepest level a cache holds: a folder with more is not one.
const MAX_ENTRIES: usize = 16_384;
const MAX_DEPTH: usize = 8;

/// Refuses, before anything is written, a cache path the importer could not replace without
/// deleting what is not its own (see [`check_own_directory`]); a link or a file there is refused
/// as it always was.
pub(super) fn check_replaceable(cache: &Path) -> Result<()> {
    match entry(cache)? {
        Entry::Missing => Ok(()),
        Entry::Other => Err(occupied(cache)),
        Entry::Directory(_) => check_own_directory(cache),
    }
}

/// A folder at the cache path is replaced (moved aside and removed) only when the importer made
/// it: nothing is in it, or it holds a finished cache (an owned marker, or a stamp and an index
/// with a dbota header) and nothing but that cache's own files, listed in its index. Anything
/// else is somebody's data.
fn check_own_directory(cache: &Path) -> Result<()> {
    let empty = fs::read_dir(cache)
        .map_err(|why| managed_io(cache, why))?
        .next()
        .is_none();
    if empty {
        return Ok(());
    }
    let index = index_text(cache);
    let marked = marker_matches(&cache.join(OWNER_MARKER), OWNER_TEXT.as_bytes())?;
    let finished = index
        .as_deref()
        .is_some_and(|text| text.starts_with(INDEX_HEADER))
        && read_stamp(cache).is_some()
        && fs::symlink_metadata(cache.join(STAMP)).is_ok_and(|meta| meta.is_file());
    if !marked && !finished {
        return Err(refused(
            cache,
            "it is not a dbota cache and is not empty. Choose an empty folder or remove it yourself.",
        ));
    }
    match first_foreign(cache, index.as_deref().unwrap_or_default())? {
        Some(name) => Err(refused(
            cache,
            &format!(
                "it holds {name:?}, which dbota did not make. Move it somewhere else, or choose an empty folder."
            ),
        )),
        None => Ok(()),
    }
}

fn refused(cache: &Path, why: &str) -> heatel::Error {
    managed_io(cache, io::Error::new(io::ErrorKind::DirectoryNotEmpty, why))
}

/// The cache's index, when it is a plain file no longer than an index may be.
fn index_text(cache: &Path) -> Option<String> {
    let path = cache.join(INDEX);
    if !fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_INDEX + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_INDEX).then_some(text)
}

/// The first thing below `cache` the importer did not write, as its path: a file the index does
/// not list (the importer's own metadata at the top excepted), a link or a special file, or
/// more than the importer ever makes.
fn first_foreign(cache: &Path, index: &str) -> Result<Option<String>> {
    let listed: HashSet<&str> = index
        .lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            (words.next() == Some("file"))
                .then(|| words.nth(1))
                .flatten()
        })
        .collect();
    let mut pending = vec![(cache.to_path_buf(), String::new(), 0usize)];
    let mut count = 0usize;
    while let Some((directory, prefix, depth)) = pending.pop() {
        for found in fs::read_dir(&directory).map_err(|why| managed_io(&directory, why))? {
            count += 1;
            let found = found.map_err(|why| managed_io(&directory, why))?;
            let name = found.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let kind = found
                .file_type()
                .map_err(|why| managed_io(&directory, why))?;
            let own = if kind.is_dir() {
                depth < MAX_DEPTH
            } else if kind.is_file() {
                listed.contains(path.as_str())
                    || (depth == 0 && (is_metadata(&name) || name.starts_with(".dbota-")))
            } else {
                false
            };
            if !own || count > MAX_ENTRIES {
                return Ok(Some(path));
            }
            if kind.is_dir() {
                pending.push((found.path(), path, depth + 1));
            }
        }
    }
    Ok(None)
}

fn report_unowned(path: &Path) {
    eprintln!(
        "dbota import: leaving {} untouched because it is not part of this cache's import",
        path.display()
    );
}

pub(super) fn recover(cache: &Path) -> Result<()> {
    let old = sibling(cache, "old")?;
    if Role::Previous.owns(&old, cache)? {
        match entry(cache)? {
            Entry::Missing => fs::rename(&old, cache).map_err(|why| managed_io(cache, why))?,
            Entry::Directory(_) if Role::Staging.owns(cache, cache)? => remove(&old)?,
            _ => eprintln!(
                "dbota import: previous cache stays at {} because {} is occupied",
                old.display(),
                cache.display()
            ),
        }
    } else if !matches!(entry(&old)?, Entry::Missing) {
        report_unowned(&old);
    }
    Role::Previous.clear(cache)?;
    Role::Staging.clear(cache)?;
    discard_staging(&sibling(cache, "importing")?, cache)
}

pub(super) fn begin_staging(cache: &Path) -> Result<PathBuf> {
    let staging = sibling(cache, "importing")?;
    fs::create_dir(&staging).map_err(|why| {
        if why.kind() == io::ErrorKind::AlreadyExists {
            occupied(&staging)
        } else {
            managed_io(&staging, why)
        }
    })?;
    let result = entry(&staging).and_then(|entry| match entry {
        Entry::Directory(directory) => Role::Staging.mark(&staging, cache, directory),
        _ => Err(occupied(&staging)),
    });
    if let Err(why) = result {
        if let Err(cleanup) = fs::remove_dir(&staging) {
            eprintln!(
                "dbota import: unfinished staging directory {} stays: {cleanup}",
                staging.display()
            );
        }
        return Err(why);
    }
    Ok(staging)
}

pub(super) fn discard_staging(staging: &Path, cache: &Path) -> Result<()> {
    if Role::Staging.owns(staging, cache)? {
        remove(staging)?;
    } else if !matches!(entry(staging)?, Entry::Missing) {
        report_unowned(staging);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Checkpoint {
    BeforeMoveAside,
    AfterMoveAside,
    BeforePublish,
    AfterPublish,
}

pub(super) fn publish(staging: &Path, cache: &Path, checkpoint: impl Fn(Checkpoint)) -> Result<()> {
    if !Role::Staging.owns(staging, cache)? {
        return Err(occupied(staging));
    }
    let old = sibling(cache, "old")?;
    match entry(cache)? {
        Entry::Missing => {}
        Entry::Other => return Err(occupied(cache)),
        Entry::Directory(directory) => {
            check_own_directory(cache)?;
            if !matches!(entry(&old)?, Entry::Missing) {
                return Err(occupied(&old));
            }
            Role::Previous.mark(cache, cache, directory)?;
            checkpoint(Checkpoint::BeforeMoveAside);
            if let Err(why) = fs::rename(cache, &old) {
                Role::Previous.clear(cache)?;
                return Err(managed_io(cache, why));
            }
            checkpoint(Checkpoint::AfterMoveAside);
        }
    }
    checkpoint(Checkpoint::BeforePublish);
    if let Err(why) = fs::rename(staging, cache) {
        if let Err(recovery) = recover(cache) {
            eprintln!("dbota import: could not settle the interrupted publication: {recovery}");
        }
        return Err(managed_io(cache, why));
    }
    checkpoint(Checkpoint::AfterPublish);
    // Publication has happened. A cleanup error must not tell the gate it can still cancel.
    if let Err(why) = recover(cache) {
        eprintln!("dbota import: published the cache but could not finish cleanup: {why}");
    }
    Ok(())
}
