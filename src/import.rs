//! `dbota import`: the asset cache made from the local Dota 2 install, by the same converter
//! the browser runs. Valve's files land only in the cache, never in a repository.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use dbota_import::cache_dir::{
    ManagedCacheLock, PruneReport, begin_managed_import, import_into, is_current,
    lock_managed_root, prune, read_stamp, record_managed, stamp, touch_last_use,
};
use dbota_import::import::{Flow, IDS, MANIFEST, plan, plan_id, stamped_here};
use dbota_import::pack_dir;
use heatel::find_game_dir;

/// What the cache at a path lacks for this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// No finished import is there.
    Import,
    /// An import another build made is (its files may lack what the client now asks for), or
    /// one of this build's stamp whose files are of a layout older than this build reads
    /// ([`layouts_current`](dbota_import::import::layouts_current)).
    Reimport,
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
        Need::Import => format!(
            "no asset cache at {}: dbota draws Dota's art and sound from the cache and nothing else; \
             run `dbota import`, or `dbota import-cache` for a cache someone prepared",
            cache.display()
        ),
        Need::Reimport => format!(
            "the asset cache {} is out of date for this version (it needs assets {}): \
             run `dbota import`, or `dbota import-cache` for a cache prepared by this version",
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

/// How far an import is: the share done (0 to 1) and the asset at hand.
pub type Progress<'a> = &'a mut dyn FnMut(f32, &str);

/// Imports into `cache` from the install at `dota` (a `dota 2 beta` folder, else the usual
/// Steam places); does nothing when the cache was made from the same install already, unless
/// `force`. Returns whether every asset converted.
pub fn run(dota: Option<&Path>, cache: &Path, force: bool) -> Result<bool, String> {
    run_with(dota, cache, force, &mut tenths())
}

/// A progress that prints a line per tenth of the way: a terminal, not a progress bar.
pub fn tenths() -> impl FnMut(f32, &str) {
    let mut shown = 0;
    move |done, item| {
        let tenth = (done * 10.0) as u32;
        if tenth > shown {
            shown = tenth;
            println!("  {tenth}0% ({item})");
        }
    }
}

/// Puts the prepared cache at `from` (a folder, or a zip of one, as `dbota export-cache` makes it) in
/// place of the cache at `cache`, once it has been checked whole: a cache of another build, or
/// with a file it must not hold, is refused and the old one stays.
pub fn load_cache(from: &Path, cache: &Path, progress: Progress) -> Result<(), String> {
    println!(
        "dbota import-cache: {} -> {}",
        from.display(),
        cache.display()
    );
    let root = managed_cache_root();
    let _lease = managed_import(root.as_deref(), cache)?;
    let started = std::time::Instant::now();
    let summary = pack_dir::install(from, cache, &mut |done, total, item| {
        progress(done as f32 / total.max(1) as f32, item);
    })
    .map_err(|e| e.to_string())?;
    println!(
        "dbota import-cache: {} files, {} MB, assets {} in {:.0} s",
        summary.files,
        summary.bytes >> 20,
        summary.stamp,
        started.elapsed().as_secs_f32()
    );
    report_maintenance(root.as_deref(), cache);
    Ok(())
}

/// Writes the cache at `cache` as the zip `to`, for someone without Dota 2 to load.
pub fn export_cache(cache: &Path, to: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let summary = pack_dir::export(cache, to).map_err(|e| e.to_string())?;
    println!(
        "dbota export-cache: {} files, {} MB of assets {} -> {} in {:.0} s",
        summary.files,
        summary.bytes >> 20,
        summary.stamp,
        to.display(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// [`run`], telling `progress` how far it is.
pub fn run_with(
    dota: Option<&Path>,
    cache: &Path,
    force: bool,
    progress: Progress,
) -> Result<bool, String> {
    let root = managed_cache_root();
    run_with_in(dota, cache, force, progress, root.as_deref())
}

fn run_with_in(
    dota: Option<&Path>,
    cache: &Path,
    force: bool,
    progress: Progress,
    root: Option<&Path>,
) -> Result<bool, String> {
    let game: PathBuf = find_game_dir(dota).ok_or_else(|| {
        "Dota 2 not found; pass --dota-path \"<...>/steamapps/common/dota 2 beta\"".to_string()
    })?;
    let plan = plan()?;
    let stamp = stamp(&game, &plan_id(plan)).map_err(|e| e.to_string())?;
    let _lease = managed_import(root, cache)?;
    if !force && is_current(cache, &stamp) && dbota_import::import::layouts_current(cache) {
        println!("dbota import: {} is up to date", cache.display());
        report_maintenance(root, cache);
        return Ok(true);
    }
    println!("dbota import: {} -> {}", game.display(), cache.display());
    let started = std::time::Instant::now();
    let report = import_into(&game, cache, plan, &stamp, &mut |p| {
        progress(p.done as f32 / p.total.max(1) as f32, p.item);
        Flow::Continue
    })
    .map_err(|e| e.to_string())?;
    let bytes: u64 = report.files.values().sum();
    println!(
        "dbota import: {} files, {} MB written ({} MB read) in {:.0} s",
        report.files.len(),
        bytes >> 20,
        report.read >> 20,
        started.elapsed().as_secs_f32()
    );
    report_maintenance(root, cache);
    for note in &report.notes {
        println!("dbota import: {note}");
    }
    for (asset, why) in &report.warnings {
        eprintln!("dbota import: warning: {asset}: {why}");
    }
    for (asset, why) in &report.failed {
        eprintln!("dbota import: {asset} did not convert: {why}");
    }
    Ok(report.failed.is_empty())
}

/// The only root whose caches this client manages. Custom locations carry no deletion authority.
pub fn managed_cache_root() -> Option<PathBuf> {
    managed_cache_root_in(std::env::var_os("HOME").as_deref())
}

pub fn managed_cache_root_in(home: Option<&OsStr>) -> Option<PathBuf> {
    home.map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".cache/dbota"))
}

pub fn cache_dir_in(home: Option<&OsStr>, assets: Option<&OsStr>) -> PathBuf {
    assets
        .filter(|assets| !assets.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            managed_cache_root_in(home)
                .unwrap_or_else(|| PathBuf::from(".cache/dbota"))
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
        let selected = cache_dir_in(Some(caches.home.as_os_str()), Some(custom.as_os_str()));
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
        assert!(
            run_with_in(
                Some(&install),
                &caches.current,
                false,
                &mut |_, _| panic!("an up-to-date cache is not converted"),
                Some(&caches.root)
            )
            .unwrap()
        );
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
        let selected = cache_dir_in(Some(caches.home.as_os_str()), None);
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
