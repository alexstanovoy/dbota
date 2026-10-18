//! The whole asset cache made by the importer against the one Source2Viewer's pipeline exported
//! (the v6 cache): the same files, and in each the same counts, durations and dimensions; the
//! golden tests of each converter (`models.rs`, `world.rs`, `golden_textures.rs`, ...) check the
//! data itself. Ignored by default, like them:
//!
//! ```sh
//! cargo test --release -p dbota_assets --test cache -- --ignored
//! DBOTA_IMPORTED=/path/to/cache cargo test ...   # check a cache `dbota import` made
//! ```

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dbota_assets::cache_dir::{INDEX, STAMP, import_into};
use dbota_assets::import::{Flow, Plan};
use dbota_assets::{Wanted, find_game_dir};
use serde_json::Value;

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The exported cache.
fn exported() -> PathBuf {
    common::reference()
}

/// The cache to check: `DBOTA_IMPORTED`, or one imported now into the test's scratch folder.
fn imported() -> PathBuf {
    if let Some(dir) = std::env::var_os("DBOTA_IMPORTED") {
        return PathBuf::from(dir);
    }
    let cache = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cache");
    let game = find_game_dir(None).expect("Dota 2 not found");
    let plan = Plan::parse(
        &read(&repo().join("assets-manifest/manifest.txt")),
        &read(&repo().join("assets-manifest/ids.txt")),
    );
    let started = std::time::Instant::now();
    let report = import_into(&game, &cache, &plan, "test", &mut |_| Flow::Continue).unwrap();
    let bytes: u64 = report.files.values().sum();
    println!(
        "imported {} files, {} MB in {:.1} s",
        report.files.len(),
        bytes >> 20,
        started.elapsed().as_secs_f64()
    );
    assert!(report.failed.is_empty(), "{:#?}", report.failed);
    cache
}

/// Every file under `root` by its path relative to it (`/`-separated), less hidden ones and
/// the exporter's own tools.
fn files(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            if path.is_dir() {
                if rel != "tools" {
                    stack.push(path);
                }
            } else if rel != INDEX && rel != STAMP {
                out.insert(rel, path);
            }
        }
    }
    out
}

/// An exported file's name as the importer writes it: Source2Viewer's made-up images carry a
/// hash (`x_orm_123.png` is `x_orm.png`).
fn unhashed(rel: &str) -> String {
    match rel.split_once("_orm_") {
        Some((stem, _)) => format!("{stem}_orm.png"),
        None => rel.to_owned(),
    }
}

/// The JSON of a `.glb` (its first chunk) or a `.gltf`.
fn gltf_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "glb") {
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[20..20 + len]).unwrap()
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

/// What a glTF document must agree on: nodes, each mesh's primitives (vertices, indices), the
/// skins' joint counts, each animation's name and length.
fn gltf_summary(path: &Path) -> Vec<String> {
    let json = gltf_json(path);
    let list = |key: &str| json[key].as_array().cloned().unwrap_or_default();
    let accessors = list("accessors");
    let count = |i: &Value| accessors[i.as_u64().unwrap() as usize]["count"].clone();
    let mut lines = vec![format!("{} nodes", list("nodes").len())];
    let mut meshes: Vec<String> = list("meshes")
        .iter()
        .map(|m| {
            let prims: Vec<String> = m["primitives"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    format!(
                        "{}/{}",
                        count(&p["attributes"]["POSITION"]),
                        count(&p["indices"])
                    )
                })
                .collect();
            format!("mesh {} {prims:?}", m["name"])
        })
        .collect();
    meshes.sort();
    lines.extend(meshes);
    for skin in list("skins") {
        lines.push(format!(
            "skin of {}",
            skin["joints"].as_array().unwrap().len()
        ));
    }
    lines.dedup();
    for a in list("animations") {
        let end = a["samplers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| accessors[s["input"].as_u64()? as usize]["max"][0].as_f64())
            .fold(0.0, f64::max);
        lines.push(format!("animation {} {:.3}", a["name"], end));
    }
    lines
}

/// A PNG's width and height.
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    (be(16), be(20))
}

/// Whether two text files say the same: JSON as values, placement lines number by number
/// (the exporter writes `-0.0` where the importer writes `0.0`).
fn same_text(ours: &Path, theirs: &Path) -> bool {
    let (a, b) = (read(ours), read(theirs));
    if ours.extension().is_some_and(|e| e == "json") {
        // The tooltip texts: every entry the ids ask for, as the exported file has it (the
        // exported file may hold more, for the ids of other versions).
        let (a, b): (Value, Value) = (
            serde_json::from_str(&a).unwrap(),
            serde_json::from_str(&b).unwrap(),
        );
        let wanted = Wanted::parse(&read(&repo().join("assets-manifest/ids.txt")));
        return wanted.names.iter().all(|name| a.get(name) == b.get(name));
    }
    let words = |t: &str| -> Vec<Vec<String>> {
        t.lines()
            .map(|l| {
                l.split_whitespace()
                    .map(|w| match w.parse::<f64>() {
                        Ok(v) => format!("{}", v + 0.0),
                        Err(_) => w.to_owned(),
                    })
                    .collect()
            })
            .collect()
    };
    words(&a) == words(&b)
}

/// How file `rel` differs between the two caches, if it does.
fn differs(rel: &str, ours: &Path, theirs: &Path) -> Option<String> {
    let ext = rel.rsplit('.').next().unwrap_or_default();
    match ext {
        "glb" | "gltf" => {
            let (a, b) = (gltf_summary(ours), gltf_summary(theirs));
            let diff: Vec<String> = a
                .iter()
                .zip(&b)
                .filter(|(x, y)| x != y)
                .map(|(x, y)| format!("{x} vs {y}"))
                .take(3)
                .collect();
            (a.len() != b.len() || !diff.is_empty())
                .then(|| format!("{} vs {} lines; {diff:?}", a.len(), b.len()))
        }
        "png" => {
            let (a, b) = (png_size(ours), png_size(theirs));
            // The exporter wrote a material's constant mask as one pixel.
            let constant = b == (1, 1);
            (a != b && !constant).then(|| format!("{a:?} vs {b:?}"))
        }
        "txt" | "json" => (!same_text(ours, theirs)).then(|| "text differs".to_owned()),
        "wav" | "mp3" => (std::fs::read(ours).unwrap() != std::fs::read(theirs).unwrap())
            .then(|| "bytes differ".to_owned()),
        _ => {
            let size = |p: &Path| std::fs::metadata(p).unwrap().len();
            (size(ours) != size(theirs))
                .then(|| format!("{} vs {} bytes", size(ours), size(theirs)))
        }
    }
}

/// The outputs the manifest lists (each line's fourth word).
fn manifest_outputs() -> BTreeSet<String> {
    read(&repo().join("assets-manifest/manifest.txt"))
        .lines()
        .filter_map(|l| {
            l.split('#')
                .next()?
                .split_whitespace()
                .nth(3)
                .map(str::to_owned)
        })
        .collect()
}

/// The exported files this manifest asks for: its outputs, the images its glTF files name, and
/// the ground layers' folder.
fn required(theirs: &BTreeMap<String, PathBuf>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for rel in manifest_outputs() {
        let Some(path) = theirs.get(&rel) else {
            continue;
        };
        if rel.ends_with(".glb") || rel.ends_with(".gltf") {
            let dir = rel.rsplit_once('/').map_or("", |(dir, _)| dir);
            for image in gltf_json(path)["images"].as_array().into_iter().flatten() {
                if let Some(uri) = image["uri"].as_str() {
                    out.insert(unhashed(&format!("{dir}/{uri}")));
                }
            }
        }
        out.insert(rel);
    }
    let ground = theirs.keys().filter(|k| k.starts_with("textures/ground/"));
    out.extend(ground.cloned());
    out
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn the_imported_cache_matches_the_exported_one() {
    let (ours, theirs) = (files(&imported()), files(&exported()));
    let theirs: BTreeMap<String, PathBuf> = theirs
        .into_iter()
        .map(|(rel, p)| (unhashed(&rel), p))
        .collect();
    // The exported cache may hold more than this manifest asks for (it was exported for the
    // manifests of other versions too); what it asks for must all be here.
    let required = required(&theirs);
    let missing: Vec<&String> = required.iter().filter(|k| !ours.contains_key(*k)).collect();
    assert!(
        missing.is_empty(),
        "{} files not imported: {missing:?}",
        missing.len()
    );
    let other_versions = theirs.keys().filter(|k| !ours.contains_key(*k)).count();
    // What the exported cache lacks may only be what the manifest lists (lines added after it
    // was exported).
    let listed = manifest_outputs();
    let extra: Vec<&String> = ours.keys().filter(|k| !theirs.contains_key(*k)).collect();
    let unlisted: Vec<&&String> = extra.iter().filter(|k| !listed.contains(**k)).collect();
    assert!(
        unlisted.is_empty(),
        "files the exported cache lacks: {unlisted:?}"
    );
    let mut kinds: BTreeMap<String, (usize, u64, u64)> = BTreeMap::new();
    let mut failed = Vec::new();
    for (rel, path) in &ours {
        let Some(other) = theirs.get(rel) else {
            continue;
        };
        let ext = rel.rsplit('.').next().unwrap_or_default().to_owned();
        let kind = kinds.entry(ext).or_default();
        kind.0 += 1;
        kind.1 += std::fs::metadata(path).unwrap().len();
        kind.2 += std::fs::metadata(other).unwrap().len();
        if let Some(why) = differs(rel, path, other) {
            failed.push(format!("{rel}: {why}"));
        }
    }
    for (ext, (n, a, b)) in &kinds {
        println!(
            "{ext}: {n} files, {} MB here, {} MB exported",
            a >> 20,
            b >> 20
        );
    }
    println!(
        "{} files compared ({} required); also made here: {extra:?}; {other_versions} \
         exported files this manifest does not ask for",
        ours.len() - extra.len(),
        required.len()
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
