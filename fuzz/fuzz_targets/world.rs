//! A world (`.vwrld_c`) exported as the importer exports the map: its nodes (`.vwnod_c`),
//! entity lumps, the models and materials they place, cut to a band that keeps everything.
//! The input is the world, then the files it names (common/files.rs). No input panics.
#![no_main]

#[path = "common/files.rs"]
mod files;

use dbota_assets::{Band, export_world};
use libfuzzer_sys::fuzz_target;

const WORLD: &str = "maps/w/world.vwrld_c";

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let pack = files::resolve(&[WORLD], &parts);
    let src = files::install(&[("maps/w.vpk", &pack)]);
    let Some(vpk) = files::open(&src, "maps/w.vpk") else {
        return;
    };
    let band = Band {
        half_width: 1e9,
        v_min: -1e9,
        v_max: 1e9,
    };
    let uri = |vtex: &str| format!("{vtex}.png");
    let _ = export_world(&src, &[&vpk], WORLD, &band, "mid.bin", &uri);
});
