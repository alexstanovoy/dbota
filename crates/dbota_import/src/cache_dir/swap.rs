//! Ownership of the temporary directories in a cache swap. The cache lock covers every step.

use super::{ensure_marker, managed_io, marker_matches, remove, sibling};
use heatel::Result;
use std::fs::{self, Metadata};
use std::io;
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
            "the import cannot use this path because it is occupied by an unrelated entry",
        ),
    )
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
