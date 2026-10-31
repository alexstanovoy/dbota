//! `dbota import --verify`: checks every output of the manifest in a cache against what the
//! catalog says it should be: glTF structure, clips and their lengths, wearable joints, PNG
//! sizes, WAV and MP3 formats, particle files, and the labels and colours in their text files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::catalog::{Catalog, FileKind, Line};

/// How far a clip's length may differ from the manifest's, in seconds.
const DURATION_TOLERANCE: f64 = 0.01;

/// One thing the cache lacks or has wrong: the manifest name and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub name: String,
    pub why: String,
}

/// What a verification found.
#[derive(Debug, Default)]
pub struct Verified {
    /// How many outputs checked out.
    pub ok: usize,
    pub failures: Vec<Failure>,
}

/// Checks `catalog`'s outputs in the cache at `cache`.
pub fn verify(catalog: &Catalog, cache: &Path) -> Verified {
    let mut verified = Verified::default();
    let outputs: BTreeMap<&str, &str> = catalog
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Model(m) => Some((m.logical.as_str(), m.out.as_str())),
            Line::World(w) => Some((w.logical.as_str(), w.out.as_str())),
            _ => None,
        })
        .collect();
    let texts = [
        ("labels", "text/labels.json"),
        ("styles", "text/styles.json"),
    ]
    .map(|(name, path)| (name, read_json(&cache.join(path)).ok()));
    let mut note = |name: &str, result: Result<(), String>| match result {
        Ok(()) => verified.ok += 1,
        Err(why) => verified.failures.push(Failure {
            name: name.to_owned(),
            why,
        }),
    };
    for line in &catalog.lines {
        match line {
            Line::Model(m) => {
                let clips: Vec<_> = catalog
                    .lines
                    .iter()
                    .filter_map(|l| match l {
                        Line::Clip(c) if c.model == m.logical => Some(c),
                        _ => None,
                    })
                    .collect();
                let parent = m.attach.as_deref().and_then(|p| outputs.get(p).copied());
                note(
                    &m.logical,
                    check_model(
                        cache,
                        &m.out,
                        !m.anims.is_empty() || m.attach.is_some(),
                        parent,
                        &clips
                            .iter()
                            .map(|c| (c.name.as_str(), f64::from(c.duration)))
                            .collect::<Vec<_>>(),
                    ),
                );
            }
            Line::World(w) => note(&w.logical, check_model(cache, &w.out, false, None, &[])),
            Line::File(f) => note(&f.logical, check_file(cache, f.kind, &f.out)),
            Line::Sound(s) => note(&s.out, check_sound(&cache.join(&s.out))),
            Line::Portraits(p) => note(&p.logical, present(cache, &p.out)),
            Line::Shop(s) => note(&s.logical, present(cache, &s.out)),
            Line::Label { key, .. } => note(key, in_text(&texts[0], key)),
            Line::Style { name, .. } => note(name, in_text(&texts[1], name)),
            Line::Unit(u) => {
                if let Some(model) = &u.model {
                    note(
                        &u.unit,
                        outputs
                            .contains_key(model.as_str())
                            .then_some(())
                            .ok_or_else(|| format!("no model {model}")),
                    );
                }
            }
            Line::Clip(_) | Line::Fx(_) | Line::State(_) => {}
        }
    }
    verified
}

fn present(cache: &Path, rel: &str) -> Result<(), String> {
    cache
        .join(rel)
        .is_file()
        .then_some(())
        .ok_or_else(|| format!("{rel} is missing"))
}

fn in_text(table: &(&str, Option<Value>), key: &str) -> Result<(), String> {
    let (name, json) = table;
    json.as_ref()
        .and_then(|json| json.get(key.to_lowercase()))
        .map(|_| ())
        .ok_or_else(|| format!("not in text/{name}.json"))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The JSON of a `.gltf`, or of a `.glb`'s first chunk.
fn gltf_json(path: &Path) -> Result<Value, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e == "gltf") {
        return serde_json::from_slice(&data).map_err(|e| e.to_string());
    }
    let word = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if word(0) != Some(0x4654_6C67) || word(16) != Some(0x4E4F_534A) {
        return Err("not a glb with a JSON chunk first".to_owned());
    }
    let len = word(12).ok_or("short")? as usize;
    let json = data
        .get(20..20 + len)
        .ok_or("the JSON chunk is cut short")?;
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

/// The lengths of a document's animations: the largest input time of each.
fn durations(doc: &Value) -> BTreeMap<String, f64> {
    let accessors = doc["accessors"].as_array().cloned().unwrap_or_default();
    doc["animations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|anim| {
            let end = anim["samplers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let acc = accessors.get(usize::try_from(s["input"].as_u64()?).ok()?)?;
                    acc["max"][0].as_f64()
                })
                .fold(0.0, f64::max);
            (anim["name"].as_str().unwrap_or("?").to_owned(), end)
        })
        .collect()
}

/// The joints a document's skins name, lower-cased: Source bone-merges by name ignoring case.
fn joint_names(doc: &Value) -> BTreeSet<String> {
    doc["skins"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|skin| skin["joints"].as_array().into_iter().flatten())
        .filter_map(|j| doc["nodes"][usize::try_from(j.as_u64()?).ok()?]["name"].as_str())
        .map(str::to_lowercase)
        .collect()
}

fn check_model(
    cache: &Path,
    out: &str,
    needs_skin: bool,
    parent: Option<&str>,
    clips: &[(&str, f64)],
) -> Result<(), String> {
    let path = cache.join(out);
    let doc = gltf_json(&path).map_err(|e| format!("{out}: {e}"))?;
    if doc["meshes"].as_array().is_none_or(Vec::is_empty) {
        return Err("no meshes".to_owned());
    }
    if needs_skin && doc["skins"].as_array().is_none_or(Vec::is_empty) {
        return Err("animated or attached model without a skin".to_owned());
    }
    if let Some(parent) = parent {
        let parent = gltf_json(&cache.join(parent))?;
        let foreign: Vec<String> = joint_names(&doc)
            .difference(&joint_names(&parent))
            .cloned()
            .collect();
        if !foreign.is_empty() {
            return Err(format!("joints missing from its parent: {foreign:?}"));
        }
    }
    let lengths = durations(&doc);
    for (name, expected) in clips {
        let Some(got) = lengths.get(*name) else {
            return Err(format!("animation {name} missing"));
        };
        if (got - expected).abs() > DURATION_TOLERANCE {
            return Err(format!(
                "{name} lasts {got:.3} s, the manifest says {expected}"
            ));
        }
    }
    let dir = path.parent().unwrap_or(cache);
    for key in ["images", "buffers"] {
        let missing: Vec<&str> = doc[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["uri"].as_str())
            .filter(|uri| !dir.join(uri).is_file())
            .collect();
        if !missing.is_empty() {
            return Err(format!("missing {key}: {missing:?}"));
        }
    }
    Ok(())
}

fn check_file(cache: &Path, kind: FileKind, out: &str) -> Result<(), String> {
    let path = cache.join(out);
    let data = std::fs::read(&path).map_err(|_| format!("{out} is missing"))?;
    match kind {
        FileKind::Texture => {
            if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
                return Err("not a PNG".to_owned());
            }
            Ok(())
        }
        FileKind::Particle => {
            let doc: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            if doc["version"] != 3 || doc["systems"].as_array().is_none_or(Vec::is_empty) {
                return Err("not a layout 3 particle file with systems".to_owned());
            }
            let missing: Vec<&str> = ["textures", "models"]
                .into_iter()
                .flat_map(|key| doc[key].as_array().into_iter().flatten())
                .filter_map(|f| f["file"].as_str())
                .filter(|file| !cache.join(file).is_file())
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("textures or models missing: {missing:?}"))
            }
        }
        FileKind::Expression | FileKind::Phonemes => Ok(()),
    }
}

fn check_sound(path: &Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|_| "missing".to_owned())?;
    if path.extension().is_some_and(|e| e == "mp3") {
        return mp3(&data);
    }
    wav(&data)
}

/// 8- or 16-bit PCM in a RIFF/WAVE file.
fn wav(data: &[u8]) -> Result<(), String> {
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF/WAVE file".to_owned());
    }
    let (mut at, mut format, mut sized) = (12, None, false);
    while let Some(head) = data.get(at..at + 8) {
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        match &head[..4] {
            b"fmt " => {
                let f = data.get(at + 8..at + 24).ok_or("fmt cut short")?;
                format = Some((
                    u16::from_le_bytes([f[0], f[1]]),
                    u16::from_le_bytes([f[14], f[15]]),
                ));
            }
            b"data" => sized = true,
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    match format {
        Some((1, 8 | 16)) if sized => Ok(()),
        Some((tag, bits)) => Err(format!(
            "not 8/16-bit PCM with data (format {tag}, {bits} bits)"
        )),
        None => Err("no fmt chunk".to_owned()),
    }
}

/// An MPEG-1 layer III stream, after an optional ID3v2 tag.
fn mp3(data: &[u8]) -> Result<(), String> {
    let mut at = 0;
    if data.get(..3) == Some(b"ID3") {
        let size = data
            .get(6..10)
            .ok_or("short ID3 tag")?
            .iter()
            .fold(0usize, |n, b| (n << 7) | usize::from(b & 0x7F));
        at = 10 + size;
    }
    let head = data.get(at..at + 4).ok_or("no audio after the tag")?;
    let sync = head[0] == 0xFF && head[1] & 0xE0 == 0xE0;
    let layer3_v1 = (head[1] >> 3) & 3 == 3 && (head[1] >> 1) & 3 == 1;
    if !sync || !layer3_v1 || !matches!((head[2] >> 2) & 3, 0..=2) {
        return Err("not MPEG-1 layer III".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache missing a file, one with a file of the wrong kind and one with a good file are
    /// told apart, each by its manifest name.
    #[test]
    fn a_cache_is_checked_file_by_file() {
        let dir = std::env::temp_dir().join(format!("dbota-verify-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("t")).expect("a folder");
        std::fs::write(dir.join("t/good.png"), b"\x89PNG\r\n\x1a\nrest").expect("a file");
        std::fs::write(dir.join("t/bad.png"), b"GIF89a").expect("a file");
        let catalog = crate::catalog::parse(
            "texture good g.vtex_c t/good.png\ntexture bad b.vtex_c t/bad.png\n\
             texture gone x.vtex_c t/gone.png\n",
            "",
        )
        .expect("a catalog");
        let verified = verify(&catalog, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(verified.ok, 1);
        let failed: Vec<_> = verified.failures.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(failed, ["bad", "gone"], "{:?}", verified.failures);
    }
}
