//! Index corruption and resource limits shared by runtime discovery and prepared caches.
use dbota_import::import::{INDEX, STAMP, fingerprint};
use dbota_import::index::{CacheIndex, MAX_FILES, MAX_INDEX};
use dbota_import::pack::{Container, Entry, Pack, PackError};

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

struct Prepared {
    entries: Vec<Entry>,
    stamp: Vec<u8>,
    index: Vec<u8>,
}

impl Container for Prepared {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }
    fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
        match at {
            0 => Ok(self.stamp.clone()),
            1 => Ok(self.index.clone()),
            _ => panic!("validation must not read assets"),
        }
    }
}

#[test]
fn runtime_and_prepared_cache_reads_share_the_validated_index() {
    let root = std::env::temp_dir().join(format!("dbota-index-read-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for lines in [
        "file 1 icons/items/blink.png\n",
        "file 1 icons/items/blink.png small\n",
    ] {
        let bytes = index(lines);
        std::fs::write(root.join(INDEX), &bytes).unwrap();
        let stamp = format!("{}/1/2", fingerprint()).into_bytes();
        let prepared = Prepared {
            entries: vec![
                Entry {
                    path: STAMP.to_owned(),
                    size: stamp.len() as u64,
                },
                Entry {
                    path: INDEX.to_owned(),
                    size: bytes.len() as u64,
                },
                Entry {
                    path: "icons/items/blink.png".to_owned(),
                    size: 1,
                },
            ],
            stamp,
            index: bytes,
        };
        match (CacheIndex::read(&root), Pack::open(prepared)) {
            (Ok(runtime), Ok(pack)) => {
                assert_eq!(runtime.text(), pack.index());
                assert_eq!(runtime.files()[0].path, pack.entry(0).unwrap().path);
                assert_eq!(runtime.bytes(), pack.bytes());
            }
            (Err(runtime), Err(prepared)) => assert_eq!(runtime, prepared),
            _ => panic!("runtime and prepared validation diverged"),
        }
    }
    // Requires a sparse-file filesystem (the Linux gate uses one): no payload is allocated or read.
    // A corrupt index cannot reserve memory from its advertised length.
    std::fs::File::options()
        .write(true)
        .open(root.join(INDEX))
        .unwrap()
        .set_len(64 << 30)
        .unwrap();
    let why = CacheIndex::read(&root)
        .expect_err("sparse oversized index")
        .to_string();
    assert!(
        why.contains("index.txt is 68719476736 bytes, at most 2097152"),
        "{why}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_cache_ownership_metadata_is_validated_but_not_an_asset() {
    use dbota_import::import::{OWNER_MARKER, OWNER_TEXT};
    struct OwnedFolder {
        entries: Vec<Entry>,
        files: Vec<Vec<u8>>,
    }
    impl Container for OwnedFolder {
        fn entries(&self) -> &[Entry] {
            &self.entries
        }
        fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
            Ok(self.files[at].clone())
        }
    }
    for valid in [true, false] {
        let content = [
            (STAMP, format!("{}/1/2", fingerprint()).into_bytes()),
            (INDEX, index("file 1 icons/items/blink.png\n")),
            (
                OWNER_MARKER,
                if valid {
                    OWNER_TEXT.as_bytes().to_vec()
                } else {
                    b"forged".to_vec()
                },
            ),
            ("icons/items/blink.png", vec![1]),
        ];
        let folder = OwnedFolder {
            entries: content
                .iter()
                .map(|(path, bytes)| Entry {
                    path: (*path).to_owned(),
                    size: bytes.len() as u64,
                })
                .collect(),
            files: content.into_iter().map(|(_, bytes)| bytes).collect(),
        };
        let result = Pack::open(folder);
        if valid {
            let pack = result.expect("valid local marker");
            assert_eq!(pack.len(), 1);
            assert_eq!(pack.entry(0).unwrap().path, "icons/items/blink.png");
            assert!(!pack.index().contains(OWNER_MARKER));
        } else {
            let error = result.err().expect("invalid marker rejected").to_string();
            assert!(error.contains("invalid local ownership marker"), "{error}");
        }
    }
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

#[test]
fn local_last_use_metadata_is_bounded_and_excluded_from_asset_entries() {
    use dbota_import::import::{LAST_USE_MARKER, LAST_USE_TEXT};
    struct Folder {
        entries: Vec<Entry>,
        bytes: Vec<Vec<u8>>,
    }
    impl Container for Folder {
        fn entries(&self) -> &[Entry] {
            &self.entries
        }
        fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
            Ok(self.bytes[at].clone())
        }
    }
    for valid in [true, false] {
        let files = [
            (STAMP, format!("{}/1/2", fingerprint()).into_bytes()),
            (INDEX, index("file 1 icons/items/blink.png\n")),
            (
                LAST_USE_MARKER,
                if valid {
                    format!("{LAST_USE_TEXT}1234\n").into_bytes()
                } else {
                    b"untrusted local metadata".to_vec()
                },
            ),
            ("icons/items/blink.png", vec![1]),
        ];
        let folder = Folder {
            entries: files
                .iter()
                .map(|(path, bytes)| Entry {
                    path: (*path).to_owned(),
                    size: bytes.len() as u64,
                })
                .collect(),
            bytes: files.into_iter().map(|(_, bytes)| bytes).collect(),
        };
        match Pack::open(folder) {
            Ok(pack) if valid => {
                assert_eq!(pack.len(), 1);
                assert_eq!(pack.entry(0).unwrap().path, "icons/items/blink.png");
            }
            Err(error) if !valid => assert!(
                error.to_string().contains("invalid local last-use stamp"),
                "{error}"
            ),
            _ => panic!("last-use metadata validation diverged"),
        }
    }
}
