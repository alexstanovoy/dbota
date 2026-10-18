//! A binary KV3 document, any version and compression: no input panics.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = dbota_assets::kv3::decode(data);
});
