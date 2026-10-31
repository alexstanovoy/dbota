//! Prepared asset caches for the tests: a small one made of the real cache's small files (the
//! stamp and every file are the importer's own), and zips of such caches.
#![allow(dead_code)] // each test file uses the helpers it needs

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dbota_import::import::{INDEX, Report, STAMP};
use dbota_import::zip::ZipWriter;

/// Biggest file of the small cache.
const SMALL: u64 = 64 << 10;

/// A scratch folder of the test `what`, emptied now and when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(what: &str) -> Scratch {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("prepared-{what}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The asset cache `dbota import` made for this build: `DBOTA_ASSETS`, else the usual place.
pub fn real_cache() -> PathBuf {
    std::env::var_os("DBOTA_ASSETS").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").expect("a home"))
                .join(".cache/dbota")
                .join(dbota_import::import::folder_name())
        },
        PathBuf::from,
    )
}

/// Every file below `dir` by its `/`-separated path.
pub fn read_folder(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for item in fs::read_dir(&folder).unwrap_or_else(|e| panic!("{}: {e}", folder.display())) {
            let item = item.expect("a folder entry");
            let name = format!("{prefix}{}", item.file_name().to_string_lossy());
            if item.file_type().expect("a file type").is_dir() {
                pending.push((item.path(), format!("{name}/")));
            } else {
                files.insert(name, fs::read(item.path()).expect("a file"));
            }
        }
    }
    files
}

/// The real cache's files of at most [`SMALL`] bytes, the stamp and an index of just those: a
/// finished cache of this build, small enough to copy, zip and load at every test.
pub fn small_cache() -> BTreeMap<String, Vec<u8>> {
    let real = real_cache();
    let listed = fs::read_to_string(real.join(INDEX)).unwrap_or_else(|e| {
        panic!(
            "{} holds no asset cache: DBOTA_ASSETS names one `dbota import` made ({e})",
            real.display()
        )
    });
    let mut report = Report::default();
    let mut files = BTreeMap::new();
    for line in listed.lines().filter_map(|l| l.strip_prefix("file ")) {
        let mut words = line.split(' ');
        let size: u64 = words.next().and_then(|s| s.parse().ok()).expect("a size");
        let path = words.next().expect("a path");
        if size <= SMALL {
            report.files.insert(path.to_owned(), size);
            files.insert(
                path.to_owned(),
                fs::read(real.join(path)).expect("a cache file"),
            );
        }
    }
    let tops: std::collections::BTreeSet<_> =
        files.keys().filter_map(|p| p.split('/').next()).collect();
    assert!(
        tops.len() >= 8,
        "the small cache reaches {tops:?}: too few folders of the real cache"
    );
    files.insert(INDEX.to_owned(), report.index().into_bytes());
    files.insert(
        STAMP.to_owned(),
        fs::read(real.join(STAMP)).expect("the real cache's stamp"),
    );
    files
}

/// Writes `files` as a folder at `dir`.
pub fn write_folder(dir: &Path, files: &BTreeMap<String, Vec<u8>>) {
    for (path, bytes) in files {
        let target = dir.join(path);
        fs::create_dir_all(target.parent().expect("a parent")).expect("a folder");
        fs::write(target, bytes).expect("a file");
    }
}

/// Writes `files` as a zip at `zip`, in the order given: names are written as they are.
pub fn write_zip<'a>(zip: &Path, files: impl IntoIterator<Item = (&'a str, &'a [u8])>) {
    let mut out = ZipWriter::new(BufWriter::new(fs::File::create(zip).expect("a zip file")));
    for (name, bytes) in files {
        out.add(name, bytes).expect("a member");
    }
    out.finish()
        .expect("the directory")
        .flush()
        .expect("the zip");
}

/// `dbota <command> <argument>` run with the asset cache at `cache`.
pub fn dbota(cache: &Path, command: &str, argument: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dbota"))
        .arg("--cache-dir")
        .arg(cache)
        .arg(command)
        .arg(argument)
        .env_remove("DBOTA_ASSETS")
        .output()
        .expect("dbota runs")
}

/// What a finished `dbota` said on its error output.
pub fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
