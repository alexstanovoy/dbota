use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::gltf::{GltfOptions, write_glb};
use crate::material::{Material, orm_uri};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// A model converted to `.glb`, and the images its materials use, which the caller writes
/// next to it.
pub struct ExportedModel {
    pub glb: Vec<u8>,
    pub textures: Vec<ExportedTexture>,
}

/// An image the `.glb` refers to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExportedTexture {
    /// The texture (`materials/.../x.vtex`) it is made from.
    pub vtex: String,
    /// Its file name relative to the `.glb`.
    pub uri: String,
    /// Whether it is the texture's pixels run through [`pack_orm`](crate::pack_orm) rather
    /// than the texture itself.
    pub orm: bool,
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
/// with its skeleton when `skeleton`.
pub fn export_model(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    meshes: Option<&[&str]>,
    skeleton: bool,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<ExportedModel> {
    let bytes = read_from(src, packages, path)?;
    let model = Model::read(&Resource::parse(&bytes)?)?;
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
    let mut textures: Vec<ExportedTexture> = Vec::new();
    for m in materials.values() {
        for (vtex, orm) in [(&m.base_color, false), (&m.normal, false), (&m.masks, true)] {
            if let Some(vtex) = vtex {
                let uri = texture_uri(vtex);
                let uri = if orm { orm_uri(&uri) } else { uri };
                textures.push(ExportedTexture {
                    vtex: vtex.clone(),
                    uri,
                    orm,
                });
            }
        }
    }
    textures.sort();
    textures.dedup();
    let options = GltfOptions {
        name: path,
        meshes,
        skeleton,
        materials: &materials,
        texture_uri,
    };
    Ok(ExportedModel {
        glb: write_glb(&model, &options),
        textures,
    })
}
