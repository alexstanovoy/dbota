//! The importer: reads what the client's manifest lists out of a Dota install (a [`Source`]),
//! converts it and hands the files to a [`Sink`] under their cache paths. The same code runs in
//! a browser worker (`File.slice` reads, an OPFS sink) and natively.
//!
//! The install is hostile input: every read goes through a byte budget, every output path is
//! checked before the sink sees it, an asset that fails to convert is reported and skipped (the
//! client draws a stand-in for it), and the sizes of what is produced are capped.

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};

use crate::error::{Error, Result, malformed};
use crate::export::{ExportedTexture, export_model, read_from};
use crate::material::Pack;
use crate::sound::Sound;
use crate::source::Source;
use crate::texture::Texture;
use crate::vpk::Vpk;

/// Bumped when the importer's output changes for the same install; part of a cache's stamp.
pub const IMPORT_REV: u32 = 1;

/// Most bytes one import reads from the install: the manifest's sources are ~62 MB plus the
/// package directories (23 MB); a hostile install must not make the page read the whole 71 GB.
const READ_LIMIT: u64 = 1 << 30;
/// Most bytes one import writes.
const WRITE_LIMIT: u64 = 1536 << 20;
/// Longest cache path.
const PATH_LIMIT: usize = 200;
/// The package directory every path of the manifest lives in.
const DIR_VPK: &str = "pak01_dir.vpk";

/// One thing the manifest has the importer make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A model: `meshes` keeps only those render meshes, `skeleton` writes its skin.
    Model {
        logical: String,
        src: String,
        out: String,
        meshes: Option<Vec<String>>,
        skeleton: bool,
    },
    /// A texture written as a PNG (the first frame of a sprite sheet).
    Texture {
        logical: String,
        src: String,
        out: String,
    },
    /// A sound written as WAV or MP3.
    Sound {
        logical: String,
        src: String,
        out: String,
    },
}

impl Item {
    pub fn logical(&self) -> &str {
        match self {
            Self::Model { logical, .. }
            | Self::Texture { logical, .. }
            | Self::Sound { logical, .. } => logical,
        }
    }
}

/// What the importer does for a manifest, and what it leaves to later versions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<Item>,
    /// Models with animations and the world: not converted yet, the client draws stand-ins.
    pub deferred: Vec<String>,
}

impl Plan {
    /// The plan of a manifest (`assets-manifest/manifest.txt`).
    pub fn parse(manifest: &str) -> Self {
        let mut plan = Self::default();
        for line in manifest.lines() {
            let words: Vec<&str> = line
                .split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            match words.as_slice() {
                ["model", logical, src, out, rest @ ..] => {
                    let field = |key: &str| rest.iter().find_map(|w| w.strip_prefix(key));
                    if field("anims=").is_some() {
                        plan.deferred.push((*logical).to_owned());
                        continue;
                    }
                    plan.items.push(Item::Model {
                        logical: (*logical).to_owned(),
                        src: (*src).to_owned(),
                        out: (*out).to_owned(),
                        meshes: field("meshes=").map(|m| m.split(',').map(str::to_owned).collect()),
                        skeleton: field("attach=").is_some(),
                    });
                }
                ["texture", logical, src, out, ..] => plan.items.push(Item::Texture {
                    logical: (*logical).to_owned(),
                    src: (*src).to_owned(),
                    out: (*out).to_owned(),
                }),
                ["sound", logical, src, out, ..] => plan.items.push(Item::Sound {
                    logical: (*logical).to_owned(),
                    src: (*src).to_owned(),
                    out: (*out).to_owned(),
                }),
                ["world", logical, ..] => plan.deferred.push((*logical).to_owned()),
                _ => {}
            }
        }
        plan
    }
}

/// Where converted files go.
pub trait Sink {
    /// Stores `data` at the cache path `path` (relative, `/`-separated, already checked).
    fn write(&mut self, path: &str, data: &[u8]) -> Result<()>;
}

/// Whether to go on after a progress report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Where an import is, reported before each item.
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    pub done: usize,
    pub total: usize,
    pub item: &'a str,
    pub bytes_written: u64,
}

/// What an import made.
#[derive(Debug, Default)]
pub struct Report {
    /// Files written: cache path and size.
    pub files: BTreeMap<String, u64>,
    /// Items that did not convert, with why; their assets stay stand-ins.
    pub failed: Vec<(String, String)>,
    pub deferred: Vec<String>,
    pub stopped: bool,
}

impl Report {
    /// The cache's index: a header line, then `file <size> <path>` per file, marking the small
    /// ones a client reads before it starts (`small`). Written last: its presence says the
    /// import finished.
    pub fn index(&self) -> String {
        let mut text = format!("dbota-cache 1 rev {IMPORT_REV}\n");
        for (path, size) in &self.files {
            let small = if is_small(path, *size) { " small" } else { "" };
            text.push_str(&format!("file {size} {path}{small}\n"));
        }
        text
    }
}

/// Files the client reads synchronously at start: text tables and the map's ground layers.
pub fn is_small(path: &str, size: u64) -> bool {
    size <= 4 << 20
        && (path.ends_with(".txt")
            || path.ends_with(".json")
            || path.starts_with("textures/ground/")
            || path.starts_with("map/"))
}

/// A cache path the sink may be given: relative, `/`-separated, no empty, `.` or `..`
/// segments, only plain characters, bounded.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= PATH_LIMIT
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        })
}

/// A source that stops answering once it has read `limit` bytes.
struct Metered<'a, S: ?Sized> {
    inner: &'a S,
    read: Cell<u64>,
    limit: u64,
}

impl<S: Source + ?Sized> Source for Metered<'_, S> {
    fn size(&self, path: &str) -> Result<u64> {
        self.inner.size(path)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.read.get().saturating_add(len as u64);
        if total > self.limit {
            return Err(malformed(
                "import",
                format!("the install asks for more than {} bytes", self.limit),
            ));
        }
        self.read.set(total);
        self.inner.read_at(path, offset, len)
    }
}

/// Makes everything in `plan` out of `src` and hands it to `sink`; `progress` is told before
/// each item and may stop the import. Opening the package is the only error that ends it: an
/// item that fails is in the report.
pub fn run(
    src: &(impl Source + ?Sized),
    plan: &Plan,
    sink: &mut dyn Sink,
    progress: &mut dyn FnMut(&Progress<'_>) -> Flow,
) -> Result<Report> {
    let src = Metered {
        inner: src,
        read: Cell::new(0),
        limit: READ_LIMIT,
    };
    let pak = Vpk::open(&src, DIR_VPK)?;
    let mut out = Out {
        sink,
        report: Report {
            deferred: plan.deferred.clone(),
            ..Report::default()
        },
        written: 0,
    };
    for (done, item) in plan.items.iter().enumerate() {
        let report = Progress {
            done,
            total: plan.items.len(),
            item: item.logical(),
            bytes_written: out.written,
        };
        if progress(&report) == Flow::Stop {
            out.report.stopped = true;
            break;
        }
        if let Err(err) = make(&src, &pak, item, &mut out) {
            out.report
                .failed
                .push((item.logical().to_owned(), err.to_string()));
        }
    }
    Ok(out.report)
}

struct Out<'a> {
    sink: &'a mut dyn Sink,
    report: Report,
    written: u64,
}

impl Out<'_> {
    fn put(&mut self, path: &str, data: &[u8]) -> Result<()> {
        if !safe_path(path) {
            return Err(malformed("import", format!("unsafe output path {path:?}")));
        }
        if self.written.saturating_add(data.len() as u64) > WRITE_LIMIT {
            return Err(malformed("import", "the output would exceed its limit"));
        }
        self.sink.write(path, data)?;
        self.written += data.len() as u64;
        self.report.files.insert(path.to_owned(), data.len() as u64);
        Ok(())
    }
}

fn make(src: &impl Source, pak: &Vpk, item: &Item, out: &mut Out<'_>) -> Result<()> {
    match item {
        Item::Model {
            src: path,
            out: dest,
            meshes,
            skeleton,
            ..
        } => model(src, pak, path, dest, meshes.as_deref(), *skeleton, out),
        Item::Texture {
            src: path,
            out: dest,
            ..
        } => {
            let texture = Texture::parse(&read_from(src, &[pak], path)?)?.first_frame();
            out.put(dest, &texture.to_png()?)
        }
        Item::Sound {
            src: path,
            out: dest,
            ..
        } => {
            let bytes = read_from(src, &[pak], path)?;
            let sound = Sound::parse(&bytes)?;
            out.put(dest, &sound.file())
        }
    }
}

fn model(
    src: &impl Source,
    pak: &Vpk,
    path: &str,
    dest: &str,
    meshes: Option<&[String]>,
    skeleton: bool,
    out: &mut Out<'_>,
) -> Result<()> {
    let keep: Option<Vec<&str>> = meshes.map(|m| m.iter().map(String::as_str).collect());
    let uri = |vtex: &str| {
        let name = vtex.rsplit('/').next().unwrap_or(vtex);
        format!("{}.png", name.trim_end_matches(".vtex"))
    };
    let exported = export_model(src, &[pak], path, keep.as_deref(), skeleton, &uri)?;
    let dir = dest.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut seen = HashSet::new();
    let mut decoded: Option<(String, Texture)> = None;
    for texture in &exported.textures {
        if !seen.insert(texture.uri.clone()) {
            continue;
        }
        let image = image(src, pak, texture, &mut decoded)?;
        out.put(&format!("{dir}/{}", texture.uri), &image)?;
    }
    // The model last: its presence says its images are there.
    out.put(dest, &exported.glb)
}

/// The PNG of one image of a model: the texture itself, or its pixels packed.
fn image(
    src: &impl Source,
    pak: &Vpk,
    texture: &ExportedTexture,
    decoded: &mut Option<(String, Texture)>,
) -> Result<Vec<u8>> {
    if decoded
        .as_ref()
        .is_none_or(|(vtex, _)| *vtex != texture.vtex)
    {
        let bytes = read_from(src, &[pak], &format!("{}_c", texture.vtex))?;
        *decoded = Some((texture.vtex.clone(), Texture::parse(&bytes)?));
    }
    let Some((_, source)) = decoded.as_ref() else {
        return Err(Error::NotFound(texture.vtex.clone()));
    };
    match texture.pack {
        None => source.to_png(),
        Some(pack) => packed(source, pack),
    }
}

fn packed(source: &Texture, pack: Pack) -> Result<Vec<u8>> {
    let mut pixels = source.to_rgba8()?;
    pack.apply(&mut pixels);
    Texture::from_rgba8(source.width(), source.height(), pixels)?.to_png()
}
