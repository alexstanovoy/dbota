//! Locates exported Dota assets in the cache and generates stand-ins when they are missing,
//! so the client runs (and CI can screenshot it) without any Valve data.

use std::path::{Path, PathBuf};

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::CACHE_SOURCE;

/// Bound on directory entries visited while scanning the cache.
const MAX_SCAN_ENTRIES: usize = 200_000;
const ICON_SIZE: u32 = 64;

/// Where each spike asset comes from. Cache assets are `cache://<relative>` asset paths;
/// generated stand-ins are absolute filesystem paths.
#[derive(Resource, Debug, Clone)]
pub struct FoundAssets {
    pub ground: Option<String>,
    pub model: String,
    pub model_is_fallback: bool,
    /// Absolute path: loaded through the unapproved-path override on purpose (see `hud`).
    pub icon: PathBuf,
    pub sound: Option<String>,
}

/// Runs before LogPlugin exists, so the result is logged later by `log_found`.
pub fn resolve(cache_dir: &Path, fallback_dir: &Path) -> FoundAssets {
    let files = scan(cache_dir);
    // Preference lists: the first group whose keys all occur in a path wins; an empty
    // group matches any file. Normal maps and physics hulls are never picked.
    let pick = |exts: &[&str], prefs: &[&[&str]]| -> Option<PathBuf> {
        let usable: Vec<(&PathBuf, String)> = files
            .iter()
            .map(|p| (p, p.to_string_lossy().to_ascii_lowercase()))
            .filter(|(p, s)| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                exts.contains(&ext.to_ascii_lowercase().as_str())
                    && !s.contains("_normal")
                    && !s.contains("_physics")
            })
            .collect();
        prefs.iter().find_map(|keys| {
            usable
                .iter()
                .find(|(_, s)| keys.iter().all(|k| s.contains(k)))
                .map(|(p, _)| (*p).clone())
        })
    };
    let cache_path = |p: PathBuf| format!("{CACHE_SOURCE}://{}", p.display());

    std::fs::create_dir_all(fallback_dir).expect("create fallback asset dir");
    let ground = pick(
        &["png"],
        &[&["ground/grass", "color"], &["ground/", "color"]],
    )
    .map(cache_path);
    let (model, model_is_fallback) = match pick(
        &["glb", "gltf"],
        &[&["hero/nevermore/nevermore."], &["models/hero/"]],
    ) {
        Some(p) => (cache_path(p), false),
        None => (write_fallback_glb(fallback_dir).display().to_string(), true),
    };
    let icon = pick(&["png"], &[&["spellicons/"], &["icons/"]])
        .map(|p| cache_dir.join(p))
        .unwrap_or_else(|| write_fallback_icon(fallback_dir));
    let sound = pick(&["wav", "ogg"], &[&["sounds/"], &[]]).map(cache_path);
    FoundAssets {
        ground,
        model,
        model_is_fallback,
        icon,
        sound,
    }
}

/// Cache-relative paths of all files under `root` (bounded walk; missing root is empty).
fn scan(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    let mut visited = 0usize;
    while let Some(rel) = stack.pop() {
        let Ok(dir) = std::fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for entry in dir.flatten() {
            visited += 1;
            if visited > MAX_SCAN_ENTRIES {
                warn!("cache scan truncated at {MAX_SCAN_ENTRIES} entries");
                return out;
            }
            let child = rel.join(entry.file_name());
            match entry.file_type() {
                Ok(t) if t.is_dir() && child != Path::new("tools") => stack.push(child),
                Ok(t) if t.is_file() => out.push(child),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// A 64x64 PNG "ability icon": diagonal gradient with a bright diamond.
fn write_fallback_icon(dir: &Path) -> PathBuf {
    let path = dir.join("icon.png");
    if path.exists() {
        return path;
    }
    let n = ICON_SIZE;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
            let diamond = (fx - 0.5).abs() + (fy - 0.5).abs() < 0.3;
            let (r, g, b) = if diamond {
                (250, 220, 120)
            } else {
                ((60.0 + 120.0 * fx) as u8, 40, (120.0 + 100.0 * fy) as u8)
            };
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let image = Image::new(
        Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    );
    image
        .try_into_dynamic()
        .expect("rgba8 converts to DynamicImage")
        .save(&path)
        .expect("write fallback icon");
    path
}

/// A node-animated stand-in hero (body + weapon on a shoulder pivot) with two named
/// clips: "idle" (body bob, 1.0 s) and "attack" (weapon swing, 0.8 s, impact at 0.4 s).
fn write_fallback_glb(dir: &Path) -> PathBuf {
    let path = dir.join("hero.glb");
    if path.exists() {
        return path;
    }
    std::fs::write(&path, fallback_glb_bytes()).expect("write fallback glb");
    path
}

/// Time of the weapon impact keyframe inside the fallback "attack" clip.
pub const FALLBACK_ATTACK_IMPACT: f32 = 0.4;

fn fallback_glb_bytes() -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    // Unit cube, flat-shaded: 6 faces x 4 vertices.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([-1., 0., 0.], [0., 0., 1.], [0., 1., 0.]),
        ([0., 1., 0.], [0., 0., 1.], [1., 0., 0.]),
        ([0., -1., 0.], [1., 0., 0.], [0., 0., 1.]),
        ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]),
        ([0., 0., -1.], [0., 1., 0.], [1., 0., 0.]),
    ];
    let corner = |n: [f32; 3], u: [f32; 3], v: [f32; 3], su: f32, sv: f32| -> [f32; 3] {
        std::array::from_fn(|i| 0.5 * n[i] + 0.5 * su * u[i] + 0.5 * sv * v[i])
    };
    let quad = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)];
    for (n, u, v) in faces {
        for (su, sv) in quad {
            put_f32(&mut bin, &corner(n, u, v, su, sv));
        }
    }
    for (n, _, _) in faces {
        for _ in quad {
            put_f32(&mut bin, &n);
        }
    }
    let mut indices: Vec<u16> = Vec::new();
    for f in 0..6u16 {
        let b = f * 4;
        indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    indices.iter().for_each(|i| bin.extend(i.to_le_bytes()));
    put_f32(&mut bin, &[0.0, 0.5, 1.0]);
    put_f32(&mut bin, &[0., 40., 0., 0., 46., 0., 0., 40., 0.]);
    put_f32(&mut bin, &[0.0, FALLBACK_ATTACK_IMPACT, 0.8]);
    let (s45, c45) = (
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    let (s15, c15) = (15f32.to_radians().sin(), 15f32.to_radians().cos());
    put_f32(
        &mut bin,
        &[-s45, 0., 0., c45, s15, 0., 0., c15, -s45, 0., 0., c45],
    );
    assert_eq!(bin.len(), 756);

    glb(fallback_gltf_json(bin.len()).into_bytes(), bin)
}

/// glTF JSON for the buffer laid out by `fallback_glb_bytes` (byte offsets must match).
fn fallback_gltf_json(len: usize) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0","generator":"dbota-fallback"}},
"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[
 {{"name":"hero","children":[1,2]}},
 {{"name":"body","mesh":0,"translation":[0,40,0],"scale":[48,80,48]}},
 {{"name":"weapon_pivot","translation":[30,60,0],"children":[3]}},
 {{"name":"weapon","mesh":1,"translation":[0,0,30],"scale":[8,8,60]}}],
"meshes":[
 {{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"indices":2,"material":0}}]}},
 {{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"indices":2,"material":1}}]}}],
"materials":[
 {{"pbrMetallicRoughness":{{"baseColorFactor":[0.55,0.15,0.12,1],"metallicFactor":0,"roughnessFactor":0.8}}}},
 {{"pbrMetallicRoughness":{{"baseColorFactor":[0.85,0.85,0.9,1],"metallicFactor":0.6,"roughnessFactor":0.4}}}}],
"buffers":[{{"byteLength":{len}}}],
"bufferViews":[
 {{"buffer":0,"byteOffset":0,"byteLength":288}},
 {{"buffer":0,"byteOffset":288,"byteLength":288}},
 {{"buffer":0,"byteOffset":576,"byteLength":72}},
 {{"buffer":0,"byteOffset":648,"byteLength":12}},
 {{"buffer":0,"byteOffset":660,"byteLength":36}},
 {{"buffer":0,"byteOffset":696,"byteLength":12}},
 {{"buffer":0,"byteOffset":708,"byteLength":48}}],
"accessors":[
 {{"bufferView":0,"componentType":5126,"count":24,"type":"VEC3","min":[-0.5,-0.5,-0.5],"max":[0.5,0.5,0.5]}},
 {{"bufferView":1,"componentType":5126,"count":24,"type":"VEC3"}},
 {{"bufferView":2,"componentType":5123,"count":36,"type":"SCALAR"}},
 {{"bufferView":3,"componentType":5126,"count":3,"type":"SCALAR","min":[0],"max":[1]}},
 {{"bufferView":4,"componentType":5126,"count":3,"type":"VEC3"}},
 {{"bufferView":5,"componentType":5126,"count":3,"type":"SCALAR","min":[0],"max":[0.8]}},
 {{"bufferView":6,"componentType":5126,"count":3,"type":"VEC4"}}],
"animations":[
 {{"name":"idle","samplers":[{{"input":3,"output":4,"interpolation":"LINEAR"}}],
   "channels":[{{"sampler":0,"target":{{"node":1,"path":"translation"}}}}]}},
 {{"name":"attack","samplers":[{{"input":5,"output":6,"interpolation":"LINEAR"}}],
   "channels":[{{"sampler":0,"target":{{"node":2,"path":"rotation"}}}}]}}]}}"#,
    )
}

fn put_f32(bin: &mut Vec<u8>, vals: &[f32]) {
    vals.iter().for_each(|v| bin.extend(v.to_le_bytes()));
}

/// Packs a glTF JSON document and its binary buffer into a GLB container.
fn glb(mut json: Vec<u8>, mut bin: Vec<u8>) -> Vec<u8> {
    json.resize(json.len().next_multiple_of(4), b' ');
    bin.resize(bin.len().next_multiple_of(4), 0);
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    let u32le = |v: usize| u32::try_from(v).expect("glb chunk fits u32").to_le_bytes();
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend(u32le(total));
    out.extend(u32le(json.len()));
    out.extend(b"JSON");
    out.extend(json);
    out.extend(u32le(bin.len()));
    out.extend(b"BIN\0");
    out.extend(bin);
    assert_eq!(out.len(), total);
    out
}

pub fn log_found(found: Res<FoundAssets>) {
    info!("assets: {found:?}");
}
