//! The importer's plan: a manifest and an `ids.txt` (the input split at its first NUL) read
//! into items, their outputs, the tooltip sources and the plan's id; then a cache's stamp
//! (the input's tail) checked against it. No input panics.
#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use dbota_import::cache_dir::{INDEX, STAMP, is_current, matches_plan, stamp_prefix};
use dbota_import::import::{Item, Plan, is_small, plan_id, safe_path};
use libfuzzer_sys::fuzz_target;

/// A cache folder of this process, in the temporary folder.
fn cache() -> &'static PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dbota-fuzz-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        std::fs::write(dir.join(INDEX), b"").expect("a temporary file");
        dir
    })
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut parts = text.splitn(3, '\0');
    let (manifest, ids, stamp) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    // A manifest that cannot be read is told, never planned: only what reads is walked.
    let Ok(plan) = Plan::parse(manifest, ids) else {
        return;
    };
    for item in &plan.items {
        for out in item.outputs() {
            let _ = (safe_path(out), is_small(out, 1));
        }
        if let Item::Tooltips { wanted, .. } = item {
            let _ = wanted.sources();
        }
    }
    let prefix = stamp_prefix(ids, &plan_id(manifest, ids));
    std::fs::write(cache().join(STAMP), stamp).expect("a temporary file");
    let _ = (matches_plan(cache(), &prefix), is_current(cache(), &prefix));
});
