//! A material as Source2Viewer 20 wrote it into glTF (`GenerateGLTFMaterialFromRenderMaterial`
//! with adapted textures): its colour factor from the tint, alpha mode, and which image of which
//! texture's channels feeds each slot.

use std::collections::HashMap;

use serde_json::{Map, Value as Json, json};

use super::inputs::{self, Channels, RGB, RGBA, Remap, Slot};
use super::pack::{Pack, orm_uri};
use super::vmat::{Alpha, Material};
use crate::error::{Result, malformed};
use crate::export::ExportedTexture;

/// A glTF material; its textures are indices into the [`Images`] it was made with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GltfMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub alpha: Alpha,
    pub cutoff: Option<f32>,
    pub double_sided: bool,
    pub unlit: bool,
    /// `KHR_materials_specular`'s colour factor, when the material has the extension.
    pub specular: Option<[f32; 3]>,
    pub base_color_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub occlusion_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
    pub specular_texture: Option<usize>,
    pub extras: Json,
}

/// The images materials use, each made once: the first material that uses a texture decides
/// how its image is made, as Source2Viewer names images after their texture alone.
#[derive(Default)]
pub(crate) struct Images {
    pub list: Vec<ExportedTexture>,
    by_key: HashMap<String, usize>,
}

impl Images {
    fn get(&mut self, key: String, make: impl FnOnce() -> ExportedTexture) -> usize {
        *self.by_key.entry(key).or_insert_with(|| {
            self.list.push(make());
            self.list.len() - 1
        })
    }
}

/// `material` drawn with `tint` (linear colour and opacity), its images named by `texture_uri`.
pub(crate) fn convert(
    material: &Material,
    tint: [f32; 4],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<GltfMaterial> {
    let translucent = material.int("F_TRANSLUCENT") > 0 || material.shader.ends_with("_glass.vfx");
    let alpha_test = material.int("F_ALPHA_TEST") > 0;
    let remaps: Vec<(&str, Vec<Remap>)> = material
        .textures
        .iter()
        .map(|(key, path)| (path.as_str(), inputs::remaps(material, key)))
        .collect();
    let specular = remaps
        .iter()
        .any(|(_, r)| r.iter().any(|r| r.slot == Slot::SpecularFactor));
    let mut out = GltfMaterial {
        name: material.name.clone(),
        base_color: base_color(material, tint),
        metallic: material
            .float("g_flMetalness")
            .map_or(0.0, |m| m.clamp(0.0, 1.0)),
        alpha: if translucent {
            Alpha::Blend
        } else if alpha_test {
            Alpha::Mask
        } else {
            Alpha::Opaque
        },
        cutoff: material
            .float("g_flAlphaTestReference")
            .filter(|_| alpha_test),
        double_sided: material.int("F_RENDER_BACKFACES") > 0,
        unlit: material.int("F_UNLIT") > 0,
        specular: specular.then(|| {
            material
                .vector("g_vSpecularColor")
                .map_or([1.0; 3], |v| [v[0], v[1], v[2]])
        }),
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
        specular_texture: None,
        extras: material.extras.clone(),
    };
    tie_textures(&mut out, remaps, images, texture_uri)?;
    Ok(out)
}

/// The tint, faded by `g_flModelTintAmount`, times `g_vColorTint`, made linear.
fn base_color(material: &Material, tint: [f32; 4]) -> [f32; 4] {
    let mut color = tint;
    if let Some(amount) = material.float("g_flModelTintAmount") {
        color = color.map(|c| 1.0 + (c - 1.0) * amount);
    }
    if let Some(v) = material.vector("g_vColorTint") {
        color = [0, 1, 2, 3].map(|i| color[i] * v[i]);
        color[3] = 1.0;
    }
    let [r, g, b] = srgb_to_linear([color[0], color[1], color[2]]);
    [r, g, b, color[3]].map(|c| c.clamp(0.0, 1.0))
}

/// sRGB-encoded colour to linear, with Source2Viewer's rounding.
pub(crate) fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            (2.4 * c.mul_add(1.0 / 1.055, 0.055 / 1.055).ln()).exp() - 0.000_000_1
        }
    })
}

/// Puts each texture's image into the slots it feeds; the metalness goes into an
/// occlusion/roughness/metallic image of its own.
fn tie_textures(
    out: &mut GltfMaterial,
    remaps: Vec<(&str, Vec<Remap>)>,
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<()> {
    // One entry per texture, at its first parameter's place, with its last parameter's remaps.
    let mut by_texture: Vec<(&str, Vec<Remap>)> = Vec::new();
    for (path, list) in remaps.into_iter().filter(|(_, l)| !l.is_empty()) {
        match by_texture.iter_mut().find(|(p, _)| *p == path) {
            Some(slot) => slot.1 = list,
            None => by_texture.push((path, list)),
        }
    }
    let orm_occlusion = by_texture
        .iter()
        .any(|(_, l)| l.iter().any(|r| r.slot == Slot::MetallicRoughness));
    let mut orm: Vec<(&str, Vec<Remap>)> = Vec::new();
    for (path, list) in &mut by_texture {
        let (packed, rest) = list
            .iter()
            .partition(|r| matches!(r.slot, Slot::Occlusion | Slot::MetallicRoughness));
        if !Vec::is_empty(&packed) {
            orm.push((path, packed));
        }
        *list = rest;
    }
    for (path, list) in &by_texture {
        let Some(main) = list.first() else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path);
        let pack = pack_of(main, path)?;
        let image = images.get(file.to_owned(), || ExportedTexture {
            vtex: (*path).to_owned(),
            uri: texture_uri(path),
            pack,
        });
        tie(out, main.slot, image, orm_occlusion);
    }
    if !orm.is_empty() {
        let image = orm_image(&orm, images, texture_uri)?;
        tie(out, Slot::MetallicRoughness, image, orm_occlusion);
    }
    Ok(())
}

fn tie(out: &mut GltfMaterial, slot: Slot, image: usize, orm_occlusion: bool) {
    match slot {
        Slot::BaseColor => out.base_color_texture = Some(image),
        Slot::Normal => out.normal_texture = Some(image),
        Slot::MetallicRoughness => {
            out.metallic_roughness_texture = Some(image);
            out.metallic = 1.0;
            if orm_occlusion {
                out.occlusion_texture = Some(image);
            }
        }
        Slot::SpecularFactor => out.specular_texture = Some(image),
        Slot::Occlusion => out.occlusion_texture = Some(image),
        Slot::Emissive => out.emissive_texture = Some(image),
    }
}

/// How the image of a texture feeding a slot through `remap` is made from it.
fn pack_of(remap: &Remap, path: &str) -> Result<Option<Pack>> {
    let from = if remap.from == RGBA && remap.to == RGB {
        RGB
    } else {
        remap.from
    };
    match from.0 {
        [c] => Ok(Some(Pack::Channel(*c))),
        [0, 1] | [0, 1, 2] => Ok(Some(Pack::Rgb)),
        [0, 1, 2, 3] => Ok(None),
        other => Err(malformed(
            "material",
            format!("{path}: channels {other:?} for a glTF slot"),
        )),
    }
}

/// The occlusion/roughness/metallic image. Only the form Dota's materials take is made: one
/// texture's blue channel as the metalness.
fn orm_image(
    orm: &[(&str, Vec<Remap>)],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<usize> {
    let blue = |r: &Remap| channel(r.from) == 2 && channel(r.to) == 2;
    match orm {
        [(path, list)] if list.iter().all(blue) => {
            Ok(images.get(format!("orm:{path}"), || ExportedTexture {
                vtex: (*path).to_owned(),
                uri: orm_uri(&texture_uri(path)),
                pack: Some(Pack::Orm),
            }))
        }
        _ => {
            let paths: Vec<&str> = orm.iter().map(|(p, _)| *p).collect();
            Err(malformed(
                "material",
                format!("occlusion/roughness/metallic packing of {paths:?}"),
            ))
        }
    }
}

/// The channel an ORM input reads or writes: a single one, else red.
fn channel(c: Channels) -> u8 {
    match c.0 {
        [one] => *one,
        _ => 0,
    }
}

impl GltfMaterial {
    /// The glTF material, its textures given by `texture` from their image's index; factors at
    /// glTF's defaults are left out.
    pub(crate) fn to_json(&self, texture: impl Fn(usize) -> usize) -> Json {
        let slot = |image: Option<usize>| image.map(|i| json!({ "index": texture(i) }));
        let mut pbr = Map::new();
        if self.base_color != [1.0; 4] {
            pbr.insert("baseColorFactor".into(), json!(self.base_color));
        }
        if let Some(t) = slot(self.base_color_texture) {
            pbr.insert("baseColorTexture".into(), t);
        }
        if self.metallic != 1.0 {
            pbr.insert("metallicFactor".into(), json!(self.metallic));
        }
        if let Some(t) = slot(self.metallic_roughness_texture) {
            pbr.insert("metallicRoughnessTexture".into(), t);
        }
        let mut out = json!({ "name": self.name, "pbrMetallicRoughness": pbr });
        for (key, image) in [
            ("normalTexture", self.normal_texture),
            ("occlusionTexture", self.occlusion_texture),
            ("emissiveTexture", self.emissive_texture),
        ] {
            if let Some(t) = slot(image) {
                out[key] = t;
            }
        }
        match self.alpha {
            Alpha::Opaque => {}
            Alpha::Mask => out["alphaMode"] = json!("MASK"),
            Alpha::Blend => out["alphaMode"] = json!("BLEND"),
        }
        if let Some(cutoff) = self.cutoff.filter(|&c| c != 0.5) {
            out["alphaCutoff"] = json!(cutoff);
        }
        if self.double_sided {
            out["doubleSided"] = json!(true);
        }
        let mut extensions = Map::new();
        if let Some(color) = self.specular {
            let mut specular = Map::new();
            if color != [1.0; 3] {
                specular.insert("specularColorFactor".into(), json!(color));
            }
            if let Some(t) = slot(self.specular_texture) {
                specular.insert("specularTexture".into(), t);
            }
            extensions.insert("KHR_materials_specular".into(), Json::Object(specular));
        }
        if self.unlit {
            extensions.insert("KHR_materials_unlit".into(), json!({}));
        }
        if !extensions.is_empty() {
            out["extensions"] = Json::Object(extensions);
        }
        out["extras"] = json!({ "vmat": self.extras });
        out
    }
}

/// The extensions `materials` (glTF JSON) use, for `extensionsUsed`.
pub(crate) fn extensions_used(materials: &[Json]) -> Vec<String> {
    let mut used: Vec<String> = materials
        .iter()
        .filter_map(|m| m.get("extensions")?.as_object())
        .flat_map(|e| e.keys().cloned())
        .collect();
    used.sort();
    used.dedup();
    used
}
