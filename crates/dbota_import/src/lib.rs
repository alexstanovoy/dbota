//! The importer in a browser: a Web Worker builds an [`Install`] from the files the user picked
//! (read lazily with `File.slice`, never whole), runs the import item by item and gets each
//! item's converted files back to write into its cache (the Origin Private File System). Nothing leaves
//! the page: there is no network code in here.
#![cfg(target_arch = "wasm32")]

use dbota_assets::import::{self, IMPORT_REV, Importer, Plan, Sink};
use dbota_assets::{Slice, SliceSource, Source};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;
use web_sys::{File, FileReaderSync};

/// The manifest the client was built with.
const MANIFEST: &str = include_str!("../../../assets-manifest/manifest.txt");
const VERSION: &str = include_str!("../../../assets-manifest/version");

/// Most files an install may hold: Dota's has ~700 packages; a folder of more is not one.
const MAX_FILES: usize = 4096;

/// A picked file, read by slices through the synchronous reader of a worker.
struct Picked {
    file: File,
    reader: FileReaderSync,
}

impl Slice for Picked {
    fn size(&self) -> u64 {
        self.file.size() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let to_io = |e: JsValue| std::io::Error::other(format!("{e:?}"));
        let slice = self
            .file
            .slice_with_f64_and_f64(offset as f64, (offset + len as u64) as f64)
            .map_err(to_io)?;
        let buffer = self.reader.read_as_array_buffer(&slice).map_err(to_io)?;
        let bytes = Uint8Array::new(&buffer).to_vec();
        if bytes.len() == len {
            Ok(bytes)
        } else {
            Err(std::io::ErrorKind::UnexpectedEof.into())
        }
    }
}

/// The package files of an install, by path relative to its `game/dota` folder.
#[wasm_bindgen]
pub struct Install {
    files: SliceSource<Picked>,
    count: usize,
}

#[wasm_bindgen]
impl Install {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Install {
        Install {
            files: SliceSource::default(),
            count: 0,
        }
    }

    /// Adds a file; false when it is not one the importer reads (only `*.vpk` at the top or
    /// in `maps/`) or the install has too many.
    pub fn add(&mut self, path: String, file: File) -> Result<bool, JsError> {
        let wanted = path.ends_with(".vpk")
            && (!path.contains('/')
                || (path.starts_with("maps/") && path.matches('/').count() == 1));
        if !wanted || self.count >= MAX_FILES {
            return Ok(false);
        }
        let reader = FileReaderSync::new().map_err(|e| JsError::new(&format!("{e:?}")))?;
        self.files.insert(path, Picked { file, reader });
        self.count += 1;
        Ok(true)
    }

    /// Whether the install has the package directory the importer starts from.
    pub fn has_pak(&self) -> bool {
        Source::size(&self.files, "pak01_dir.vpk").is_ok()
    }

    /// What says the cache made from this install is still current: the size and modification
    /// time of the package directory (Dota's updater rewrites it with every content change),
    /// with the manifest version and the importer's revision. Empty without the directory.
    pub fn stamp(&self, modified: f64) -> String {
        match Source::size(&self.files, "pak01_dir.vpk") {
            Ok(size) => format!("v{}/rev{IMPORT_REV}/{size}/{modified}", VERSION.trim()),
            Err(_) => String::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for Install {
    fn default() -> Self {
        Self::new()
    }
}

/// Files an item made, waiting for the worker to write them.
#[derive(Default)]
struct Collected(Vec<(String, Vec<u8>)>);

impl Sink for Collected {
    fn write(&mut self, path: &str, data: &[u8]) -> dbota_assets::Result<()> {
        self.0.push((path.to_owned(), data.to_vec()));
        Ok(())
    }
}

#[wasm_bindgen]
impl Install {
    /// Starts importing the manifest's assets out of this install; fails when the package
    /// directory is missing or is not one.
    pub fn start(self) -> Result<Job, JsError> {
        let importer = Importer::new(self.files, Plan::parse(MANIFEST))
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Job(importer))
    }
}

/// An import under way: call [`step`](Job::step) until it returns nothing, writing the files
/// each returns, then [`finish`](Job::finish).
#[wasm_bindgen]
pub struct Job(Importer<SliceSource<Picked>>);

#[wasm_bindgen]
impl Job {
    pub fn total(&self) -> usize {
        self.0.total()
    }

    pub fn done(&self) -> usize {
        self.0.done()
    }

    pub fn bytes(&self) -> f64 {
        self.0.bytes_written() as f64
    }

    /// The name of the item the next step makes.
    pub fn upcoming(&self) -> Option<String> {
        self.0.upcoming().map(str::to_owned)
    }

    /// Makes the next item: an array of `[path, bytes]` pairs (empty when it failed), or
    /// nothing when every item is made.
    pub fn step(&mut self) -> Option<Array> {
        let mut made = Collected::default();
        if !self.0.step(&mut made) {
            return None;
        }
        Some(
            made.0
                .iter()
                .map(|(path, data)| {
                    Array::of2(&JsValue::from_str(path), &Uint8Array::from(&data[..]))
                })
                .collect(),
        )
    }

    /// Ends the import and gives its report.
    pub fn finish(self) -> Report {
        Report(self.0.finish())
    }
}

/// The result of an import.
#[wasm_bindgen]
pub struct Report(import::Report);

#[wasm_bindgen]
impl Report {
    /// The index file's text.
    pub fn index(&self) -> String {
        self.0.index()
    }

    /// One line per item that failed: `logical: why`.
    pub fn failures(&self) -> Vec<String> {
        self.0
            .failed
            .iter()
            .map(|(item, why)| format!("{item}: {why}"))
            .collect()
    }

    pub fn files(&self) -> usize {
        self.0.files.len()
    }

    /// How many assets are left to later versions (animated models, the world).
    pub fn deferred(&self) -> usize {
        self.0.deferred.len()
    }
}

/// The stamp prefix of what this build writes, for a cache check before any install is picked.
#[wasm_bindgen]
pub fn cache_version() -> String {
    format!("v{}/rev{IMPORT_REV}", VERSION.trim())
}
