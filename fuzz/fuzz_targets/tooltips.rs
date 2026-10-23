//! The tooltip texts: the localization (the input's first file) and the item and ability
//! scripts (the rest), read for the names the client's own `ids.txt` asks for. No input
//! panics.
#![no_main]

#[path = "common/files.rs"]
mod files;

use dbota_assets::{Wanted, tooltips_json};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let wanted = Wanted::parse(include_str!("../../assets-manifest/ids.txt"));
    let parts: Vec<&[u8]> = files::split(data).into_iter().map(|(_, b)| b).collect();
    let _ = tooltips_json(&wanted, parts[0], &parts[1..]);
});
