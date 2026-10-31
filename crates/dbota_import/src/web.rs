//! The importer in a browser: a Web Worker builds an [`Install`] from the files the user picked
//! (read lazily with `File.slice`, never whole), runs the import item by item and gets each
//! item's converted files back to write into its cache (the Origin Private File System). Nothing leaves
//! the page: there is no network code in here.

use std::rc::Rc;

use heatel::{Slice, SliceSource, Source};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;
use web_sys::{File, FileReaderSync};

use crate::import::{self, IDS, Importer, MANIFEST, Plan, Sink};
use crate::pack::{Container, MAX_FILES, Pack, SliceFolder};
use crate::zip::ZipContainer;

/// Most files an install may hold: Dota's has ~700 packages; a folder of more is not one.
const MAX_PACKAGES: usize = 4096;

/// A picked file, read by slices through the synchronous reader of a worker (one reader serves
/// every file).
struct Picked {
    file: File,
    reader: Rc<FileReaderSync>,
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
    reader: Option<Rc<FileReaderSync>>,
    count: usize,
}

#[wasm_bindgen]
impl Install {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Install {
        Install {
            files: SliceSource::default(),
            reader: None,
            count: 0,
        }
    }

    /// Adds a file; false when it is not one the importer reads (only `*.vpk` at the top or
    /// in `maps/`) or the install has too many.
    pub fn add(&mut self, path: String, file: File) -> Result<bool, JsError> {
        let wanted = path.ends_with(".vpk")
            && (!path.contains('/')
                || (path.starts_with("maps/") && path.matches('/').count() == 1));
        if !wanted || self.count >= MAX_PACKAGES {
            return Ok(false);
        }
        let reader = shared_reader(&mut self.reader)?;
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
    /// with the build's fingerprint. Empty without the directory.
    pub fn stamp(&self, modified: f64) -> String {
        match Source::size(&self.files, "pak01_dir.vpk") {
            Ok(size) => format!("{}/{size}/{modified}", cache_version()),
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

fn shared_reader(reader: &mut Option<Rc<FileReaderSync>>) -> Result<Rc<FileReaderSync>, JsError> {
    if reader.is_none() {
        let made = FileReaderSync::new().map_err(|e| JsError::new(&format!("{e:?}")))?;
        *reader = Some(Rc::new(made));
    }
    Ok(reader.clone().expect("the reader was made above"))
}

/// Files an item made, waiting for the worker to write them.
#[derive(Default)]
struct Collected(Vec<(String, Vec<u8>)>);

impl Sink for Collected {
    fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
        self.0.push((path.to_owned(), data.to_vec()));
        Ok(())
    }
}

#[wasm_bindgen]
impl Install {
    /// Starts importing the manifest's assets out of this install; fails when the package
    /// directory is missing or is not one.
    pub fn start(self) -> Result<Job, JsError> {
        let plan = Plan::parse(MANIFEST, IDS).map_err(|errors| {
            JsError::new(
                &errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let importer = Importer::new(self.files, plan).map_err(|e| JsError::new(&e.to_string()))?;
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
}

/// The asset fingerprint of this build ([`import::fingerprint`]): what a cache it may use starts
/// its stamp with, checked before any install is picked.
#[wasm_bindgen]
pub fn cache_version() -> String {
    import::fingerprint()
}

/// A prepared cache the player picked, a zip or a folder of one, checked whole before anything
/// of it is read (see [`crate::pack`]): the worker copies its files into the page's cache one by
/// one with [`read`](Prepared::read).
#[wasm_bindgen]
pub struct Prepared(Pack<Box<dyn Container>>);

#[wasm_bindgen]
impl Prepared {
    /// The cache in the zip file `file`; fails with the reason when it is not one of this build.
    pub fn from_zip(file: File) -> Result<Prepared, JsError> {
        let reader = shared_reader(&mut None)?;
        let container = ZipContainer::open(Picked { file, reader })?;
        Ok(Prepared(Pack::open(
            Box::new(container) as Box<dyn Container>
        )?))
    }

    /// The cache in the files of a picked folder: an array of `[path, File]` pairs, the paths
    /// as the picker names them (`webkitRelativePath`).
    pub fn from_folder(files: Array) -> Result<Prepared, JsError> {
        if files.length() as usize > MAX_FILES + 2 {
            return Err(JsError::new(&format!(
                "This folder has {} files, an asset cache has at most {MAX_FILES}.",
                files.length()
            )));
        }
        let mut reader = None;
        let mut picked = Vec::new();
        for pair in files.iter() {
            let pair = Array::from(&pair);
            let (path, file) = (pair.get(0), pair.get(1));
            let path = path
                .as_string()
                .ok_or_else(|| JsError::new("a file without a path"))?;
            let file = file
                .dyn_into::<File>()
                .map_err(|_| JsError::new("not a file"))?;
            picked.push((
                path,
                Picked {
                    file,
                    reader: shared_reader(&mut reader)?,
                },
            ));
        }
        let container = SliceFolder::new(picked)?;
        Ok(Prepared(Pack::open(
            Box::new(container) as Box<dyn Container>
        )?))
    }

    /// How many files it holds (besides the stamp and the index).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Their size together.
    pub fn bytes(&self) -> f64 {
        self.0.bytes() as f64
    }

    /// The `n`th file's path in the cache.
    pub fn path(&self, n: usize) -> Option<String> {
        self.0.entry(n).map(|e| e.path.clone())
    }

    /// The `n`th file's bytes, checked against the index.
    pub fn read(&self, n: usize) -> Result<Uint8Array, JsError> {
        Ok(Uint8Array::from(&self.0.read(n)?[..]))
    }

    /// The stamp to write when the files are in: the cache's own.
    pub fn stamp(&self) -> String {
        self.0.stamp().to_owned()
    }

    /// The index file's text to write when the files are in.
    pub fn index(&self) -> String {
        self.0.index().to_owned()
    }
}
