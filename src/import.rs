//! `dbota import`: the asset cache made from the local Dota 2 install, by the same converter
//! the browser runs. Valve's files land only in the cache, never in a repository.

use std::path::{Path, PathBuf};

use dbota_import::cache_dir::{import_into, is_current, prune, read_stamp, stamp};
use dbota_import::import::{Flow, IDS, MANIFEST, plan, plan_id, stamped_here};
use dbota_import::pack_dir;
use heatel::find_game_dir;

/// What the cache at a path lacks for this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// No finished import is there.
    Import,
    /// An import of another build's manifest is: its files may lack what the client now asks
    /// for.
    Reimport,
}

/// Whether the cache at `cache` needs an import before dbota draws Dota's art and sound: it
/// draws nothing without one.
pub fn need(cache: &Path) -> Option<Need> {
    if !cache.join(dbota_import::cache_dir::INDEX).is_file() {
        return Some(Need::Import);
    }
    let made_here = read_stamp(cache).is_some_and(|stamp| stamped_here(&stamp));
    (!made_here).then_some(Need::Reimport)
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
    let game: PathBuf = find_game_dir(dota).ok_or_else(|| {
        "Dota 2 not found; pass --dota-path \"<...>/steamapps/common/dota 2 beta\"".to_string()
    })?;
    let plan = plan()?;
    let stamp = stamp(&game, &plan_id(plan)).map_err(|e| e.to_string())?;
    if !force && is_current(cache, &stamp) {
        println!("dbota import: {} is up to date", cache.display());
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
    // The caches of builds long gone leave when a cache of the default place is made.
    let default_place = cache
        .file_name()
        .is_some_and(|n| n == dbota_import::import::folder_name().as_str());
    if default_place && let Err(why) = prune(cache) {
        eprintln!("dbota import: old caches stay: {why}");
    }
    for (asset, why) in &report.failed {
        eprintln!("dbota import: {asset} did not convert: {why}");
    }
    Ok(report.failed.is_empty())
}
