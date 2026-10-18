//! The world's band as the asset cache holds it: `mid.gltf`, its `.bin`, its images and the
//! terrain's layer textures.

use std::rc::Rc;

use super::crop::{Band, crop};
use super::scene::Builder;
use crate::error::{Result, malformed};
use crate::export::{ExportedTexture, read_from};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// Shaders whose materials blend several layers (terrain) or animate (water): their textures
/// are exported whole, beside the glTF that carries only one layer.
const LAYERED: [&str; 2] = ["multiblend.vfx", "water_dota.vfx"];
/// Layer textures the client has no use for.
const NOT_LAYERS: [&str; 3] = ["refl", "specular", "height"];
/// Decoded models kept while the band is written (its nodes come model by model).
const MODEL_CACHE: usize = 8;

/// A world cut to a band, ready to write.
pub struct ExportedWorld {
    /// The `.gltf` document.
    pub gltf: Vec<u8>,
    /// Its geometry, the file its buffer names.
    pub bin: Vec<u8>,
    /// The images its materials use, written next to it.
    pub textures: Vec<ExportedTexture>,
    /// The layer textures of its terrain and water materials (`.vtex` paths without `_c`),
    /// sorted; each is written whole as `<file stem>.png`.
    pub layers: Vec<String>,
}

/// Exports the world at `path` (`maps/.../world.vwrld_c`) from `packages` (the map's package
/// first) cut to `band`, its geometry named `bin_uri` and its images named by `texture_uri`.
pub fn export_world(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    band: &Band,
    bin_uri: &str,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<ExportedWorld> {
    let scene = Builder::new(src, packages, texture_uri).build(path)?;
    let mut cache: Vec<(usize, Rc<Model>)> = Vec::new();
    let mut load = |index: usize| -> Result<Rc<Model>> {
        if let Some((_, model)) = cache.iter().find(|(i, _)| *i == index) {
            return Ok(Rc::clone(model));
        }
        let model_path = &scene.models[index];
        let bytes = read_from(src, packages, &format!("{model_path}_c"))?;
        let model = Rc::new(Model::read(&Resource::parse(&bytes)?)?);
        if cache.len() == MODEL_CACHE {
            cache.remove(0);
        }
        cache.push((index, Rc::clone(&model)));
        Ok(model)
    };
    let cropped = crop(&scene, band, bin_uri, &mut load)?;
    let gltf =
        serde_json::to_vec(&cropped.doc).map_err(|e| malformed("world", format!("glTF: {e}")))?;
    let images: Vec<usize> = cropped.images;
    let mut layers: Vec<String> = cropped
        .materials
        .iter()
        .map(|&m| &scene.materials[m])
        .filter(|m| LAYERED.contains(&m.shader.as_str()))
        .flat_map(|m| m.textures.iter())
        .filter(|t| !NOT_LAYERS.iter().any(|skip| t.contains(skip)))
        .cloned()
        .collect();
    layers.sort_by_key(|t| format!("{t}_c"));
    layers.dedup();
    Ok(ExportedWorld {
        gltf,
        bin: cropped.bin,
        textures: images
            .iter()
            .map(|&i| scene.images.list[i].clone())
            .collect(),
        layers,
    })
}
