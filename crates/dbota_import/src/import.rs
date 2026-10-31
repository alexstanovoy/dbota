//! The importer: reads what the client's manifest lists out of a Dota install (a [`Source`]),
//! converts it and hands the files to a [`Sink`] under their cache paths. The same code runs in
//! a browser worker (`File.slice` reads, an OPFS sink) and natively.
//!
//! The install is hostile input: every read goes through a byte budget, every output path is
//! checked before the sink sees it, an asset that fails to convert is reported and skipped (the
//! client draws a stand-in for it), and the sizes of what is produced are capped. An install
//! that asks for more than the budget ends the import: no cache is finished from it.

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet, VecDeque};

use heatel::export::{ExportedTexture, ModelOptions, export_model_in_group, read_from};
use heatel::face::{PhonemeTable, Scene};
use heatel::keyvalues;
use heatel::kv3::Value;
use heatel::material::Material;
use heatel::particle::{Blend, Renderer, System};
use heatel::portraits::framings;
use heatel::sound::Lipsync;
use heatel::texture::Role;
use heatel::tooltips::Wanted;
use heatel::world::{Band, EntityLump, ExportedWorld, WorldOptions, export_world, lighting};
use heatel::{Error, Model, Resource, Result, Sound, Source, Texture, Vpk};

use crate::catalog::{self, Catalog, FileKind, Id, Line, LineError};
use crate::text::{
    ParticleMaterial, ParticleModel, ParticleTexture, ambient_txt, background_png, buildings_txt,
    effect_stem, expression_txt, labels_json, lighting_txt, lipsync_txt, particle_json,
    phonemes_txt, portraits_txt, shop_json, soundscape_files, soundscapes_txt, styles_json,
    tooltips_json, tree_model_path, tree_models, trees_txt, wanted, wave_cache_path,
};

/// The file of a finished cache that says what made it.
pub const STAMP: &str = "stamp.txt";
/// The file of a finished cache that lists its files (written last but the stamp).
pub const INDEX: &str = "index.txt";
/// Longest stamp read; a stamp is a few dozen bytes.
pub const MAX_STAMP: u64 = 4096;

/// Bumped when the importer's output changes for the same install; part of a cache's stamp.
pub const IMPORT_REV: u32 = 28;

/// The manifest this build imports and draws, and its ids.
pub const MANIFEST: &str = include_str!("../../../assets-manifest/manifest.txt");
pub const IDS: &str = include_str!("../../../assets-manifest/ids.txt");

/// The plan of this build's manifest and ids, read once; the reason it cannot be read otherwise.
pub fn plan() -> Result<&'static Plan, String> {
    static PLAN: std::sync::OnceLock<Result<Plan, String>> = std::sync::OnceLock::new();
    PLAN.get_or_init(|| {
        Plan::parse(MANIFEST, IDS).map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// The asset fingerprint of this build: the importer's revision, the converters' revision
/// (`heatel::OUTPUT_REV`) and the hash of its plan ([`plan_id`]). A cache's stamp starts with the
/// fingerprint of the build that made it (then `/` and what it was made from); a cache of another
/// fingerprint may lack what this build draws, and is made again before the client reads it, on
/// the desktop and in a browser alike. An edit of a comment or of the layout of the manifest
/// changes nothing here: the plan is what the manifest asks for, read.
pub fn fingerprint() -> String {
    let id = plan().map_or_else(
        |why| format!("unreadable-{}", plan_id_of_text(&why)),
        plan_id,
    );
    fingerprint_of(&id)
}

/// The fingerprint of plan `plan_id`, made by this importer revision and these converters.
pub fn fingerprint_of(plan_id: &str) -> String {
    format!("rev{IMPORT_REV}.{}/{plan_id}", heatel::OUTPUT_REV)
}

/// The name of the folder a cache of this build lives in beside the others of its machine:
/// the fingerprint, as a name a file system takes.
pub fn folder_name() -> String {
    fingerprint().replace('/', "-")
}

/// Whether a cache stamped `stamp` was made by an importer of this build's [`fingerprint`].
pub fn stamped_here(stamp: &str) -> bool {
    stamp
        .trim()
        .strip_prefix(&fingerprint())
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Names what an import of this plan makes (FNV-1a 64 of the plan's typed items): part of a
/// cache's stamp, so a cache made from another plan is stale even from the same install.
pub fn plan_id(plan: &Plan) -> String {
    plan_id_of_text(&format!("{plan:?}"))
}

fn plan_id_of_text(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Most bytes one import reads from the install: the manifest's sources, the icon of every item
/// and ability the scripts define and the package directories come to some 995 MB (measured on
/// Dota 7.41f with the Lotus Pools, 2026-10-31); a hostile install must not make the page read
/// the whole 71 GB.
const READ_LIMIT: u64 = 2 << 30;
/// Most bytes one import writes.
pub(crate) const WRITE_LIMIT: u64 = 1536 << 20;
/// Most bytes one read may ask for.
pub(crate) const READ_ONE_LIMIT: usize = 256 << 20;
/// Longest cache path.
pub(crate) const PATH_LIMIT: usize = 200;
/// The package directory every path of the manifest lives in.
const DIR_VPK: &str = "pak01_dir.vpk";
/// Where the tooltip texts go (the client reads them there).
const TOOLTIPS: &str = "text/tooltips.json";
/// Where the interface's labels go (the client reads them there).
const LABELS: &str = "text/labels.json";
const STYLES: &str = "text/styles.json";
/// Most systems one particle line makes, its root and every child under it; Dota's missiles
/// have up to ~30.
const MAX_SYSTEMS: usize = 256;
/// What the cache's glTF files say wrote them.
pub const GENERATOR: &str = "dbota_assets";
/// Layer textures the client has no use for.
const NOT_LAYERS: [&str; 3] = ["refl", "specular", "height"];
/// The world's nodes the client leaves out of `mid.gltf`: the trees, buildings and map objectives
/// (Watchers, Outposts, Shrines of Wisdom) it spawns itself as units, and the end-game cameras.
const NOT_IN_WORLD: [&str; 19] = [
    "tree_",
    "dire_tree",
    "radiant_tower",
    "dire_tower",
    "radiant_ancient001",
    "dire_ancient_base001",
    "radiant_melee_barracks001",
    "radiant_ranged_barracks001",
    "dire_barracks_melee001",
    "dire_barracks_ranged001",
    "radiant_fountain002",
    "dire_fountain002",
    "radiant_statue001",
    "dire_column001",
    "lantern_of_sight",
    "outpost",
    "xp_shrine",
    "bad_ancient_destruction_camera",
    "radiant_endcam",
];

/// The world at `path` as the cache holds it: cut to `band`, without what the client spawns
/// itself, its geometry named `bin_uri`, its images named by `texture_uri`, and only the layer
/// textures the client draws.
pub fn export_cache_world(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    band: &Band,
    bin_uri: &str,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<ExportedWorld> {
    let options = WorldOptions {
        band: Some(*band),
        exclude: &NOT_IN_WORLD,
        scene: "mid",
        bin_uri,
        texture_uri,
        generator: GENERATOR,
    };
    let mut world = export_world(src, packages, path, &options)?;
    world
        .layers
        .retain(|t| !NOT_LAYERS.iter().any(|skip| t.contains(skip)));
    Ok(world)
}

/// The importer's error for a file that is not what it should be.
fn malformed(what: &'static str, detail: impl Into<String>) -> Error {
    Error::Malformed {
        what,
        detail: detail.into(),
    }
}

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
        /// The skin (material group) to draw it with (the manifest's `skin=`); the default one without.
        skin: Option<String>,
    },
    /// The world's tree and building placements (`trees.txt`, `buildings.txt`), read from the
    /// map package's entities, and what else the manifest asks of them: lighting, ambient
    /// particle systems and soundscapes.
    Placements {
        logical: String,
        src: String,
        vpk: String,
        trees_out: String,
        buildings_out: String,
        /// The day and night lighting (`lighting.txt`), when the manifest asks for it.
        lighting_out: Option<String>,
        /// The ambient particle systems (`ambient.txt`) and the particles they play, when the
        /// manifest asks for them.
        ambient_out: Option<String>,
        /// The soundscapes (`soundscapes.txt`) and the sounds they play, when the manifest asks
        /// for them.
        soundscapes_out: Option<String>,
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
    /// The tooltip texts of the names `ids.txt` gives and of every item and ability the
    /// install's scripts define, from the install's localization and scripts.
    Tooltips {
        logical: String,
        wanted: Wanted,
        out: String,
    },
    /// The icon of every item (or of every ability) the install's scripts define, each under
    /// the name its script gives it: `icons/items/<name>.png`, `icons/abilities/<name>.png`.
    /// What the install has no icon for is left out.
    Icons { logical: String, abilities: bool },
    /// The shop's categories and their items from the install's `shops.txt`, and the shop's
    /// words from the localization `strings`.
    Shop {
        logical: String,
        src: String,
        strings: String,
        out: String,
    },
    /// The interface's labels of the manifest's `label` lines: each key's text out of its
    /// localization file.
    Labels {
        logical: String,
        /// The localization file and the key of each label, in manifest order.
        keys: Vec<(String, String)>,
        out: String,
    },
    /// The colours of the manifest's `style` lines: each `@define` named out of its compiled
    /// stylesheet.
    Styles {
        logical: String,
        /// The stylesheet and the define of each colour, in manifest order.
        keys: Vec<(String, String)>,
        out: String,
    },
    /// A texture written as a PNG (the first frame of a sprite sheet, or a PNG copied whole).
    Texture {
        logical: String,
        src: String,
        out: String,
    },
    /// A particle system and every child under it, as one JSON file (see [`particle_json`]),
    /// their textures as KTX2 under `textures/` beside it.
    Particle {
        logical: String,
        src: String,
        out: String,
    },
    /// A sound written as WAV or MP3, and the lip sync of a voice line beside it
    /// ([`lipsync_path`]).
    Sound {
        logical: String,
        src: String,
        out: String,
    },
    /// A facial scene (a hero's idle expression), as [`expression_txt`] writes it.
    Expression {
        logical: String,
        src: String,
        out: String,
    },
    /// Valve's phoneme table, as [`phonemes_txt`] writes it.
    Phonemes {
        logical: String,
        src: String,
        out: String,
    },
    /// Dota's portrait framings of the manifest's models, as `portraits.txt` (see
    /// [`portraits_txt`]): `models` pairs each model's logical name with its `.vmdl`.
    Portraits {
        logical: String,
        src: String,
        out: String,
        models: Vec<(String, String)>,
    },
}

impl Item {
    /// The cache paths the item writes by name (a world's layers are a folder; a model's textures
    /// and the world's geometry sit beside what is named).
    pub fn outputs(&self) -> Vec<&str> {
        match self {
            Self::Model { out, .. }
            | Self::Texture { out, .. }
            | Self::Sound { out, .. }
            | Self::Expression { out, .. }
            | Self::Phonemes { out, .. }
            | Self::Tooltips { out, .. }
            | Self::Shop { out, .. }
            | Self::Labels { out, .. }
            | Self::Styles { out, .. }
            | Self::Particle { out, .. }
            | Self::Portraits { out, .. } => vec![out],
            Self::Icons { abilities, .. } => {
                vec![if *abilities {
                    "icons/abilities"
                } else {
                    "icons/items"
                }]
            }
            Self::World { out, layers, .. } => vec![out, layers],
            Self::Placements {
                trees_out,
                buildings_out,
                lighting_out,
                ..
            } => [
                Some(trees_out.as_str()),
                Some(buildings_out.as_str()),
                lighting_out.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }

    pub fn logical(&self) -> &str {
        match self {
            Self::Model { logical, .. }
            | Self::Placements { logical, .. }
            | Self::World { logical, .. }
            | Self::Tooltips { logical, .. }
            | Self::Icons { logical, .. }
            | Self::Shop { logical, .. }
            | Self::Labels { logical, .. }
            | Self::Styles { logical, .. }
            | Self::Texture { logical, .. }
            | Self::Particle { logical, .. }
            | Self::Sound { logical, .. }
            | Self::Expression { logical, .. }
            | Self::Phonemes { logical, .. }
            | Self::Portraits { logical, .. } => logical,
        }
    }
}

/// The localization of the modifiers `ids.txt` names (`modifier <effect id> <dota modifier>`): their
/// tooltip names and descriptions (`DOTA_Tooltip_<modifier>`, `..._Description`), as labels.
fn modifier_labels(ids: &[Id]) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    for id in ids {
        if let Id::Modifier { name, .. } = id {
            for suffix in ["", "_Description"] {
                keys.push((
                    MODIFIER_TEXTS.to_owned(),
                    format!("DOTA_Tooltip_{name}{suffix}"),
                ));
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

/// Where Dota's modifier tooltips are.
const MODIFIER_TEXTS: &str = "resource/localization/abilities_english.txt";

/// What the importer does for a manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub items: Vec<Item>,
}

impl Plan {
    /// The plan of a manifest (`assets-manifest/manifest.txt`) and of the ids whose tooltip
    /// texts the client shows (`assets-manifest/ids.txt`); every line that cannot be read is told.
    pub fn parse(manifest: &str, ids: &str) -> Result<Self, Vec<LineError>> {
        Ok(Self::of(&catalog::parse(manifest, ids)?))
    }

    /// The plan of a catalog.
    pub fn of(catalog: &Catalog) -> Self {
        let mut plan = Self::default();
        let mut portraits = None;
        let mut labels = Vec::new();
        let mut styles = Vec::new();
        for line in &catalog.lines {
            match line {
                Line::Model(m) => plan.items.push(Item::Model {
                    logical: m.logical.clone(),
                    src: m.src.clone(),
                    out: m.out.clone(),
                    meshes: m.meshes.clone(),
                    skeleton: m.attach.is_some() || !m.anims.is_empty(),
                    anims: m.anims.clone(),
                    skin: m.skin.clone(),
                }),
                Line::File(f) => plan.items.push(match f.kind {
                    FileKind::Texture => Item::Texture {
                        logical: f.logical.clone(),
                        src: f.src.clone(),
                        out: f.out.clone(),
                    },
                    FileKind::Particle => Item::Particle {
                        logical: f.logical.clone(),
                        src: f.src.clone(),
                        out: f.out.clone(),
                    },
                    FileKind::Expression => Item::Expression {
                        logical: f.logical.clone(),
                        src: f.src.clone(),
                        out: f.out.clone(),
                    },
                    FileKind::Phonemes => Item::Phonemes {
                        logical: f.logical.clone(),
                        src: f.src.clone(),
                        out: f.out.clone(),
                    },
                }),
                Line::Sound(s) => plan.items.push(Item::Sound {
                    logical: s.logical.clone(),
                    src: s.src.clone(),
                    out: s.out.clone(),
                }),
                Line::World(w) => {
                    let dir = w.out.rsplit_once('/').map_or("", |(dir, _)| dir);
                    if let Some(trees) = &w.trees {
                        plan.items.push(Item::Placements {
                            logical: format!("{}.placements", w.logical),
                            src: w.src.clone(),
                            vpk: w.vpk.clone(),
                            trees_out: trees.clone(),
                            buildings_out: format!("{dir}/buildings.txt"),
                            lighting_out: w.lighting.clone(),
                            ambient_out: w.ambient.clone(),
                            soundscapes_out: w.soundscapes.clone(),
                        });
                    }
                    plan.items.push(Item::World {
                        logical: w.logical.clone(),
                        src: w.src.clone(),
                        out: w.out.clone(),
                        vpk: w.vpk.clone(),
                        band: Band {
                            half_width: w.half_width,
                            v_min: w.v_min,
                            v_max: w.v_max,
                        },
                        layers: w.layers.clone(),
                    });
                }
                Line::Shop(s) => plan.items.push(Item::Shop {
                    logical: s.logical.clone(),
                    src: s.src.clone(),
                    strings: s.strings.clone(),
                    out: s.out.clone(),
                }),
                Line::Label { src, key } => labels.push((src.clone(), key.clone())),
                Line::Style { src, name } => styles.push((src.clone(), name.clone())),
                Line::Portraits(p) => portraits = Some(p),
                Line::Clip(_) | Line::Unit(_) | Line::Fx(_) | Line::State(_) => {}
            }
        }
        labels.extend(modifier_labels(&catalog.ids));
        plan.items.push(Item::Tooltips {
            logical: "text.tooltips".to_owned(),
            wanted: wanted(&catalog.ids),
            out: TOOLTIPS.to_owned(),
        });
        if !labels.is_empty() {
            plan.items.push(Item::Labels {
                logical: "text.labels".to_owned(),
                keys: labels,
                out: LABELS.to_owned(),
            });
        }
        if !styles.is_empty() {
            plan.items.push(Item::Styles {
                logical: "text.styles".to_owned(),
                keys: styles,
                out: STYLES.to_owned(),
            });
        }
        for abilities in [false, true] {
            plan.items.push(Item::Icons {
                logical: format!("icons.{}", if abilities { "abilities" } else { "items" }),
                abilities,
            });
        }
        // Last, for it frames the models the manifest lists.
        if let Some(p) = portraits {
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
                logical: p.logical.clone(),
                src: p.src.clone(),
                out: p.out.clone(),
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
    /// Why the import was cut short by the install, when it was: it asked for more than the
    /// importer reads. Nothing of such an import is a cache.
    pub cut: Option<String>,
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

/// A source that stops answering once it has read `limit` bytes, and remembers that it did.
struct Metered<S> {
    inner: S,
    read: Cell<u64>,
    limit: u64,
    over: Cell<bool>,
}

impl<S: Source> Source for Metered<S> {
    fn size(&self, path: &str) -> Result<u64> {
        self.inner.size(path)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let total = self.read.get().saturating_add(len as u64);
        if total > self.limit || len > READ_ONE_LIMIT {
            self.over.set(true);
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
    /// The model of a tree the map places: its standing and stump meshes, as [`tree_model`] keeps them.
    Tree(String),
    /// A particle system the map places (`x.vpcf_c`), converted as the manifest's particles are.
    Particle(String),
    /// A sound a soundscape plays (`x.vsnd_c`), written under the path [`wave_cache_path`] gave.
    Sound(String),
}

impl<S: Source> Importer<S> {
    pub fn new(src: S, plan: Plan) -> Result<Self> {
        Self::with_limit(src, plan, READ_LIMIT)
    }

    /// An import that reads at most `limit` bytes of the install.
    pub(crate) fn with_limit(src: S, plan: Plan, limit: u64) -> Result<Self> {
        let src = Metered {
            inner: src,
            read: Cell::new(0),
            limit,
            over: Cell::new(false),
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
            self.cut_if_over();
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
        self.cut_if_over();
        true
    }

    /// Ends the import once the install has asked for more than the budget: what it made would
    /// be a cache with holes no one sees.
    fn cut_if_over(&mut self) {
        if self.src.over.get() && self.report.cut.is_none() {
            self.report.cut = Some(format!(
                "the install asks for more than the {} bytes an import reads",
                self.src.limit
            ));
            self.next = self.plan.items.len();
            self.pending.clear();
        }
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
/// A file of the install: an addon's loose file (under [`crate::ADDONS`]),
/// else one of the main package.
/// One script of the install that defines items or abilities: what it lists, and the hero it
/// belongs to when it is a hero's.
struct InstallScript {
    entries: Vec<heatel::scripts::Entry>,
    /// `pudge` for `scripts/npc/heroes/npc_dota_hero_pudge.txt`.
    hero: Option<String>,
    item: bool,
}

/// Most hero scripts read.
const MAX_HERO_SCRIPTS: usize = 512;

/// The scripts of the install that define items and abilities: `items.txt`, `npc_abilities.txt`
/// and each hero's own.
fn install_scripts(src: &impl Source, pak: &Vpk) -> Result<Vec<InstallScript>> {
    let mut out = Vec::new();
    for (path, item) in [
        ("scripts/npc/items.txt", true),
        ("scripts/npc/npc_abilities.txt", false),
    ] {
        out.push(InstallScript {
            entries: heatel::scripts::entries(&read_from(src, &[pak], path)?)?,
            hero: None,
            item,
        });
    }
    let heroes: Vec<(String, String)> = pak
        .entries()
        .filter(|e| e.dir() == "scripts/npc/heroes" && e.ext() == "txt")
        .filter_map(|e| {
            let hero = e.name().strip_prefix("npc_dota_hero_")?.to_owned();
            Some((e.path(), hero))
        })
        .take(MAX_HERO_SCRIPTS)
        .collect();
    for (path, hero) in heroes {
        out.push(InstallScript {
            entries: heatel::scripts::entries(&read_from(src, &[pak], &path)?)?,
            hero: Some(hero),
            item: false,
        });
    }
    Ok(out)
}

/// Writes the icon of every item, or of every ability, of the install's scripts. A script's
/// entry with no icon in the install is left out; an icon that does not decode is a failure of
/// its own and the rest go on.
fn icons(src: &impl Source, pak: &Vpk, abilities: bool, out: &mut Out<'_>) -> Result<()> {
    let (from, to) = if abilities {
        ("panorama/images/spellicons", "icons/abilities")
    } else {
        ("panorama/images/items", "icons/items")
    };
    // Every recipe wears one icon, `recipe`, which no script names.
    let mut names: Vec<String> = if abilities {
        Vec::new()
    } else {
        vec!["recipe".to_owned()]
    };
    for script in install_scripts(src, pak)? {
        if script.item != abilities {
            names.extend(script.entries.iter().map(|e| e.icon_name().to_owned()));
        }
    }
    let mut done = HashSet::new();
    for name in names {
        if !safe_name(&name) || !done.insert(name.clone()) {
            continue;
        }
        let path = format!("{from}/{name}_png.vtex_c");
        let bytes = match read_from(src, &[pak], &path) {
            Ok(bytes) => bytes,
            Err(Error::NotFound(_)) => continue,
            Err(err) => return Err(err),
        };
        let png = Texture::parse(&bytes).and_then(|t| t.first_frame_png());
        match png {
            Ok(png) => out.put(&format!("{to}/{name}.png"), &png)?,
            Err(err) => out.report.failed.push((path, err.to_string())),
        }
    }
    Ok(())
}

/// Whether a script's icon name is one a file may be named after: folders (`consumables/high_five`)
/// of letters, digits and underscores.
fn safe_name(name: &str) -> bool {
    name.len() <= 96
        && name.split('/').count() <= 3
        && name.split('/').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

fn read_any(src: &impl Source, pak: &Vpk, path: &str) -> Result<Vec<u8>> {
    if path.starts_with(crate::ADDONS) {
        let size = usize::try_from(src.size(path)?).unwrap_or(usize::MAX);
        if size > ADDON_FILE_LIMIT {
            return Err(heatel::Error::NotFound(path.to_owned()));
        }
        return src.read_at(path, 0, size);
    }
    read_from(src, &[pak], path)
}

/// The largest loose addon file read: a panel's text or picture.
const ADDON_FILE_LIMIT: usize = 16 << 20;

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
    run_capped(src, plan, sink, progress, None)
}

/// [`run`], reading at most `limit` bytes of the install when given (else [`READ_LIMIT`]). An
/// install that asks for more ends the import with an error.
pub(crate) fn run_capped<S: Source>(
    src: S,
    plan: &Plan,
    sink: &mut dyn Sink,
    progress: &mut dyn FnMut(&Progress<'_>) -> Flow,
    limit: Option<u64>,
) -> Result<Report> {
    let mut importer = Importer::with_limit(src, plan.clone(), limit.unwrap_or(READ_LIMIT))?;
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
    let report = importer.finish();
    match &report.cut {
        Some(why) => Err(malformed("import", why.clone())),
        None => Ok(report),
    }
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
            skin,
            ..
        } => {
            let job = ModelJob {
                path,
                dest,
                meshes: meshes.as_deref(),
                skeleton: *skeleton,
                anims,
                skin: skin.as_deref(),
            };
            model(src, &[pak], &job, out)
        }
        Item::Placements {
            src: path,
            vpk,
            trees_out,
            buildings_out,
            lighting_out,
            ambient_out,
            soundscapes_out,
            ..
        } => {
            let at = open_map(src, maps, vpk)?;
            let outs = PlacementOuts {
                trees: trees_out,
                buildings: buildings_out,
                lighting: lighting_out.as_deref(),
                ambient: ambient_out.as_deref(),
                soundscapes: soundscapes_out.as_deref(),
            };
            placements(src, [&maps[at].1, pak], path, &outs, out)
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
            let mut wanted = wanted.clone();
            for script in install_scripts(src, pak)? {
                wanted
                    .names
                    .extend(script.entries.into_iter().map(|e| e.name));
                wanted.heroes.extend(script.hero);
            }
            wanted.names.sort();
            wanted.names.dedup();
            wanted.heroes.sort();
            wanted.heroes.dedup();
            let wanted = &wanted;
            let files = wanted
                .sources()
                .iter()
                .map(|path| read_from(src, &[pak], path))
                .collect::<Result<Vec<_>>>()?;
            let scripts: Vec<&[u8]> = files[1..].iter().map(Vec::as_slice).collect();
            out.put(dest, &tooltips_json(wanted, &files[0], &scripts)?)
        }
        Item::Icons { abilities, .. } => icons(src, pak, *abilities, out),
        Item::Shop {
            src: path,
            strings,
            out: dest,
            ..
        } => {
            let shops = read_from(src, &[pak], path)?;
            let localization = read_from(src, &[pak], strings)?;
            out.put(dest, &shop_json(&shops, &localization)?)
        }
        Item::Labels {
            keys, out: dest, ..
        } => {
            // An addon's file may be missing (an install without it, a browser's): its
            // labels are left out, the rest kept.
            let mut files: Vec<(&str, Vec<u8>)> = Vec::new();
            for (path, _) in keys {
                if files.iter().any(|(read, _)| read == path) {
                    continue;
                }
                match read_any(src, pak, path) {
                    Ok(bytes) => files.push((path, bytes)),
                    Err(_) if path.starts_with(crate::ADDONS) => {}
                    Err(err) => return Err(err),
                }
            }
            let keys: Vec<(String, String)> = keys
                .iter()
                .filter(|(path, _)| files.iter().any(|(read, _)| read == path))
                .cloned()
                .collect();
            let files: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (*p, b.as_slice())).collect();
            out.put(dest, &labels_json(&keys, &files)?)
        }
        Item::Styles {
            keys, out: dest, ..
        } => {
            let mut sheets: Vec<(&str, Vec<u8>)> = Vec::new();
            for (path, _) in keys {
                if !sheets.iter().any(|(read, _)| read == path) {
                    sheets.push((path, read_from(src, &[pak], path)?));
                }
            }
            let sheets: Vec<(&str, &[u8])> =
                sheets.iter().map(|(p, b)| (*p, b.as_slice())).collect();
            out.put(dest, &styles_json(keys, &sheets)?)
        }
        Item::Texture {
            src: path,
            out: dest,
            ..
        } => {
            // A PNG the install ships as it is (the cursors) is copied whole.
            if path.ends_with(".png") {
                let bytes = read_from(src, &[pak], path)?;
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err(Error::Malformed {
                        what: "png",
                        detail: format!("{path} is not a PNG"),
                    });
                }
                return out.put(dest, &bytes);
            }
            let texture = Texture::parse(&read_any(src, pak, path)?)?;
            out.put(dest, &texture.first_frame_png()?)
        }
        Item::Particle {
            src: path,
            out: dest,
            ..
        } => particle(src, pak, path, dest, out),
        Item::Sound {
            src: path,
            out: dest,
            ..
        } => sound(&read_from(src, &[pak], path)?, dest, out),
        Item::Expression {
            src: path,
            out: dest,
            ..
        }
        | Item::Phonemes {
            src: path,
            out: dest,
            ..
        } => out.put(
            dest,
            face_text(item, &read_from(src, &[pak], path)?)?.as_bytes(),
        ),
        Item::Portraits {
            src: path,
            out: dest,
            models,
            ..
        } => portraits(src, pak, path, dest, models, out),
    }
}

/// The particle system `path` (`x.vpcf_c`) with its children, after the textures they draw
/// with. A child that does not read is reported and left out: its parent plays without it.
fn particle(src: &impl Source, pak: &Vpk, path: &str, dest: &str, out: &mut Out<'_>) -> Result<()> {
    let root = path.strip_suffix("_c").unwrap_or(path);
    let mut systems: Vec<(String, System)> = Vec::new();
    let mut queue = VecDeque::from([root.to_owned()]);
    while let Some(next) = queue.pop_front() {
        if systems.iter().any(|(seen, _)| *seen == next) {
            continue;
        }
        if systems.len() == MAX_SYSTEMS {
            return Err(malformed(
                "particle",
                format!("{root}: more than {MAX_SYSTEMS} systems"),
            ));
        }
        let read = read_from(src, &[pak], &format!("{next}_c"))
            .and_then(|bytes| System::read(&Resource::parse(&bytes)?));
        let system = match read {
            Ok(system) => system,
            Err(err) if next != root => {
                out.report.failed.push((next, err.to_string()));
                continue;
            }
            Err(err) => return Err(err),
        };
        queue.extend(system.children.iter().map(|c| c.path.clone()));
        systems.push((next, system));
    }
    let dir = dest.rsplit_once('/').map_or("", |(dir, _)| dir);
    let materials = particle_materials(src, pak, &systems, out);
    let mut textures: Vec<ParticleTexture> = Vec::new();
    let vtexes = systems.iter().flat_map(|(_, s)| s.textures());
    let vtexes: Vec<&str> = vtexes
        .chain(materials.iter().map(|m| m.vtex.as_str()))
        .collect();
    for vtex in vtexes {
        if textures.iter().any(|t| t.vtex == vtex) {
            continue;
        }
        match particle_texture(src, pak, vtex, dir, out) {
            Ok(texture) => textures.push(texture),
            // A renderer without its texture draws nothing; the rest of the system plays. A
            // texture the install does not ship at all (Dota's own Blood Grenade names one)
            // is no failure of the import: there is nothing of it to convert.
            Err(heatel::Error::NotFound(_)) => {}
            Err(err) => out.report.failed.push((vtex.to_owned(), err.to_string())),
        }
    }
    let models = particle_models(src, pak, &systems, dir, out);
    out.put(
        dest,
        &particle_json(root, &systems, &textures, &materials, &models)?,
    )
}

/// The models the systems' model renderers draw from their lists, each exported once as a
/// glTF file under `dir/models/` (with its images beside it); one that does not export is
/// reported, and its particles draw nothing. A renderer whose model a control point carries
/// draws that model instead, which the client does not bind: its list is left out.
fn particle_models(
    src: &impl Source,
    pak: &Vpk,
    systems: &[(String, System)],
    dir: &str,
    out: &mut Out<'_>,
) -> Vec<ParticleModel> {
    let listed = systems
        .iter()
        .flat_map(|(_, s)| &s.renderers)
        .filter_map(|r| match &r.class {
            Renderer::Models(m) if m.model_input.is_none() => Some(m.paths()),
            _ => None,
        })
        .flatten();
    let mut models: Vec<ParticleModel> = Vec::new();
    for vmdl in listed {
        if models.iter().any(|m| m.vmdl == vmdl) {
            continue;
        }
        let name = vmdl.rsplit('/').next().unwrap_or(vmdl);
        let file = join(
            dir,
            &format!("models/{}.glb", name.trim_end_matches(".vmdl")),
        );
        if !out.report.files.contains_key(&file) {
            let job = ModelJob {
                path: &format!("{vmdl}_c"),
                dest: &file,
                meshes: None,
                skeleton: false,
                anims: &[],
                skin: None,
            };
            if let Err(err) = model(src, &[pak], &job, out) {
                out.report.failed.push((vmdl.to_owned(), err.to_string()));
                continue;
            }
        }
        models.push(ParticleModel {
            vmdl: vmdl.to_owned(),
            file,
        });
    }
    models
}

/// The materials the systems' projected renderers draw with, each with its colour texture and
/// blend; one that does not read is reported, and its decals are not drawn.
fn particle_materials(
    src: &impl Source,
    pak: &Vpk,
    systems: &[(String, System)],
    out: &mut Out<'_>,
) -> Vec<ParticleMaterial> {
    let mut materials: Vec<ParticleMaterial> = Vec::new();
    for vmat in systems.iter().flat_map(|(_, s)| s.materials()) {
        if materials.iter().any(|m| m.vmat == vmat) {
            continue;
        }
        match particle_material(src, pak, vmat) {
            Ok(material) => materials.push(material),
            Err(err) => out.report.failed.push((vmat.to_owned(), err.to_string())),
        }
    }
    materials
}

/// Projected material `vmat`: its `g_tColor`, and its blend as Dota's projected shader's
/// combos give it.
fn particle_material(src: &impl Source, pak: &Vpk, vmat: &str) -> Result<ParticleMaterial> {
    let bytes = read_from(src, &[pak], &format!("{vmat}_c"))?;
    let data = Resource::parse(&bytes)?
        .data()
        .ok_or_else(|| malformed("particle", format!("{vmat} has no DATA")))??;
    let material = Material::from_vmat(vmat, &data)?;
    let vtex = material
        .textures
        .iter()
        .find_map(|(name, vtex)| (name == "g_tColor").then_some(vtex))
        .ok_or_else(|| malformed("particle", format!("{vmat} has no colour texture")))?;
    let blend = if material.int("F_MOD_2X") != 0 {
        Blend::Mod2x
    } else if material.int("F_ADDITIVE_BLENDING") != 0 {
        Blend::Add
    } else {
        Blend::Alpha
    };
    Ok(ParticleMaterial {
        vmat: vmat.to_owned(),
        vtex: vtex.clone(),
        blend,
        sheets: material.int("F_USE_SHEETS") != 0,
    })
}

/// The KTX2 file of particle texture `vtex` under `dir/textures/` (written once however many
/// systems draw with it), and its sprite sheet.
fn particle_texture(
    src: &impl Source,
    pak: &Vpk,
    vtex: &str,
    dir: &str,
    out: &mut Out<'_>,
) -> Result<ParticleTexture> {
    let texture = Texture::parse(&read_from(src, &[pak], &format!("{vtex}_c"))?)?;
    let file = join(dir, &format!("textures/{}", ktx2_name(vtex)));
    if !out.report.files.contains_key(&file) {
        let exported = ExportedTexture {
            vtex: vtex.to_owned(),
            uri: file.clone(),
            pack: None,
            role: Role::Colour,
        };
        out.put(&file, &exported.to_ktx2(&texture)?)?;
    }
    Ok(ParticleTexture {
        vtex: vtex.to_owned(),
        file,
        sheet: texture.sheet().cloned(),
    })
}

/// The text of a facial scene or of the phoneme table.
fn face_text(item: &Item, bytes: &[u8]) -> Result<String> {
    Ok(match item {
        Item::Expression { .. } => {
            let data = Resource::parse(bytes)?
                .data()
                .ok_or_else(|| malformed("vcd", "no DATA"))??;
            expression_txt(&Scene::read(&data)?)
        }
        _ => phonemes_txt(&PhonemeTable::parse(bytes)?),
    })
}

/// A sound's audio file at `dest`, and a voice line's lip sync beside it. A line whose lip sync
/// does not read is still heard (its face stays still); the failure is reported.
fn sound(bytes: &[u8], dest: &str, out: &mut Out<'_>) -> Result<()> {
    let sound = Sound::parse(bytes)?;
    match Lipsync::parse(bytes) {
        Ok(lipsync) if !lipsync.is_empty() => {
            out.put(&lipsync_path(dest), lipsync_txt(&lipsync).as_bytes())?;
        }
        Ok(_) => {}
        Err(err) => out
            .report
            .failed
            .push((lipsync_path(dest), err.to_string())),
    }
    out.put(dest, &sound.file())
}

/// Where the lip sync of the sound written at `sound` goes: beside it, `<stem>.lipsync.txt`.
pub fn lipsync_path(sound: &str) -> String {
    let stem = sound.rsplit_once('.').map_or(sound, |(stem, _)| stem);
    format!("{stem}.lipsync.txt")
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
    let framed = framings(&read_from(src, &[pak], path)?, &wanted)?;
    let mut written = HashSet::new();
    for vmat in framed
        .iter()
        .filter_map(|f| Some(&f.background.as_ref()?.material))
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
    let text = portraits_txt(&framed, |vmat| {
        written.contains(vmat).then(|| background_png(vmat))
    });
    out.put(dest, text.as_bytes())
}

/// The image of portrait background material `vmat` (its `g_tColor`), as a PNG where
/// [`background_png`] puts it.
fn background(src: &impl Source, pak: &Vpk, vmat: &str, out: &mut Out<'_>) -> Result<()> {
    let bytes = read_from(src, &[pak], &format!("{vmat}_c"))?;
    let data = Resource::parse(&bytes)?
        .data()
        .ok_or_else(|| malformed("portraits", "a background material has no DATA"))??;
    let material = Material::from_vmat(vmat, &data)?;
    let vtex = material
        .textures
        .iter()
        .find_map(|(name, vtex)| (name == "g_tColor").then_some(vtex))
        .ok_or_else(|| malformed("portraits", "a background material has no image"))?;
    let texture = Texture::parse(&read_from(src, &[pak], &format!("{vtex}_c"))?)?;
    out.put(&background_png(vmat), &texture.first_frame_png()?)
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
    let exported = export_cache_world(src, packages, job.path, job.band, &bin, &ktx2_name)?;
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
            texture.to_ktx2(&Texture::parse(&bytes)?)?
        }
        Later::Layer(vtex) => {
            let bytes = read_from(src, packages, &format!("{vtex}_c"))?;
            Texture::parse(&bytes)?.first_frame_png()?
        }
        Later::Bytes(data) => return out.put(&job.path, data),
        Later::Tree(vmdl) => return tree_model(src, packages, vmdl, &job.path, out),
        Later::Particle(vpcf) => {
            let pak = packages
                .last()
                .ok_or_else(|| malformed("import", "no main package"))?;
            return particle(src, pak, vpcf, &job.path, out);
        }
        Later::Sound(vsnd) => {
            let bytes = read_from(src, packages, vsnd)?;
            let sound = Sound::parse(&bytes)?;
            if !job.path.ends_with(sound.extension()) {
                return Err(malformed(
                    "sound",
                    format!(
                        "{vsnd} is a {}, not the cache's {}",
                        sound.extension(),
                        job.path
                    ),
                ));
            }
            sound.file().into_owned()
        }
    };
    out.put(&job.path, &data)
}

/// The image file of texture `vtex` (`materials/x_color_psd_1.vtex` is `x_color_psd_1.png`).
fn png_name(vtex: &str) -> String {
    let name = vtex.rsplit('/').next().unwrap_or(vtex);
    format!("{}.png", name.trim_end_matches(".vtex"))
}

/// The texture file of a model's or the map's texture `vtex` (`materials/x_color_psd_1.vtex` is
/// `x_color_psd_1.ktx2`).
fn ktx2_name(vtex: &str) -> String {
    let name = vtex.rsplit('/').next().unwrap_or(vtex);
    format!("{}.ktx2", name.trim_end_matches(".vtex"))
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
    ambient: Option<&'a str>,
    soundscapes: Option<&'a str>,
}

/// `trees.txt`, `buildings.txt`, `lighting.txt`, `ambient.txt` and `soundscapes.txt` from the
/// entities of the world's default lump (the map's package, then the main one, which holds the
/// soundscape scripts), and the models, particles and sounds they name.
fn placements(
    src: &impl Source,
    [map, pak]: [&Vpk; 2],
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
    // Every tree the map places is drawn: its model is made from the map itself, not listed by hand.
    for tree in tree_models(&lump.entities) {
        let Some(path) = tree_model_path(&tree) else {
            continue;
        };
        out.pending.push_back(Pending {
            logical: format!("tree.{tree}"),
            path,
            what: Later::Tree(format!("models/props_tree/{tree}.vmdl_c")),
        });
    }
    out.put(outs.buildings, buildings_txt(&lump.entities).as_bytes())?;
    if let Some(path) = outs.lighting {
        out.put(path, lighting_txt(&lighting(&lump.entities)?).as_bytes())?;
    }
    if let Some(path) = outs.ambient {
        let (file, effects) = ambient_txt(&lump.entities);
        out.put(path, file.as_bytes())?;
        // Each effect is a logical of its own: one that fails does not drop the others.
        for effect in effects {
            let Some(stem) = effect_stem(&effect) else {
                continue;
            };
            out.pending.push_back(Pending {
                logical: format!("map.ambient.{stem}"),
                path: format!("particles/map/{stem}.json"),
                what: Later::Particle(format!("{effect}_c")),
            });
        }
    }
    if let Some(path) = outs.soundscapes {
        soundscapes(src, pak, &lump.entities, path, out)?;
    }
    Ok(())
}

/// `soundscapes.txt` from the entities and the scripts the main package's soundscape manifest
/// lists (one that is missing or unreadable is left out), and the sounds it plays.
fn soundscapes(
    src: &impl Source,
    pak: &Vpk,
    entities: &[Value],
    dest: &str,
    out: &mut Out<'_>,
) -> Result<()> {
    let manifest = read_from(src, &[pak], SOUNDSCAPE_MANIFEST)?;
    let mut files = Vec::new();
    for path in soundscape_files(keyvalues::text(&manifest)?) {
        if let Ok(bytes) = read_from(src, &[pak], &path) {
            files.push(bytes);
        }
    }
    let scripts: Vec<&str> = files
        .iter()
        .filter_map(|f| keyvalues::text(f).ok())
        .collect();
    let (file, waves) = soundscapes_txt(entities, &scripts);
    out.put(dest, file.as_bytes())?;
    for vsnd in waves {
        let name = vsnd.trim_start_matches("sounds/").trim_end_matches(".vsnd");
        out.pending.push_back(Pending {
            logical: format!("map.soundscape.{}", name.replace('/', ".")),
            path: wave_cache_path(&vsnd),
            what: Later::Sound(format!("{vsnd}_c")),
        });
    }
    Ok(())
}

/// Where Dota lists the files its soundscapes are in.
const SOUNDSCAPE_MANIFEST: &str = "scripts/soundscapes_manifest.txt";

/// What one model line asks for.
struct ModelJob<'a> {
    path: &'a str,
    dest: &'a str,
    meshes: Option<&'a [String]>,
    skeleton: bool,
    anims: &'a [String],
    skin: Option<&'a str>,
}

fn model(
    src: &impl Source,
    packages: &[&Vpk],
    job: &ModelJob<'_>,
    out: &mut Out<'_>,
) -> Result<()> {
    let ModelJob {
        path,
        dest,
        meshes,
        skeleton,
        anims,
        skin,
    } = *job;
    let keep: Option<Vec<&str>> = meshes.map(|m| m.iter().map(String::as_str).collect());
    let anims: Vec<&str> = anims.iter().map(String::as_str).collect();
    let options = ModelOptions {
        meshes: keep.as_deref(),
        skeleton,
        anims: &anims,
        texture_uri: &ktx2_name,
        generator: GENERATOR,
    };
    let exported = export_model_in_group(src, packages, path, &options, skin)?;
    let dir = dest.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut seen = HashSet::new();
    let mut decoded: Option<(String, Texture)> = None;
    for texture in &exported.textures {
        let file = format!("{dir}/{}", texture.uri);
        // An image another model of the folder wrote already (a model exported in each of its
        // skins shares most of them) is read once.
        if !seen.insert(texture.uri.clone()) || out.report.files.contains_key(&file) {
            continue;
        }
        let image = image(src, packages, texture, &mut decoded)?;
        out.put(&file, &image)?;
    }
    // The model last: its presence says its images are there.
    out.put(dest, &exported.glb)
}

/// The model of a tree at `vmdl`, written to `dest`: every mesh but the model viewer's
/// `_inspector` one, which is the tree standing (named `<tree>_model`, or the tree itself) and its
/// stump (named for it, when the tree leaves one); the client tells them by the word "stump".
fn tree_model(
    src: &impl Source,
    packages: &[&Vpk],
    vmdl: &str,
    dest: &str,
    out: &mut Out<'_>,
) -> Result<()> {
    let bytes = read_from(src, packages, vmdl)?;
    let resource = Resource::parse(&bytes)?;
    let parts = Model::read(&resource)?;
    let kept: Vec<String> = parts
        .meshes
        .iter()
        .filter(|m| !m.name.ends_with("_inspector"))
        .map(|m| m.name.clone())
        .collect();
    let job = ModelJob {
        path: vmdl,
        dest,
        meshes: Some(&kept),
        skeleton: false,
        anims: &[],
        skin: None,
    };
    model(src, packages, &job, out)
}

/// The KTX2 file of one image of a model: the texture itself, or its pixels packed.
fn image(
    src: &impl Source,
    packages: &[&Vpk],
    texture: &ExportedTexture,
    decoded: &mut Option<(String, Texture)>,
) -> Result<Vec<u8>> {
    if decoded
        .as_ref()
        .is_none_or(|(vtex, _)| *vtex != texture.vtex)
    {
        let bytes = read_from(src, packages, &format!("{}_c", texture.vtex))?;
        *decoded = Some((texture.vtex.clone(), Texture::parse(&bytes)?));
    }
    let Some((_, source)) = decoded.as_ref() else {
        return Err(Error::NotFound(texture.vtex.clone()));
    };
    texture.to_ktx2(source)
}
