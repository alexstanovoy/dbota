//! The importer on the desktop: a Dota install's folder in, the client's cache folder out. The
//! import is made in a folder beside the cache and takes its place when it is whole, so a cache
//! is either the old one or the new one, never half of each.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heatel::{Error, Result};

use crate::import::{
    Flow, LAST_USE_MARKER, LAST_USE_TEXT, MAX_STAMP, OWNER_MARKER, OWNER_TEXT, Plan, Progress,
    ROOT_MARKER, ROOT_TEXT, Report, Sink, run_capped, stamped_here,
};
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

/// Retention protects caches used within this window, up to the managed capacity.
pub const CACHE_USE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_CACHES: usize = 8;

#[derive(Default, Debug)]
pub struct PruneReport {
    pub failures: Vec<(PathBuf, String)>,
    pub remaining: usize,
    pub admission_blocked: bool,
}

/// Marks a cache as owned by the explicitly configured root.
pub fn record_managed(root: &Path, current: &Path) -> Result<()> {
    managed_name(root, current)?;
    plain_directory(root)?;
    plain_directory(current)?;
    ensure_marker(&root.join(ROOT_MARKER), ROOT_TEXT.as_bytes())?;
    ensure_marker(&current.join(OWNER_MARKER), OWNER_TEXT.as_bytes())
}

/// Updates local usage metadata without copying it into portable asset packs.
pub fn touch_last_use(cache: &Path) -> Result<()> {
    touch_last_use_at(cache, SystemTime::now())
}

fn touch_last_use_at(cache: &Path, used: SystemTime) -> Result<()> {
    plain_directory(cache)?;
    let path = cache.join(LAST_USE_MARKER);
    if fs::symlink_metadata(&path).is_ok() && last_use(cache)?.is_none() {
        return Err(managed_io(
            &path,
            io::Error::other("invalid cache last-use stamp"),
        ));
    }
    let seconds = used
        .duration_since(UNIX_EPOCH)
        .map_err(|e| managed_io(&path, io::Error::other(e)))?
        .as_secs();
    let text = format!("{LAST_USE_TEXT}{seconds}\n");
    complete_file(&path, text.as_bytes(), |temporary| {
        fs::rename(temporary, &path)
    })
}

pub fn prune(root: &Path, current: &Path) -> Result<PruneReport> {
    prune_at(root, current, SystemTime::now())
}

fn prune_at(root: &Path, current: &Path, now: SystemTime) -> Result<PruneReport> {
    let current_name = managed_name(root, current)?;
    plain_directory(root)?;
    plain_directory(current)?;
    let report = PruneReport::default();
    if !marker_matches(&root.join(ROOT_MARKER), ROOT_TEXT.as_bytes())?
        || !marker_matches(&current.join(OWNER_MARKER), OWNER_TEXT.as_bytes())?
    {
        return Ok(report);
    }
    prune_records(root, current_name, now, false)
}

fn prune_records(
    root: &Path,
    current_name: &str,
    now: SystemTime,
    reserve: bool,
) -> Result<PruneReport> {
    let mut report = PruneReport::default();
    let mut caches = owned_caches(root, &mut report, now)?;
    let present = caches.iter().any(|(name, _)| name == current_name);
    let limit = MAX_CACHES - usize::from(reserve && !present);
    caches.sort_by_key(|(name, used)| (*used, name.clone()));
    let mut surplus = caches.len().saturating_sub(limit);
    report.remaining = caches.len();
    for (name, used) in caches {
        if surplus == 0 {
            break;
        }
        let expired = now
            .duration_since(used)
            .is_ok_and(|age| age > CACHE_USE_WINDOW);
        if name == current_name || !expired {
            continue;
        }
        let path = root.join(&name);
        // These checks inspect the final components. Other processes with the same filesystem
        // access must not replace cache parents while the operation resolves its paths.
        match remove_owned(&path) {
            Ok(()) => {
                surplus -= 1;
                report.remaining -= 1;
            }
            Err(why) => report.failures.push((path, why.to_string())),
        }
    }
    if surplus > 0 {
        let why = format!(
            "managed cache capacity is {MAX_CACHES}; {} caches are recent or could not be removed",
            report.remaining
        );
        report.admission_blocked = reserve && !present;
        report.failures.push((root.to_path_buf(), why));
    }
    Ok(report)
}

/// A managed import holds the root lock through publication, so concurrent imports cannot
/// both admit a ninth cache. Startup use updates take the same lock before protecting a cache.
pub struct ManagedCacheLock {
    _file: fs::File,
}

pub fn lock_managed_root(root: &Path) -> Result<ManagedCacheLock> {
    fs::create_dir_all(root).map_err(|e| managed_io(root, e))?;
    plain_directory(root)?;
    ensure_marker(&root.join(ROOT_MARKER), ROOT_TEXT.as_bytes())?;
    let path = root.join(".dbota-cache-lock");
    ensure_marker(&path, b"dbota cache lock 1\n")?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| managed_io(&path, e))?;
    file.lock().map_err(|e| managed_io(&path, e))?;
    Ok(ManagedCacheLock { _file: file })
}

pub fn begin_managed_import(root: &Path, current: &Path) -> Result<ManagedCacheLock> {
    begin_managed_import_at(root, current, SystemTime::now())
}

fn begin_managed_import_at(
    root: &Path,
    current: &Path,
    now: SystemTime,
) -> Result<ManagedCacheLock> {
    let name = managed_name(root, current)?;
    let lock = lock_managed_root(root)?;
    let report = prune_records(root, name, now, true)?;
    for (path, why) in report.failures {
        eprintln!("dbota import: cache {} stays: {why}", path.display());
    }
    if report.admission_blocked {
        return Err(managed_io(
            root,
            io::Error::other(format!(
                "managed cache capacity is {MAX_CACHES}; recent or unremovable caches occupy every slot"
            )),
        ));
    }
    Ok(lock)
}

fn remove_owned(path: &Path) -> Result<()> {
    plain_directory(path)?;
    if !marker_matches(&path.join(OWNER_MARKER), OWNER_TEXT.as_bytes())? {
        return Err(managed_io(
            path,
            io::Error::other("cache ownership marker changed"),
        ));
    }
    bounded_tree(path)?;
    remove(path)
}

fn owned_caches(
    root: &Path,
    report: &mut PruneReport,
    now: SystemTime,
) -> Result<Vec<(String, SystemTime)>> {
    let mut caches = Vec::new();
    let entries = fs::read_dir(root).map_err(|e| managed_io(root, e))?;
    for (count, entry) in entries.enumerate() {
        if count >= 4096 {
            return Err(managed_io(
                root,
                io::Error::other("managed cache root has more than 4096 entries"),
            ));
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(why) => {
                report.failures.push((root.to_path_buf(), why.to_string()));
                continue;
            }
        };
        match owned_cache(&entry, report, now) {
            Ok(Some(cache)) => caches.push(cache),
            Ok(None) => {}
            Err(why) => {
                // An unreadable candidate is not a free slot. Keep it protected without
                // claiming ownership; a disappeared entry needs no capacity reservation.
                if !matches!(&why, Error::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
                    && let Some(name) = entry.file_name().to_str().filter(|name| cache_name(name))
                {
                    caches.push((name.to_owned(), now));
                }
                report.failures.push((entry.path(), why.to_string()));
            }
        }
    }
    Ok(caches)
}

fn owned_cache(
    entry: &fs::DirEntry,
    report: &mut PruneReport,
    now: SystemTime,
) -> Result<Option<(String, SystemTime)>> {
    let path = entry.path();
    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
        return Ok(None);
    };
    if !cache_name(&name)
        || !entry
            .file_type()
            .map_err(|e| managed_io(&path, e))?
            .is_dir()
    {
        return Ok(None);
    }
    let marker = path.join(OWNER_MARKER);
    let owned = marker_matches(&marker, OWNER_TEXT.as_bytes())?;
    if !owned && (fs::symlink_metadata(&marker).is_ok() || !valid_legacy_cache(&path)) {
        return Ok(None);
    }
    let used = match cache_use_time(&path) {
        Ok(used) => used,
        Err(why) if owned => {
            // Unknown age cannot authorize deletion or make an owned directory disappear
            // from the capacity calculation. It stays protected until its metadata is repaired.
            report.failures.push((path.clone(), why.to_string()));
            now
        }
        Err(why) => return Err(why),
    };
    if !owned {
        ensure_marker(&marker, OWNER_TEXT.as_bytes())?;
    }
    Ok(Some((name, used)))
}

fn valid_legacy_cache(path: &Path) -> bool {
    fs::symlink_metadata(path.join(STAMP)).is_ok_and(|meta| meta.is_file())
        && crate::index::CacheIndex::read(path).is_ok()
        && read_stamp(path).is_some_and(|stamp| stamped_here(&stamp))
}

fn cache_use_time(path: &Path) -> Result<SystemTime> {
    if let Some(used) = last_use(path)? {
        return Ok(used);
    }
    let stamp = path.join(STAMP);
    fs::symlink_metadata(&stamp)
        .and_then(|meta| meta.modified())
        .map_err(|why| managed_io(&stamp, why))
}

fn last_use(cache: &Path) -> Result<Option<SystemTime>> {
    let path = cache.join(LAST_USE_MARKER);
    let bytes = match metadata_bytes(&path, 96)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let text = std::str::from_utf8(&bytes).ok();
    let seconds = text.and_then(crate::index::last_use_seconds);
    seconds
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .map(Some)
        .ok_or_else(|| managed_io(&path, io::Error::other("invalid cache last-use stamp")))
}

fn bounded_tree(root: &Path) -> Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 8 {
            return Err(managed_io(
                root,
                io::Error::other("owned cache exceeds directory depth 8"),
            ));
        }
        for entry in fs::read_dir(&directory).map_err(|e| managed_io(root, e))? {
            count += 1;
            if count > 16384 {
                return Err(managed_io(
                    root,
                    io::Error::other("owned cache exceeds 16384 entries"),
                ));
            }
            let entry = entry.map_err(|e| managed_io(root, e))?;
            let kind = entry.file_type().map_err(|e| managed_io(root, e))?;
            if kind.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if !kind.is_file() {
                return Err(managed_io(
                    root,
                    io::Error::other("owned cache contains a link or special file"),
                ));
            }
        }
    }
    Ok(())
}

fn managed_name<'a>(root: &Path, current: &'a Path) -> Result<&'a str> {
    let relative = current.strip_prefix(root).ok();
    let name = relative
        .and_then(Path::to_str)
        .filter(|name| cache_name(name));
    name.ok_or_else(|| {
        managed_io(
            current,
            io::Error::other("cache is not a named child of the managed root"),
        )
    })
}

fn cache_name(name: &str) -> bool {
    let Some((revisions, hash)) = name.strip_prefix("rev").and_then(|s| s.split_once('-')) else {
        return false;
    };
    let number =
        |s: &str| s.len() <= 10 && s.parse::<u32>().is_ok_and(|value| value.to_string() == s);
    revisions
        .split_once('.')
        .is_some_and(|(a, b)| number(a) && number(b))
        && hash.len() == 16
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn plain_directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| managed_io(path, e))?;
    if !meta.is_dir() {
        return Err(managed_io(
            path,
            io::Error::other("managed cache must be a directory, not a link"),
        ));
    }
    Ok(())
}

fn marker_matches(path: &Path, expected: &[u8]) -> Result<bool> {
    Ok(metadata_bytes(path, expected.len() as u64)?.is_some_and(|bytes| bytes == expected))
}

fn metadata_bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(managed_io(path, e)),
    };
    if !meta.is_file() || meta.len() > limit {
        return Ok(Some(Vec::new()));
    }
    // This bounded open is not atomic with the metadata check: a same-user replacement
    // can still follow a link or block on a FIFO. Cache parents must not be replaced concurrently.
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|e| managed_io(path, e))?;
    Ok(Some(bytes))
}

fn ensure_marker(path: &Path, expected: &[u8]) -> Result<()> {
    if marker_matches(path, expected)? {
        return Ok(());
    }
    if fs::symlink_metadata(path).is_ok() {
        // Another publisher can win after the first read; its complete marker is valid too.
        if marker_matches(path, expected)? {
            return Ok(());
        }
        return Err(managed_io(
            path,
            io::Error::other("invalid cache ownership marker"),
        ));
    }
    match complete_file(path, expected, |temporary| fs::hard_link(temporary, path)) {
        Ok(()) => Ok(()),
        Err(_) if marker_matches(path, expected)? => Ok(()),
        Err(why) => Err(why),
    }
}

fn complete_file(
    path: &Path,
    bytes: &[u8],
    publish: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| managed_io(path, io::Error::other("metadata path has no parent")))?;
    let mut temporary = None;
    for _ in 0..32 {
        let candidate = parent.join(format!(
            ".dbota-metadata-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(managed_io(path, e)),
        }
    }
    let (temporary, mut file) = temporary.ok_or_else(|| {
        managed_io(
            path,
            io::Error::other("metadata temporary names are occupied"),
        )
    })?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| publish(&temporary));
    drop(file);
    let cleanup = fs::remove_file(&temporary);
    let cleanup = match cleanup {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    result.and(cleanup).map_err(|e| managed_io(path, e))
}

fn managed_io(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        source,
    }
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
    import_capped(game, cache, plan, stamp, progress, None)
}

/// [`import_into`], reading at most `limit` bytes of the install when given. An import the
/// install cut short (it asked for more) leaves no cache: the old one stays.
fn import_capped(
    game: &Path,
    cache: &Path,
    plan: &Plan,
    stamp: &str,
    progress: &mut dyn FnMut(&Progress<'_>) -> Flow,
    limit: Option<u64>,
) -> Result<Report> {
    let staging = sibling(cache, "importing")?;
    remove(&staging)?;
    let made = run_capped(
        InstallSource::new(game),
        plan,
        &mut DirSink::new(&staging),
        progress,
        limit,
    );
    let report = match made {
        Ok(report) => report,
        Err(err) => {
            remove(&staging)?;
            return Err(err);
        }
    };
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

    /// A VPK (version 1) holding `files` after its directory tree, each in its own entry.
    fn vpk(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let (mut tree, mut data) = (Vec::new(), Vec::new());
        for (path, bytes) in files {
            let (name, ext) = path.rsplit_once('.').unwrap();
            for part in [ext, " ", name] {
                tree.extend_from_slice(part.as_bytes());
                tree.push(0);
            }
            tree.extend_from_slice(&0u32.to_le_bytes());
            tree.extend_from_slice(&0u16.to_le_bytes());
            tree.extend_from_slice(&0x7FFFu16.to_le_bytes());
            tree.extend_from_slice(&(data.len() as u32).to_le_bytes());
            tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
            tree.extend_from_slice(&[0, 0]);
            data.extend_from_slice(bytes);
        }
        tree.push(0);
        let mut out = 0x55AA_1234u32.to_le_bytes().to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
        out.extend(tree);
        out.extend(data);
        out
    }

    /// An install that asks for more than the import reads ends it with an error and finishes
    /// no cache: the cache that was there stays as it was, its stamp unchanged.
    #[test]
    fn an_import_over_its_read_limit_fails_and_stamps_nothing() {
        let dir = std::env::temp_dir().join(format!("dbota-cut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (game, cache) = (dir.join("game"), dir.join("cache"));
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let pak = vpk(&[("t.vtex_c", vec![7; 256 << 10])]);
        fs::write(game.join("pak01_dir.vpk"), &pak).unwrap();
        fs::write(cache.join(STAMP), b"before").unwrap();
        fs::write(cache.join(INDEX), b"").unwrap();
        let plan = Plan::parse("texture t t.vtex_c t.png\n", "").unwrap();
        let limit = Some(64 << 10);
        let made = import_capped(
            &game,
            &cache,
            &plan,
            "after",
            &mut |_| Flow::Continue,
            limit,
        );
        let err = made.expect_err("an import over its limit fails");
        assert!(err.to_string().contains("more than"), "{err}");
        assert_eq!(read_stamp(&cache).as_deref(), Some("before"));
        assert!(!sibling(&cache, "importing").unwrap().exists());
        // Within its limit, the same install imports, its failures reported, and is stamped.
        let made = import_capped(&game, &cache, &plan, "after", &mut |_| Flow::Continue, None);
        assert!(made.is_ok_and(|r| r.cut.is_none()));
        assert_eq!(read_stamp(&cache).as_deref(), Some("after"));
        fs::remove_dir_all(&dir).unwrap();
    }

    pub(super) fn fresh_dir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        for _ in 0..128 {
            let path = std::env::temp_dir().join(format!(
                "dbota-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("cannot create test directory: {e}"),
            }
        }
        panic!("test directory names occupied");
    }

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000)
    }

    pub(super) fn cache(root: &Path, name: &str, made: u64, owned: bool) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join(INDEX),
            format!("{}\n", crate::index::CacheIndex::header()),
        )
        .unwrap();
        fs::write(
            path.join(STAMP),
            format!("{}/1/2", crate::import::fingerprint()),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(path.join(STAMP))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(made))
            .unwrap();
        if owned {
            record_managed(root, &path).unwrap();
        }
        path
    }

    fn many_caches(root: &Path, count: usize, owned: bool) -> Vec<PathBuf> {
        (0..count)
            .map(|n| cache(root, &format!("rev29.1-{n:016x}"), n as u64, owned))
            .collect()
    }

    #[test]
    fn a_recently_used_cache_survives_a_newer_import() {
        let root = fresh_dir("recent-use");
        let old = many_caches(&root, MAX_CACHES, true);
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        touch_last_use_at(&old[0], now() - Duration::from_secs(3600)).unwrap();
        let report = prune_at(&root, &current, now()).unwrap();
        assert!(
            old[0].exists(),
            "a recently used cache survives the newer import"
        );
        assert!(
            !old[1].exists(),
            "an expired cache releases the slot instead"
        );
        assert_eq!(report.remaining, MAX_CACHES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_legacy_caches_are_adopted_and_pruned() {
        let root = fresh_dir("adoption");
        let old = many_caches(&root, MAX_CACHES, false);
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        prune_at(&root, &current, now()).unwrap();
        assert!(
            !old[0].exists(),
            "the oldest validated legacy cache is pruned"
        );
        assert!(
            old[1].join(OWNER_MARKER).is_file(),
            "retained legacy ownership is recorded"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pruning_preserves_unowned_siblings_with_cache_like_names_and_files() {
        let root = fresh_dir("prune-hostile");
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        for n in 100..100 + MAX_CACHES {
            cache(&root, &format!("rev29.1-{n:016x}"), n as u64, true);
        }
        let invalid = [
            "revolution",
            "v",
            "rev29.1-0000000000000000",
            "rev29.1-0000000000000001",
        ];
        for name in invalid {
            let path = root.join(name);
            fs::create_dir(&path).unwrap();
            fs::write(path.join(INDEX), b"not an index").unwrap();
            fs::write(path.join(STAMP), b"not a stamp").unwrap();
            if name == "rev29.1-0000000000000000" {
                fs::write(
                    path.join(STAMP),
                    format!("{}/1/2", crate::import::fingerprint()),
                )
                .unwrap();
            }
            if name == "rev29.1-0000000000000001" {
                fs::write(
                    path.join(INDEX),
                    format!("{}\n", crate::index::CacheIndex::header()),
                )
                .unwrap();
            }
            fs::write(path.join("keep"), b"unrelated data").unwrap();
            fs::File::options()
                .write(true)
                .open(path.join(STAMP))
                .unwrap()
                .set_modified(UNIX_EPOCH)
                .unwrap();
            if !cache_name(name) {
                fs::write(path.join(OWNER_MARKER), OWNER_TEXT).unwrap();
            }
        }
        prune_at(&root, &current, now()).unwrap();
        for name in invalid {
            assert_eq!(
                fs::read(root.join(name).join("keep")).unwrap(),
                b"unrelated data"
            );
        }
        for name in ["rev29.1-0000000000000000", "rev29.1-0000000000000001"] {
            assert!(!root.join(name).join(OWNER_MARKER).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owned_expired_caches_are_bounded_by_the_eight_cache_capacity() {
        let root = fresh_dir("prune-capacity");
        let old = many_caches(&root, MAX_CACHES + 1, true);
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        let report = prune_at(&root, &current, now()).unwrap();
        assert_eq!(report.remaining, MAX_CACHES);
        assert!(!old[0].exists());
        assert!(!old[1].exists());
        assert!(current.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ninth_import_is_refused_when_all_eight_caches_are_recent() {
        let root = fresh_dir("prune-admission");
        let caches = many_caches(&root, MAX_CACHES, true);
        for cache in &caches {
            touch_last_use_at(cache, now()).unwrap();
        }
        let requested = root.join(crate::import::folder_name());
        let error = begin_managed_import_at(&root, &requested, now())
            .err()
            .expect("capacity refusal")
            .to_string();
        assert!(error.contains("capacity is 8"), "{error}");
        assert!(!requested.exists());
        assert!(caches.iter().all(|cache| cache.exists()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_cache_with_a_damaged_use_stamp_still_consumes_capacity() {
        let root = fresh_dir("prune-damaged-use");
        let caches = many_caches(&root, MAX_CACHES, true);
        for cache in &caches {
            touch_last_use_at(cache, now()).unwrap();
        }
        fs::write(caches[0].join(LAST_USE_MARKER), b"damaged").unwrap();
        let requested = root.join(crate::import::folder_name());
        let error = begin_managed_import_at(&root, &requested, now())
            .err()
            .expect("damaged owned cache still occupies a slot")
            .to_string();
        assert!(error.contains("capacity is 8"), "{error}");
        assert!(caches.iter().all(|cache| cache.exists()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_marker_writers_publish_complete_markers() {
        let root = fresh_dir("marker-publication");
        let current = cache(&root, &crate::import::folder_name(), 1000, false);
        let ready = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let ready = &ready;
                let root = &root;
                let current = &current;
                scope.spawn(move || {
                    ready.wait();
                    record_managed(root, current).unwrap();
                });
            }
        });
        assert_eq!(
            fs::read(root.join(ROOT_MARKER)).unwrap(),
            ROOT_TEXT.as_bytes()
        );
        assert_eq!(
            fs::read(current.join(OWNER_MARKER)).unwrap(),
            OWNER_TEXT.as_bytes()
        );
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".dbota-metadata-")
        }));
        fs::remove_dir_all(root).unwrap();
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

#[cfg(all(test, unix))]
mod managed_tests {
    use super::tests::{cache, fresh_dir};
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn managed_pruning_preserves_cache_symlink_targets() {
        let scratch = fresh_dir("prune-links");
        let root = scratch.join("managed");
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        let outside = scratch.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"keep").unwrap();
        fs::write(outside.join(OWNER_MARKER), OWNER_TEXT).unwrap();
        symlink(&outside, root.join("rev29.1-0000000000000000")).unwrap();
        prune(&root, &current).unwrap();
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn managed_pruning_refuses_a_symlink_current_cache() {
        let scratch = fresh_dir("prune-current-link");
        let root = scratch.join("managed");
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        let alias = root.join("rev29.1-0000000000000000");
        symlink(&current, &alias).unwrap();
        let error = prune(&root, &alias).expect_err("current link").to_string();
        assert!(error.contains("not a link"), "{error}");
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn managed_pruning_refuses_a_cache_outside_its_root() {
        let scratch = fresh_dir("prune-outside");
        let root = scratch.join("managed");
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        let error = prune(&root, &scratch.join(current.file_name().unwrap()))
            .expect_err("outside root")
            .to_string();
        assert!(error.contains("not a named child"), "{error}");
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn managed_pruning_refuses_a_symlink_root() {
        let scratch = fresh_dir("prune-root-link");
        let root = scratch.join("managed");
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        let alias = scratch.join("alias");
        symlink(&root, &alias).unwrap();
        let error = prune(&alias, &alias.join(current.file_name().unwrap()))
            .expect_err("root link")
            .to_string();
        assert!(error.contains("not a link"), "{error}");
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn one_unremovable_cache_does_not_stop_pruning_other_expired_caches() {
        let root = fresh_dir("prune-entry-failure");
        let old: Vec<_> = (0..MAX_CACHES + 1)
            .map(|n| cache(&root, &format!("rev29.1-{n:016x}"), n as u64, true))
            .collect();
        let current = cache(&root, &crate::import::folder_name(), 1000, true);
        symlink(&current, old[0].join("link")).unwrap();
        let report =
            prune_at(&root, &current, UNIX_EPOCH + Duration::from_secs(1_000_000)).unwrap();
        assert!(old[0].exists());
        assert!(!old[1].exists());
        assert!(
            report
                .failures
                .iter()
                .any(|(path, why)| path == &old[0] && why.contains("link or special"))
        );
        fs::remove_dir_all(root).unwrap();
    }
}
