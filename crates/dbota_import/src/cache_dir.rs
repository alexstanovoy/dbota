//! The importer on the desktop: a Dota install's folder in, the client's cache folder out. The
//! import is made in a folder beside the cache and takes its place when it is whole, so a cache
//! is either the old one or the new one, never half of each.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use heatel::{Error, Result};

use crate::import::{
    Flow, MAX_STAMP, OWNER_MARKER, OWNER_TEXT, Plan, Progress, Report, Sink, run_capped,
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

/// How many owned caches stay under the managed root, including the current one.
pub const KEPT_CACHES: usize = 2;

/// Marks a successfully installed cache as owned by this explicitly managed root.
/// Ownership travels with the directory; asset packs omit this local metadata.
pub fn record_managed(root: &Path, current: &Path) -> Result<()> {
    managed_name(root, current)?;
    plain_directory(root)?;
    plain_directory(current)?;
    if !marker(
        root.join(".dbota-cache-root"),
        b"dbota managed root 1\n",
        true,
    )? {
        return Err(managed_io(
            root,
            io::Error::other("invalid managed root ownership marker"),
        ));
    }
    if !marker(current.join(OWNER_MARKER), OWNER_TEXT.as_bytes(), true)? {
        return Err(managed_io(
            root,
            io::Error::other("invalid managed cache ownership marker"),
        ));
    }
    Ok(())
}

/// Keeps the current cache and the newest other owned cache. An unmarked directory is never
/// adopted by pruning, even when its name, stamp and index look like a cache.
pub fn prune(root: &Path, current: &Path) -> Result<()> {
    let current_name = managed_name(root, current)?;
    plain_directory(root)?;
    plain_directory(current)?;
    if !marker(
        root.join(".dbota-cache-root"),
        b"dbota managed root 1\n",
        false,
    )? || !marker(current.join(OWNER_MARKER), OWNER_TEXT.as_bytes(), false)?
    {
        return Ok(());
    }
    let mut caches = owned_caches(root)?;
    caches
        .sort_by_key(|(name, made)| (name != current_name, std::cmp::Reverse(*made), name.clone()));
    for (name, _) in caches.into_iter().skip(KEPT_CACHES) {
        let path = root.join(&name);
        // remove_dir_all does not follow links in the tree. Recheck ownership immediately before
        // removal; never resolve a cache symlink to a directory outside the managed root.
        plain_directory(&path)?;
        if marker(path.join(OWNER_MARKER), OWNER_TEXT.as_bytes(), false)? {
            bounded_tree(&path)?;
            remove(&path)?;
        }
    }
    Ok(())
}

fn owned_caches(root: &Path) -> Result<Vec<(String, std::time::SystemTime)>> {
    let mut caches = Vec::new();
    let entries = fs::read_dir(root).map_err(|e| managed_io(root, e))?;
    for (count, entry) in entries.enumerate() {
        if count >= 4096 {
            return Err(managed_io(
                root,
                io::Error::other("managed cache root has more than 4096 entries"),
            ));
        }
        let entry = entry.map_err(|e| managed_io(root, e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !cache_name(&name) || !entry.file_type().map_err(|e| managed_io(root, e))?.is_dir() {
            continue;
        }
        if !marker(
            entry.path().join(OWNER_MARKER),
            OWNER_TEXT.as_bytes(),
            false,
        )? {
            continue;
        }
        let path = entry.path();
        if !fs::symlink_metadata(path.join(INDEX)).is_ok_and(|meta| meta.is_file()) {
            continue;
        }
        let made = fs::symlink_metadata(path.join(STAMP))
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok());
        if let Some(made) = made {
            caches.push((name, made));
        }
    }
    Ok(caches)
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

fn marker(path: PathBuf, expected: &[u8], create: bool) -> Result<bool> {
    if create {
        use std::io::Write;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => file.write_all(expected).map_err(|e| managed_io(&path, e))?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(managed_io(&path, e)),
        }
    }
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(managed_io(&path, e)),
    };
    if !meta.is_file() || meta.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(expected.len() as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| managed_io(&path, e))?;
    Ok(bytes == expected)
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

    #[test]
    fn pruning_preserves_unowned_siblings_with_cache_like_names_and_files() {
        let dir = std::env::temp_dir().join(format!("dbota-prune-hostile-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let current = dir.join("rev29.1-1111111111111111");
        for name in [
            "revolution",
            "v",
            "rev29.1-0000000000000000",
            "rev29.1-1111111111111111",
        ] {
            let cache = dir.join(name);
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join(STAMP), b"not ownership").unwrap();
            fs::write(cache.join(INDEX), b"not ownership").unwrap();
            fs::write(cache.join("keep"), b"unrelated data").unwrap();
        }
        fs::write(dir.join(".dbota-cache-root"), b"dbota managed root 1\n").unwrap();
        fs::write(current.join(".dbota-owned-cache"), b"dbota owned cache 1\n").unwrap();
        fs::write(dir.join("revolution").join(OWNER_MARKER), OWNER_TEXT).unwrap();
        fs::write(dir.join("v").join(OWNER_MARKER), OWNER_TEXT).unwrap();
        prune(&dir, &current).unwrap();
        for name in ["revolution", "v", "rev29.1-0000000000000000"] {
            assert_eq!(
                fs::read(dir.join(name).join("keep")).ok(),
                Some(b"unrelated data".to_vec()),
                "deleted unowned {name}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// Caches of other builds beyond the two newest are removed; folders that are no cache stay.
    #[test]
    fn old_caches_are_pruned_to_the_two_newest() {
        let dir = std::env::temp_dir().join(format!("dbota-prune-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        for (at, name) in [
            "rev13.1-0000000000000000",
            "rev13.1-1111111111111111",
            "rev13.1-2222222222222222",
            "rev13.1-3333333333333333",
        ]
        .iter()
        .enumerate()
        {
            let cache = dir.join(name);
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join(INDEX), b"").unwrap();
            fs::write(cache.join(STAMP), b"s").unwrap();
            record_managed(&dir, &cache).unwrap();
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
        prune(&dir, &dir.join("rev13.1-2222222222222222")).unwrap();
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| !name.starts_with('.'))
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "replays",
                "rev13.1-2222222222222222",
                "rev13.1-3333333333333333",
                "revolution"
            ]
        );
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

#[cfg(all(test, unix))]
mod managed_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn managed_pruning_refuses_outside_paths_and_preserves_symlink_targets() {
        let scratch =
            std::env::temp_dir().join(format!("dbota-prune-links-{}", std::process::id()));
        fs::create_dir(&scratch).unwrap();
        let root = scratch.join("managed");
        let outside = scratch.join("outside");
        let current = root.join("rev29.1-1111111111111111");
        for path in [&current, &outside] {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join("keep"), b"keep").unwrap();
        }
        fs::write(current.join(STAMP), b"stamp").unwrap();
        fs::write(current.join(INDEX), b"index").unwrap();
        record_managed(&root, &current).unwrap();
        let name = "rev29.1-0000000000000000";
        symlink(&outside, root.join(name)).unwrap();
        fs::write(outside.join(OWNER_MARKER), OWNER_TEXT).unwrap();
        prune(&root, &current).unwrap();
        let error = prune(&root, &root.join(name)).expect_err("symlink current cache");
        assert!(error.to_string().contains("not a link"), "{error}");
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
        let fake_current = outside.join(current.file_name().unwrap());
        let error = prune(&root, &fake_current).expect_err("outside the explicit managed root");
        assert!(error.to_string().contains("not a named child"), "{error}");
        let alias = scratch.join("alias");
        symlink(&root, &alias).unwrap();
        let error =
            prune(&alias, &alias.join(current.file_name().unwrap())).expect_err("symlink root");
        assert!(error.to_string().contains("not a link"), "{error}");
        fs::remove_dir_all(scratch).unwrap();
    }
}
