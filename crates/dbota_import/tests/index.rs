//! Index corruption and resource limits of the asset cache the client reads.
use dbota_import::index::{CacheIndex, MAX_FILES, MAX_INDEX};

fn index(lines: &str) -> Vec<u8> {
    format!("{}\n{lines}", CacheIndex::header()).into_bytes()
}

#[test]
fn cache_indexes_reject_corruption_with_specific_errors() {
    let valid = "file 1 icons/items/blink.png\n";
    let cases = [
        (
            index("file 1 icons/items/../../outside.png\n"),
            "file it must not",
        ),
        (
            index(&format!("{valid}{valid}")),
            "lists icons/items/blink.png again",
        ),
        (
            index("file 1 icons/items/blink.png small\n"),
            "marks icons/items/blink.png wrongly as small",
        ),
        (
            index("file -1 icons/items/blink.png\n"),
            "a line it cannot read",
        ),
        (
            index("file 18446744073709551615 icons/items/blink.png\n"),
            "a line it cannot read",
        ),
        (
            index("file 1 icons/items/blink.png"),
            "does not end with a new line",
        ),
        (
            b"wrong\nfile 1 icons/items/blink.png\n".to_vec(),
            "index.txt starts with",
        ),
        (vec![255], "index.txt is not text"),
        (
            index("file 67108865 icons/items/blink.png\n"),
            "at most 67108864",
        ),
    ];
    for (bytes, expected) in cases {
        let why = CacheIndex::parse(&bytes).expect_err(expected).to_string();
        assert!(why.contains(expected), "expected {expected:?}, got {why:?}");
    }
}

#[test]
fn cache_index_limits_bound_entries_total_bytes_and_resident_bytes() {
    let entries: String = (0..=MAX_FILES)
        .map(|n| format!("file 1 icons/items/{n}.png\n"))
        .collect();
    let total: String = (0..7)
        .map(|n| format!("file 268435456 models/{n}.bin\n"))
        .collect();
    let resident: String = (0..33)
        .map(|n| format!("file 4194304 text/{n}.json small\n"))
        .collect();
    for (lines, expected) in [
        (entries, "more than 8192 files"),
        (total, "bytes of files"),
        (resident, "to keep in memory"),
    ] {
        let why = CacheIndex::parse(&index(&lines))
            .expect_err(expected)
            .to_string();
        assert!(why.contains(expected), "{why}");
    }
    let oversized = vec![b' '; MAX_INDEX as usize + 1];
    let why = CacheIndex::parse(&oversized)
        .expect_err("byte limit")
        .to_string();
    assert!(
        why.contains("index.txt is 2097153 bytes, at most 2097152"),
        "{why}"
    );
}

#[test]
#[ignore = "reads the real imported cache; the gate install-tests step supplies DBOTA_ASSETS"]
fn the_real_imported_cache_passes_shared_index_validation() {
    let root = std::path::PathBuf::from(
        std::env::var_os("DBOTA_IMPORTED")
            .or_else(|| std::env::var_os("DBOTA_ASSETS"))
            .expect("an imported cache"),
    );
    let index = CacheIndex::read(&root).expect("the real cache passes the bounded reader");
    assert!(!index.files().is_empty());
    for file in index.files() {
        let metadata =
            std::fs::symlink_metadata(root.join(&file.path)).expect("every listed file exists");
        assert!(metadata.is_file(), "{} is not a regular file", file.path);
        assert_eq!(metadata.len(), file.size, "{} changed size", file.path);
    }
    println!(
        "real cache index: {} files, {} bytes accepted",
        index.files().len(),
        index.bytes()
    );
}
