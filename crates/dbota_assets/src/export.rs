use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::gltf::{GltfOptions, write_glb};
use crate::material::Material;
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// A model converted to `.glb`, and the textures (`.vtex` paths) its materials use, which
/// the caller converts next to it under the names `texture_uri` gave them.
pub struct ExportedModel {
    pub glb: Vec<u8>,
    pub textures: Vec<String>,
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
    let mut textures: Vec<String> = materials
        .values()
        .flat_map(|m| [&m.base_color, &m.normal])
        .flatten()
        .cloned()
        .collect();
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
