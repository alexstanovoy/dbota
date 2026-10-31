//! Golden comparison of the world files this crate writes with the ones in the reference cache
//! Source2Viewer exported (see `common`). Ignored by default, like `golden.rs`:
//! `cargo test --release -p dbota_import --test world -- --ignored`.

mod common;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use dbota_import::import::export_cache_world;
use dbota_import::text::{buildings_txt, trees_txt};
use heatel::material::Pack;
use heatel::texture::PixelFormat;
use heatel::world::{Band, EntityLump};
use heatel::{DirSource, Resource, SliceSource, Source, Texture, Vpk, find_game_dir, read_from};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn cache() -> PathBuf {
    common::reference()
}

/// The manifest's `world` line: (source, output, its `key=value` fields).
fn world_line() -> (String, String, Vec<(String, String)>) {
    let text = std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("world "))
        .expect("no world line");
    let words: Vec<&str> = line.split_whitespace().collect();
    let fields = words[4..]
        .iter()
        .filter_map(|w| w.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    (words[2].to_owned(), words[3].to_owned(), fields)
}

/// The entities of the world's first (default) lump.
fn entities(src: &DirSource, map: &Vpk) -> EntityLump {
    let (source, _, _) = world_line();
    let world = read_from(src, &[map], &source).unwrap();
    let world = Resource::parse(&world).unwrap().data().unwrap().unwrap();
    let lump = world.get("m_entityLumps").unwrap().as_array().unwrap()[0]
        .as_str()
        .unwrap()
        .to_owned();
    let bytes = read_from(src, &[map], &format!("{lump}_c")).unwrap();
    EntityLump::read(&Resource::parse(&bytes).unwrap()).unwrap()
}

/// The placements of `ours` agree with the reference's, which differ only in float formatting
/// (`-0.0` for `0.0`): the reference was exported before the buildings the client spawns as units
/// (towers, barracks) left the file and the Watchers, Outposts and Shrines of Wisdom joined it, so
/// a line of ours whose model the reference places must be one of the reference's lines.
fn same_placements(ours: &str, theirs: &str) -> Result<(), String> {
    let words = |l: &str| {
        l.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<String>>()
    };
    let theirs: Vec<Vec<String>> = theirs
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(words)
        .collect();
    // (The writer gave each line more columns since: the reference's are compared.)
    let same = |p: &[String], q: &[String]| {
        let p = &p[..p.len().min(q.len())];
        p.len() == q.len()
            && p.iter()
                .zip(q)
                .all(|(u, v)| match (u.parse::<f64>(), v.parse::<f64>()) {
                    (Ok(u), Ok(v)) => u == v,
                    _ => u == v,
                })
    };
    for line in ours.lines().filter(|l| !l.starts_with('#')) {
        let x = words(line);
        let known = theirs.iter().any(|y| y.first() == x.first());
        if known && !theirs.iter().any(|y| same(&x, y)) {
            return Err(format!("not among the reference's lines: {line}"));
        }
    }
    Ok(())
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn trees_and_buildings_match_the_exported_ones() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let lump = entities(&src, &map);
    let (_, out, fields) = world_line();
    let trees = fields.iter().find(|(k, _)| k == "trees").unwrap();
    let dir = cache().join(&out).parent().unwrap().to_owned();
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
    same_placements(&trees_txt(&lump.entities), &read(cache().join(&trees.1))).unwrap();
    same_placements(
        &buildings_txt(&lump.entities),
        &read(dir.join("buildings.txt")),
    )
    .unwrap();
}

/// The texture stems (`x_psd_1234abcd`) reachable from `roots` through external references
/// (model → material → texture), read from `packages`.
fn texture_closure(src: &DirSource, packages: &[&Vpk], roots: &[String]) -> BTreeSet<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = roots.to_vec();
    let mut textures = BTreeSet::new();
    while let Some(path) = stack.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(stem) = path.strip_suffix(".vtex") {
            textures.insert(stem.rsplit('/').next().unwrap().to_owned());
            continue;
        }
        let Ok(bytes) = read_from(src, packages, &format!("{path}_c")) else {
            continue;
        };
        let refs = Resource::parse(&bytes).unwrap().external_refs().unwrap();
        stack.extend(refs.into_iter().map(|r| r.name));
    }
    textures
}

/// Every image of the exported world comes from a texture the world reaches: through its nodes'
/// references, or through the models its entities place (shops, outposts, banners, lanterns...).
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn world_images_come_from_its_nodes_and_entities() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let (source, out, _) = world_line();
    let world = read_from(&src, &[&map], &source).unwrap();
    let world = Resource::parse(&world).unwrap().data().unwrap().unwrap();
    let nodes = world.get("m_worldNodes").unwrap().as_array().unwrap();
    let mut roots: Vec<String> = nodes
        .iter()
        .map(|n| {
            let prefix = n.get("m_worldNodePrefix").unwrap().as_str().unwrap();
            format!("{}.vwnod", prefix.replace('\\', "/"))
        })
        .collect();
    let nodes_only = texture_closure(&src, &[&map, &pak], &roots);
    let lump = entities(&src, &map);
    roots.extend(lump.models().iter().map(|m| (*m).to_owned()));
    let all = texture_closure(&src, &[&map, &pak], &roots);
    let dir = cache().join(&out).parent().unwrap().to_owned();
    let mut missing = Vec::new();
    let mut from_entities = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let Some(stem) = name.strip_suffix(".png") else {
            continue;
        };
        // Images Source2Viewer made from one: `<texture>_orm_<hash>`.
        let stem = stem.split("_orm_").next().unwrap();
        if !all.contains(stem) {
            missing.push(name);
        } else if !nodes_only.contains(stem) {
            from_entities += 1;
        }
    }
    println!(
        "{} textures reached, {from_entities} images only through entities",
        all.len()
    );
    assert!(
        missing.is_empty(),
        "{} images from no texture: {missing:?}",
        missing.len()
    );
    assert!(from_entities > 0);
}

/// A glTF document and its one buffer.
struct Gltf {
    json: serde_json::Value,
    bin: Vec<u8>,
}

impl Gltf {
    /// Accessor `i` as numbers, tightly packed.
    fn values(&self, i: &serde_json::Value) -> Vec<f64> {
        let a = &self.json["accessors"][i.as_u64().unwrap() as usize];
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
            other => panic!("component type {other}"),
        };
        let stride = view["byteStride"]
            .as_u64()
            .map_or(size * width, |s| s as usize);
        let count = a["count"].as_u64().unwrap() as usize;
        (0..count)
            .flat_map(|v| (0..width).map(move |c| start + v * stride + c * size))
            .map(|at| read(&self.bin[at..at + size]))
            .collect()
    }

    /// The image file of texture slot `slot` (`["normalTexture"]`) of material `m`, without
    /// the hash Source2Viewer gave the images it made up (`x_orm_123.png` is `x_orm.png`).
    fn image(&self, m: &serde_json::Value, slot: &[&str]) -> Option<String> {
        let texture = slot.iter().fold(m, |v, k| &v[*k])["index"].as_u64()? as usize;
        let source = self.json["textures"][texture]["source"].as_u64()? as usize;
        let uri = self.json["images"][source]["uri"].as_str()?;
        Some(match uri.split_once("_orm_") {
            Some((stem, _)) => format!("{stem}_orm.png"),
            None => uri.to_owned(),
        })
    }

    /// What a material means: its names, modes and images by slot as a line, and its factors.
    fn material(&self, index: Option<&serde_json::Value>) -> (String, Vec<f64>) {
        let Some(m) = index.map(|i| &self.json["materials"][i.as_u64().unwrap() as usize]) else {
            return ("none".to_owned(), Vec::new());
        };
        let pbr = &m["pbrMetallicRoughness"];
        let numbers = |v: &serde_json::Value, default: &[f64]| -> Vec<f64> {
            v.as_array().map_or_else(
                || default.to_vec(),
                |a| a.iter().map(|c| c.as_f64().unwrap()).collect(),
            )
        };
        let specular = &m["extensions"]["KHR_materials_specular"];
        let mask = m["alphaMode"].as_str() == Some("MASK");
        let mut factors = numbers(&pbr["baseColorFactor"], &[1.0; 4]);
        factors.push(pbr["metallicFactor"].as_f64().unwrap_or(1.0));
        factors.push(pbr["roughnessFactor"].as_f64().unwrap_or(1.0));
        factors.push(if mask {
            m["alphaCutoff"].as_f64().unwrap_or(0.5)
        } else {
            0.0
        });
        if !specular.is_null() {
            factors.extend(numbers(&specular["specularColorFactor"], &[1.0; 3]));
        }
        let line = format!(
            "{} {} alpha {} double {} unlit {} specular {} {:?} | base {:?} mr {:?} normal {:?} occlusion {:?} emissive {:?}",
            m["name"],
            m["extras"]["vmat"]["Name"],
            m["alphaMode"].as_str().unwrap_or("OPAQUE"),
            m["doubleSided"].as_bool().unwrap_or(false),
            !m["extensions"]["KHR_materials_unlit"].is_null(),
            !specular.is_null(),
            self.image(
                m,
                &["extensions", "KHR_materials_specular", "specularTexture"]
            ),
            self.image(m, &["pbrMetallicRoughness", "baseColorTexture"]),
            self.image(m, &["pbrMetallicRoughness", "metallicRoughnessTexture"]),
            self.image(m, &["normalTexture"]),
            self.image(m, &["occlusionTexture"]),
            self.image(m, &["emissiveTexture"]),
        );
        (line, factors)
    }
}

/// How far value `k` of an attribute may differ from `theirs`: positions and unit vectors by
/// the rounding of the old exporter's frame change (its rotation is a float quaternion a few
/// 1e-8 off, so a vertex 260 m from the origin moves by up to 1e-4 m), the rest not at all.
fn tolerance(attribute: &str, theirs: &[f64], k: usize) -> f64 {
    match attribute {
        "POSITION" => {
            let vertex = &theirs[k / 3 * 3..k / 3 * 3 + 3];
            1e-5 + 1e-6 * vertex.iter().fold(0.0, |m: f64, c| m.max(c.abs()))
        }
        "NORMAL" | "TANGENT" => 1e-6,
        _ => 0.0,
    }
}

/// Differences between two documents, as lines (at most `limit` per kind).
fn differences(ours: &Gltf, theirs: &Gltf) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut kinds: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut report = |kind: &'static str, line: String| {
        let n = kinds.entry(kind).or_default();
        *n += 1;
        if *n <= 5 {
            out.push(format!("{kind}: {line}"));
        }
    };
    // The reference was exported before the cache's world left out what the client spawns as
    // units (Watchers, Outposts, Shrines of Wisdom): those nodes are not asked for.
    let a = ours.json["nodes"].as_array().unwrap();
    let b: Vec<serde_json::Value> = theirs.json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| {
            let name = n["name"].as_str().unwrap_or_default();
            !dbota_import::import::NOT_IN_WORLD
                .iter()
                .any(|p| name.starts_with(p))
        })
        .cloned()
        .collect();
    if a.len() != b.len() {
        report("node count", format!("{} vs {}", a.len(), b.len()));
    }
    let mut seen = HashSet::new();
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        compare_node(ours, theirs, i, (x, y), &mut seen, &mut report);
    }
    for (kind, n) in kinds {
        out.push(format!("{kind}: {n} in all"));
    }
    out
}

/// Reports how node `i` of ours differs from theirs, and its mesh the first time it is seen.
fn compare_node(
    ours: &Gltf,
    theirs: &Gltf,
    i: usize,
    (x, y): (&serde_json::Value, &serde_json::Value),
    seen: &mut HashSet<u64>,
    report: &mut dyn FnMut(&'static str, String),
) {
    if x["name"] != y["name"] {
        report("node name", format!("#{i} {} vs {}", x["name"], y["name"]));
        return;
    }
    let far = x["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .zip(y["matrix"].as_array().unwrap())
        .enumerate()
        .any(|(k, (p, q))| {
            let limit = if (12..15).contains(&k) { 1e-4 } else { 1e-5 };
            (p.as_f64().unwrap() - q.as_f64().unwrap()).abs() > limit
        });
    if far {
        report(
            "node matrix",
            format!("#{i} {}: {} vs {}", x["name"], x["matrix"], y["matrix"]),
        );
    }
    let (mx, my) = (x["mesh"].as_u64().unwrap(), y["mesh"].as_u64().unwrap());
    if !seen.insert(mx) {
        return;
    }
    let (mx, my) = (
        &ours.json["meshes"][mx as usize],
        &theirs.json["meshes"][my as usize],
    );
    if mx["name"] != my["name"] {
        report(
            "mesh name",
            format!("#{i} {} vs {}", mx["name"], my["name"]),
        );
        return;
    }
    let (pa, pb) = (
        mx["primitives"].as_array().unwrap(),
        my["primitives"].as_array().unwrap(),
    );
    if pa.len() != pb.len() {
        report(
            "primitive count",
            format!("{} {} vs {}", mx["name"], pa.len(), pb.len()),
        );
        return;
    }
    for (p, q) in pa.iter().zip(pb) {
        compare_primitive(ours, theirs, mx["name"].as_str().unwrap(), p, q, report);
    }
}

/// Reports how primitive `p` of ours differs from `q` of theirs (of mesh `at`).
fn compare_primitive(
    ours: &Gltf,
    theirs: &Gltf,
    at: &str,
    p: &serde_json::Value,
    q: &serde_json::Value,
    report: &mut dyn FnMut(&'static str, String),
) {
    let (m, n) = (
        ours.material(p.get("material")),
        theirs.material(q.get("material")),
    );
    let factors_close =
        m.1.len() == n.1.len() && m.1.iter().zip(&n.1).all(|(x, y)| (x - y).abs() <= 1e-5);
    if m.0 != n.0 || !factors_close {
        report("material", format!("{at}: {m:?}\n   vs {n:?}"));
    }
    let keys = |p: &serde_json::Value| -> Vec<String> {
        let mut k: Vec<String> = p["attributes"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    };
    if keys(p) != keys(q) {
        report(
            "attributes",
            format!("{at}: {:?} vs {:?}", keys(p), keys(q)),
        );
        return;
    }
    let mut streams: Vec<(String, &serde_json::Value, &serde_json::Value)> = keys(p)
        .into_iter()
        .map(|k| (k.clone(), &p["attributes"][&k], &q["attributes"][&k]))
        .collect();
    streams.push(("indices".to_owned(), &p["indices"], &q["indices"]));
    for (name, s, t) in streams {
        let (u, v) = (ours.values(s), theirs.values(t));
        if u.len() != v.len() {
            report(
                "value count",
                format!("{at} {name}: {} vs {}", u.len(), v.len()),
            );
            continue;
        }
        if let Some(k) = (0..u.len()).find(|&k| (u[k] - v[k]).abs() > tolerance(&name, &v, k)) {
            report("values", format!("{at} {name}[{k}]: {} vs {}", u[k], v[k]));
        }
    }
}

/// The band of the manifest's `world` line.
fn band(fields: &[(String, String)]) -> Band {
    let get = |key: &str| -> f64 {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .unwrap()
            .1
            .parse()
            .unwrap()
    };
    Band {
        half_width: get("half_width"),
        v_min: get("v_min"),
        v_max: get("v_max"),
    }
}

/// The image file of texture `vtex`: its file stem, `.png`.
fn png_name(vtex: &str) -> String {
    let name = vtex.rsplit('/').next().unwrap_or(vtex);
    format!("{}.png", name.trim_end_matches(".vtex"))
}

/// The cropped world written here equals the exported one: every node (name, matrix, mesh),
/// every primitive's vertex data and indices, and what each material means.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn world_matches_the_exported_one() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let (source, out, fields) = world_line();
    let started = std::time::Instant::now();
    let world = export_cache_world(
        &src,
        &[&map, &pak],
        &source,
        &band(&fields),
        "mid.bin",
        &png_name,
    )
    .unwrap();
    println!(
        "exported in {:.1} s: {} bytes of JSON, {} of geometry, {} images, {} layers",
        started.elapsed().as_secs_f64(),
        world.gltf.len(),
        world.bin.len(),
        world.textures.len(),
        world.layers.len()
    );
    let ours = Gltf {
        json: serde_json::from_slice(&world.gltf).unwrap(),
        bin: world.bin,
    };
    let path = cache().join(&out);
    let theirs = Gltf {
        json: serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
        bin: std::fs::read(path.with_extension("bin")).unwrap(),
    };
    let diff = differences(&ours, &theirs);
    assert!(diff.is_empty(), "{}", diff.join("\n"));
}

/// A PNG's size and pixels as RGBA8.
fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buf).unwrap();
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!("expanded by the decoder"),
    };
    (frame.width, frame.height, rgba)
}

/// The largest and the mean difference of two images' bytes.
fn distance(a: &[u8], b: &[u8]) -> (u8, f64) {
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    (max, sum as f64 / a.len().max(1) as f64)
}

/// How far a texture's pixels decoded here may be from the exported image (largest and mean
/// difference), by how the file stores them (its `DATA` byte 26): plain texels exactly, blocks
/// by one level (decoders round interpolated colours differently), DXT5 normals by what one
/// level of x or y does to the z made from them near the unit circle.
fn image_tolerance(vtex: &[u8], decoded: PixelFormat) -> (u8, f64) {
    let stored = Resource::parse(vtex).unwrap().block(b"DATA").unwrap()[26];
    match (stored, decoded) {
        (4 | 16 | 18 | 28, _) => (0, 0.0),
        (1 | 2, PixelFormat::Bc1 | PixelFormat::Bc3) => (1, 1.0),
        (2, PixelFormat::Rgba8) => (20, 1.0),
        other => panic!("no tolerance for {other:?}"),
    }
}

/// Compares the image made of texture `vtex` (packed by `pack`) with the exported `png`. The
/// exporter wrote a material's constant mask as one pixel: that pixel stands for all.
fn compare_image(
    src: &DirSource,
    pak: &Vpk,
    vtex: &str,
    pack: Option<Pack>,
    png: &Path,
) -> Result<(), String> {
    let bytes = read_from(src, &[pak], &format!("{vtex}_c")).unwrap();
    let texture = Texture::parse(&bytes).unwrap();
    let mut ours = texture.to_rgba8().map_err(|e| format!("{vtex}: {e}"))?;
    if let Some(pack) = pack {
        pack.apply(&mut ours);
    }
    let (w, h, mut theirs) = read_png(png);
    if (w, h) == (1, 1) && texture.width() * texture.height() > 1 {
        theirs = theirs.repeat(ours.len() / 4);
    } else if (w, h) != (texture.width(), texture.height()) {
        return Err(format!(
            "{vtex}: {}x{} vs {w}x{h}",
            texture.width(),
            texture.height()
        ));
    }

    let (max, mean) = distance(&ours, &theirs);
    let (max_ok, mean_ok) = image_tolerance(&bytes, texture.format());
    if max > max_ok || mean > mean_ok {
        return Err(format!(
            "{vtex} ({pack:?}): differs by {max} at most, {mean:.3} on average"
        ));
    }
    Ok(())
}

/// The exported world's images are the ones made here, pixel for pixel (within the decoders'
/// rounding); its terrain and water layer textures are the ones listed here.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn world_images_and_layers_match_the_exported_ones() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let (source, out, fields) = world_line();
    let world = export_cache_world(
        &src,
        &[&map, &pak],
        &source,
        &band(&fields),
        "mid.bin",
        &png_name,
    )
    .unwrap();
    let dir = cache().join(&out).parent().unwrap().to_owned();
    let mut exported: std::collections::HashMap<String, PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .map(|p| {
            let name = p.file_name().unwrap().to_str().unwrap().to_owned();
            let name = match name.split_once("_orm_") {
                Some((stem, _)) => format!("{stem}_orm.png"),
                None => name,
            };
            (name, p)
        })
        .collect();
    let mut failed = Vec::new();
    for t in &world.textures {
        let Some(png) = exported.remove(&t.uri) else {
            failed.push(format!("{}: not exported", t.uri));
            continue;
        };
        if let Err(why) = compare_image(&src, &pak, &t.vtex, t.pack, &png) {
            failed.push(why);
        }
    }
    // (The textures of the nodes the cache's world leaves out are not asked for.)
    let extra: Vec<&String> = exported
        .keys()
        .filter(|name| {
            !dbota_import::import::NOT_IN_WORLD
                .iter()
                .any(|p| name.starts_with(p))
        })
        .collect();
    assert!(extra.is_empty(), "exported images not made here: {extra:?}");
    let ground = cache().join(&fields.iter().find(|(k, _)| k == "layers").unwrap().1);
    compare_layers(&src, &pak, &world.layers, &ground, &mut failed);
    println!(
        "{} images and {} layers compared",
        world.textures.len(),
        world.layers.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Compares the layer textures `layers_made` with the exported `ground` folder, which also
/// holds the manifest's own ground textures.
fn compare_layers(
    src: &DirSource,
    pak: &Vpk,
    layers_made: &[String],
    ground: &Path,
    failed: &mut Vec<String>,
) {
    let listed: HashSet<String> =
        std::fs::read_to_string(repo().join("assets-manifest/manifest.txt"))
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("texture "))
            .filter_map(|l| l.split_whitespace().nth(3).map(str::to_owned))
            .collect();
    let mut layers: BTreeSet<String> = std::fs::read_dir(ground)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    for layer in layers_made {
        let name = png_name(layer);
        if !layers.remove(&name) {
            failed.push(format!("layer {layer}: not exported"));
        } else if let Err(why) = compare_image(src, pak, layer, None, &ground.join(&name)) {
            failed.push(why);
        }
    }
    let unlisted: Vec<&String> = layers
        .iter()
        .filter(|name| !listed.iter().any(|out| out.ends_with(&format!("/{name}"))))
        .collect();
    assert!(
        unlisted.is_empty(),
        "exported layers not listed here: {unlisted:?}"
    );
}

/// Writes the world made here into a copy of the asset cache, for `tools/visual_check.sh`:
/// `map/mid/` (the glTF, its geometry and images, trees and buildings) and the layer textures
/// of `textures/ground/`, all from the game's files.
///
/// ```sh
/// cp -rs ~/.cache/dbota/v6/. /path/to/copy   # a farm of symlinks, nothing is duplicated
/// DBOTA_WORLD_CACHE=/path/to/copy cargo test --release -p dbota_import --test world \
///     -- --ignored write_world_into_a_cache_copy
/// DBOTA_ASSETS=/path/to/copy tools/visual_check.sh replay.brp
/// ```
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache; writes only with DBOTA_WORLD_CACHE"]
fn write_world_into_a_cache_copy() {
    let Some(copy) = std::env::var_os("DBOTA_WORLD_CACHE").map(PathBuf::from) else {
        println!("DBOTA_WORLD_CACHE is not set: nothing to write");
        return;
    };
    assert_ne!(
        copy.canonicalize().unwrap(),
        cache().canonicalize().unwrap(),
        "DBOTA_WORLD_CACHE must be a copy of the cache"
    );
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let (source, out, fields) = world_line();
    let world = export_cache_world(
        &src,
        &[&map, &pak],
        &source,
        &band(&fields),
        "mid.bin",
        &png_name,
    )
    .unwrap();
    let target = copy.join(&out);
    let dir = target.parent().unwrap().to_owned();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let ours = ["png", "gltf", "bin", "txt"];
        if path
            .extension()
            .is_some_and(|e| ours.iter().any(|o| e == *o))
        {
            std::fs::remove_file(&path).unwrap(); // a link into the original cache
        }
    }
    std::fs::write(&target, &world.gltf).unwrap();
    std::fs::write(dir.join("mid.bin"), &world.bin).unwrap();
    let lump = entities(&src, &map);
    std::fs::write(dir.join("trees.txt"), trees_txt(&lump.entities)).unwrap();
    std::fs::write(dir.join("buildings.txt"), buildings_txt(&lump.entities)).unwrap();
    for t in &world.textures {
        write_image(&src, &pak, &t.vtex, t.pack, &dir.join(&t.uri));
    }
    let ground = copy.join(&fields.iter().find(|(k, _)| k == "layers").unwrap().1);
    for layer in &world.layers {
        let target = ground.join(png_name(layer));
        let _ = std::fs::remove_file(&target); // a link into the original cache
        write_image(&src, &pak, layer, None, &target);
    }
    println!(
        "{} images and {} layers written",
        world.textures.len(),
        world.layers.len()
    );
}

/// Writes texture `vtex`, packed by `pack`, as a PNG.
fn write_image(src: &DirSource, pak: &Vpk, vtex: &str, pack: Option<Pack>, target: &Path) {
    let bytes = read_from(src, &[pak], &format!("{vtex}_c")).unwrap();
    let texture = Texture::parse(&bytes).unwrap();
    let mut pixels = texture.to_rgba8().unwrap();
    if let Some(pack) = pack {
        pack.apply(&mut pixels);
    }
    let png = Texture::from_rgba8(texture.width(), texture.height(), pixels)
        .unwrap()
        .to_png()
        .unwrap();
    std::fs::write(target, png).unwrap();
}

/// The install's files, and a package of replacement files read before its own.
struct Patched {
    install: DirSource,
    patch: SliceSource<Vec<u8>>,
}

impl Source for Patched {
    fn size(&self, path: &str) -> heatel::Result<u64> {
        match path {
            PATCH => self.patch.size(path),
            _ => self.install.size(path),
        }
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> heatel::Result<Vec<u8>> {
        match path {
            PATCH => self.patch.read_at(path, offset, len),
            _ => self.install.read_at(path, offset, len),
        }
    }
}

const PATCH: &str = "patch_dir.vpk";

/// A package (VPK 1) holding `bytes` as file `path`, in its directory file.
fn package(path: &str, bytes: &[u8]) -> Vec<u8> {
    let (dir, file) = path.rsplit_once('/').unwrap();
    let (name, ext) = file.rsplit_once('.').unwrap();
    let mut tree = Vec::new();
    for s in [ext, dir, name] {
        tree.extend_from_slice(s.as_bytes());
        tree.push(0);
    }
    tree.extend_from_slice(&crc32fast::hash(bytes).to_le_bytes());
    tree.extend_from_slice(&0u16.to_le_bytes()); // preload
    tree.extend_from_slice(&0x7FFFu16.to_le_bytes()); // in the directory file
    tree.extend_from_slice(&0u32.to_le_bytes());
    tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
    tree.extend_from_slice(&[0, 0, 0]);
    let mut out = Vec::new();
    for word in [0x55AA_1234u32, 1, tree.len() as u32] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&tree);
    out.extend_from_slice(bytes);
    out
}

/// A small deterministic generator for the damage below.
struct XorShift(u64);

impl XorShift {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// The world's own files and one of each kind it places, damaged (bits flipped where blocks
/// start, or cut short), make the export fail or succeed, never panic.
#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn a_damaged_world_is_an_error_not_a_panic() {
    let install = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let pak = Vpk::open(&install, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&install, "maps/dota.vpk").unwrap();
    let (source, _, fields) = world_line();
    let band = band(&fields);
    let targets = [
        source.as_str(),
        "maps/dota/worldnodes/n0.vwnod_c",
        "maps/dota/entities/default_ents.vents_c",
        "maps/dota/worldnodes/n0_lr0_c0_s_mesh_overlay0.vmdl_c",
        "maps/dota/worldnodes/n0_lr0_agg_prop_ancient_giant_skeleton_0.vmdl_c",
        "materials/blends/mod_radiant_000.vmat_c",
    ];
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    let (mut ok, mut failed) = (0, 0);
    for target in targets {
        let bytes = read_from(&install, &[&map, &pak], target).unwrap();
        let starts: Vec<usize> = Resource::parse(&bytes)
            .unwrap()
            .blocks()
            .iter()
            .map(|b| b.offset as usize)
            .collect();
        for round in 0..8 {
            let damaged = if round % 4 == 3 {
                bytes[..rng.below(bytes.len())].to_vec()
            } else {
                let mut damaged = bytes.clone();
                let at = (starts[rng.below(starts.len())] + rng.below(256)).min(bytes.len() - 1);
                damaged[at] ^= 1 << rng.below(8);
                damaged
            };
            let mut patch = SliceSource::default();
            patch.insert(PATCH, package(target, &damaged));
            let src = Patched {
                install: DirSource::new(find_game_dir(None).unwrap()),
                patch,
            };
            let patched = Vpk::open(&src, PATCH).unwrap();
            match export_cache_world(
                &src,
                &[&patched, &map, &pak],
                &source,
                &band,
                "mid.bin",
                &png_name,
            ) {
                Ok(_) => ok += 1,
                Err(_) => failed += 1,
            }
        }
    }
    println!("damaged worlds: {ok} exported, {failed} errors");
    assert!(failed > 0);
}
