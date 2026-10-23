//! The `.glb` writer given any model, materials and clips its public types can hold, not only
//! ones a resource reads into: it refuses what glTF cannot say instead of panicking, and
//! every file it writes validates as glTF (as the client's loader validates it).
#![no_main]

use std::collections::HashMap;

use dbota_assets::{
    Attribute, Bone, Clip, GltfOptions, Keys, Material, Mesh, Model, Primitive, Skin, Track,
    Vertices, write_glb,
};
use libfuzzer_sys::arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// A list of up to `max` items.
fn list<'a, T>(
    u: &mut Unstructured<'a>,
    max: usize,
    item: impl Fn(&mut Unstructured<'a>) -> Result<T>,
) -> Result<Vec<T>> {
    let n = u.int_in_range(0..=max)?;
    (0..n).map(|_| item(u)).collect()
}

/// A count within reach of the lists' lengths, now and then any.
fn index(u: &mut Unstructured) -> Result<usize> {
    Ok(if u.ratio(1, 16)? {
        usize::arbitrary(u)?
    } else {
        u.int_in_range(0..=8)?
    })
}

/// A list's length: now and then one other than the `n` its neighbours have.
fn length(u: &mut Unstructured, n: usize) -> Result<usize> {
    Ok(if u.ratio(1, 8)? {
        u.int_in_range(0..=n + 3)?
    } else {
        n
    })
}

fn vertices(u: &mut Unstructured) -> Result<Vertices> {
    let n = u.int_in_range(0..=48)?;
    let sized = length;
    let normals = sized(u, n)?;
    let tangents = sized(u, n)?;
    Ok(Vertices {
        positions: (0..n)
            .map(|_| <[f32; 3]>::arbitrary(u))
            .collect::<Result<_>>()?,
        normals: bool::arbitrary(u)?
            .then(|| (0..normals).map(|_| <[f32; 3]>::arbitrary(u)).collect())
            .transpose()?,
        tangents: bool::arbitrary(u)?
            .then(|| (0..tangents).map(|_| <[f32; 4]>::arbitrary(u)).collect())
            .transpose()?,
        uvs: list(u, 2, |u| {
            let len = sized(u, n)?;
            (0..len).map(|_| <[f32; 2]>::arbitrary(u)).collect()
        })?,
        extra: list(u, 2, |u| {
            let len = sized(u, n)?;
            Ok(Attribute {
                name: String::arbitrary(u)?,
                width: index(u)?,
                values: (0..len)
                    .map(|_| <[f32; 4]>::arbitrary(u))
                    .collect::<Result<_>>()?,
            })
        })?,
        skin: bool::arbitrary(u)?
            .then(|| -> Result<Skin> {
                let len = sized(u, n)?;
                Ok(Skin {
                    joints: (0..len)
                        .map(|_| <[u16; 4]>::arbitrary(u))
                        .collect::<Result<_>>()?,
                    weights: (0..len)
                        .map(|_| <[f32; 4]>::arbitrary(u))
                        .collect::<Result<_>>()?,
                })
            })
            .transpose()?,
    })
}

fn model(u: &mut Unstructured) -> Result<Model> {
    Ok(Model {
        bones: list(u, 24, |u| {
            Ok(Bone {
                name: String::arbitrary(u)?,
                parent: Option::<()>::arbitrary(u)?.map(|()| index(u)).transpose()?,
                translation: <[f32; 3]>::arbitrary(u)?,
                rotation: <[f32; 4]>::arbitrary(u)?,
            })
        })?,
        meshes: list(u, 3, |u| {
            Ok(Mesh {
                name: String::arbitrary(u)?,
                vertices: list(u, 2, vertices)?,
                primitives: list(u, 3, |u| {
                    Ok(Primitive {
                        vertices: index(u)?,
                        indices: list(u, 48, |u| Ok(u32::from(u8::arbitrary(u)?)))?,
                        material: String::arbitrary(u)?,
                        tint: Option::<[f32; 4]>::arbitrary(u)?,
                    })
                })?,
            })
        })?,
        material_groups: Vec::new(),
    })
}

fn keys<const N: usize>(u: &mut Unstructured) -> Result<Keys<N>> {
    let n = u.int_in_range(0..=8)?;
    let values = length(u, n)?;
    Ok(Keys {
        times: (0..n).map(|_| f32::arbitrary(u)).collect::<Result<_>>()?,
        values: (0..values)
            .map(|_| <[f32; N]>::arbitrary(u))
            .collect::<Result<_>>()?,
    })
}

fn clip(u: &mut Unstructured) -> Result<Clip> {
    Ok(Clip {
        name: String::arbitrary(u)?,
        tracks: list(u, 26, |u| {
            Ok(Track {
                rotation: keys(u)?,
                translation: keys(u)?,
                scale: keys(u)?,
            })
        })?,
    })
}

fn material(u: &mut Unstructured, path: &str) -> Result<Material> {
    let pairs = |u: &mut Unstructured| list(u, 3, <(String, String)>::arbitrary);
    Ok(Material {
        path: path.to_owned(),
        name: String::arbitrary(u)?,
        shader: String::arbitrary(u)?,
        ints: list(u, 3, <(String, i64)>::arbitrary)?,
        floats: list(u, 3, <(String, f32)>::arbitrary)?,
        vectors: list(u, 3, <(String, [f32; 4])>::arbitrary)?,
        textures: pairs(u)?,
        extras: serde_json::Value::Null,
    })
}

fn run(u: &mut Unstructured) -> Result<()> {
    let model = model(u)?;
    let mut materials = HashMap::new();
    for mesh in &model.meshes {
        for p in &mesh.primitives {
            if bool::arbitrary(u)? {
                materials.insert(p.material.clone(), material(u, &p.material)?);
            }
        }
    }
    let clips = list(u, 2, clip)?;
    let names: Vec<&str> = model.meshes.iter().map(|m| m.name.as_str()).collect();
    let options = GltfOptions {
        name: "models/m.vmdl_c",
        meshes: bool::arbitrary(u)?.then_some(&names[..names.len().min(1)]),
        skeleton: bool::arbitrary(u)?,
        materials: &materials,
        texture_uri: &|vtex: &str| format!("{vtex}.png"),
        clips: &clips,
    };
    if let Ok((glb, _)) = write_glb(&model, &options)
        && let Err(e) = gltf::Gltf::from_slice(&glb)
    {
        panic!("the writer wrote a file glTF refuses: {e}");
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
