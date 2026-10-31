//! The whole asset cache made by the importer against the one Source2Viewer's pipeline exported
//! (the v6 cache): the same files, and in each the same counts, durations and dimensions; the
//! golden tests of each converter (`models.rs`, `world.rs`, `golden_textures.rs`, ...) check the
//! data itself. Ignored by default, like them:
//!
//! ```sh
//! cargo test --release -p dbota_import --test cache -- --ignored
//! DBOTA_IMPORTED=/path/to/cache cargo test ...   # check a cache `dbota import` made
//! ```

mod common;
mod images;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dbota_import::cache_dir::{INDEX, STAMP, import_into};
use dbota_import::import::{Flow, Plan};
use dbota_import::text::wanted;
use heatel::find_game_dir;
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

/// The cache to check: `DBOTA_IMPORTED`, or one imported now into a scratch folder of the test
/// `test`'s own (the tests run at once, and an import replaces its folder), removed when the
/// test is done with it.
fn imported(test: &str) -> Imported {
    if let Some(dir) = std::env::var_os("DBOTA_IMPORTED") {
        return Imported {
            dir: PathBuf::from(dir),
            scratch: false,
        };
    }
    let dir = format!("cache-{test}-{}", std::process::id());
    let cache = Path::new(env!("CARGO_TARGET_TMPDIR")).join(dir);
    let game = find_game_dir(None).expect("Dota 2 not found");
    let plan = Plan::parse(
        &read(&repo().join("assets-manifest/manifest.txt")),
        &read(&repo().join("assets-manifest/ids.txt")),
    )
    .expect("the committed manifest and ids read");
    let started = std::time::Instant::now();
    let report = import_into(&game, &cache, &plan, "test", &mut |_| Flow::Continue).unwrap();
    let bytes: u64 = report.files.values().sum();
    println!(
        "imported {} files, {} MB in {:.1} s",
        report.files.len(),
        bytes >> 20,
        started.elapsed().as_secs_f64()
    );
    let cache = Imported {
        dir: cache,
        scratch: true,
    };
    assert!(report.failed.is_empty(), "{:#?}", report.failed);
    cache
}

/// An imported cache; a scratch one is removed with it.
struct Imported {
    dir: PathBuf,
    scratch: bool,
}

impl Drop for Imported {
    fn drop(&mut self) {
        if self.scratch {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
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
    let rel = match rel.split_once("_orm_") {
        Some((stem, _)) => format!("{stem}_orm.png"),
        None => rel.to_owned(),
    };
    as_imported(&rel)
}

/// Where the importer puts what the exporter wrote at `rel`: a model's or the map's image is a
/// KTX2 file of the same name (block-compressed, with its mip levels), not a PNG.
fn as_imported(rel: &str) -> String {
    let scene = (rel.starts_with("models/") && !rel.starts_with("models/portrait_backgrounds/"))
        || rel.starts_with("map/");
    match rel.strip_suffix(".png") {
        Some(stem) if scene => format!("{stem}.ktx2"),
        _ => rel.to_owned(),
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

/// What a glTF document must agree on: nodes (but the attachments only the importer writes),
/// each mesh's primitives (vertices, indices), the skins' joint counts, each animation's name
/// and length.
fn gltf_summary(path: &Path) -> Vec<String> {
    let json = gltf_json(path);
    let list = |key: &str| json[key].as_array().cloned().unwrap_or_default();
    let accessors = list("accessors");
    let count = |i: &Value| accessors[i.as_u64().unwrap() as usize]["count"].clone();
    let nodes = list("nodes").len() - attachments(&json);
    let mut lines = vec![format!("{nodes} nodes")];
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

/// The attachment nodes of a glTF document, which the importer writes and Source2Viewer does
/// not: a leaf that is no joint and carries no mesh.
fn attachments(json: &Value) -> usize {
    let joints: BTreeSet<u64> = json["skins"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|skin| skin["joints"].as_array().cloned().unwrap_or_default())
        .filter_map(|joint| joint.as_u64())
        .collect();
    let nodes = json["nodes"].as_array().cloned().unwrap_or_default();
    (0..nodes.len() as u64)
        .filter(|i| {
            let node = &nodes[*i as usize];
            !joints.contains(i) && node.get("mesh").is_none() && node.get("children").is_none()
        })
        .count()
}

/// Whether two summary lines are one clip, ours never the shorter: ours lasts to the clip's
/// last frame as Dota plays it, where the exporter stops at its last key, so a pose held to the
/// end makes ours the longer (`golden.rs` checks every length against the install's frames,
/// `models.rs` the values).
fn same_clip(ours: &str, theirs: &str) -> bool {
    let clip = |line: &str| {
        let (name, end) = line.strip_prefix("animation ")?.rsplit_once(' ')?;
        Some((name.to_owned(), end.parse::<f64>().ok()?))
    };
    match (clip(ours), clip(theirs)) {
        (Some((a, x)), Some((b, y))) => a == b && x + 1e-3 >= y,
        _ => false,
    }
}

/// A PNG's width and height.
/// Mean difference of any channel (that the format holds) a KTX2 file's largest level may be
/// from the exported PNG: block-compressing an image made from Dota's textures (packed
/// channels, decoded normals) costs about this; Dota's own blocks pass through unchanged.
const KTX2_MEAN: f64 = 3.0;

/// How a KTX2 file's largest level, decoded, differs from the exported PNG, if more than
/// [`KTX2_MEAN`] on average: the colours (and alpha where the format has it), the red and green
/// of a two-channel normal (the exporter wrote its blue worked out).
fn ktx2_differs(ours: &Path, theirs: &Path) -> Option<String> {
    use heatel::texture::PixelFormat as P;
    let bytes = std::fs::read(ours).unwrap();
    let reader = ktx2::Reader::new(&bytes[..]).unwrap();
    let header = reader.header();
    let format = match header.format.map(|f| f.value()) {
        Some(131..=134) => P::Bc1,
        Some(137 | 138) => P::Bc3,
        Some(139) => P::Bc4,
        Some(141) => P::Bc5,
        Some(145 | 146) => P::Bc7,
        Some(37 | 43) => P::Rgba8,
        other => return Some(format!("a KTX2 of format {other:?}")),
    };
    let level = reader.levels().next().unwrap().data;
    let ours = images::to_rgba(format, header.pixel_width, header.pixel_height, level);
    let (_, _, theirs) = images::read_png(theirs);
    let channels: &[usize] = match format {
        P::Bc5 => &[0, 1],
        P::Bc1 | P::Bc4 => &[0, 1, 2],
        _ => &[0, 1, 2, 3],
    };
    let pick = |px: &[u8]| -> Vec<u8> {
        px.chunks(4)
            .flat_map(|p| channels.iter().map(|&c| p[c]))
            .collect()
    };
    let (max, mean) = images::distance(&pick(&ours), &pick(&theirs));
    (mean > KTX2_MEAN)
        .then(|| format!("{format:?} differs by {mean:.2} on average (at most {max})"))
}

/// The width and height of a PNG or a KTX2 file, from its header.
fn image_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let le = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if bytes.starts_with(b"\x89PNG") {
        (be(16), be(20))
    } else {
        (le(20), le(24))
    }
}

/// Whether two text files say the same: JSON as values, placement lines number by number
/// (the exporter writes `-0.0` where the importer writes `0.0`).
fn text_differs(ours: &Path, theirs: &Path) -> Option<String> {
    let (a, b) = (read(ours), read(theirs));
    if ours.extension().is_some_and(|e| e == "json") {
        // The tooltip texts: every entry the ids ask for, as the exported file has it (the
        // exported file may hold more, for the ids of other versions).
        let (a, b): (Value, Value) = (
            serde_json::from_str(&a).unwrap(),
            serde_json::from_str(&b).unwrap(),
        );
        let ids = dbota_import::catalog::parse(
            &read(&repo().join("assets-manifest/manifest.txt")),
            &read(&repo().join("assets-manifest/ids.txt")),
        )
        .expect("the committed manifest and ids read")
        .ids;
        let wanted = wanted(&ids);
        // Fields the importer has added since the export (`aliases`) are not in the exported one.
        let kept = |name: &String| match (a.get(name), b.get(name)) {
            (Some(Value::Object(ours)), Some(Value::Object(theirs))) => theirs
                .iter()
                .all(|(field, value)| ours.get(field) == Some(value)),
            (ours, theirs) => ours == theirs,
        };
        let wrong: Vec<&String> = wanted.names.iter().filter(|n| !kept(n)).collect();
        return (!wrong.is_empty()).then(|| format!("texts differ: {wrong:?}"));
    }
    let words = |t: &str| -> Vec<Vec<String>> {
        t.lines()
            .filter(|l| !l.starts_with('#'))
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
    let (ours, theirs) = (words(&a), words(&b));
    // Columns the importer has added since the export come after the exported ones.
    let same =
        ours.len() == theirs.len() && ours.iter().zip(&theirs).all(|(o, t)| o.starts_with(t));
    (!same).then(|| "text differs".to_owned())
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
                .filter(|(x, y)| x != y && !same_clip(x, y))
                .map(|(x, y)| format!("{x} vs {y}"))
                .take(3)
                .collect();
            (a.len() != b.len() || !diff.is_empty())
                .then(|| format!("{} vs {} lines; {diff:?}", a.len(), b.len()))
        }
        "png" | "ktx2" => {
            let (a, b) = (image_size(ours), image_size(theirs));
            // The exporter wrote a material's constant mask as one pixel.
            let constant = b == (1, 1);
            if a != b {
                return (!constant).then(|| format!("{a:?} vs {b:?}"));
            }
            (ext == "ktx2")
                .then(|| ktx2_differs(ours, theirs))
                .flatten()
        }
        "txt" | "json" => text_differs(ours, theirs),
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

/// The folders of the manifest's lines the reference lacks (lines added after it was exported):
/// a model's folder, which holds its `.glb` and the images beside it, when the reference has
/// nothing in it. What lies there comes with its line.
fn folders_of_newer_lines(
    listed: &BTreeSet<String>,
    theirs: &BTreeMap<String, PathBuf>,
) -> BTreeSet<String> {
    listed
        .iter()
        .filter(|rel| !theirs.contains_key(*rel))
        .filter_map(|rel| Some(format!("{}/", rel.rsplit_once('/')?.0)))
        .filter(|dir| !theirs.keys().any(|k| k.starts_with(dir.as_str())))
        .collect()
}

/// Whether the importer alone makes `rel`, so the exported cache has nothing like it: a
/// world's `lighting=`, `ambient=` and `soundscapes=` files, the interface labels (`label` lines), and the portrait
/// backgrounds (`portraits` lines); the manifest's header says which kinds Source2Viewer never
/// wrote. Their contents are checked by tests of their own.
fn made_by_the_importer(rel: &str, manifest: &str) -> bool {
    let lines = || manifest.lines().filter_map(|l| l.split('#').next());
    let kind = |k: &str| lines().any(|l| l.split_whitespace().next() == Some(k));
    (rel == "text/labels.json" && kind("label"))
        || (rel.starts_with("models/portrait_backgrounds/") && kind("portraits"))
        || lines().any(|l| match l.split_whitespace().collect::<Vec<_>>()[..] {
            // A particle effect's textures go beside its file.
            ["particle", _, _, out, ..] => out
                .rsplit_once('/')
                .is_some_and(|(dir, _)| rel.starts_with(&format!("{dir}/textures/"))),
            _ => false,
        })
        // The map's ambient particles and the sounds of its soundscapes.
        || rel.starts_with("particles/map/")
        || rel.starts_with("sounds/ambient/soundscapes/")
        || lines().flat_map(str::split_whitespace).any(|w| {
            ["lighting=", "ambient=", "soundscapes="]
                .iter()
                .any(|key| w.strip_prefix(key) == Some(rel))
        })
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
    let cache = imported("matches");
    let (ours, theirs) = (files(&cache.dir), files(&exported()));
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
    // was exported) or what only the importer makes.
    let listed = manifest_outputs();
    let manifest = read(&repo().join("assets-manifest/manifest.txt"));
    let newer = folders_of_newer_lines(&listed, &theirs);
    let extra: Vec<&String> = ours.keys().filter(|k| !theirs.contains_key(*k)).collect();
    let unlisted: Vec<&&String> = extra
        .iter()
        .filter(|k| {
            !listed.contains(**k)
                && !newer.iter().any(|dir| k.starts_with(dir.as_str()))
                && !made_by_the_importer(k, &manifest)
        })
        .collect();
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

/// `lighting.txt` of an import against the map's own light entities: every blended colour lies
/// between the default weather's lights, the night is bluer than the day, and the turn is a few
/// seconds. (The values are Valve's, so the test reads them from the install instead of
/// holding them.)
#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_lighting_is_the_maps_blend_of_its_default_lights() {
    use heatel::world::EntityLump;
    use heatel::{DirSource, Resource, Vpk, read_from};
    let text = read(&imported("lighting").dir.join("map/mid/lighting.txt"));
    let mut colors = BTreeMap::new();
    let mut turn = 0.0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            [light, time, v @ ..] if v.len() == 4 => {
                let v: Vec<f64> = v.iter().map(|n| n.parse().unwrap()).collect();
                colors.insert((light.to_string(), time.to_string()), v);
            }
            ["turn", t] => turn = t.parse().unwrap(),
            other => panic!("unexpected line {other:?}"),
        }
    }
    assert_eq!(colors.len(), 4, "{text}");
    assert!((1.0..=30.0).contains(&turn), "turn {turn}");

    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let map = Vpk::open(&src, "maps/dota.vpk").unwrap();
    let world = read_from(&src, &[&map], "maps/dota/world.vwrld_c").unwrap();
    let world = Resource::parse(&world).unwrap().data().unwrap().unwrap();
    let lump = world.get("m_entityLumps").unwrap().as_array().unwrap()[0]
        .as_str()
        .unwrap();
    let bytes = read_from(&src, &[&map], &format!("{lump}_c")).unwrap();
    let lump = EntityLump::read(&Resource::parse(&bytes).unwrap()).unwrap();
    let defaults: Vec<_> = lump
        .entities
        .iter()
        .filter(|e| {
            e.get("classname").and_then(|c| c.as_str()) == Some("ent_dota_lightinfo")
                && e.get("targetname")
                    .and_then(|n| n.as_str())
                    .is_some_and(|n| n.starts_with("[PR#]default_"))
        })
        .collect();
    assert_eq!(defaults.len(), 4);
    for (light, key, scale) in [
        ("sun", "color", "light_scale"),
        ("ambient", "ambient_color", "ambient_scale"),
    ] {
        for time in ["day", "night"] {
            let ours = &colors[&(light.to_owned(), time.to_owned())];
            for (channel, value) in ours.iter().enumerate().take(3) {
                let of = |e: &&heatel::kv3::Value| {
                    e.get(&format!("{key}_{time}")).unwrap().as_array().unwrap()[channel]
                        .as_f64()
                        .unwrap()
                };
                let lo = defaults.iter().map(of).fold(f64::MAX, f64::min);
                let hi = defaults.iter().map(of).fold(f64::MIN, f64::max);
                assert!(
                    (lo - 0.05..=hi + 0.05).contains(value),
                    "{light} {time} channel {channel}: {value} outside {lo}..{hi}"
                );
            }
            let scales: Vec<f64> = defaults
                .iter()
                .map(|e| e.get(&format!("{scale}_{time}")).unwrap().as_f64().unwrap())
                .collect();
            let (lo, hi) = (
                scales.iter().copied().fold(f64::MAX, f64::min),
                scales.iter().copied().fold(f64::MIN, f64::max),
            );
            assert!(
                (lo - 0.01..=hi + 0.01).contains(&ours[3]),
                "{light} {time} scale"
            );
        }
        let blue = |time: &str| {
            let c = &colors[&(light.to_owned(), time.to_owned())];
            c[2] / c[0]
        };
        assert!(blue("night") > blue("day"), "{light}: night is not bluer");
    }
}
