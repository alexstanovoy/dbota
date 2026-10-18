//! A VPK package, as a directory file with archive parts and as a single
//! file: the tree, every entry's path and lookup, and the files read back
//! with their CRC. No input panics.
#![no_main]

use dbota_assets::{SliceSource, Vpk};
use libfuzzer_sys::fuzz_target;

/// Entries read back at most: enough to reach every kind of location.
const READS: usize = 64;

fuzz_target!(|data: &[u8]| {
    let mut src = SliceSource::default();
    for path in ["pak01_dir.vpk", "pak01_000.vpk", "pak01_001.vpk", "single.vpk"] {
        src.insert(path, data.to_vec());
    }
    for dir in ["pak01_dir.vpk", "single.vpk"] {
        let Ok(vpk) = Vpk::open(&src, dir) else {
            continue;
        };
        for entry in vpk.entries().take(READS) {
            let path = entry.path();
            let _ = (entry.size(), vpk.get(&path).is_some());
            let _ = vpk.read(&src, entry);
        }
    }
});
