//! The shop's data: `scripts/shops.txt` (the input's first file) and the localization (its
//! second) into the cache's `text/shop.json`. No input panics.
#![no_main]

#[path = "common/files.rs"]
mod files;

use dbota_assets::shop_json;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let localization = parts.get(1).map_or(&b""[..], |(_, bytes)| bytes);
    let _ = shop_json(parts[0].1, localization);
});
