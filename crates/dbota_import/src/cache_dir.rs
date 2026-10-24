//! The importer on the desktop: a Dota install's folder in, the client's cache folder out. The
//! import is made in a folder beside the cache and takes its place when it is whole, so a cache
//! is either the old one or the new one, never half of each.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use heatel::{Error, Result};

use crate::import::{Flow, IMPORT_REV, Plan, Progress, Report, Sink, run};
use crate::source::InstallSource;

/// The file of a finished cache that says what made it.
pub const STAMP: &str = "stamp.txt";
/// The file of a finished cache that lists its files (written last but the stamp).
pub const INDEX: &str = "index.txt";
/// Longest stamp read; a stamp is a few dozen bytes.
const MAX_STAMP: u64 = 4096;

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
/// `version` and plan `plan_id` ([`plan_id`](crate::import::plan_id)) is stamped with: the
/// version, the importer's revision, the plan, and the size and change time of the install's
/// package directory.
pub fn stamp(game: &Path, version: &str, plan_id: &str) -> Result<String> {
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
        "{}/{}/{modified}",
        stamp_prefix(version, plan_id),
        meta.len()
    ))
}

/// The part of a stamp that says what the importer would make, not from which install.
pub fn stamp_prefix(version: &str, plan_id: &str) -> String {
    format!("v{}/rev{IMPORT_REV}/{plan_id}", version.trim())
}

/// Whether the cache at `cache` was made by an importer that writes `prefix`
/// ([`stamp_prefix`]), whatever install it read.
pub fn matches_plan(cache: &Path, prefix: &str) -> bool {
    read_stamp(cache).is_some_and(|s| s.starts_with(&format!("{prefix}/")))
}

/// Whether the cache at `cache` was finished with `stamp`.
pub fn is_current(cache: &Path, stamp: &str) -> bool {
    read_stamp(cache).is_some_and(|s| s.trim() == stamp) && cache.join(INDEX).is_file()
}

/// The cache's stamp; none when it is missing or longer than any stamp is (the cache is
/// anyone's to write, and the file is read whole).
fn read_stamp(cache: &Path) -> Option<String> {
    let file = fs::File::open(cache.join(STAMP)).ok()?;
    let mut text = String::new();
    file.take(MAX_STAMP + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MAX_STAMP).then_some(text)
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
        InstallSource::new(game),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::plan_id;

    /// A cache stamped for one manifest is not current for an edited one, and says so before
    /// any install is read; the same manifest stays current.
    #[test]
    fn a_manifest_change_makes_the_cache_stale() {
        let dir = std::env::temp_dir().join(format!("dbota-stamp-{}", std::process::id()));
        let (game, cache) = (dir.join("game"), dir.join("cache"));
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(game.join("pak01_dir.vpk"), b"x").unwrap();
        let before = stamp(&game, "7", &plan_id("model a\n", "ids")).unwrap();
        fs::write(cache.join(STAMP), &before).unwrap();
        fs::write(cache.join(INDEX), b"").unwrap();
        assert!(is_current(
            &cache,
            &stamp(&game, "7", &plan_id("model a\n", "ids")).unwrap()
        ));
        assert!(matches_plan(
            &cache,
            &stamp_prefix("7", &plan_id("model a\n", "ids"))
        ));
        let edited = plan_id("model a\nportraits p\n", "ids");
        assert!(!is_current(&cache, &stamp(&game, "7", &edited).unwrap()));
        assert!(!matches_plan(&cache, &stamp_prefix("7", &edited)));
        assert_ne!(plan_id("a", "bc"), plan_id("ab", "c"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The stamp is read whole: a huge one planted in the cache filled the memory.
    #[test]
    fn a_stamp_longer_than_any_is_not_read() {
        let cache = std::env::temp_dir().join(format!("dbota-big-stamp-{}", std::process::id()));
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join(INDEX), b"").unwrap();
        let stamp = "v7/rev3/0123456789abcdef/1/2";
        fs::write(cache.join(STAMP), stamp).unwrap();
        assert!(is_current(&cache, stamp));
        // Sparse: nothing is written past the stamp.
        fs::File::options()
            .write(true)
            .open(cache.join(STAMP))
            .unwrap()
            .set_len(64 << 30)
            .unwrap();
        assert!(!is_current(&cache, stamp));
        assert!(!matches_plan(&cache, "v7/rev3/0123456789abcdef"));
        fs::remove_dir_all(&cache).unwrap();
    }
}
