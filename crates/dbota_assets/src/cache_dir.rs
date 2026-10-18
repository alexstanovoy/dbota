//! The importer on the desktop: a Dota install's folder in, the client's cache folder out. The
//! import is made in a folder beside the cache and takes its place when it is whole, so a cache
//! is either the old one or the new one, never half of each.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::import::{Flow, IMPORT_REV, Plan, Progress, Report, Sink, run};
use crate::source::DirSource;

/// The file of a finished cache that says what made it.
pub const STAMP: &str = "stamp.txt";
/// The file of a finished cache that lists its files (written last but the stamp).
pub const INDEX: &str = "index.txt";

/// Writes each file under a folder (paths are [`safe_path`](crate::import::safe_path)s).
pub struct DirSink {
    root: PathBuf,
}

impl DirSink {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Sink for DirSink {
    fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let target = self.root.join(path);
        let io = |source| Error::Io {
            path: path.to_owned(),
            source,
        };
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(io)?;
        }
        fs::write(&target, data).map_err(io)
    }
}

/// What a cache made from the install at `game` (its `game/dota` folder) for manifest version
/// `version` is stamped with: the version, the importer's revision, and the size and change
/// time of the install's package directory.
pub fn stamp(game: &Path, version: &str) -> Result<String> {
    let path = game.join("pak01_dir.vpk");
    let meta = fs::metadata(&path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok(format!(
        "v{}/rev{IMPORT_REV}/{}/{modified}",
        version.trim(),
        meta.len()
    ))
}

/// Whether the cache at `cache` was finished with `stamp`.
pub fn is_current(cache: &Path, stamp: &str) -> bool {
    fs::read_to_string(cache.join(STAMP)).is_ok_and(|s| s.trim() == stamp)
        && cache.join(INDEX).is_file()
}

/// Imports `plan` from the install at `game` into `cache`, stamped `stamp`. The new cache is
/// made beside the old one and replaces it once whole; a stopped import leaves the old one.
pub fn import_into(
    game: &Path,
    cache: &Path,
    plan: &Plan,
    stamp: &str,
    progress: &mut dyn FnMut(&Progress<'_>) -> Flow,
) -> Result<Report> {
    let staging = sibling(cache, "importing")?;
    remove(&staging)?;
    let report = run(
        DirSource::new(game),
        plan,
        &mut DirSink::new(&staging),
        progress,
    )?;
    if report.stopped {
        remove(&staging)?;
        return Ok(report);
    }
    let mut sink = DirSink::new(&staging);
    sink.write(INDEX, report.index().as_bytes())?;
    sink.write(STAMP, stamp.as_bytes())?;
    let old = sibling(cache, "old")?;
    remove(&old)?;
    let io = |source| Error::Io {
        path: cache.display().to_string(),
        source,
    };
    if cache.exists() {
        fs::rename(cache, &old).map_err(io)?;
    }
    fs::rename(&staging, cache).map_err(io)?;
    remove(&old)?;
    Ok(report)
}

/// `.<name>.<what>` beside `cache`.
fn sibling(cache: &Path, what: &str) -> Result<PathBuf> {
    let name = cache.file_name().ok_or_else(|| Error::Io {
        path: cache.display().to_string(),
        source: io::Error::other("the cache folder has no name"),
    })?;
    let mut sibling = std::ffi::OsString::from(".");
    sibling.push(name);
    sibling.push(format!(".{what}"));
    Ok(cache.with_file_name(sibling))
}

fn remove(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: dir.display().to_string(),
            source,
        }),
    }
}
