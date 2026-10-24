//! What the golden tests compare with.

use std::path::PathBuf;

/// The reference cache: the one Source2Viewer 20 exported for version 6 of the manifest, kept
/// as the converters' golden reference (the client's own cache is made by `dbota import` since
/// version 7). `DBOTA_REFERENCE` names another.
pub fn reference() -> PathBuf {
    std::env::var_os("DBOTA_REFERENCE").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/dbota/v6"),
        PathBuf::from,
    )
}

/// The reference's file of manifest output `out`: none for a line added after the reference
/// was exported, which it has nothing to compare with.
#[allow(dead_code, reason = "not every test file compares single outputs")]
pub fn exported(out: &str) -> Option<PathBuf> {
    let path = reference().join(out);
    path.is_file().then_some(path)
}
