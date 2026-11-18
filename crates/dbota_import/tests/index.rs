//! Index corruption and resource limits of the asset cache the client reads.
use dbota_import::index::{CacheIndex, MAX_FILES, MAX_INDEX};

fn index(lines: &str) -> Vec<u8> {
    format!("{}\n{lines}", CacheIndex::header()).into_bytes()
}

/// A scratch folder of a test's own, removed when it ends.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("dbota-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The index of a cache folder is judged by its length before any of it is read: a corrupt file
/// that advertises 64 GiB (sparse here: nothing is allocated or read) reserves no memory.
#[test]
fn a_cache_index_file_over_its_bound_is_refused_from_its_length_alone() {
    let scratch = Scratch::new("sparse");
    let path = scratch.0.join("index.txt");
    std::fs::write(&path, index("file 1 icons/items/blink.png\n")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(64 << 30)
        .unwrap();
    let why = CacheIndex::read(&scratch.0)
        .expect_err("a sparse oversized index")
        .to_string();
    assert!(
        why.contains("index.txt is 68719476736 bytes, at most 2097152"),
        "{why}"
    );
}

/// A cache folder's index file is read whole and validated like any other.
#[test]
fn a_valid_cache_index_file_is_read_and_a_link_in_its_place_is_refused() {
    let scratch = Scratch::new("read");
    let bytes = index("file 1 icons/items/blink.png\n");
    std::fs::write(scratch.0.join("index.txt"), &bytes).unwrap();
    let read = CacheIndex::read(&scratch.0).expect("a valid index");
    assert_eq!(read.files()[0].path, "icons/items/blink.png");
    #[cfg(unix)]
    {
        let elsewhere = scratch.0.join("elsewhere.txt");
        std::fs::rename(scratch.0.join("index.txt"), &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, scratch.0.join("index.txt")).unwrap();
        let why = CacheIndex::read(&scratch.0)
            .expect_err("a link")
            .to_string();
        assert!(why.contains("index.txt is not a regular file"), "{why}");
    }
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
