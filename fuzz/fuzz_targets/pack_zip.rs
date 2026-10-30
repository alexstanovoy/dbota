//! A prepared cache's zip: the input is the whole archive. It is read as a container (the
//! directory, then every member inflated into its declared size and checked), then taken as a
//! cache of this build. No input panics, and whatever is taken holds the contracts the install
//! relies on: plain paths in the cache's folders, sizes within the limits, every file as long
//! as its index says.
#![no_main]

use dbota_import::import::{INDEX, STAMP, safe_path};
use dbota_import::pack::{Container, MAX_FILE, MAX_TOTAL, Pack};
use dbota_import::zip::ZipContainer;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The container alone: every member it lists reads or fails, none past its limit.
    let Ok(container) = ZipContainer::open(data.to_vec()) else {
        return;
    };
    for at in 0..container.entries().len() {
        if let Ok(bytes) = container.read(at) {
            assert_eq!(bytes.len() as u64, container.entries()[at].size);
            assert!(bytes.len() as u64 <= MAX_FILE);
        }
    }
    let Ok(pack) = Pack::open(container) else {
        return;
    };
    let mut total = 0;
    for n in 0..pack.len() {
        let entry = pack.entry(n).expect("a listed file");
        assert!(safe_path(&entry.path) && entry.path != STAMP && entry.path != INDEX);
        if let Ok(bytes) = pack.read(n) {
            total += bytes.len() as u64;
        }
    }
    assert!(total <= MAX_TOTAL);
});
