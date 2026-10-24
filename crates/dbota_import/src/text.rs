//! The asset cache's text files, made from what heatel reads out of the install: the tooltip
//! texts (`text/tooltips.json`), the interface's labels (`text/labels.json`), the shop's layout
//! and words (`text/shop.json`), the portrait framings (`models/portraits.txt`), and the map's
//! trees, buildings and lighting (`trees.txt`, `buildings.txt`, `lighting.txt`), and the
//! particle systems (`particles/*.json`). The client reads each (`src/cache_text.rs`,
//! `src/tooltip.rs`, `src/particles/`, ...).

use std::collections::{BTreeMap, HashSet};

use heatel::Result;
use heatel::keyvalues::{localization_table, text};
use heatel::kv3::Value;
use heatel::particle::{Blend, System};
use heatel::portraits::{Framing, WHITE};
use heatel::texture::Sheet;
use heatel::tooltips::{Tooltip, Wanted, tooltips};
use heatel::world::{Lighting, placements};
use serde_json::{Map, Value as Json, json};

/// The names of `ids.txt` whose texts the client shows: items and abilities (not recipes),
/// `text` entries, and the heroes whose ability scripts hold them.
pub fn wanted(ids: &str) -> Wanted {
    let mut names = Vec::new();
    let mut heroes = Vec::new();
    for line in ids.lines() {
        let words: Vec<&str> = line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        match words.as_slice() {
            ["item", _, name, ..] if *name != "recipe" => names.push(format!("item_{name}")),
            ["ability", _, name, ..] if *name != "recipe" => names.push((*name).to_owned()),
            ["text", _, _, name, ..] => names.push((*name).to_owned()),
            ["hero", _, name, ..] => heroes.push((*name).to_owned()),
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    Wanted { names, heroes }
}

/// `text/tooltips.json` from the localization and the scripts (the files of
/// [`Wanted::sources`], in that order).
pub fn tooltips_json(wanted: &Wanted, localization: &[u8], scripts: &[&[u8]]) -> Result<Vec<u8>> {
    let out: Map<String, Json> = tooltips(wanted, localization, scripts)?
        .into_iter()
        .map(|(name, tooltip)| (name, tooltip_entry(&tooltip)))
        .collect();
    to_vec("tooltips", &Json::Object(out))
}

/// One tooltip as the client reads it: `name`, then the texts there are, `values` always.
fn tooltip_entry(t: &Tooltip) -> Json {
    let mut entry = Map::new();
    entry.insert("name".into(), json!(t.name));
    for (field, text) in [("description", &t.description), ("lore", &t.lore)] {
        if let Some(text) = text {
            entry.insert(field.into(), json!(text));
        }
    }
    if !t.notes.is_empty() {
        entry.insert("notes".into(), json!(t.notes));
    }
    if !t.aliases.is_empty() {
        entry.insert("aliases".into(), json!(t.aliases));
    }
    let values: Map<String, Json> = t
        .values
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    entry.insert("values".into(), Json::Object(values));
    let fields = [
        ("cost", &t.cost),
        ("cooldown", &t.cooldown),
        ("mana", &t.mana),
        ("range", &t.range),
    ];
    for (field, value) in fields {
        if let Some(v) = value {
            entry.insert(field.into(), json!(v));
        }
    }
    if !t.labels.is_empty() {
        let labels: Vec<Json> = t.labels.iter().map(|(k, v)| json!([k, v])).collect();
        entry.insert("labels".into(), json!(labels));
    }
    Json::Object(entry)
}

/// Longest label text kept: an interface line, not a document.
const LABEL_LIMIT: usize = 1024;

/// `text/labels.json`: the text of each `(file, key)` of `keys` out of its localization file
/// among `files` (install path and bytes), by the key lower-cased. A key a file lacks, or whose
/// text is longer than [`LABEL_LIMIT`], is left out: the client says it in its own words.
pub fn labels_json(keys: &[(String, String)], files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let tables = files
        .iter()
        .map(|(path, bytes)| Ok((*path, localization_table(text(bytes)?))))
        .collect::<Result<Vec<_>>>()?;
    let mut out = BTreeMap::new();
    for (file, key) in keys {
        let key = key.to_lowercase();
        let found = tables
            .iter()
            .find(|(path, _)| path == file)
            .and_then(|(_, table)| table.get(&key))
            .filter(|text| text.len() <= LABEL_LIMIT);
        if let Some(text) = found {
            out.insert(key, text.clone());
        }
    }
    let out: Map<String, Json> = out.into_iter().map(|(k, v)| (k, json!(v))).collect();
    to_vec("labels", &Json::Object(out))
}

/// Most localization strings kept for the shop.
const MAX_STRINGS: usize = 512;
/// Longest localization string kept, in bytes.
const MAX_STRING: usize = 512;
/// The localization keys (lower-cased) the shop's words are under: whole prefixes, then single
/// keys.
const STRING_PREFIXES: [&str; 2] = ["dota_subshop_", "dota_shop_"];
const STRING_KEYS: [&str; 5] = [
    "dota_inventory_grab_all",
    "dota_inventorystash",
    "dota_neutrals",
    "dota_search",
    "dota_quick_buy_hint",
];

/// `text/shop.json` from `scripts/shops.txt` and the localization: `shops`, each category's
/// name and its items (`item_<name>`) in file order, and `strings`, the shop's words by their
/// lower-cased localization key.
pub fn shop_json(shops: &[u8], localization: &[u8]) -> Result<Vec<u8>> {
    let categories: Vec<Json> = heatel::shops::categories(shops)?
        .into_iter()
        .map(|c| json!([c.name, c.items]))
        .collect();
    let mut strings = Map::new();
    let table = localization_table(text(localization)?);
    let mut wanted: Vec<(&String, &String)> = table
        .iter()
        .filter(|(key, value)| {
            value.len() <= MAX_STRING
                && (STRING_PREFIXES.iter().any(|p| key.starts_with(p))
                    || STRING_KEYS.contains(&key.as_str()))
        })
        .collect();
    wanted.sort();
    for (key, value) in wanted.into_iter().take(MAX_STRINGS) {
        strings.insert(key.clone(), json!(value));
    }
    to_vec("shops", &json!({ "shops": categories, "strings": strings }))
}

/// The version of a particle file's layout; the client refuses others.
pub const PARTICLE_VERSION: u32 = 1;

/// A texture a particle file's systems draw with.
pub struct ParticleTexture {
    /// The texture (`materials/.../x.vtex`) the renderers name.
    pub vtex: String,
    /// Its KTX2 file in the cache.
    pub file: String,
    /// Its sprite sheet, if it is one.
    pub sheet: Option<Sheet>,
}

/// A material a particle file's projected renderers draw with.
pub struct ParticleMaterial {
    /// The material (`materials/.../x.vmat`) the renderers name.
    pub vmat: String,
    /// Its colour texture, one of the file's textures.
    pub vtex: String,
    /// How it blends: heatel's serde form of [`Blend`].
    pub blend: Blend,
    /// Whether it draws its texture's sprite sheet.
    pub sheets: bool,
}

/// A particle file: `{"version", "root", "systems": [{"path", "system"}], "textures":
/// [{"vtex", "file", "sheet"}], "materials": [{"vmat", "vtex", "blend", "sheets"}]}`, the root
/// system first, each system as heatel's serde form of [`System`] (children by path), a sheet
/// as its sequences of `{"clamp", "frames": [[display time, cropped u0 v0 u1 v1, uncropped u0
/// v0 u1 v1]]}` or `null`.
pub fn particle_json(
    root: &str,
    systems: &[(String, System)],
    textures: &[ParticleTexture],
    materials: &[ParticleMaterial],
) -> Result<Vec<u8>> {
    let systems: Vec<Json> = systems
        .iter()
        .map(|(path, system)| {
            let system = serde_json::to_value(system).map_err(|e| heatel::Error::Malformed {
                what: "particle",
                detail: e.to_string(),
            })?;
            Ok(json!({ "path": path, "system": system }))
        })
        .collect::<Result<_>>()?;
    let textures: Vec<Json> = textures
        .iter()
        .map(|t| json!({ "vtex": t.vtex, "file": t.file, "sheet": t.sheet.as_ref().map(sheet_json) }))
        .collect();
    let materials: Vec<Json> = materials
        .iter()
        .map(|m| json!({ "vmat": m.vmat, "vtex": m.vtex, "blend": m.blend, "sheets": m.sheets }))
        .collect();
    to_vec(
        "particle",
        &json!({
            "version": PARTICLE_VERSION,
            "root": root,
            "systems": systems,
            "textures": textures,
            "materials": materials,
        }),
    )
}

fn sheet_json(sheet: &Sheet) -> Json {
    let sequences: Vec<Json> = sheet
        .sequences
        .iter()
        .map(|sequence| {
            let frames: Vec<Vec<f32>> = sequence
                .frames
                .iter()
                .map(|f| {
                    std::iter::once(f.display_time)
                        .chain(f.cropped)
                        .chain(f.uncropped)
                        .collect()
                })
                .collect();
            json!({ "clamp": sequence.clamp, "frames": frames })
        })
        .collect();
    json!(sequences)
}

/// `value` as JSON text; the readers' error, named `what`, if it cannot be.
fn to_vec(what: &'static str, value: &Json) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| heatel::Error::Malformed {
        what,
        detail: e.to_string(),
    })
}

/// `models/portraits.txt`: a line per framing, `<logical> <numbers...> <background>`: camera
/// position and angles in the model's own inches and degrees, horizontal field of view, far
/// plane, the light's position, angles and field of view, light colour and ambient colour as
/// 0-255, the light's and the ambient colour's scales, the background's four corner tints as
/// 0-1 (top left, top right, bottom right, bottom left), and the background image's cache path,
/// `-` where there is none. `image` names the cache path a background material's image was
/// written to, if it was.
pub fn portraits_txt(framed: &[Framing], image: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::from(
        "# logical, camera position (3), camera angles (3), fov, far, light position (3), light angles (3), light fov, light colour (3), ambient colour (3), light scale, ambient scale, background tints (4x3), background image\n",
    );
    for f in framed {
        out.push_str(&f.label);
        let (c, l) = (&f.camera, &f.light);
        let (tints, path) = match &f.background {
            Some(b) => (b.tints, image(&b.material)),
            None => (WHITE, None),
        };
        let numbers = [
            &c.position[..],
            &c.angles,
            &[c.fov, c.far],
            &l.position,
            &l.angles,
            &[l.fov],
            &l.color,
            &l.ambient,
            &[l.scale, l.ambient_scale],
            tints.as_flattened(),
        ];
        for n in numbers.concat() {
            out.push_str(&format!(" {n}"));
        }
        out.push(' ');
        out.push_str(path.as_deref().unwrap_or("-"));
        out.push('\n');
    }
    out
}

/// Where the image of portrait background material `vmat` goes in the cache.
pub fn background_png(vmat: &str) -> String {
    let file = vmat.rsplit('/').next().unwrap_or(vmat);
    format!(
        "models/portrait_backgrounds/{}.png",
        file.trim_end_matches(".vmat")
    )
}

/// The models the client spawns as trees.
const TREES: [&str; 2] = ["tree_", "dire_tree"];
/// The models the client spawns as buildings.
const BUILDINGS: [&str; 12] = [
    "radiant_tower",
    "dire_tower",
    "radiant_ancient001",
    "dire_ancient_base001",
    "radiant_melee_barracks001",
    "radiant_ranged_barracks001",
    "dire_barracks_melee001",
    "dire_barracks_ranged001",
    "radiant_fountain002",
    "dire_fountain002",
    "radiant_statue001",
    "dire_column001",
];

/// Every tree `entities` place, once per model and whole Dota unit:
/// `model dota_x dota_y height yaw_deg scale`.
pub fn trees_txt(entities: &[Value]) -> String {
    let mut out = String::from(
        "# model dota_x dota_y height yaw_deg scale (bota = dota + 9216 on x and y)\n",
    );
    let mut seen = HashSet::new();
    for p in placements(entities) {
        let key = (
            p.model.clone(),
            cell(p.origin[0], 1.0),
            cell(p.origin[1], 1.0),
        );
        if TREES.iter().any(|t| p.model.starts_with(t)) && seen.insert(key) {
            let [x, y, z] = p.origin;
            out += &format!(
                "{} {x:.1} {y:.1} {z:.1} {:.1} {:.3}\n",
                p.model, p.yaw, p.scale
            );
        }
    }
    out
}

/// Every building `entities` place, once per model and 32-unit cell:
/// `model dota_x dota_y yaw_deg`.
pub fn buildings_txt(entities: &[Value]) -> String {
    let mut out = String::from("# model dota_x dota_y yaw_deg (bota = dota + 9216 on x and y)\n");
    let mut seen = HashSet::new();
    for p in placements(entities) {
        let key = (
            p.model.clone(),
            cell(p.origin[0], 32.0),
            cell(p.origin[1], 32.0),
        );
        if BUILDINGS.iter().any(|b| p.model.starts_with(b)) && seen.insert(key) {
            let [x, y, _] = p.origin;
            out += &format!("{} {x:.1} {y:.1} {:.1}\n", p.model, p.yaw);
        }
    }
    out
}

/// `v / size` rounded half to even, as the old exporter keyed its duplicates.
fn cell(v: f64, size: f64) -> i64 {
    (v / size).round_ties_even() as i64
}

/// `lighting.txt`: the sun's and the ambient colour and scale by day and by night, and the
/// seconds day takes to turn into night. One line per value: `sun day r g b scale`,
/// `sun night ...`, `ambient day ...`, `ambient night ...`, `turn seconds`. Colours are 0 to
/// 255.
pub fn lighting_txt(lighting: &Lighting) -> String {
    let mut out = String::from(
        "# Dota's lighting at the map centre: colour 0-255, scale; turn = seconds day takes to become night\n",
    );
    for (name, light) in [("sun", &lighting.sun), ("ambient", &lighting.ambient)] {
        for (time, shade) in [("day", &light.day), ("night", &light.night)] {
            let [r, g, b] = shade.color;
            out += &format!("{name} {time} {r:.1} {g:.1} {b:.1} {:.3}\n", shade.scale);
        }
    }
    out += &format!("turn {:.1}\n", lighting.turn);
    out
}

#[cfg(test)]
mod tests {
    use heatel::portraits::framings;

    use super::{background_png, portraits_txt};

    const FILE: &str = r#"
"Portraits"
{
    "models/a.vmdl"
    {
        "PortraitLightPosition" "1 2 3"
        "PortraitLightAngles" "49.61 263.39 0.00"
        "PortraitLightFOV" "90"
        "PortraitLightColor" "231 254 187"
        "PortraitAmbientColor" "97 114 0"
        "PortraitLightScale" "2.8"
        "PortraitAmbientScale" "5"
        "PortraitBackgroundTexture" "materials/vgui/hud/heroportraits/portraitbackground_desert.vmat"
        "PortraitBackgroundColor3" ".8 .2 .1"
        "cameras" { "default" { "PortraitPosition" "162.31 63.73 144.54" "PortraitAngles" "2.75 199.94 0" "PortraitFOV" "25" } }
    }
}"#;

    /// The line the client's `cache_text::framing` reads: every number in its place.
    #[test]
    fn a_framing_line_lists_the_camera_light_and_background() {
        let framed = framings(FILE.as_bytes(), &[("a", "models/a.vmdl")]).unwrap();
        let out = portraits_txt(&framed, |vmat| Some(background_png(vmat)));
        let line = out.lines().find(|l| !l.starts_with('#')).unwrap();
        assert_eq!(
            line,
            "a 162.31 63.73 144.54 2.75 199.94 0 25 10000 1 2 3 49.61 263.39 0 90 231 254 187 97 114 0 \
             2.8 5 1 1 1 1 1 1 0.8 0.2 0.1 1 1 1 models/portrait_backgrounds/portraitbackground_desert.png"
        );
        let out = portraits_txt(&framed, |_| None);
        assert!(out.trim_end().ends_with(" -"), "no image, no path: {out}");
    }
}
