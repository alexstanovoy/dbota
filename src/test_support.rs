//! Where the binary's tests keep their files: under the target folder, never beside the sources.
//! (The integration tests in `tests/` are separate crates that create their own scratch folders, so
//! none needs this; one that ever does can `#[path]`-include this file.)

use std::path::PathBuf;

/// The folder tests keep their files in: `$CARGO_TARGET_DIR`, else this checkout's `target`.
pub(crate) fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    )
}

/// A path for a file or folder a test writes, under a `scratch` folder of the target folder (never the
/// source tree), whose parent folders are created: nothing else makes them in a fresh checkout.
pub(crate) fn scratch_path(name: &str) -> PathBuf {
    let path = target_dir().join("scratch").join(name);
    std::fs::create_dir_all(path.parent().expect("a path has a parent"))
        .expect("the scratch folder");
    path
}
