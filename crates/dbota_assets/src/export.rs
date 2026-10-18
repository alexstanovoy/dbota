use std::collections::HashMap;

use crate::anim::read_clips;
use crate::error::{Error, Result};
use crate::gltf::{GltfOptions, image_uri, write_glb};
use crate::material::{Material, Pack};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// A model converted to `.glb`, and the images its materials use, which the caller writes
/// next to it.
pub struct ExportedModel {
    pub glb: Vec<u8>,
    pub textures: Vec<ExportedTexture>,
    /// The animations written, in file order; a name asked for that the model lacks (or an
    /// additive clip) is not among them.
    pub clips: Vec<ExportedClip>,
}

/// An animation in the `.glb`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportedClip {
    pub name: String,
    /// Seconds to its last keyframe: a clip that ends holding a pose stops where the pose
    /// is reached.
    pub duration: f32,
}

/// An image the `.glb` refers to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExportedTexture {
    /// The texture (`materials/.../x.vtex`) it is made from.
    pub vtex: String,
    /// Its file name relative to the `.glb`.
    pub uri: String,
    /// How its pixels derive from the texture's: [`Pack::apply`]. `None` is the texture as it is.
    pub pack: Option<Pack>,
}

/// A compiled file from the first package that has it.
pub fn read_from(src: &(impl Source + ?Sized), packages: &[&Vpk], path: &str) -> Result<Vec<u8>> {
    let vpk = packages
        .iter()
        .find(|vpk| vpk.get(path).is_some())
        .ok_or_else(|| Error::NotFound(path.to_owned()))?;
    vpk.read_file(src, path)
}

/// Converts the model at `path` (`models/.../x.vmdl_c`), keeping only `meshes` when given,
/// with its skeleton when `skeleton` and then the animations named `anims`.
pub fn export_model(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    meshes: Option<&[&str]>,
    skeleton: bool,
    anims: &[&str],
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<ExportedModel> {
    let bytes = read_from(src, packages, path)?;
    let resource = Resource::parse(&bytes)?;
    let model = Model::read(&resource)?;
    let clips = if skeleton {
        read_clips(&resource, anims)?
    } else {
        Vec::new()
    };
    let mut materials = HashMap::new();
    let used = model
        .meshes
        .iter()
        .filter(|m| meshes.is_none_or(|keep| keep.contains(&m.name.as_str())))
        .flat_map(|m| &m.primitives)
        .map(|p| p.material.as_str());
    for material in used {
        if materials.contains_key(material) || material.is_empty() {
            continue;
        }
        let bytes = read_from(src, packages, &format!("{material}_c"))?;
        let resource = Resource::parse(&bytes)?;
        if let Some(data) = resource.data() {
            materials.insert(material.to_owned(), Material::from_vmat(material, &data?));
        }
    }
    let textures = images(&materials, texture_uri);
    let options = GltfOptions {
        name: path,
        meshes,
        skeleton,
        materials: &materials,
        texture_uri,
        clips: &clips,
    };
    Ok(ExportedModel {
        glb: write_glb(&model, &options),
        textures,
        clips: clips
            .iter()
            .map(|c| ExportedClip {
                name: c.name.clone(),
                duration: c.duration(),
            })
            .collect(),
    })
}

/// The images the materials use, each once.
fn images(
    materials: &HashMap<String, Material>,
    texture_uri: &dyn Fn(&str) -> String,
) -> Vec<ExportedTexture> {
    let mut textures: Vec<ExportedTexture> = Vec::new();
    for m in materials.values() {
        let wanted = [
            (&m.base_color, None),
            (&m.normal, None),
            (&m.masks1, Some(Pack::Orm)),
            (&m.masks1, Some(Pack::Channel(3))),
            (&m.masks2, Some(Pack::Channel(0))),
        ];
        for (vtex, pack) in wanted {
            if let Some(vtex) = vtex {
                textures.push(ExportedTexture {
                    vtex: vtex.clone(),
                    uri: image_uri(vtex, pack, texture_uri),
                    pack,
                });
            }
        }
    }
    textures.sort();
    textures.dedup();
    textures
}
