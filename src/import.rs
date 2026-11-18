//! `dbota import`: the asset cache made from the local Dota 2 install, by the same converter
//! the browser runs. Valve's files land only in the cache, never in a repository.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use dbota_import::cache_dir::{
    ManagedCacheLock, PruneReport, begin_managed_import, import_into, is_current,
    lock_managed_root, prune, read_stamp, record_managed, stamp, touch_last_use,
};
use dbota_import::import::{Flow, IDS, MANIFEST, plan, plan_id, stamped_here};
use heatel::locate::Install;

/// What the cache at a path lacks for this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// No finished import is there.
    Import,
    /// An import another build made is (its files may lack what the client now asks for), or
    /// one of this build's stamp whose files are of a layout older than this build reads
    /// ([`layouts_current`](dbota_import::import::layouts_current)).
    Reimport,
    /// The cache is this build's, and the player asked to make it again (the settings' RE-IMPORT
    /// GAME FILES): until a new one is whole, the one in place stays.
    Requested,
}

/// Whether the cache at `cache` needs an import before dbota draws Dota's art and sound: it
/// draws nothing without one.
pub fn need(cache: &Path) -> Option<Need> {
    if !cache.join(dbota_import::cache_dir::INDEX).is_file() {
        return Some(Need::Import);
    }
    let made_here = read_stamp(cache).is_some_and(|stamp| stamped_here(&stamp));
    let current = made_here && dbota_import::import::layouts_current(cache);
    (!current).then_some(Need::Reimport)
}

/// Why a run that cannot show the asset screen (a capture, a check) ends: what the cache at `cache`
/// lacks and what to do.
pub fn refusal(need: Need, cache: &Path) -> String {
    match need {
        Need::Import | Need::Requested => format!(
            "no asset cache at {}: dbota draws Dota's art and sound from the cache it makes of \
             your own Dota 2 install and from nothing else; start dbota, or run `dbota import`",
            cache.display()
        ),
        Need::Reimport => format!(
            "the asset cache {} is out of date for this version (it needs assets {}): \
             start dbota, or run `dbota import`",
            cache.display(),
            dbota_import::import::fingerprint()
        ),
    }
}

/// Checks the cache at `cache` against the manifest and prints what is missing or wrong;
/// returns whether everything checked out.
pub fn verify(cache: &Path) -> Result<bool, String> {
    let catalog = dbota_import::catalog::parse(MANIFEST, IDS).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    if !cache.is_dir() {
        return Err(format!("no asset cache at {}", cache.display()));
    }
    let verified = dbota_import::verify::verify(&catalog, cache);
    for failure in &verified.failures {
        println!("FAIL {:44} {}", failure.name, failure.why);
    }
    println!(
        "dbota import --verify: {} ok, {} failure(s)",
        verified.ok,
        verified.failures.len()
    );
    Ok(verified.failures.is_empty())
}

/// The Dota 2 install to import from: the one in `dota` (any usual level of it: the `dota 2
/// beta` folder, its `game/dota`, the Steam library), else the first in this machine's Steam
/// libraries.
pub fn locate(dota: Option<&Path>) -> Result<Install, String> {
    match dota {
        Some(folder) => heatel::locate::install_in(folder).ok_or_else(|| not_dota(folder)),
        None => crate::desktop::installs()
            .into_iter()
            .next()
            .ok_or_else(|| {
                "Dota 2 was not found in this computer's Steam libraries; pass --dota-path \
             \"<...>/steamapps/common/dota 2 beta\""
                    .to_owned()
            }),
    }
}

/// Why a folder the player named is not a Dota 2 install.
pub fn not_dota(folder: &Path) -> String {
    format!(
        "{} holds no Dota 2: no game/dota/{} in it. Pick the \"dota 2 beta\" folder (in a Steam \
         library, under steamapps/common).",
        folder.display(),
        heatel::locate::PACKAGE
    )
}

/// The most an import writes; [`room`] asks for this much free space.
pub const IMPORT_ROOM: u64 = dbota_import::cache_dir::MAX_WRITTEN;

/// Whether the drive of `cache` has room for an import; why not, when it has not.
pub fn room(cache: &Path, needed: u64) -> Result<(), String> {
    match crate::desktop::free_space(cache) {
        Some(free) if free < needed => Err(no_room(cache, needed, free)),
        _ => Ok(()),
    }
}

/// What a player is told when the drive is too full.
fn no_room(cache: &Path, needed: u64, free: u64) -> String {
    let gb = |bytes: u64| bytes as f64 / 1e9;
    format!(
        "Not enough disk space: the copy needs up to {:.1} GB on the drive of {}, and {:.1} GB \
         is free. Free some space and try again.",
        gb(needed),
        cache.display(),
        gb(free)
    )
}

/// How an import ended.
#[derive(Debug, Default)]
pub struct Imported {
    /// Files written.
    pub files: usize,
    /// Assets that did not convert, with why: what they draw is missing.
    pub failed: Vec<(String, String)>,
    /// The player stopped it: the cache in place stays.
    pub stopped: bool,
}

/// How far an import is, told before each step; the answer stops it or lets it go on.
pub type Progress<'a> = &'a mut dyn FnMut(&dbota_import::import::Progress<'_>) -> Flow;

/// Imports into `cache` from `install`; does nothing when the cache was made from the same
/// install already, unless `force`. A stopped or failed import leaves the cache that was there.
pub fn import_from(
    install: &Install,
    cache: &Path,
    force: bool,
    progress: Progress,
) -> Result<Imported, String> {
    let root = managed_cache_root();
    import_in(install, cache, force, progress, root.as_deref())
}

fn import_in(
    install: &Install,
    cache: &Path,
    force: bool,
    progress: Progress,
    root: Option<&Path>,
) -> Result<Imported, String> {
    let plan = plan()?;
    let stamp = stamp(&install.game, &plan_id(plan)).map_err(|e| e.to_string())?;
    let _lease = managed_import(root, cache)?;
    if !force && is_current(cache, &stamp) && dbota_import::import::layouts_current(cache) {
        println!("dbota import: {} is up to date", cache.display());
        report_maintenance(root, cache);
        return Ok(Imported::default());
    }
    room(cache, IMPORT_ROOM)?;
    println!(
        "dbota import: {} -> {}",
        install.game.display(),
        cache.display()
    );
    let started = std::time::Instant::now();
    let report =
        import_into(&install.game, cache, plan, &stamp, progress).map_err(|e| match &e {
            heatel::Error::Io { source, .. }
                if source.kind() == std::io::ErrorKind::StorageFull =>
            {
                no_room(
                    cache,
                    IMPORT_ROOM,
                    crate::desktop::free_space(cache).unwrap_or(0),
                )
            }
            _ => e.to_string(),
        })?;
    if report.stopped {
        println!("dbota import: stopped; {} stays as it was", cache.display());
        return Ok(Imported {
            stopped: true,
            ..Imported::default()
        });
    }
    let bytes: u64 = report.files.values().sum();
    println!(
        "dbota import: {} files, {} MB written ({} MB read) in {:.0} s",
        report.files.len(),
        bytes >> 20,
        report.read >> 20,
        started.elapsed().as_secs_f32()
    );
    report_maintenance(root, cache);
    if !report.notes.is_empty() {
        // What a converter left out on purpose (a voice pack's lines no game event plays): the
        // count only, the lines are for a developer reading the converter.
        println!(
            "dbota import: {} assets left parts out on purpose",
            report.notes.len()
        );
    }
    for (asset, why) in &report.warnings {
        eprintln!("dbota import: warning: {asset}: {why}");
    }
    for (asset, why) in &report.failed {
        eprintln!("dbota import: {asset} did not convert: {why}");
    }
    Ok(Imported {
        files: report.files.len(),
        failed: report.failed,
        stopped: false,
    })
}

/// `dbota import`: imports from `dota` (else the install the Steam libraries hold), printing a
/// line per tenth of the way. Returns whether every asset converted.
pub fn run(dota: Option<&Path>, cache: &Path, force: bool) -> Result<bool, String> {
    let install = locate(dota)?;
    let mut shown = 0;
    let imported = import_from(&install, cache, force, &mut |p| {
        let tenth = (p.done * 10 / p.total.max(1)) as u32;
        if tenth > shown {
            shown = tenth;
            println!("  {tenth}0% ({})", p.item);
        }
        Flow::Continue
    })?;
    Ok(imported.failed.is_empty())
}

/// The only root whose caches this client manages. Custom locations carry no deletion authority.
pub fn managed_cache_root() -> Option<PathBuf> {
    match crate::desktop::windows_data() {
        Some(data) => Some(data.join("cache")),
        None => managed_cache_root_in(std::env::var_os("HOME").as_deref()),
    }
}

pub fn managed_cache_root_in(home: Option<&OsStr>) -> Option<PathBuf> {
    home.map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".cache/dbota"))
}

/// The cache of this build: `$DBOTA_ASSETS`, else its folder in the managed root (`root`), else,
/// on a machine that names no home, beside the working folder.
pub fn cache_dir_in(root: Option<PathBuf>, assets: Option<&OsStr>) -> PathBuf {
    assets
        .filter(|assets| !assets.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.unwrap_or_else(|| PathBuf::from(".cache/dbota"))
                .join(dbota_import::import::folder_name())
        })
}

fn matching_root<'a>(root: Option<&'a Path>, cache: &Path) -> Option<&'a Path> {
    root.filter(|root| {
        cache == root.join(dbota_import::import::folder_name())
            && !std::fs::symlink_metadata(root).is_ok_and(|meta| meta.file_type().is_symlink())
    })
}

fn managed_import(root: Option<&Path>, cache: &Path) -> Result<Option<ManagedCacheLock>, String> {
    matching_root(root, cache)
        .map(|root| begin_managed_import(root, cache))
        .transpose()
        .map_err(|why| why.to_string())
}

fn maintain_managed_cache(root: Option<&Path>, cache: &Path) -> Result<PruneReport, String> {
    let Some(root) = matching_root(root, cache) else {
        return Ok(PruneReport::default());
    };
    dbota_import::index::CacheIndex::read(cache).map_err(|why| why.to_string())?;
    if !std::fs::symlink_metadata(cache.join(dbota_import::import::STAMP))
        .is_ok_and(|meta| meta.is_file())
        || !read_stamp(cache).is_some_and(|stamp| stamped_here(&stamp))
    {
        return Err("the cache stamp does not belong to this build".to_owned());
    }
    record_managed(root, cache).map_err(|why| why.to_string())?;
    touch_last_use(cache).map_err(|why| why.to_string())?;
    prune(root, cache).map_err(|why| why.to_string())
}

fn report_maintenance(root: Option<&Path>, cache: &Path) {
    match maintain_managed_cache(root, cache) {
        Ok(report) => {
            for (path, why) in report.failures {
                eprintln!("dbota import: cache {} stays: {why}", path.display());
            }
        }
        Err(why) => eprintln!("dbota import: old caches stay: {why}"),
    }
}

pub fn note_cache_use(root: Option<&Path>, cache: &Path) -> Result<(), String> {
    let _lease = matching_root(root, cache)
        .map(lock_managed_root)
        .transpose()
        .map_err(|why| why.to_string())?;
    dbota_import::index::CacheIndex::read(cache).map_err(|why| why.to_string())?;
    if matching_root(root, cache).is_some() {
        let report = maintain_managed_cache(root, cache)?;
        for (path, why) in report.failures {
            eprintln!("dbota: cache {} stays: {why}", path.display());
        }
    } else if let Err(why) = touch_last_use(cache) {
        // Unmanaged/read-only caches are never pruned by this client.
        eprintln!("dbota: unmanaged cache use could not be recorded: {why}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbota_import::cache_dir::{MAX_CACHES, record_managed};
    use dbota_import::import::{INDEX, OWNER_MARKER, STAMP, fingerprint, folder_name};
    use dbota_import::index::CacheIndex;
    use std::fs;

    struct Caches {
        scratch: PathBuf,
        home: PathBuf,
        root: PathBuf,
        current: PathBuf,
    }

    impl Caches {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let scratch = (0..128)
                .find_map(|_| {
                    let path = std::env::temp_dir().join(format!(
                        "dbota-root-{}-{}",
                        std::process::id(),
                        NEXT.fetch_add(1, Ordering::Relaxed)
                    ));
                    fs::create_dir(&path).ok().map(|()| path)
                })
                .expect("a fresh root fixture");
            let home = scratch.join("home");
            let root = managed_cache_root_in(Some(home.as_os_str())).unwrap();
            let current = root.join(folder_name());
            for n in 0..MAX_CACHES + 1 {
                let path = if n == MAX_CACHES {
                    current.clone()
                } else {
                    root.join(format!("rev29.1-{n:016x}"))
                };
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join(INDEX), format!("{}\n", CacheIndex::header())).unwrap();
                fs::write(path.join(STAMP), format!("{}/1/2", fingerprint())).unwrap();
                fs::File::options()
                    .write(true)
                    .open(path.join(STAMP))
                    .unwrap()
                    .set_modified(std::time::UNIX_EPOCH)
                    .unwrap();
                record_managed(&root, &path).unwrap();
            }
            Self {
                scratch,
                home,
                root,
                current,
            }
        }

        fn count(&self) -> usize {
            fs::read_dir(&self.root)
                .unwrap()
                .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_dir())
                .count()
        }
    }

    impl Drop for Caches {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.scratch);
        }
    }

    /// A cache stamped by this build whose portrait framings are of another layout (another
    /// build's under the same importer revision) needs importing again; one of this layout does
    /// not.
    #[test]
    fn a_cache_with_framings_of_another_layout_needs_a_reimport() {
        let caches = Caches::new();
        let cache = &caches.current;
        let file = dbota_import::import::PORTRAITS_FILE;
        fs::create_dir_all(cache.join("models")).unwrap();
        let index = format!("{}\nfile 1 {file} small\n", CacheIndex::header());
        fs::write(cache.join(INDEX), index).unwrap();
        fs::write(cache.join(file), "# logical, camera position (3) ...\n").unwrap();
        assert_eq!(need(cache), Some(Need::Reimport));
        let ours = format!("{}\n", dbota_import::text::PORTRAITS_LAYOUT);
        fs::write(cache.join(file), ours).unwrap();
        assert_eq!(need(cache), None);
    }

    #[test]
    fn a_custom_assets_override_does_not_authorize_managed_root_pruning() {
        let caches = Caches::new();
        let custom = caches.scratch.join("custom").join(folder_name());
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("keep"), b"custom data").unwrap();
        let selected = cache_dir_in(
            managed_cache_root_in(Some(caches.home.as_os_str())),
            Some(custom.as_os_str()),
        );
        maintain_managed_cache(Some(&caches.root), &selected).unwrap();
        assert_eq!(caches.count(), MAX_CACHES + 1);
        assert_eq!(fs::read(custom.join("keep")).unwrap(), b"custom data");
        assert!(!custom.join(OWNER_MARKER).exists());
    }

    #[test]
    fn unset_and_relative_homes_grant_no_deletion_authority() {
        let caches = Caches::new();
        for home in [
            None,
            Some(OsStr::new("relative-home")),
            Some(OsStr::new("")),
        ] {
            let root = managed_cache_root_in(home);
            assert!(root.is_none());
            maintain_managed_cache(root.as_deref(), &caches.current).unwrap();
            assert_eq!(caches.count(), MAX_CACHES + 1);
        }
    }

    #[test]
    fn an_up_to_date_import_records_use_and_maintains_its_root() {
        let caches = Caches::new();
        let install = caches.scratch.join("install");
        let game = install.join("game/dota");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("pak01_dir.vpk"), b"x").unwrap();
        let expected = stamp(&game, &plan_id(plan().unwrap())).unwrap();
        fs::write(caches.current.join(STAMP), expected).unwrap();
        fs::remove_file(caches.current.join(OWNER_MARKER)).unwrap();
        let install = Install {
            folder: install,
            game,
        };
        let imported = import_in(
            &install,
            &caches.current,
            false,
            &mut |_| panic!("an up-to-date cache is not converted"),
            Some(&caches.root),
        )
        .unwrap();
        assert!(imported.failed.is_empty());
        assert!(
            caches
                .current
                .join(dbota_import::import::LAST_USE_MARKER)
                .is_file()
        );
        assert_eq!(caches.count(), MAX_CACHES);
    }

    #[test]
    fn the_real_default_folder_records_use_and_prunes_expired_caches() {
        let caches = Caches::new();
        fs::remove_file(caches.current.join(OWNER_MARKER)).unwrap();
        let selected = cache_dir_in(managed_cache_root_in(Some(caches.home.as_os_str())), None);
        assert_eq!(selected, caches.current);
        maintain_managed_cache(Some(&caches.root), &selected).unwrap();
        assert_eq!(caches.count(), MAX_CACHES);
        assert!(selected.join(OWNER_MARKER).is_file());
        assert!(
            selected
                .join(dbota_import::import::LAST_USE_MARKER)
                .is_file()
        );
    }
}
