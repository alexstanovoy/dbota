//! The importer: reads what the client's manifest lists out of a Dota install (a [`Source`]),
//! converts it and hands the files to a [`Sink`] under their cache paths. The same code runs in
//! a browser worker (`File.slice` reads, an OPFS sink) and natively.
//!
//! The install is hostile input: every read goes through a byte budget, every output path is
//! checked before the sink sees it, an asset that fails to convert is reported and skipped (the
//! client draws a stand-in for it), and the sizes of what is produced are capped.

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::error::{Error, Result, malformed};
use crate::export::{ExportedTexture, export_model, read_from};
use crate::material::Pack;
use crate::resource::Resource;
use crate::sound::Sound;
use crate::source::Source;
use crate::texture::Texture;
use crate::tooltips::{Wanted, tooltips_json};
use crate::vpk::Vpk;
use crate::world::{Band, EntityLump, buildings_txt, export_world, lighting_txt, trees_txt};

/// Bumped when the importer's output changes for the same install; part of a cache's stamp.
pub const IMPORT_REV: u32 = 4;

/// Names what an import of this manifest and these ids makes (FNV-1a 64 of both texts): part of
/// a cache's stamp, so a cache made from other texts is stale even from the same install.
pub fn plan_id(manifest: &str, ids: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in manifest.bytes().chain([0]).chain(ids.bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Most bytes one import reads from the install: the manifest's sources are ~62 MB plus the
/// package directories (23 MB); a hostile install must not make the page read the whole 71 GB.
const READ_LIMIT: u64 = 1 << 30;
/// Most bytes one import writes.
const WRITE_LIMIT: u64 = 1536 << 20;
/// Most bytes one read may ask for.
const READ_ONE_LIMIT: usize = 256 << 20;
/// Longest cache path.
const PATH_LIMIT: usize = 200;
/// The package directory every path of the manifest lives in.
const DIR_VPK: &str = "pak01_dir.vpk";
/// Where the tooltip texts go (the client reads them there).
const TOOLTIPS: &str = "text/tooltips.json";

/// One thing the manifest has the importer make.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A model: `meshes` keeps only those render meshes, `skeleton` writes its skin.
    Model {
        logical: String,
        src: String,
        out: String,
        meshes: Option<Vec<String>>,
        skeleton: bool,
        /// The animations to write (the manifest's `anims=`); a model with any has its skeleton.
        anims: Vec<String>,
    },
    /// The world's tree and building placements (`trees.txt`, `buildings.txt`), read from the
    /// map package's entities.
    Placements {
        logical: String,
        src: String,
        vpk: String,
        trees_out: String,
        buildings_out: String,
        /// The day and night lighting (`lighting.txt`), when the manifest asks for it.
        lighting_out: Option<String>,
    },
    /// The world cut to `band`: its glTF (`out`), geometry and images beside it, and its terrain
    /// layer textures in `layers`.
    World {
        logical: String,
        src: String,
        out: String,
        vpk: String,
        band: Band,
        layers: String,
    },
    /// The tooltip texts of the names `ids.txt` gives, from the install's localization and
    /// scripts.
    Tooltips {
        logical: String,
        wanted: Wanted,
        out: String,
    },
    /// The shop's categories and their items from the install's `shops.txt`, and the shop's
    /// words from the localization `strings`.
    Shop {
        logical: String,
        src: String,
        strings: String,
        out: String,
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
    /// Dota's portrait framings of the manifest's models, as `portraits.txt` (see
    /// [`framings`](crate::framings)): `models` pairs each model's logical name with its `.vmdl`.
    Portraits {
        logical: String,
        src: String,
        out: String,
        models: Vec<(String, String)>,
    },
}

impl Item {
    pub fn logical(&self) -> &str {
        match self {
            Self::Model { logical, .. }
            | Self::Placements { logical, .. }
            | Self::World { logical, .. }
            | Self::Tooltips { logical, .. }
            | Self::Shop { logical, .. }
            | Self::Texture { logical, .. }
            | Self::Sound { logical, .. }
            | Self::Portraits { logical, .. } => logical,
        }
    }
}

/// What the importer does for a manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub items: Vec<Item>,
}

impl Plan {
    /// The plan of a manifest (`assets-manifest/manifest.txt`) and of the ids whose tooltip
    /// texts the client shows (`assets-manifest/ids.txt`).
    pub fn parse(manifest: &str, ids: &str) -> Self {
        let mut plan = Self::default();
        let mut portraits_line = None;
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
                    let list = |key: &str| {
                        field(key).map(|v| v.split(',').map(str::to_owned).collect::<Vec<_>>())
                    };
                    let anims = list("anims=").unwrap_or_default();
                    plan.items.push(Item::Model {
                        logical: (*logical).to_owned(),
                        src: (*src).to_owned(),
                        out: (*out).to_owned(),
                        meshes: list("meshes="),
                        skeleton: field("attach=").is_some() || !anims.is_empty(),
                        anims,
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
                ["world", logical, src, out, rest @ ..] => {
                    let field = |key: &str| rest.iter().find_map(|w| w.strip_prefix(key));
                    let dir = out.rsplit_once('/').map_or("", |(dir, _)| dir);
                    let Some(vpk) = field("vpk=") else {
                        continue;
                    };
                    if let Some(trees) = field("trees=") {
                        plan.items.push(Item::Placements {
                            logical: format!("{logical}.placements"),
                            src: (*src).to_owned(),
                            vpk: vpk.to_owned(),
                            trees_out: trees.to_owned(),
                            buildings_out: format!("{dir}/buildings.txt"),
                            lighting_out: field("lighting=").map(str::to_owned),
                        });
                    }
                    let number =
                        |key: &str| field(key)?.parse::<f64>().ok().filter(|v| v.is_finite());
                    let band = (number("half_width="), number("v_min="), number("v_max="));
                    if let ((Some(half_width), Some(v_min), Some(v_max)), Some(layers)) =
                        (band, field("layers="))
                    {
                        plan.items.push(Item::World {
                            logical: (*logical).to_owned(),
                            src: (*src).to_owned(),
                            out: (*out).to_owned(),
                            vpk: vpk.to_owned(),
                            band: Band {
                                half_width,
                                v_min,
                                v_max,
                            },
                            layers: layers.to_owned(),
                        });
                    }
                }
                ["shop", logical, src, out, rest @ ..] => {
                    let Some(strings) = rest.iter().find_map(|w| w.strip_prefix("strings=")) else {
                        continue;
                    };
                    plan.items.push(Item::Shop {
                        logical: (*logical).to_owned(),
                        src: (*src).to_owned(),
                        strings: strings.to_owned(),
                        out: (*out).to_owned(),
                    });
                }
                ["portraits", logical, src, out, ..] => {
                    portraits_line = Some([logical, src, out].map(|w| (*w).to_owned()));
                }
                _ => {}
            }
        }
        let wanted = Wanted::parse(ids);
        if !wanted.names.is_empty() {
            plan.items.push(Item::Tooltips {
                logical: "text.tooltips".to_owned(),
                wanted,
                out: TOOLTIPS.to_owned(),
            });
        }
        // Last, for it frames the models the manifest lists.
        if let Some([logical, src, out]) = portraits_line {
            let models = plan
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Model { logical, src, .. } => Some((
                        logical.clone(),
                        src.strip_suffix("_c").unwrap_or(src).to_owned(),
                    )),
                    _ => None,
                })
                .collect();
            plan.items.push(Item::Portraits {
                logical,
                src,
                out,
                models,
            });
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
    pub stopped: bool,
    /// Bytes read from the install.
    pub read: u64,
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
            || path.starts_with("textures/ground/"))
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
struct Metered<S> {
    inner: S,
    read: Cell<u64>,
    limit: u64,
}

impl<S: Source> Source for Metered<S> {
    fn size(&self, path: &str) -> Result<u64> {
        self.inner.size(path)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.read.get().saturating_add(len as u64);
        if total > self.limit || len > READ_ONE_LIMIT {
            return Err(malformed(
                "import",
                format!("the install asks for more than {} bytes", self.limit),
            ));
        }
        self.read.set(total);
        self.inner.read_at(path, offset, len)
    }
}

/// An import under way, one manifest item (or one image of the world) per
/// [`step`](Self::step): a browser worker writes each step's files between steps (its file
/// system calls cannot be waited for from inside), and can stop between them. Opening the
/// package is the only thing that can fail outright; an item that fails is in the [`Report`].
pub struct Importer<S> {
    src: Metered<S>,
    pak: Vpk,
    /// Map packages opened so far, by path (opened when an item first needs one).
    maps: Vec<(String, Vpk)>,
    plan: Plan,
    next: usize,
    /// Files an item left to the steps after it (the world's images), and how many were made.
    pending: VecDeque<Pending>,
    pending_done: usize,
    written: u64,
    report: Report,
}

/// A file an item leaves to a step of its own.
struct Pending {
    /// What it is part of, for progress and failures.
    logical: String,
    path: String,
    what: Later,
}

enum Later {
    /// The image of a texture, packed or not.
    Image(ExportedTexture),
    /// A whole texture (a terrain layer).
    Layer(String),
    /// Bytes made already, written once the files before it are.
    Bytes(Vec<u8>),
}

impl<S: Source> Importer<S> {
    pub fn new(src: S, plan: Plan) -> Result<Self> {
        let src = Metered {
            inner: src,
            read: Cell::new(0),
            limit: READ_LIMIT,
        };
        let pak = Vpk::open(&src, DIR_VPK)?;
        Ok(Self {
            src,
            pak,
            maps: Vec::new(),
            plan,
            next: 0,
            pending: VecDeque::new(),
            pending_done: 0,
            written: 0,
            report: Report::default(),
        })
    }

    /// Steps in all, as far as known: an item may leave steps to come.
    pub fn total(&self) -> usize {
        self.plan.items.len() + self.pending_done + self.pending.len()
    }

    /// Steps made so far.
    pub fn done(&self) -> usize {
        self.next + self.pending_done
    }

    /// What the next step makes, by its logical name.
    pub fn upcoming(&self) -> Option<&str> {
        match self.pending.front() {
            Some(p) => Some(&p.logical),
            None => self.plan.items.get(self.next).map(Item::logical),
        }
    }

    pub fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Makes the next step's files, handing them to `sink`; false when there is none left.
    pub fn step(&mut self, sink: &mut dyn Sink) -> bool {
        let mut out = Out {
            sink,
            report: &mut self.report,
            written: &mut self.written,
            pending: &mut self.pending,
        };
        if let Some(job) = out.pending.pop_front() {
            self.pending_done += 1;
            let packages = packages(&self.pak, &self.maps);
            if let Err(err) = later(&self.src, &packages, &job, &mut out) {
                // The rest of what it was part of goes too: a world without one of its files
                // is not written whole.
                out.pending.retain(|p| p.logical != job.logical);
                self.report.failed.push((job.logical, err.to_string()));
            }
            return true;
        }
        let Some(item) = self.plan.items.get(self.next) else {
            return false;
        };
        self.next += 1;
        if let Err(err) = make(&self.src, &self.pak, &mut self.maps, item, &mut out) {
            out.pending.retain(|p| p.logical != item.logical());
            self.report
                .failed
                .push((item.logical().to_owned(), err.to_string()));
        }
        true
    }

    /// Ends the import early.
    pub fn stop(&mut self) {
        self.report.stopped = true;
        self.next = self.plan.items.len();
        self.pending.clear();
    }

    pub fn finish(mut self) -> Report {
        self.report.read = self.src.read.get();
        self.report
    }
}

/// The map packages opened so far, then the manifest's package (as the world reads them).
fn packages<'a>(pak: &'a Vpk, maps: &'a [(String, Vpk)]) -> Vec<&'a Vpk> {
    maps.iter()
        .map(|(_, v)| v)
        .chain(std::iter::once(pak))
        .collect()
}

/// Makes everything in `plan` out of `src` and hands it to `sink`; `progress` is told before
/// each item and may stop the import.
pub fn run<S: Source>(
    src: S,
    plan: &Plan,
    sink: &mut dyn Sink,
    progress: &mut dyn FnMut(&Progress<'_>) -> Flow,
) -> Result<Report> {
    let mut importer = Importer::new(src, plan.clone())?;
    while let Some(item) = importer.upcoming() {
        let report = Progress {
            done: importer.done(),
            total: importer.total(),
            item,
            bytes_written: importer.bytes_written(),
        };
        if progress(&report) == Flow::Stop {
            importer.stop();
            break;
        }
        importer.step(sink);
    }
    Ok(importer.finish())
}

struct Out<'a> {
    sink: &'a mut dyn Sink,
    report: &'a mut Report,
    written: &'a mut u64,
    pending: &'a mut VecDeque<Pending>,
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
        *self.written += data.len() as u64;
        self.report.files.insert(path.to_owned(), data.len() as u64);
        Ok(())
    }
}

fn make(
    src: &impl Source,
    pak: &Vpk,
    maps: &mut Vec<(String, Vpk)>,
    item: &Item,
    out: &mut Out<'_>,
) -> Result<()> {
    match item {
        Item::Model {
            src: path,
            out: dest,
            meshes,
            skeleton,
            anims,
            ..
        } => {
            let job = ModelJob {
                path,
                dest,
                meshes: meshes.as_deref(),
                skeleton: *skeleton,
                anims,
            };
            model(src, pak, &job, out)
        }
        Item::Placements {
            src: path,
            vpk,
            trees_out,
            buildings_out,
            lighting_out,
            ..
        } => {
            let at = open_map(src, maps, vpk)?;
            let outs = PlacementOuts {
                trees: trees_out,
                buildings: buildings_out,
                lighting: lighting_out.as_deref(),
            };
            placements(src, &maps[at].1, path, &outs, out)
        }
        Item::World {
            logical,
            src: path,
            out: dest,
            vpk,
            band,
            layers,
        } => {
            let at = open_map(src, maps, vpk)?;
            let job = WorldJob {
                logical,
                path,
                dest,
                band,
                layers,
            };
            world(src, &[&maps[at].1, pak], &job, out)
        }
        Item::Tooltips {
            wanted, out: dest, ..
        } => {
            let files = wanted
                .sources()
                .iter()
                .map(|path| read_from(src, &[pak], path))
                .collect::<Result<Vec<_>>>()?;
            let scripts: Vec<&[u8]> = files[1..].iter().map(Vec::as_slice).collect();
            out.put(dest, &tooltips_json(wanted, &files[0], &scripts)?)
        }
        Item::Shop {
            src: path,
            strings,
            out: dest,
            ..
        } => {
            let shops = read_from(src, &[pak], path)?;
            let localization = read_from(src, &[pak], strings)?;
            out.put(dest, &crate::shop_json(&shops, &localization)?)
        }
        Item::Texture {
            src: path,
            out: dest,
            ..
        } => {
            let texture = Texture::parse(&read_from(src, &[pak], path)?)?;
            out.put(dest, &texture.first_frame_png()?)
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
        Item::Portraits {
            src: path,
            out: dest,
            models,
            ..
        } => portraits(src, pak, path, dest, models, out),
    }
}

/// `portraits.txt` from Dota's `portraits.txt`, after the background images it names.
fn portraits(
    src: &impl Source,
    pak: &Vpk,
    path: &str,
    dest: &str,
    models: &[(String, String)],
    out: &mut Out<'_>,
) -> Result<()> {
    let wanted: Vec<(&str, &str)> = models
        .iter()
        .map(|(logical, vmdl)| (logical.as_str(), vmdl.as_str()))
        .collect();
    let framed = crate::framings(&read_from(src, &[pak], path)?, &wanted)?;
    let mut written = HashSet::new();
    for vmat in framed
        .iter()
        .filter_map(|f| Some(&f.background.as_ref()?.0))
    {
        if written.contains(vmat) {
            continue;
        }
        // A background that does not convert is reported, and leaves its portraits on a plain
        // backdrop.
        match background(src, pak, vmat, out) {
            Ok(()) => {
                written.insert(vmat.clone());
            }
            Err(err) => out.report.failed.push((vmat.clone(), err.to_string())),
        }
    }
    let text = crate::portraits_txt(&framed, |vmat| {
        written.contains(vmat).then(|| crate::background_png(vmat))
    });
    out.put(dest, text.as_bytes())
}

/// The image of portrait background material `vmat` (its `g_tColor`), as a PNG where
/// [`background_png`](crate::background_png) puts it.
fn background(src: &impl Source, pak: &Vpk, vmat: &str, out: &mut Out<'_>) -> Result<()> {
    let bytes = read_from(src, &[pak], &format!("{vmat}_c"))?;
    let data = Resource::parse(&bytes)?
        .data()
        .ok_or_else(|| malformed("portraits", "a background material has no DATA"))??;
    let material = crate::material::Material::from_vmat(vmat, &data);
    let vtex = material
        .textures
        .iter()
        .find_map(|(name, vtex)| (name == "g_tColor").then_some(vtex))
        .ok_or_else(|| malformed("portraits", "a background material has no image"))?;
    let texture = Texture::parse(&read_from(src, &[pak], &format!("{vtex}_c"))?)?;
    out.put(&crate::background_png(vmat), &texture.first_frame_png()?)
}

/// The index of map package `vpk` among `maps`, opened on first use.
fn open_map(src: &impl Source, maps: &mut Vec<(String, Vpk)>, vpk: &str) -> Result<usize> {
    if let Some(at) = maps.iter().position(|(name, _)| name == vpk) {
        return Ok(at);
    }
    maps.push((vpk.to_owned(), Vpk::open(src, vpk)?));
    Ok(maps.len() - 1)
}

/// What the world line asks for.
struct WorldJob<'a> {
    logical: &'a str,
    path: &'a str,
    dest: &'a str,
    band: &'a Band,
    layers: &'a str,
}

/// The world: its geometry now, its images and layers in the steps after, its glTF last (its
/// presence says the rest is there).
fn world(
    src: &impl Source,
    packages: &[&Vpk],
    job: &WorldJob<'_>,
    out: &mut Out<'_>,
) -> Result<()> {
    let (dir, file) = job.dest.rsplit_once('/').unwrap_or(("", job.dest));
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let bin = format!("{stem}.bin");
    let exported = export_world(src, packages, job.path, job.band, &bin, &png_name)?;
    out.put(&join(dir, &bin), &exported.bin)?;
    let mut queue = |path: String, what: Later| {
        out.pending.push_back(Pending {
            logical: job.logical.to_owned(),
            path,
            what,
        });
    };
    let mut seen = HashSet::new();
    for texture in exported.textures {
        if seen.insert(texture.uri.clone()) {
            queue(join(dir, &texture.uri), Later::Image(texture));
        }
    }
    for layer in exported.layers {
        queue(join(job.layers, &png_name(&layer)), Later::Layer(layer));
    }
    queue(job.dest.to_owned(), Later::Bytes(exported.gltf));
    Ok(())
}

/// Makes a file an item left for later.
fn later(src: &impl Source, packages: &[&Vpk], job: &Pending, out: &mut Out<'_>) -> Result<()> {
    let data = match &job.what {
        Later::Image(texture) => {
            let bytes = read_from(src, packages, &format!("{}_c", texture.vtex))?;
            let source = Texture::parse(&bytes)?;
            match texture.pack {
                None => source.to_png()?,
                Some(pack) => packed(&source, pack)?,
            }
        }
        Later::Layer(vtex) => {
            let bytes = read_from(src, packages, &format!("{vtex}_c"))?;
            Texture::parse(&bytes)?.first_frame_png()?
        }
        Later::Bytes(data) => return out.put(&job.path, data),
    };
    out.put(&job.path, &data)
}

/// The image file of texture `vtex` (`materials/x_color_psd_1.vtex` is `x_color_psd_1.png`).
fn png_name(vtex: &str) -> String {
    let name = vtex.rsplit('/').next().unwrap_or(vtex);
    format!("{}.png", name.trim_end_matches(".vtex"))
}

fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_owned()
    } else {
        format!("{dir}/{file}")
    }
}

/// Where the files made from the world's entities go.
struct PlacementOuts<'a> {
    trees: &'a str,
    buildings: &'a str,
    lighting: Option<&'a str>,
}

/// `trees.txt`, `buildings.txt` and `lighting.txt` from the entities of the world's default lump.
fn placements(
    src: &impl Source,
    map: &Vpk,
    world: &str,
    outs: &PlacementOuts<'_>,
    out: &mut Out<'_>,
) -> Result<()> {
    let bytes = read_from(src, &[map], world)?;
    let resource = Resource::parse(&bytes)?;
    let data = resource
        .data()
        .ok_or_else(|| malformed("world", "no DATA"))??;
    let lump = data
        .get("m_entityLumps")
        .and_then(|l| l.as_array())
        .and_then(|l| l.first())
        .and_then(|l| l.as_str())
        .ok_or_else(|| malformed("world", "no entity lump"))?;
    let bytes = read_from(src, &[map], &format!("{lump}_c"))?;
    let lump = EntityLump::read(&Resource::parse(&bytes)?)?;
    out.put(outs.trees, trees_txt(&lump.entities).as_bytes())?;
    out.put(outs.buildings, buildings_txt(&lump.entities).as_bytes())?;
    if let Some(path) = outs.lighting {
        out.put(path, lighting_txt(&lump.entities)?.as_bytes())?;
    }
    Ok(())
}

/// What one model line asks for.
struct ModelJob<'a> {
    path: &'a str,
    dest: &'a str,
    meshes: Option<&'a [String]>,
    skeleton: bool,
    anims: &'a [String],
}

fn model(src: &impl Source, pak: &Vpk, job: &ModelJob<'_>, out: &mut Out<'_>) -> Result<()> {
    let ModelJob {
        path,
        dest,
        meshes,
        skeleton,
        anims,
    } = *job;
    let keep: Option<Vec<&str>> = meshes.map(|m| m.iter().map(String::as_str).collect());
    let anims: Vec<&str> = anims.iter().map(String::as_str).collect();
    let exported = export_model(
        src,
        &[pak],
        path,
        keep.as_deref(),
        skeleton,
        &anims,
        &png_name,
    )?;
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
