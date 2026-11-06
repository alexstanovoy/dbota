//! What the golden tests compare with.

use std::path::PathBuf;

/// The reference cache: the one Source2Viewer 20 exported for version 6 of the manifest, kept
/// as the converters' golden reference (the client's own cache is made by `dbota import` since
/// version 7). `DBOTA_REFERENCE` names another.
///
/// Fails the test, with how to make the cache, when it is absent or empty: a golden test with
/// nothing to compare with must not pass.
pub fn reference() -> PathBuf {
    let path = std::env::var_os("DBOTA_REFERENCE")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .expect("DBOTA_REFERENCE must name the reference cache");
    let held = std::fs::read_dir(&path).map(|mut entries| entries.next().is_some());
    assert!(
        held.as_ref().is_ok_and(|held| *held),
        "the reference cache is {} (DBOTA_REFERENCE = {}): the golden tests compare with it and \
         cannot pass without it; make it with `tools/reference_cache.sh` (needs a Dota 2 install; \
         it makes DBOTA_REFERENCE, by default ~/.cache/dbota-gate/reference-v6) and point \
         DBOTA_REFERENCE at it",
        match held {
            Ok(_) => "empty".to_owned(),
            Err(e) => format!("unreadable: {e}"),
        },
        path.display(),
    );
    path
}

/// Fails the test when it compared nothing in `what`: a comparison over an empty set holds
/// vacuously and says nothing about the code.
#[allow(dead_code, reason = "not every test file counts what it compared")]
#[track_caller]
pub fn assert_compared(what: &str, count: usize) {
    println!("{count} {what} compared");
    assert!(
        count > 0,
        "compared 0 {what} with the reference cache {} (DBOTA_REFERENCE): it holds no file for them; \
         make it with `tools/reference_cache.sh`",
        reference().display()
    );
}

/// The reference's file of manifest output `out`: none for a line added after the reference
/// was exported, which it has nothing to compare with.
#[allow(dead_code, reason = "not every test file compares single outputs")]
pub fn exported(out: &str) -> Option<PathBuf> {
    let path = reference().join(out);
    path.is_file().then_some(path)
}
