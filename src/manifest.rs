//! What the asset cache holds, by the logical names the client asks for:
//! read from `assets-manifest/manifest.txt` and `ids.txt`, and checked
//! against the files actually present in the cache.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

/// The manifest, compiled in.
pub const MANIFEST: &str = include_str!("../assets-manifest/manifest.txt");
/// Our ids to Dota names, compiled in.
const IDS: &str = include_str!("../assets-manifest/ids.txt");

/// One animation of a model.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The logical name: `idle`, `run`, `attack`, `cast1`, `death`, ...
    pub logical: String,
    /// The animation's name in the glTF file.
    pub name: String,
    /// Seconds the clip lasts at its own pace.
    pub duration: f32,
    /// Seconds into the clip the attack or cast point falls, at base speed.
    pub point: Option<f32>,
}

/// One model of the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// The logical name, such as `hero.nevermore.model`.
    pub logical: String,
    /// The file, relative to the cache.
    pub path: String,
    /// The unit's own scale in Dota.
    pub scale: f32,
    /// The model this one is worn on, for a wearable.
    pub attach: Option<String>,
    /// Its animations.
    pub clips: Vec<Clip>,
}

/// How one kind of unit is drawn: which model, at what size.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitArt {
    /// The unit's name in Dota's `npc_units.txt`.
    pub unit: String,
    /// The logical name of its model; absent when it is drawn as nothing.
    pub model: Option<String>,
    /// The unit's own `ModelScale`, when it differs from its model's.
    pub scale: Option<f32>,
}

/// Everything the cache was exported with, and which of it is there.
#[derive(Resource, Clone, Debug, Default)]
pub struct Manifest {
    /// The cache folder; absent when it does not exist.
    pub root: Option<PathBuf>,
    /// The models whose files are present.
    pub models: Vec<Model>,
    /// Textures and sounds whose files are present: logical name and path.
    pub files: Vec<(String, String)>,
    /// The map file, when present.
    pub map: Option<String>,
    /// How the units named by Dota are drawn (every `unit` line, whether or
    /// not its model is in the cache).
    pub units: Vec<UnitArt>,
    /// Our ids to Dota names: kind (`item`, `ability`, `hero`, `effect`), id,
    /// name (for an effect, the logical name of its icon).
    pub ids: Vec<(String, u16, String)>,
    /// The Dota name whose text describes an item or an ability where that
    /// is not the one its icon is named after: kind, id, name.
    pub text_keys: Vec<(String, u16, String)>,
}

impl Manifest {
    /// Reads the manifest against a cache folder.
    pub fn load(root: &Path) -> Manifest {
        // A browser's page publishes its cache and its index (webcache.rs); a
        // page without one has no assets.
        #[cfg(target_arch = "wasm32")]
        let (present, has_root): (&dyn Fn(&str) -> bool, bool) =
            (&crate::webcache::present, crate::webcache::available());
        #[cfg(not(target_arch = "wasm32"))]
        let (present, has_root): (&dyn Fn(&str) -> bool, bool) =
            (&|rel: &str| root.join(rel).is_file(), root.is_dir());
        let mut manifest = Manifest {
            root: has_root.then(|| root.to_path_buf()),
            ..Manifest::default()
        };
        for line in MANIFEST.lines().map(str::trim) {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["model", logical, _, path, rest @ ..] if present(path) => {
                    manifest.models.push(model(logical, path, rest));
                }
                ["unit", unit, model, rest @ ..] => {
                    manifest.units.push(UnitArt {
                        unit: (*unit).to_string(),
                        model: (*model != "none").then(|| (*model).to_string()),
                        scale: rest
                            .iter()
                            .find_map(|w| w.strip_prefix("model_scale="))
                            .and_then(|s| s.parse().ok()),
                    });
                }
                ["clip", model, logical, name, duration, rest @ ..] => {
                    let point = rest
                        .iter()
                        .find_map(|w| w.strip_prefix("point="))
                        .and_then(|p| p.parse().ok());
                    if let Some(m) = manifest.models.iter_mut().find(|m| m.logical == *model) {
                        m.clips.push(Clip {
                            logical: (*logical).to_string(),
                            name: (*name).to_string(),
                            duration: duration.parse().unwrap_or(1.0),
                            point,
                        });
                    }
                }
                ["texture" | "sound", logical, _, path, ..] if present(path) => {
                    manifest
                        .files
                        .push(((*logical).to_string(), (*path).to_string()));
                }
                ["world", _, _, path, ..] if present(path) => {
                    manifest.map = Some(cache_path(path));
                }
                _ => {}
            }
        }
        manifest.ids = IDS
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                let kind = words.next()?;
                if !matches!(kind, "item" | "ability" | "hero" | "effect") {
                    return None;
                }
                let id = words.next()?.parse().ok()?;
                Some((kind.to_string(), id, words.next()?.to_string()))
            })
            .collect();
        manifest.text_keys = IDS
            .lines()
            .filter_map(|line| {
                let mut words = line.split('#').next()?.split_whitespace();
                (words.next()? == "text").then_some(())?;
                let kind = words.next()?.to_string();
                let id = words.next()?.parse().ok()?;
                Some((kind, id, words.next()?.to_string()))
            })
            .collect();
        manifest
    }

    /// How the unit of a Dota name is drawn, if the manifest says.
    pub fn unit_art(&self, unit: &str) -> Option<&UnitArt> {
        self.units.iter().find(|a| a.unit == unit)
    }

    /// The model of a logical name, if its file is there.
    pub fn model(&self, logical: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.logical == logical)
    }

    /// The asset path of a texture or sound of a logical name, if its file
    /// is there.
    pub fn file(&self, logical: &str) -> Option<String> {
        self.path(logical).map(cache_path)
    }

    /// The file of a texture or sound of a logical name, relative to the
    /// cache, if it is there.
    pub fn path(&self, logical: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|(name, _)| name == logical)
            .map(|(_, path)| path.as_str())
    }

    /// Every sound whose logical name starts with a prefix, as asset paths.
    pub fn sounds(&self, prefix: &str) -> Vec<String> {
        self.files
            .iter()
            .filter(|(name, path)| {
                name.starts_with(prefix) && (path.ends_with(".wav") || path.ends_with(".mp3"))
            })
            .map(|(_, path)| cache_path(path))
            .collect()
    }

    /// The Dota name of one of our ids of a kind.
    pub fn dota_name(&self, kind: &str, id: u16) -> Option<&str> {
        self.ids
            .iter()
            .find(|(k, i, _)| k == kind && *i == id)
            .map(|(_, _, name)| name.as_str())
    }

    /// The key of the text that describes an item or an ability in the
    /// cache's text file.
    pub fn text_key(&self, kind: &str, id: u16) -> Option<String> {
        if let Some((_, _, name)) = self
            .text_keys
            .iter()
            .find(|(k, i, _)| k == kind && *i == id)
        {
            return Some(name.clone());
        }
        let name = self.dota_name(kind, id)?;
        Some(if kind == "item" {
            format!("item_{name}")
        } else {
            name.to_string()
        })
    }

    /// The asset path of a hero's portrait, the tall one where the cache
    /// has it.
    pub fn portrait(&self, hero: u16) -> Option<String> {
        let name = self.dota_name("hero", hero)?;
        self.file(&format!("portrait.hero.{name}.tall"))
            .or_else(|| self.file(&format!("portrait.hero.{name}")))
    }

    /// The asset path of an item's icon.
    pub fn item_icon(&self, id: u16) -> Option<String> {
        self.file(&format!("icon.item.{}", self.dota_name("item", id)?))
    }

    /// The asset path of an ability's icon.
    pub fn ability_icon(&self, id: u16) -> Option<String> {
        self.file(&format!("icon.ability.{}", self.dota_name("ability", id)?))
    }

    /// The asset path of the icon Dota shows for an effect on the buff bar.
    pub fn effect_icon(&self, id: u16) -> Option<String> {
        self.file(self.dota_name("effect", id)?)
    }

    /// How much of the cache is there, in words, for the settings overlay.
    pub fn census(&self) -> String {
        match &self.root {
            None => "no Dota asset cache: built-in stand-ins".to_string(),
            Some(root) => format!(
                "Dota assets from {}: {} models, {} textures and sounds, map {}",
                root.display(),
                self.models.len(),
                self.files.len(),
                if self.map.is_some() { "yes" } else { "no" }
            ),
        }
    }
}

/// A model line's fields.
fn model(logical: &str, path: &str, rest: &[&str]) -> Model {
    let field = |key: &str| rest.iter().find_map(|w| w.strip_prefix(key));
    Model {
        logical: logical.to_string(),
        path: cache_path(path),
        scale: field("model_scale=")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),
        attach: field("attach=").map(str::to_string),
        clips: Vec::new(),
    }
}

/// Settings for loading a model or the map: its textures are kept on the
/// GPU alone. Nothing reads their pixels on the CPU, and a copy of every
/// one would hold most of a gigabyte. Every load of a file must use the
/// same settings, or the file is read twice.
pub fn gpu_textures(settings: &mut bevy::gltf::GltfLoaderSettings) {
    settings.load_materials = bevy::asset::RenderAssetUsages::RENDER_WORLD;
}

/// Loads (a part of) a tree model's glTF file with [`gpu_textures`]: the
/// forest copies the trees' meshes into its chunks, so they stay on the CPU.
pub fn load_model<A: Asset>(
    server: &AssetServer,
    path: impl Into<bevy::asset::AssetPath<'static>>,
) -> Handle<A> {
    server.load_builder().with_settings(gpu_textures).load(path)
}

/// Loads (a part of) a unit model's glTF file, its meshes as well as its
/// textures on the GPU alone: units draw them and nothing reads them back.
pub fn load_unit_model<A: Asset>(
    server: &AssetServer,
    path: impl Into<bevy::asset::AssetPath<'static>>,
) -> Handle<A> {
    server
        .load_builder()
        .with_settings(|settings: &mut bevy::gltf::GltfLoaderSettings| {
            gpu_textures(settings);
            settings.load_meshes = bevy::asset::RenderAssetUsages::RENDER_WORLD;
        })
        .load(path)
}

/// The asset path of a file relative to the cache.
pub fn cache_path(rel: &str) -> String {
    format!("{}://{rel}", crate::CACHE_SOURCE)
}
