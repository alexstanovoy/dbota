//! Golden comparison of the models this crate writes with the ones in the asset cache that
//! `tools/import_assets.sh` exported from the same files. Ignored by default, like
//! `golden.rs`: `cargo test --release -p dbota_assets --test models -- --ignored`.

use std::path::{Path, PathBuf};

use dbota_assets::{DirSource, ExportedTexture, Vpk, export_model, find_game_dir};
use serde_json::Value;

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn cache() -> PathBuf {
    let version = std::fs::read_to_string(repo().join("assets-manifest/version")).unwrap();
    std::env::var_os("DBOTA_ASSETS").map_or_else(
        || {
            let home = PathBuf::from(std::env::var_os("HOME").unwrap());
            home.join(".cache/dbota")
                .join(format!("v{}", version.trim()))
        },
        PathBuf::from,
    )
}

/// `(source, output, meshes=, skeleton)` of every `model` line; models that animate
/// (`anims=`) or wear onto one (`attach=`) keep their skeleton.
fn models() -> Vec<(String, String, Option<String>, bool)> {
    let text = std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap();
    text.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split('#').next()?.split_whitespace().collect();
            if words.first() != Some(&"model") {
                return None;
            }
            let meshes = words[4..].iter().find_map(|w| w.strip_prefix("meshes="));
            let skeleton = words[4..]
                .iter()
                .any(|w| w.starts_with("anims=") || w.starts_with("attach="));
            let (src, out) = (words[2].to_owned(), words[3].to_owned());
            Some((src, out, meshes.map(str::to_owned), skeleton))
        })
        .collect()
}

fn glb_json(bytes: &[u8]) -> Value {
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}

/// A `.glb`'s JSON and binary chunk.
struct Glb {
    json: Value,
    bin: Vec<u8>,
}

impl Glb {
    fn parse(bytes: &[u8]) -> Self {
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let bin = bytes.get(20 + len + 8..).unwrap_or_default().to_vec();
        Self {
            json: glb_json(bytes),
            bin,
        }
    }

    /// Accessor `i` as numbers (floats, or integers as floats), tightly packed.
    fn values(&self, i: u64) -> Vec<f64> {
        let a = &self.json["accessors"][i as usize];
        let view = &self.json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let start = (view["byteOffset"].as_u64().unwrap_or(0)
            + a["byteOffset"].as_u64().unwrap_or(0)) as usize;
        let width = match a["type"].as_str().unwrap() {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 16,
        };
        let (size, read): (usize, fn(&[u8]) -> f64) = match a["componentType"].as_u64().unwrap() {
            5126 => (4, |b| f32::from_le_bytes(b.try_into().unwrap()).into()),
            5125 => (4, |b| u32::from_le_bytes(b.try_into().unwrap()).into()),
            5123 => (2, |b| u16::from_le_bytes(b.try_into().unwrap()).into()),
            5121 => (1, |b| b[0].into()),
            other => panic!("component type {other}"),
        };
        let stride = view["byteStride"]
            .as_u64()
            .map_or(size * width, |s| s as usize);
        let count = a["count"].as_u64().unwrap() as usize;
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let scale = if normalized {
            [1.0, 255.0, 65535.0][size.min(2)]
        } else {
            1.0
        };
        (0..count)
            .flat_map(|v| (0..width).map(move |c| start + v * stride + c * size))
            .map(|at| read(&self.bin[at..at + size]) / scale)
            .collect()
    }
}

/// Named value streams of one primitive.
type Streams = Vec<(String, Vec<f64>)>;

/// Per primitive (meshes sorted by name), each attribute's values and the indices.
fn primitive_data(glb: &Glb) -> Vec<(String, Streams)> {
    let mut meshes: Vec<&Value> = glb.json["meshes"].as_array().unwrap().iter().collect();
    meshes.sort_by_key(|m| m["name"].as_str().unwrap().to_owned());
    let mut out = Vec::new();
    for mesh in meshes {
        for p in mesh["primitives"].as_array().unwrap() {
            let mut data: Vec<(String, Vec<f64>)> = p["attributes"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, a)| (k.clone(), glb.values(a.as_u64().unwrap())))
                .collect();
            data.push((
                "indices".to_owned(),
                glb.values(p["indices"].as_u64().unwrap()),
            ));
            data.sort_by(|a, b| a.0.cmp(&b.0));
            out.push((mesh["name"].as_str().unwrap().to_owned(), data));
        }
    }
    out
}

/// How far two attribute streams may differ: positions and UVs to rounding, unit vectors to
/// the precision of their packed forms, bone weights to 1/255.
fn tolerance(attribute: &str) -> f64 {
    match attribute {
        "POSITION" => 1e-4,
        "TEXCOORD_0" | "TEXCOORD_1" => 1e-3,
        "NORMAL" | "TANGENT" => 2e-2,
        "WEIGHTS_0" => 1.0 / 255.0 + 1e-4,
        _ => 0.0,
    }
}

/// The first value of the first stream that differs, as a message.
fn compare_data(ours: &Glb, theirs: &Glb) -> Option<String> {
    let (ours, theirs) = (primitive_data(ours), primitive_data(theirs));
    for ((mesh, a), (_, b)) in ours.iter().zip(&theirs) {
        for ((name, x), (other, y)) in a.iter().zip(b) {
            if name != other || x.len() != y.len() {
                return Some(format!(
                    "{mesh} {name}/{other}: {} vs {} values",
                    x.len(),
                    y.len()
                ));
            }
            let limit = tolerance(name);
            if let Some(i) = (0..x.len()).find(|&i| (x[i] - y[i]).abs() > limit) {
                return Some(format!("{mesh} {name}[{i}]: {} vs {}", x[i], y[i]));
            }
        }
    }
    None
}

/// Bone translations and inverse bind matrices may differ by float rounding only.
const POSE_TOLERANCE: f64 = 1e-4;

/// Per skin joint, in order: its name, parent's name, local translation and rotation, and
/// inverse bind matrix.
type Pose = Vec<(String, String, [f64; 3], [f64; 4], Vec<f64>)>;

/// The distinct skins (Source2Viewer writes one per mesh, all of the same skeleton).
fn skin_poses(glb: &Glb) -> Vec<Pose> {
    let nodes = glb.json["nodes"].as_array().unwrap();
    let name = |i: usize| nodes[i]["name"].as_str().unwrap().to_owned();
    let mut parent = vec![String::new(); nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        for child in node["children"].as_array().into_iter().flatten() {
            parent[child.as_u64().unwrap() as usize] = name(i);
        }
    }
    let numbers = |v: &Value, default: &[f64]| -> Vec<f64> {
        v.as_array().map_or_else(
            || default.to_vec(),
            |a| a.iter().map(|c| c.as_f64().unwrap()).collect(),
        )
    };
    glb.json["skins"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|skin| {
            let matrices = glb.values(skin["inverseBindMatrices"].as_u64().unwrap());
            skin["joints"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(k, j)| {
                    let j = j.as_u64().unwrap() as usize;
                    let t = numbers(&nodes[j]["translation"], &[0.0; 3]);
                    let r = numbers(&nodes[j]["rotation"], &[0.0, 0.0, 0.0, 1.0]);
                    (
                        name(j),
                        parent[j].clone(),
                        t.try_into().unwrap(),
                        r.try_into().unwrap(),
                        matrices[k * 16..k * 16 + 16].to_vec(),
                    )
                })
                .collect()
        })
        .fold(Vec::new(), |mut skins: Vec<Pose>, pose| {
            if !skins.contains(&pose) {
                skins.push(pose);
            }
            skins
        })
}

/// The first bone whose parent, pose or inverse bind matrix differs, as a message.
fn compare_poses(ours: &Glb, theirs: &Glb) -> Option<String> {
    let (ours, theirs) = (skin_poses(ours), skin_poses(theirs));
    if ours.len() != theirs.len() {
        return Some(format!("{} skins vs {}", ours.len(), theirs.len()));
    }
    let far = |a: &[f64], b: &[f64]| a.iter().zip(b).any(|(x, y)| (x - y).abs() > POSE_TOLERANCE);
    for (a, b) in ours.iter().zip(&theirs) {
        for (x, y) in a.iter().zip(b) {
            if x.0 != y.0 || x.1 != y.1 {
                return Some(format!("bone {}/{} under {}/{}", x.0, y.0, x.1, y.1));
            }
            // q and -q are the same rotation.
            let sign = if x.3.iter().zip(&y.3).map(|(p, q)| p * q).sum::<f64>() < 0.0 {
                -1.0
            } else {
                1.0
            };
            let flipped = y.3.map(|c| c * sign);
            if far(&x.2, &y.2) || far(&x.3, &flipped) {
                return Some(format!(
                    "bone {} local {:?} {:?} vs {:?} {:?}",
                    x.0, x.2, x.3, y.2, y.3
                ));
            }
            if far(&x.4, &y.4) {
                return Some(format!("bone {} inverse bind {:?} vs {:?}", x.0, x.4, y.4));
            }
        }
    }
    None
}

/// The image file of texture slot `slot` (`["normalTexture"]`) of material `m`.
fn image(doc: &Value, m: &Value, slot: &[&str]) -> Option<String> {
    let texture = slot.iter().fold(m, |v, k| &v[*k])["index"].as_u64()? as usize;
    let source = doc["textures"][texture]["source"].as_u64()? as usize;
    doc["images"][source]["uri"].as_str().map(str::to_owned)
}

/// An image name without the hash Source2Viewer appended to the ones it made up
/// (`x_orm_123.png` is `x_orm.png`).
fn unhashed(name: String) -> String {
    match name.split_once("_orm_") {
        Some((stem, hash)) => format!("{stem}_orm.{}", hash.rsplit('.').next().unwrap()),
        None => name,
    }
}

/// What both files must agree on, as lines, so a mismatch reads as a diff.
fn summary(doc: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let accessors = &doc["accessors"];
    let mut meshes: Vec<&Value> = doc["meshes"].as_array().unwrap().iter().collect();
    meshes.sort_by_key(|m| m["name"].as_str().unwrap().to_owned());
    for mesh in meshes {
        for (i, p) in mesh["primitives"].as_array().unwrap().iter().enumerate() {
            let position = &accessors[p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let mut attributes: Vec<&String> =
                p["attributes"].as_object().unwrap().keys().collect();
            attributes.sort();
            let round = |v: &Value| -> Vec<String> {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|c| format!("{:.2}", c.as_f64().unwrap()))
                    .collect()
            };
            let material = &doc["materials"][p["material"].as_u64().unwrap() as usize];
            lines.push(format!(
                "{} #{i}: {} vertices, {} indices, {attributes:?}, bounds {:?}..{:?}, material {} color {:?} normal {:?} orm {:?} occlusion {:?} metallic {} roughness {} emissive {:?} specular {:?}",
                mesh["name"].as_str().unwrap(),
                position["count"],
                accessors[p["indices"].as_u64().unwrap() as usize]["count"],
                round(&position["min"]),
                round(&position["max"]),
                material["name"],
                image(doc, material, &["pbrMetallicRoughness", "baseColorTexture"]),
                image(doc, material, &["normalTexture"]),
                image(
                    doc,
                    material,
                    &["pbrMetallicRoughness", "metallicRoughnessTexture"]
                )
                .map(unhashed),
                image(doc, material, &["occlusionTexture"]).map(unhashed),
                material["pbrMetallicRoughness"]["metallicFactor"]
                    .as_f64()
                    .unwrap_or(1.0),
                material["pbrMetallicRoughness"]["roughnessFactor"]
                    .as_f64()
                    .unwrap_or(1.0),
                image(doc, material, &["emissiveTexture"]),
                image(
                    doc,
                    material,
                    &["extensions", "KHR_materials_specular", "specularTexture"]
                ),
            ));
        }
    }
    for skin in doc["skins"].as_array().into_iter().flatten() {
        let names: Vec<&str> = skin["joints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|j| {
                doc["nodes"][j.as_u64().unwrap() as usize]["name"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        lines.push(format!("skin of {}: {names:?}", names.len()));
    }
    lines.dedup();
    lines
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn models_match_the_exported_ones() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let texture_uri = |vtex: &str| {
        let name = vtex.rsplit('/').next().unwrap_or(vtex);
        format!("{}.png", name.trim_end_matches(".vtex"))
    };
    let mut differ = Vec::new();
    let mut data = Vec::new();
    for (source, out, meshes, skeleton) in models() {
        let keep: Option<Vec<&str>> = meshes.as_deref().map(|m| m.split(',').collect());
        let ours = export_model(
            &src,
            &[&pak],
            &source,
            keep.as_deref(),
            skeleton,
            &texture_uri,
        )
        .unwrap_or_else(|e| panic!("{source}: {e}"));
        let theirs = Glb::parse(&std::fs::read(cache().join(&out)).unwrap());
        let ours = Glb::parse(&ours.glb);
        if let Some(diff) = compare_data(&ours, &theirs) {
            data.push(format!("{source}: {diff}"));
        }
        if let Some(diff) = compare_poses(&ours, &theirs) {
            data.push(format!("{source}: {diff}"));
        }
        let (ours, theirs) = (summary(&ours.json), summary(&theirs.json));
        if ours != theirs {
            differ.push(format!(
                "{source}\n  ours:   {ours:#?}\n  theirs: {theirs:#?}"
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "{} models differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert!(data.is_empty(), "vertex data differs:\n{}", data.join("\n"));
}

/// Writes the models without clips (static ones and wearables) over a copy of the cache, to
/// look at them in the client:
///
/// ```sh
/// cp -rs ~/.cache/dbota/v6/. /path/to/copy   # a farm of symlinks, nothing is duplicated
/// DBOTA_MODEL_CACHE=/path/to/copy cargo test --release -p dbota_assets --test models \
///     -- --ignored write_models_into_a_cache_copy
/// DBOTA_ASSETS=/path/to/copy tools/visual_check.sh replay.brp
/// ```
///
/// The images are the cache's own; the occlusion/roughness/metallic ones, which the cache has
/// under a hashed name, are linked under ours. Animated models wait for their clips.
#[test]
#[ignore = "needs a local Dota 2 install, its asset cache and DBOTA_MODEL_CACHE"]
fn write_models_into_a_cache_copy() {
    let copy = PathBuf::from(std::env::var_os("DBOTA_MODEL_CACHE").expect("DBOTA_MODEL_CACHE"));
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let texture_uri = |vtex: &str| {
        let name = vtex.rsplit('/').next().unwrap_or(vtex);
        format!("{}.png", name.trim_end_matches(".vtex"))
    };
    let manifest = std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap();
    let animated: Vec<&str> = manifest
        .lines()
        .filter(|l| l.split('#').next().unwrap().contains(" anims="))
        .filter_map(|l| l.split_whitespace().nth(3))
        .collect();
    let mut written = 0;
    for (source, out, meshes, skeleton) in models() {
        if animated.contains(&out.as_str()) {
            continue;
        }
        let keep: Option<Vec<&str>> = meshes.as_deref().map(|m| m.split(',').collect());
        let model = export_model(
            &src,
            &[&pak],
            &source,
            keep.as_deref(),
            skeleton,
            &texture_uri,
        )
        .unwrap_or_else(|e| panic!("{source}: {e}"));
        let dir = copy.join(&out).parent().unwrap().to_owned();
        for texture in &model.textures {
            link_cached_image(&dir, texture);
        }
        let target = copy.join(&out);
        let _ = std::fs::remove_file(&target); // a symlink into the original cache
        std::fs::write(&target, &model.glb).unwrap();
        written += 1;
    }
    println!("{written} models written");
}

/// Makes `texture`'s image exist in `dir` for a model written over the cache copy: images of
/// whole textures and of single channels are there under their names, the occlusion/
/// roughness/metallic ones under a hashed name.
fn link_cached_image(dir: &Path, texture: &ExportedTexture) {
    if dir.join(&texture.uri).exists() {
        return;
    }
    let stem = texture.uri.trim_end_matches("_orm.png");
    let hashed = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with(&format!("{stem}_orm_")) && name.ends_with(".png")
        })
        .unwrap_or_else(|| panic!("no cached image for {}", texture.uri));
    std::os::unix::fs::symlink(
        std::fs::canonicalize(hashed).unwrap(),
        dir.join(&texture.uri),
    )
    .unwrap();
}
