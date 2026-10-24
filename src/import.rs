//! `dbota import`: the asset cache made from the local Dota 2 install, by the same converter
//! the browser runs. Valve's files land only in the cache, never in a repository.

use std::path::{Path, PathBuf};

use dbota_import::cache_dir::{import_into, is_current, matches_plan, stamp, stamp_prefix};
use dbota_import::import::{Flow, Plan, plan_id};
use heatel::find_game_dir;

const MANIFEST: &str = include_str!("../assets-manifest/manifest.txt");
const IDS: &str = include_str!("../assets-manifest/ids.txt");

/// Warns when the cache at `cache` was not made by an import of this build's manifest: its files
/// may lack what the client now asks for.
pub fn warn_if_stale(cache: &Path) {
    if cache.is_dir()
        && !matches_plan(
            cache,
            &stamp_prefix(crate::ASSET_VERSION, &plan_id(MANIFEST, IDS)),
        )
    {
        eprintln!(
            "dbota: the asset cache {} was not made from this build's manifest; run `dbota import`",
            cache.display()
        );
    }
}

/// Imports into `cache` from the install at `dota` (a `dota 2 beta` folder, else the usual
/// Steam places); does nothing when the cache was made from the same install already, unless
/// `force`. Returns whether every asset converted.
pub fn run(dota: Option<&Path>, cache: &Path, force: bool) -> Result<bool, String> {
    let game: PathBuf = find_game_dir(dota).ok_or_else(|| {
        "Dota 2 not found; pass --dota-path \"<...>/steamapps/common/dota 2 beta\"".to_string()
    })?;
    let stamp =
        stamp(&game, crate::ASSET_VERSION, &plan_id(MANIFEST, IDS)).map_err(|e| e.to_string())?;
    if !force && is_current(cache, &stamp) {
        println!("dbota import: {} is up to date", cache.display());
        return Ok(true);
    }
    println!("dbota import: {} -> {}", game.display(), cache.display());
    let plan = Plan::parse(MANIFEST, IDS);
    let started = std::time::Instant::now();
    let mut shown = 0;
    let report = import_into(&game, cache, &plan, &stamp, &mut |p| {
        // A line per tenth of the way: a terminal, not a progress bar.
        let tenth = p.done * 10 / p.total.max(1);
        if tenth > shown {
            shown = tenth;
            println!("  {}0% ({}, {} MB)", tenth, p.item, p.bytes_written >> 20);
        }
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
    for (asset, why) in &report.failed {
        eprintln!("dbota import: {asset} drawn as a stand-in: {why}");
    }
    Ok(report.failed.is_empty())
}
