//! Golden tests: decode the Valve files behind the reference cache (exported by Source2Viewer
//! 20, see `common`) from the local Dota 2 install, and compare with it. They need both, so
//! they are ignored by default:
//!
//! ```sh
//! cargo test --release -p dbota_import -- --ignored
//! ```
//!
//! `DBOTA_DOTA_PATH` (a `dota 2 beta` folder) and `DBOTA_REFERENCE` (the exported cache) override
//! where they are looked for.

mod common;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use dbota_import::catalog::{FileKind, Line as Cl};
use heatel::anim::read_clips;
use heatel::kv3::{self, Value};
use heatel::{DirSource, Resource, SliceSource, Source, Vpk, find_game_dir};

struct Install {
    src: DirSource,
    pak: Vpk,
    map: Vpk,
    manifest: Vec<Line>,
}

/// A manifest line: its first four words, then the rest (`key=value` fields, a clip's duration).
struct Line {
    kind: String,
    name: String,
    src: String,
    out: String,
    rest: Vec<String>,
}

impl Line {
    fn field(&self, key: &str) -> Option<&str> {
        self.rest
            .iter()
            .find_map(|word| word.strip_prefix(key)?.strip_prefix('='))
    }
}

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn install() -> Install {
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref())
        .expect("Dota 2 not found; set DBOTA_DOTA_PATH to its \"dota 2 beta\" folder");
    let cache = common::reference();
    assert!(cache.is_dir(), "no asset cache at {}", cache.display());
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    Install {
        src,
        pak,
        map,
        manifest: manifest(),
    }
}

/// The committed manifest as the catalog reads it, in the golden tests' line shape.
fn manifest() -> Vec<Line> {
    let read =
        |name: &str| std::fs::read_to_string(repo().join("assets-manifest").join(name)).unwrap();
    let catalog = dbota_import::catalog::parse(&read("manifest.txt"), &read("ids.txt"))
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let line = |kind: &str, name: &str, src: &str, out: &str, rest: Vec<String>| Line {
        kind: kind.to_owned(),
        name: name.to_owned(),
        src: src.to_owned(),
        out: out.to_owned(),
        rest,
    };
    let list = |key: &str, values: &[String]| format!("{key}={}", values.join(","));
    catalog
        .lines
        .iter()
        .filter_map(|l| match l {
            Cl::Model(m) => {
                let mut rest = Vec::new();
                if !m.anims.is_empty() {
                    rest.push(list("anims", &m.anims));
                }
                if let Some(meshes) = &m.meshes {
                    rest.push(list("meshes", meshes));
                }
                Some(line("model", &m.logical, &m.src, &m.out, rest))
            }
            Cl::Clip(c) => Some(line(
                "clip",
                &c.model,
                &c.logical,
                &c.name,
                vec![c.duration.to_string()],
            )),
            Cl::Sound(s) => Some(line(
                "sound",
                &s.logical,
                &s.src,
                &s.out,
                vec![format!("event={}", s.event)],
            )),
            Cl::World(w) => Some(line(
                "world",
                &w.logical,
                &w.src,
                &w.out,
                vec![format!("vpk={}", w.vpk)],
            )),
            Cl::File(f) if f.kind == FileKind::Texture => {
                Some(line("texture", &f.logical, &f.src, &f.out, Vec::new()))
            }
            _ => None,
        })
        .collect()
}

impl Install {
    fn lines(&self, kind: &str) -> impl Iterator<Item = &Line> {
        self.manifest.iter().filter(move |l| l.kind == kind)
    }

    /// A compiled file from pak01, or else from the map's package.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        for vpk in [&self.pak, &self.map] {
            if vpk.get(path).is_some() {
                return Some(vpk.read_file(&self.src, path).unwrap());
            }
        }
        None
    }

    fn data(&self, path: &str) -> Value {
        let bytes = self
            .read(path)
            .unwrap_or_else(|| panic!("{path} is in no package"));
        let resource = Resource::parse(&bytes).unwrap();
        resource
            .data()
            .unwrap_or_else(|| panic!("{path}: no DATA tree"))
            .unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Every compiled file `roots` reference, transitively (`RERL`), with each one's references.
    fn closure(&self, roots: &[String]) -> Vec<(String, Vec<String>)> {
        let mut queue: VecDeque<String> = roots.iter().cloned().collect();
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        while let Some(path) = queue.pop_front() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let bytes = self
                .read(&path)
                .unwrap_or_else(|| panic!("{path} is in no package"));
            let resource = Resource::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
            let refs: Vec<String> = resource
                .external_refs()
                .unwrap()
                .into_iter()
                .map(|r| format!("{}_c", r.name))
                .collect();
            queue.extend(refs.iter().cloned());
            files.push((path, refs));
        }
        files
    }
}

#[test]
#[ignore = "needs a local Dota 2 install"]
fn pak01_directory_parses_fast_and_finds_every_entry_by_its_path() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let start = Instant::now();
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let elapsed = start.elapsed();
    println!("pak01_dir.vpk: {} entries in {elapsed:?}", pak.len());
    // The Python prototype took 0.61 s; Rust must not be slower.
    assert!(elapsed.as_secs_f64() < 0.6, "{elapsed:?}");
    assert!(pak.len() > 300_000, "{} entries", pak.len());
    for entry in pak.entries() {
        let found = pak.get(&entry.path()).unwrap();
        assert_eq!(
            (found.crc(), found.archive_index(), found.archive_offset()),
            (entry.crc(), entry.archive_index(), entry.archive_offset()),
            "{}",
            entry.path()
        );
    }
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn every_source_and_its_references_decode() {
    let dota = install();
    let mut roots: Vec<String> = dota
        .manifest
        .iter()
        .filter(|l| matches!(l.kind.as_str(), "model" | "texture" | "sound" | "world"))
        .map(|l| l.src.clone())
        .collect();
    let world = dota.lines("world").next().unwrap();
    assert_eq!(world.field("vpk"), Some("maps/dota.vpk"), "{}", world.name);
    roots.extend(world_files(&dota, &world.src));
    let files = dota.closure(&roots);
    let mut trees = 0;
    for (path, _) in &files {
        let bytes = dota.read(path).unwrap();
        let resource = Resource::parse(&bytes).unwrap();
        assert!(resource.file_size() as usize <= bytes.len(), "{path}");
        for block in resource.blocks() {
            let raw = resource.bytes(block);
            if kv3::is_kv3(raw) {
                kv3::decode(raw).unwrap_or_else(|e| panic!("{path} {:?}: {e}", block.kind));
            }
        }
        if let Some(tree) = resource.data() {
            tree.unwrap_or_else(|e| panic!("{path}: {e}"));
            trees += 1;
        }
    }
    println!("{} files, {trees} DATA trees", files.len());
    assert!(files.len() > roots.len());
}

/// The world, its nodes and its entity lumps.
fn world_files(dota: &Install, world: &str) -> Vec<String> {
    let data = dota.data(world);
    let nodes = data.get("m_worldNodes").and_then(Value::as_array).unwrap();
    let lumps = data.get("m_entityLumps").and_then(Value::as_array).unwrap();
    assert!(!nodes.is_empty() && !lumps.is_empty());
    let nodes = nodes.iter().map(|node| {
        let prefix = node.get("m_worldNodePrefix").and_then(Value::as_str);
        format!("{}.vwnod_c", prefix.unwrap())
    });
    let lumps = lumps
        .iter()
        .map(|lump| format!("{}_c", lump.as_str().unwrap()));
    let files: Vec<String> = std::iter::once(world.to_owned())
        .chain(nodes)
        .chain(lumps)
        .collect();
    for file in &files {
        assert!(
            dota.map.get(file).is_some(),
            "{file} is not in maps/dota.vpk"
        );
    }
    files
}

/// `(name, value)` pairs of a material's `m_<kind>Params` list; a repeated name: the last wins.
fn params<'a>(material: &'a Value, list: &str, value: &str) -> HashMap<&'a str, &'a Value> {
    material
        .get(list)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|p| {
            (
                p.get("m_name").and_then(Value::as_str).unwrap(),
                p.get(value).unwrap(),
            )
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5 * a.abs().max(1.0)
}

/// Every parameter Source2Viewer wrote into a material's `extras.vmat`, against the material
/// decoded here (KV3 for newer materials, NTRO for older ones).
fn compare_material(dota: &Install, exported: &serde_json::Value, from: &Path) {
    let name = exported["Name"].as_str().unwrap();
    let material = dota.data(&format!("{name}_c"));
    let at = format!("{name} (in {})", from.display());
    assert_eq!(
        material.get("m_shaderName").and_then(Value::as_str),
        exported["ShaderName"].as_str(),
        "{at}"
    );
    let ints = params(&material, "m_intParams", "m_nValue");
    for (key, want) in exported["IntParams"].as_object().unwrap() {
        assert_eq!(ints[key.as_str()].as_i64(), want.as_i64(), "{at} {key}");
    }
    let floats = params(&material, "m_floatParams", "m_flValue");
    for (key, want) in exported["FloatParams"].as_object().unwrap() {
        let got = floats[key.as_str()].as_f64().unwrap();
        assert!(
            close(got, want.as_f64().unwrap()),
            "{at} {key}: {got} vs {want}"
        );
    }
    let vectors = params(&material, "m_vectorParams", "m_value");
    for (key, want) in exported["VectorParams"].as_object().unwrap() {
        let got: Vec<f64> = vectors[key.as_str()]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let want: Vec<f64> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert!(
            got.len() == want.len() && got.iter().zip(&want).all(|(&a, &b)| close(a, b)),
            "{at} {key}: {got:?} vs {want:?}"
        );
    }
    let textures = params(&material, "m_textureParams", "m_pValue");
    for (key, want) in exported["TextureParams"].as_object().unwrap() {
        assert_eq!(textures[key.as_str()].as_str(), want.as_str(), "{at} {key}");
    }
}

/// The JSON of a `.glb` (its first chunk) or a `.gltf`.
fn gltf_json(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path).unwrap();
    let json = if path.extension().is_some_and(|e| e == "glb") {
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        &bytes[20..20 + len]
    } else {
        &bytes[..]
    };
    serde_json::from_slice(json).unwrap()
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn materials_match_the_exported_ones() {
    let dota = install();
    let mut compared = 0;
    let outputs = dota
        .manifest
        .iter()
        .filter(|l| matches!(l.kind.as_str(), "model" | "world"));
    for line in outputs {
        let Some(path) = common::exported(&line.out) else {
            continue;
        };
        let json = gltf_json(&path);
        for material in json["materials"].as_array().unwrap() {
            compare_material(&dota, &material["extras"]["vmat"], &path);
            compared += 1;
        }
    }
    println!("{compared} materials match");
    assert!(compared > 300, "{compared}");
}

/// Animation (name, frames, fps) of a model: its embedded `ANIM` blocks and those of the
/// models it includes animations from.
fn animations(dota: &Install, model: &str) -> Vec<(String, f64, f64)> {
    let mut anims = Vec::new();
    let mut queue = vec![model.to_owned()];
    let mut seen = HashSet::new();
    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = dota.read(&path).unwrap();
        let resource = Resource::parse(&bytes).unwrap();
        for block in resource.blocks().iter().filter(|b| &b.kind == b"ANIM") {
            let doc = kv3::decode(resource.bytes(block)).unwrap();
            for anim in doc
                .root
                .get("m_animArray")
                .and_then(Value::as_array)
                .unwrap()
            {
                let name = anim.get("m_name").and_then(Value::as_str).unwrap();
                let frames = anim.get("m_pData").and_then(|d| d.get("m_nFrames"));
                let fps = anim.get("fps").and_then(Value::as_f64).unwrap();
                anims.push((
                    name.to_owned(),
                    frames.and_then(Value::as_f64).unwrap(),
                    fps,
                ));
            }
        }
        let data = resource.data().unwrap().unwrap();
        let includes = data.get("m_refAnimIncludeModels").and_then(Value::as_array);
        queue.extend(
            includes
                .unwrap_or_default()
                .iter()
                .map(|m| format!("{}_c", m.as_str().unwrap())),
        );
    }
    anims
}

/// Render mesh names of a model, as its `CTRL` block embeds them (`name` in older models).
fn mesh_names(dota: &Install, model: &str) -> BTreeSet<String> {
    let bytes = dota.read(model).unwrap();
    let resource = Resource::parse(&bytes).unwrap();
    let ctrl = resource.kv3(b"CTRL").unwrap().unwrap().root;
    let meshes = ctrl
        .get("embedded_meshes")
        .and_then(Value::as_array)
        .unwrap();
    meshes
        .iter()
        .map(|m| {
            let name = m.get("m_Name").or_else(|| m.get("name"));
            name.and_then(Value::as_str).unwrap().to_owned()
        })
        .collect()
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn models_have_the_meshes_and_clips_the_manifest_and_export_name() {
    let dota = install();
    for line in dota.lines("model") {
        let Some(path) = common::exported(&line.out) else {
            continue;
        };
        let json = gltf_json(&path);
        let meshes = mesh_names(&dota, &line.src);
        let prefix = format!("{}.", line.src);
        for mesh in json["meshes"].as_array().unwrap() {
            let name = mesh["name"].as_str().unwrap();
            let short = name.strip_prefix(&prefix).unwrap_or(name);
            assert!(
                meshes.contains(short),
                "{}: exported mesh {name} not in {meshes:?}",
                line.name
            );
        }
        if let Some(wanted) = line.field("meshes") {
            for mesh in wanted.split(',') {
                assert!(
                    meshes.contains(mesh),
                    "{}: meshes= {mesh} not in {meshes:?}",
                    line.name
                );
            }
        }
        let anims = animations(&dota, &line.src);
        let exported: BTreeSet<&str> = json
            .get("animations")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().map(|a| a["name"].as_str().unwrap()).collect())
            .unwrap_or_default();
        if let Some(listed) = line.field("anims") {
            assert_eq!(exported, listed.split(',').collect(), "{}", line.name);
        }
        for clip in dota.lines("clip").filter(|c| c.name == line.name) {
            check_clip(clip, &anims);
        }
    }
}

/// A clip's animation exists, and the manifest's duration is its `(frames - 1) / fps`, held
/// last frames and all, as Dota plays it; `models.rs` checks the exported clips against it.
fn check_clip(clip: &Line, anims: &[(String, f64, f64)]) {
    let gltf_name = clip.out.as_str();
    let seconds: f64 = clip.rest[0].parse().unwrap();
    // A sequence that blends directions plays the hidden `@<name>` animation.
    let found = anims
        .iter()
        .find(|(name, ..)| name == gltf_name || name.strip_prefix('@') == Some(gltf_name));
    let (_, frames, fps) =
        found.unwrap_or_else(|| panic!("{}: no animation {gltf_name}", clip.name));
    let duration = (frames - 1.0) / fps;
    assert!(
        (seconds - duration).abs() < 0.0015,
        "{} {gltf_name}: {duration:.3} s, manifest {seconds} s",
        clip.name
    );
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn textures_exported_with_models_come_from_files_they_reference() {
    // Not the world's: entity-placed models (shopkeepers, outposts, banners) bring textures
    // that only the entity lumps name.
    let dota = install();
    for line in dota.lines("model") {
        let roots = [line.src.clone()];
        let reachable: HashSet<String> = dota
            .closure(&roots)
            .into_iter()
            .filter(|(path, _)| path.ends_with(".vtex_c"))
            .map(|(path, _)| {
                let file = path.rsplit('/').next().unwrap();
                file.trim_end_matches(".vtex_c").to_owned()
            })
            .collect();
        let Some(path) = common::exported(&line.out) else {
            continue;
        };
        let dir = path.parent().unwrap();
        for png in std::fs::read_dir(dir).unwrap() {
            let png = png.unwrap().file_name().into_string().unwrap();
            let Some(stem) = png.strip_suffix(".png") else {
                continue;
            };
            // Source2Viewer packs occlusion, roughness and metalness into new `_orm_` images, and
            // gives physics hulls a material of its own.
            if stem.contains("_orm_") || stem == "physics_default" {
                continue;
            }
            assert!(
                reachable.contains(stem),
                "{}: {png} comes from no referenced texture",
                line.name
            );
        }
    }
}

/// Every `vsnd_files*` entry of every sound event, by event name.
fn sound_events(dota: &Install) -> HashMap<String, Vec<String>> {
    let mut events: HashMap<String, Vec<String>> = HashMap::new();
    let scripts = dota
        .pak
        .entries()
        .filter(|e| e.ext() == "vsndevts_c" && e.dir().starts_with("soundevents"));
    for script in scripts {
        let data = dota.data(&script.path());
        for (event, body) in data.as_object().unwrap().iter() {
            let files = events.entry(event.to_owned()).or_default();
            for (key, value) in body.as_object().into_iter().flat_map(|o| o.iter()) {
                if !key.starts_with("vsnd_files") {
                    continue;
                }
                match value {
                    Value::String(file) => files.push(file.clone()),
                    Value::Array(list) => {
                        files.extend(list.iter().filter_map(Value::as_str).map(str::to_owned))
                    }
                    _ => {}
                }
            }
        }
    }
    events
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn sound_events_play_the_manifest_sounds() {
    let dota = install();
    let events = sound_events(&dota);
    println!("{} sound events", events.len());
    for line in dota.lines("sound") {
        let event = line.field("event").unwrap();
        let files = events
            .get(event)
            .unwrap_or_else(|| panic!("{}: no event {event}", line.name));
        let file = line.src.trim_end_matches("_c");
        assert!(
            files.iter().any(|f| f == file),
            "{}: {event} plays {files:?}, not {file}",
            line.name
        );
    }
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

/// Runs every parser over `bytes` (the clips called `clips` too): an error is fine, a panic
/// fails the test.
fn parse_everything(bytes: &[u8], clips: &[&str]) {
    let Ok(resource) = Resource::parse(bytes) else {
        return;
    };
    for block in resource.blocks() {
        let _ = kv3::decode(resource.bytes(block));
    }
    let _ = resource.data();
    let _ = resource.external_refs();
    let _ = read_clips(&resource, clips);
}

/// The KV3 and NTRO blocks of `files`, one file per kind of block (type, KV3 version and
/// compression) so every decoder path gets damaged.
fn samples(dota: &Install, files: &[(String, Vec<String>)]) -> Vec<Vec<u8>> {
    let mut kinds = HashSet::new();
    let mut samples = Vec::new();
    for (path, _) in files {
        let bytes = dota.read(path).unwrap();
        let resource = Resource::parse(&bytes).unwrap();
        let mut new = false;
        for block in resource.blocks() {
            let raw = resource.bytes(block);
            let kind = match raw.get(..24) {
                Some(head) if kv3::is_kv3(raw) => (block.kind, head[0], head[3], head[20]),
                _ => (block.kind, 0, 0, 0),
            };
            new |= kinds.insert(kind);
        }
        if new {
            samples.push(bytes);
        }
    }
    samples
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn damaged_files_are_errors_not_panics() {
    let dota = install();
    let mut roots: Vec<String> = dota.lines("model").map(|l| l.src.clone()).collect();
    roots.extend(world_files(&dota, &dota.lines("world").next().unwrap().src));
    let samples = samples(&dota, &dota.closure(&roots));
    let clips: Vec<&str> = dota
        .lines("model")
        .filter_map(|l| l.field("anims"))
        .flat_map(|a| a.split(','))
        .collect();
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for bytes in &samples {
        let resource = Resource::parse(bytes).unwrap();
        // Damage concentrates where the structure is: each block's first bytes.
        let starts: Vec<usize> = resource
            .blocks()
            .iter()
            .map(|b| b.offset as usize)
            .collect();
        for _ in 0..200 {
            let mut damaged = bytes.clone();
            let start = starts[rng.below(starts.len())];
            let at = (start + rng.below(256)).min(damaged.len() - 1);
            damaged[at] ^= 1 << rng.below(8);
            parse_everything(&damaged, &clips);
            let cut = rng.below(bytes.len());
            parse_everything(&bytes[..cut], &clips);
        }
    }
    let tree_len = {
        let head = dota.src.read_at("maps/dota.vpk", 0, 28).unwrap();
        u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize
    };
    let directory = dota.src.read_at("maps/dota.vpk", 0, 28 + tree_len).unwrap();
    for _ in 0..400 {
        let mut damaged = directory.clone();
        let at = rng.below(damaged.len());
        damaged[at] ^= 1 << rng.below(8);
        let mut src = SliceSource::default();
        src.insert("maps/dota.vpk", damaged);
        if let Ok(vpk) = Vpk::open(&src, "maps/dota.vpk") {
            // A damaged name may no longer round-trip as a path; only a panic would be wrong.
            for entry in vpk.entries() {
                let _ = vpk.get(&entry.path());
            }
        }
    }
    println!("{} sample files damaged", samples.len());
}
