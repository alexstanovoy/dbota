//! A prepared cache's folder as a browser lists it: files by path (under a folder it was picked
//! with) and an index that is either the one the files call for or the input's own text; the
//! stamp is this build's, so the index, the paths and the sizes are what is checked. No input
//! panics, and what is taken holds the install's contracts (see `pack_zip`).
#![no_main]

use dbota_import::import::{IMPORT_REV, INDEX, STAMP, fingerprint, is_small, safe_path};
use dbota_import::pack::{MAX_TOTAL, Pack, SliceFolder};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input {
    files: Vec<(String, Vec<u8>)>,
    /// The index text instead of the right one.
    index: Option<String>,
    /// The folder picked, around the files.
    root: bool,
}

fuzz_target!(|input: Input| {
    let mut index = format!("dbota-cache 1 rev {IMPORT_REV}\n");
    for (path, bytes) in &input.files {
        let small = if is_small(path, bytes.len() as u64) { " small" } else { "" };
        index.push_str(&format!("file {} {path}{small}\n", bytes.len()));
    }
    let mut files = vec![
        (STAMP.to_owned(), format!("{}/1/2", fingerprint()).into_bytes()),
        (INDEX.to_owned(), input.index.unwrap_or(index).into_bytes()),
    ];
    files.extend(input.files);
    if input.root {
        for (path, _) in &mut files {
            *path = format!("picked/{path}");
        }
    }
    let Ok(folder) = SliceFolder::new(files) else {
        return;
    };
    let Ok(pack) = Pack::open(folder) else {
        return;
    };
    let mut total = 0;
    for n in 0..pack.len() {
        let entry = pack.entry(n).expect("a listed file");
        assert!(safe_path(&entry.path) && entry.path != STAMP && entry.path != INDEX);
        total += pack.read(n).expect("a file of a taken cache reads").len() as u64;
    }
    assert!(total <= MAX_TOTAL);
});
