//! Golden comparison of the models this crate writes with the ones in the reference cache
//! Source2Viewer exported from the same files (see `common`). Ignored by default, like
//! `golden.rs`: `cargo test --release -p dbota_import --test models -- --ignored`.

mod common;

use std::path::{Path, PathBuf};

use dbota_import::import::GENERATOR;
use heatel::export::{
    ExportedClip, ExportedModel, ExportedTexture, ModelOptions, export_model, read_from,
};
use heatel::{DirSource, Texture, Vpk, find_game_dir};
use serde_json::Value;

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A `model` line of the manifest.
struct Line {
    source: String,
    out: String,
    /// `meshes=`.
    meshes: Option<Vec<String>>,
    /// `anims=`.
    anims: Vec<String>,
    /// Models that animate (`anims=`) or wear onto one (`attach=`) keep their skeleton.
    skeleton: bool,
}

impl Line {
    fn export(&self, src: &DirSource, pak: &Vpk) -> ExportedModel {
        let texture_uri = |vtex: &str| {
            let name = vtex.rsplit('/').next().unwrap_or(vtex);
            format!("{}.png", name.trim_end_matches(".vtex"))
        };
        let meshes: Option<Vec<&str>> = self
            .meshes
            .as_ref()
            .map(|m| m.iter().map(String::as_str).collect());
        let anims: Vec<&str> = self.anims.iter().map(String::as_str).collect();
        let options = ModelOptions {
            meshes: meshes.as_deref(),
            skeleton: self.skeleton,
            anims: &anims,
            texture_uri: &texture_uri,
            generator: GENERATOR,
        };
        export_model(src, &[pak], &self.source, &options)
            .unwrap_or_else(|e| panic!("{}: {e}", self.source))
    }
}

fn manifest() -> String {
    std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap()
}

fn models() -> Vec<Line> {
    manifest()
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split('#').next()?.split_whitespace().collect();
            if words.first() != Some(&"model") {
                return None;
            }
            let list = |key: &str| {
                words[4..]
                    .iter()
                    .find_map(|w| w.strip_prefix(key))
                    .map(|l| l.split(',').map(str::to_owned).collect::<Vec<_>>())
            };
            let skeleton = words[4..]
                .iter()
                .any(|w| w.starts_with("anims=") || w.starts_with("attach="));
            Some(Line {
                source: words[2].to_owned(),
                out: words[3].to_owned(),
                meshes: list("meshes="),
                anims: list("anims=").unwrap_or_default(),
                skeleton,
            })
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

/// How far two attribute streams may differ: positions and unit vectors by the rounding of the
/// exporter's frame change (its rotation is a float quaternion, a few 1e-8 off), UVs not at
/// all, bone weights to 1/255.
fn tolerance(attribute: &str) -> f64 {
    match attribute {
        "POSITION" => 1e-5,
        "NORMAL" | "TANGENT" => 1e-6,
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
    let mut differ = Vec::new();
    let mut data = Vec::new();
    for line in models() {
        let Some(reference) = common::exported(&line.out) else {
            continue;
        };
        let (source, ours) = (&line.source, line.export(&src, &pak));
        let theirs = Glb::parse(&std::fs::read(reference).unwrap());
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

/// Writes every model of the manifest, clips included, over a copy of the cache, to
/// look at them in the client:
///
/// ```sh
/// cp -rs ~/.cache/dbota/v6/. /path/to/copy   # a farm of symlinks, nothing is duplicated
/// DBOTA_MODEL_CACHE=/path/to/copy cargo test --release -p dbota_import --test models \
///     -- --ignored write_models_into_a_cache_copy
/// DBOTA_ASSETS=/path/to/copy tools/visual_check.sh replay.brp
/// ```
///
/// The images of whole textures are the cache's own; the packed ones (occlusion/roughness/
/// metallic, emissive and specular masks) are made from the game's textures.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache; writes only with DBOTA_MODEL_CACHE"]
fn write_models_into_a_cache_copy() {
    let Some(copy) = std::env::var_os("DBOTA_MODEL_CACHE").map(PathBuf::from) else {
        println!("DBOTA_MODEL_CACHE is not set: nothing to write");
        return;
    };
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let mut written = 0;
    for line in models() {
        let model = line.export(&src, &pak);
        let target = copy.join(&line.out);
        let dir = target.parent().unwrap().to_owned();
        for texture in &model.textures {
            write_packed_image(&src, &pak, &dir, texture);
        }
        let _ = std::fs::remove_file(&target); // a symlink into the original cache
        std::fs::write(&target, &model.glb).unwrap();
        written += 1;
    }
    println!("{written} models written");
}

/// Writes the image `texture` asks for into `dir`, made from its source texture. Images of
/// whole textures are the cache's own.
fn write_packed_image(src: &DirSource, pak: &Vpk, dir: &Path, texture: &ExportedTexture) {
    let Some(pack) = texture.pack else { return };
    let bytes = read_from(src, &[pak], &format!("{}_c", texture.vtex)).unwrap();
    let source = Texture::parse(&bytes).unwrap();
    let mut pixels = source.to_rgba8().unwrap();
    pack.apply(&mut pixels);
    let png = Texture::from_rgba8(source.width(), source.height(), pixels)
        .unwrap()
        .to_png()
        .unwrap();
    let target = dir.join(&texture.uri);
    let _ = std::fs::remove_file(&target); // a link into the original cache
    std::fs::write(target, png).unwrap();
}

/// How far a sampled bone transform may be from the exported one, absolutely and relative to
/// its size: float rounding, and Source2Viewer taking the root motion out of a matrix where we
/// compose it directly (destruction debris flies hundreds of metres).
const CLIP_TOLERANCE: f64 = 1e-4;
const CLIP_RELATIVE: f64 = 1e-6;

/// One channel of an animation: its node's name, path, key times and values.
type Channel = (String, String, Vec<f64>, Vec<Vec<f64>>);

/// The animations of a `.glb` by name, each with its channels.
fn animations(glb: &Glb) -> Vec<(String, Vec<Channel>)> {
    let nodes = &glb.json["nodes"];
    let list = glb.json["animations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    list.iter()
        .map(|a| {
            let channels = a["channels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let sampler = &a["samplers"][c["sampler"].as_u64().unwrap() as usize];
                    let target = &c["target"];
                    let node = nodes[target["node"].as_u64().unwrap() as usize]["name"]
                        .as_str()
                        .unwrap()
                        .to_owned();
                    let times = glb.values(sampler["input"].as_u64().unwrap());
                    let values = glb.values(sampler["output"].as_u64().unwrap());
                    let width = values.len() / times.len().max(1);
                    let values = values.chunks(width.max(1)).map(<[f64]>::to_vec).collect();
                    let path = target["path"].as_str().unwrap().to_owned();
                    (node, path, times, values)
                })
                .collect();
            (a["name"].as_str().unwrap().to_owned(), channels)
        })
        .collect()
}

/// The channel's value at `t`, interpolated linearly (normalised for rotations).
fn sample(times: &[f64], values: &[Vec<f64>], t: f64, rotation: bool) -> Vec<f64> {
    let next = times.partition_point(|&k| k < t);
    // Before the first key and after the last the value holds, as glTF plays it.
    if next == times.len() {
        return values[next - 1].clone();
    }
    if next == 0 || times[next] == t {
        return values[next].clone();
    }
    let (a, b) = (&values[next - 1], &values[next]);
    let f = (t - times[next - 1]) / (times[next] - times[next - 1]);
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let sign = if rotation && dot < 0.0 { -1.0 } else { 1.0 };
    let mixed: Vec<f64> = a
        .iter()
        .zip(b)
        .map(|(x, y)| x + (sign * y - x) * f)
        .collect();
    if !rotation {
        return mixed;
    }
    let length = mixed.iter().map(|c| c * c).sum::<f64>().sqrt();
    mixed.iter().map(|c| c / length).collect()
}

/// The first channel whose sampled values differ, as a message.
fn compare_channels(ours: &[Channel], theirs: &[Channel]) -> Option<String> {
    for (node, path, times, values) in theirs {
        let Some((_, _, our_times, ours)) = ours.iter().find(|c| (&c.0, &c.1) == (node, path))
        else {
            return Some(format!("no {path} channel of {node}"));
        };
        let rotation = path == "rotation";
        for &t in times.iter().chain(our_times) {
            let (a, b) = (
                sample(our_times, ours, t, rotation),
                sample(times, values, t, rotation),
            );
            let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            let sign = if rotation && dot < 0.0 { -1.0 } else { 1.0 };
            let far = |x: f64, y: f64| (x - y).abs() > CLIP_TOLERANCE + CLIP_RELATIVE * y.abs();
            if a.iter().zip(&b).any(|(x, y)| far(*x, sign * y)) {
                return Some(format!("{node} {path} at {t:.4}: {a:?} vs {b:?}"));
            }
        }
    }
    None
}

/// The manifest's clip durations by (model output, glTF animation name).
fn manifest_durations() -> Vec<(String, String, f64)> {
    let text = manifest();
    let outputs: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            let w: Vec<&str> = l.split('#').next()?.split_whitespace().collect();
            (w.first() == Some(&"model")).then(|| (w[1].to_owned(), w[3].to_owned()))
        })
        .collect();
    text.lines()
        .filter_map(|l| {
            let w: Vec<&str> = l.split('#').next()?.split_whitespace().collect();
            if w.first() != Some(&"clip") {
                return None;
            }
            let out = &outputs.iter().find(|(logical, _)| logical == w[1])?.1;
            Some((out.clone(), w[3].to_owned(), w[4].parse().ok()?))
        })
        .collect()
}

/// How clip `ours` (its channels and what `export_model` said of it) differs from `theirs` and
/// from the duration the manifest lists. The number of keys may differ: on a root bone under
/// root motion Source2Viewer decomposed a matrix, whose rounding breaks runs of equal values.
fn compare_clip(
    ours: (&[Channel], Option<&ExportedClip>),
    theirs: &[Channel],
    listed: Option<f64>,
) -> Vec<String> {
    let end = |c: &[Channel]| c.iter().map(|c| *c.2.last().unwrap()).fold(0.0, f64::max);
    let (channels, exported) = ours;
    let mut wrong = Vec::new();
    let said = exported.map_or(-1.0, |c| f64::from(c.duration));
    // Ours lasts to the clip's last frame, as Dota plays it; the exporter's stops at its last
    // key, so a pose held to the end makes ours the longer, never the shorter. The values
    // sampled (below) must still agree, the exporter's held past its end.
    let longer = |a: f64, b: f64| a - b > 1e-4;
    if longer(end(theirs), end(channels)) || (said - end(channels)).abs() > 1e-4 {
        wrong.push(format!(
            "lasts {} ({said} said) vs {}",
            end(channels),
            end(theirs)
        ));
    }
    if listed.is_some_and(|d| (end(channels) - d).abs() > 6e-4) {
        wrong.push(format!("lasts {} vs {listed:?} listed", end(channels)));
    }
    wrong.extend(compare_channels(channels, theirs));
    wrong
}

/// Every animated model's clips equal the exported ones: the same names in the same order,
/// the same durations (to the manifest's too), and every bone's rotation, translation and
/// scale sampled at every key time of either within `CLIP_TOLERANCE`.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn clips_match_the_exported_ones() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let durations = manifest_durations();
    let mut wrong = Vec::new();
    let mut clips = 0;
    for line in models().into_iter().filter(|l| !l.anims.is_empty()) {
        let Some(reference) = common::exported(&line.out) else {
            continue;
        };
        let exported = line.export(&src, &pak);
        let ours = animations(&Glb::parse(&exported.glb));
        let theirs = animations(&Glb::parse(&std::fs::read(reference).unwrap()));
        let names = |list: &[(String, Vec<Channel>)]| -> Vec<String> {
            list.iter().map(|a| a.0.clone()).collect()
        };
        // The manifest's clip lists moved on after the reference was exported (clips added and
        // renamed): only the clips both have are compared, and the reference's that the list
        // still names must be made.
        let listed_still: Vec<String> = names(&theirs)
            .into_iter()
            .filter(|n| line.anims.contains(n))
            .collect();
        let missing: Vec<&String> = listed_still
            .iter()
            .filter(|n| !names(&ours).contains(n))
            .collect();
        if !missing.is_empty() {
            wrong.push(format!("{}: clips {missing:?} not made", line.source));
            continue;
        }
        let both = ours
            .iter()
            .filter_map(|o| theirs.iter().find(|t| t.0 == o.0).map(|t| (o, t)));
        for ((name, ours), (_, theirs)) in both {
            clips += 1;
            let ours = (
                ours.as_slice(),
                exported.clips.iter().find(|c| &c.name == name),
            );
            let listed = durations
                .iter()
                .find(|(out, clip, _)| (out, clip) == (&line.out, name))
                .map(|d| d.2);
            for diff in compare_clip(ours, theirs, listed) {
                wrong.push(format!("{} {name}: {diff}", line.source));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} clips differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(clips >= 104, "only {clips} clips compared");
}
