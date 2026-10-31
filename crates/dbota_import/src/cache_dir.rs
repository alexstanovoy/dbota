//! The importer on the desktop: a Dota install's folder in, the client's cache folder out. The
//! import is made in a folder beside the cache and takes its place when it is whole, so a cache
//! is either the old one or the new one, never half of each.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use heatel::{Error, Result};

use crate::import::{Flow, MAX_STAMP, Plan, Progress, Report, Sink, run};
pub use crate::import::{INDEX, STAMP};
use crate::source::InstallSource;

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

/// What a cache made from the install at `game` (its `game/dota` folder) for plan `plan_id`
/// ([`plan_id`](crate::import::plan_id)) is stamped with: the fingerprint (the importer's and the
/// converters' revisions, the plan), and the size and change time of the install's package
/// directory.
pub fn stamp(game: &Path, plan_id: &str) -> Result<String> {
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
        stamp_prefix(plan_id),
        meta.len()
    ))
}

/// The part of a stamp that says what the importer would make, not from which install.
pub fn stamp_prefix(plan_id: &str) -> String {
    crate::import::fingerprint_of(plan_id)
}

/// How many caches of other fingerprints stay beside the current one: the build before this
/// one, for a machine that runs both.
pub const KEPT_CACHES: usize = 2;

/// Removes the caches beside `current` (in its parent folder) beyond the [`KEPT_CACHES`]
/// newest, `current` counted. Only folders that are caches (a stamp and an index) and whose
/// names this importer or an older one gives (`rev<N>.<N>-<hash>`, `v<N>`) are touched.
pub fn prune(current: &Path) -> Result<()> {
    let Some(parent) = current.parent() else {
        return Ok(());
    };
    let io = |source| Error::Io {
        path: parent.display().to_string(),
        source,
    };
    let mut caches = Vec::new();
    for entry in fs::read_dir(parent).map_err(io)? {
        let path = entry.map_err(io)?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let ours = name.starts_with("rev")
            || (name.starts_with('v') && name[1..].bytes().all(|b| b.is_ascii_digit()));
        if !ours || !path.join(STAMP).is_file() || !path.join(INDEX).is_file() {
            continue;
        }
        let made = fs::metadata(path.join(STAMP))
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        caches.push((path != current, std::cmp::Reverse(made), path));
    }
    // The current one first, then the newest.
    caches.sort();
    for (_, _, path) in caches.into_iter().skip(KEPT_CACHES) {
        remove(&path)?;
    }
    Ok(())
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
pub fn read_stamp(cache: &Path) -> Option<String> {
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
    replace(&staging, cache)?;
    Ok(report)
}

/// Puts the whole cache in `staging` in the place of the one at `cache`.
pub(crate) fn replace(staging: &Path, cache: &Path) -> Result<()> {
    let old = sibling(cache, "old")?;
    remove(&old)?;
    let io = |source| Error::Io {
        path: cache.display().to_string(),
        source,
    };
    if cache.exists() {
        fs::rename(cache, &old).map_err(io)?;
    }
    fs::rename(staging, cache).map_err(io)?;
    remove(&old)
}

/// `.<name>.<what>` beside `cache`.
pub(crate) fn sibling(cache: &Path, what: &str) -> Result<PathBuf> {
    let name = cache.file_name().ok_or_else(|| Error::Io {
        path: cache.display().to_string(),
        source: io::Error::other("the cache folder has no name"),
    })?;
    let mut sibling = std::ffi::OsString::from(".");
    sibling.push(name);
    sibling.push(format!(".{what}"));
    Ok(cache.with_file_name(sibling))
}

pub(crate) fn remove(dir: &Path) -> Result<()> {
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
    use crate::import::{Plan, plan_id};

    /// A cache stamped for one plan is not current for an edited one, and says so before any
    /// install is read; the same plan stays current.
    #[test]
    fn a_plan_change_makes_the_cache_stale() {
        let dir = std::env::temp_dir().join(format!("dbota-stamp-{}", std::process::id()));
        let (game, cache) = (dir.join("game"), dir.join("cache"));
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(game.join("pak01_dir.vpk"), b"x").unwrap();
        let plan = |text: &str| Plan::parse(text, "").unwrap();
        let (a, b) = (plan("texture t t.vtex_c t.png\n"), plan(""));
        let before = stamp(&game, &plan_id(&a)).unwrap();
        fs::write(cache.join(STAMP), &before).unwrap();
        fs::write(cache.join(INDEX), b"").unwrap();
        assert!(is_current(&cache, &stamp(&game, &plan_id(&a)).unwrap()));
        assert!(matches_plan(&cache, &stamp_prefix(&plan_id(&a))));
        assert!(!is_current(&cache, &stamp(&game, &plan_id(&b)).unwrap()));
        assert!(!matches_plan(&cache, &stamp_prefix(&plan_id(&b))));
        // A comment or the layout of the manifest is no plan change.
        let spaced = plan("# a note\n\ntexture   t  t.vtex_c   t.png   # why\n");
        assert_eq!(plan_id(&a), plan_id(&spaced));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Caches of other builds beyond the two newest are removed; folders that are no cache stay.
    #[test]
    fn old_caches_are_pruned_to_the_two_newest() {
        let dir = std::env::temp_dir().join(format!("dbota-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (at, name) in ["v20", "rev13.1-a", "rev13.1-b", "rev13.1-c"]
            .iter()
            .enumerate()
        {
            let cache = dir.join(name);
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join(INDEX), b"").unwrap();
            fs::write(cache.join(STAMP), b"s").unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + at as u64);
            fs::File::options()
                .write(true)
                .open(cache.join(STAMP))
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        fs::create_dir_all(dir.join("revolution")).unwrap();
        fs::create_dir_all(dir.join("replays")).unwrap();
        // The oldest of the three newest is the current one, and stays beside the newest.
        prune(&dir.join("rev13.1-b")).unwrap();
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["replays", "rev13.1-b", "rev13.1-c", "revolution"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The stamp is read whole: a huge one planted in the cache filled the memory.
    #[test]
    fn a_stamp_longer_than_any_is_not_read() {
        let cache = std::env::temp_dir().join(format!("dbota-big-stamp-{}", std::process::id()));
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join(INDEX), b"").unwrap();
        let stamp = "rev3.1/0123456789abcdef/1/2";
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
        assert!(!matches_plan(&cache, "rev3.1/0123456789abcdef"));
        fs::remove_dir_all(&cache).unwrap();
    }
}
