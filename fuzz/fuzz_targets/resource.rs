//! A compiled resource and everything read out of it: the block table, each
//! block taken as KV3, the external references and the `DATA` tree (KV3, or
//! the struct an `NTRO` block lays out). No input panics.
#![no_main]

use dbota_assets::Resource;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    for block in resource.blocks() {
        let _ = resource.kv3(&block.kind);
    }
    let _ = resource.external_refs();
    let _ = resource.data();
    let _ = resource.trailing();
});
