//! Golden comparison of the world files this crate writes with the ones in the asset cache that
//! `tools/import_assets.sh` exported. Ignored by default, like `golden.rs`:
//! `cargo test --release -p dbota_assets --test world -- --ignored`.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use dbota_assets::{
    DirSource, EntityLump, Resource, Vpk, buildings_txt, find_game_dir, read_from, trees_txt,
};

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

/// The lines of two placement files differ only in float formatting (`-0.0` for `0.0`).
fn same_placements(ours: &str, theirs: &str) -> Result<(), String> {
    let (a, b): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
    if a.len() != b.len() {
        return Err(format!("{} lines vs {}", a.len(), b.len()));
    }
    for (x, y) in a.iter().zip(&b) {
        let (p, q): (Vec<&str>, Vec<&str>) = (
            x.split_whitespace().collect(),
            y.split_whitespace().collect(),
        );
        let number = |w: &str| w.parse::<f64>().ok();
        let same = p.len() == q.len()
            && p.iter().zip(&q).all(|(u, v)| match (number(u), number(v)) {
                (Some(u), Some(v)) => u == v,
                _ => u == v,
            });
        if !same {
            return Err(format!("{x:?} vs {y:?}"));
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
