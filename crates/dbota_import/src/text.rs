//! The asset cache's text files, made from what heatel reads out of the install: the tooltip
//! texts (`text/tooltips.json`), the interface's labels (`text/labels.json`), the shop's layout
//! and words (`text/shop.json`), the portrait framings (`models/portraits.txt`), and the map's
//! trees, buildings, lighting, ambient particle systems and soundscapes (`trees.txt`,
//! `buildings.txt`, `lighting.txt`, `ambient.txt`, `soundscapes.txt`), the faces' texts (a voice
//! line's lip sync, the phoneme table, the idle expressions), and the particle systems
//! (`particles/*.json`). The client reads each (`src/cache_text.rs`,
//! `src/tips.rs`, `src/particles/`, ...).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::catalog::Id;

use heatel::Result;
use heatel::keyvalues::{Token, localization_table, text, tokens};
use heatel::kv3::Value;
use heatel::particle::{Blend, System};
use heatel::portraits::{Framing, WHITE};
use heatel::texture::Sheet;
use heatel::tooltips::{Tooltip, Wanted, tooltips};
use heatel::world::{Lighting, placements};
use serde_json::{Map, Value as Json, json};

use crate::import::safe_path;

/// The names of `ids.txt` whose texts the client shows: items and abilities (not recipes),
/// `text` entries, and the heroes whose ability scripts hold them.
pub fn wanted(ids: &[Id]) -> Wanted {
    let mut names = Vec::new();
    let mut heroes = Vec::new();
    for id in ids {
        match id {
            Id::Item { name, .. } if name != "recipe" => names.push(format!("item_{name}")),
            Id::Ability { name, .. } if name != "recipe" => names.push(name.clone()),
            Id::Text { key, .. } => names.push(key.clone()),
            Id::Hero { name, .. } => heroes.push(name.clone()),
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

/// `text/styles.json`: the colour each `(sheet, name)` of `keys` defines (`@define name: #…;`)
/// in its compiled stylesheet among `sheets` (install path and the `.vcss_c` bytes), by the name
/// lower-cased, as `#rrggbbaa`. A define a sheet lacks, or one that is no plain colour, is left
/// out: the client draws its own.
pub fn styles_json(keys: &[(String, String)], sheets: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut sources = Vec::new();
    for (path, bytes) in sheets {
        let resource = heatel::Resource::parse(bytes)?;
        let data = resource.block(b"DATA").unwrap_or_default();
        sources.push((*path, data));
    }
    let mut out = Map::new();
    for (sheet, name) in keys {
        let found = sources
            .iter()
            .find(|(path, _)| path == sheet)
            .and_then(|(_, data)| style_define(data, name));
        if let Some(colour) = found {
            out.insert(name.to_lowercase(), json!(colour));
        }
    }
    to_vec("styles", &Json::Object(out))
}

/// The colour `@define name: #rgb[a];` gives in a stylesheet's source, as `#rrggbbaa`.
fn style_define(source: &[u8], name: &str) -> Option<String> {
    let head = format!("@define {name}:");
    let at = source
        .windows(head.len())
        .position(|w| w == head.as_bytes())?;
    let rest = &source[at + head.len()..];
    let end = rest.iter().take(64).position(|b| *b == b';')?;
    let value = std::str::from_utf8(&rest[..end]).ok()?.trim();
    let hex = value.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => Some(format!("#{}ff", hex.to_lowercase())),
        8 => Some(format!("#{}", hex.to_lowercase())),
        _ => None,
    }
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
pub const PARTICLE_VERSION: u32 = 3;

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

/// A model a particle file's model renderers draw.
pub struct ParticleModel {
    /// The model (`models/.../x.vmdl`) the renderers name.
    pub vmdl: String,
    /// Its glTF file in the cache.
    pub file: String,
}

/// A particle file: `{"version", "root", "systems": [{"path", "system"}], "textures":
/// [{"vtex", "file", "sheet"}], "materials": [{"vmat", "vtex", "blend", "sheets"}], "models":
/// [{"vmdl", "file"}]}`, the root system first, each system as heatel's serde form of
/// [`System`] (children by path), a sheet as its sequences of `{"clamp", "frames": [[display
/// time, cropped u0 v0 u1 v1, uncropped u0 v0 u1 v1]]}` or `null`.
pub fn particle_json(
    root: &str,
    systems: &[(String, System)],
    textures: &[ParticleTexture],
    materials: &[ParticleMaterial],
    models: &[ParticleModel],
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
    let models: Vec<Json> = models
        .iter()
        .map(|m| json!({ "vmdl": m.vmdl, "file": m.file }))
        .collect();
    to_vec(
        "particle",
        &json!({
            "version": PARTICLE_VERSION,
            "root": root,
            "systems": systems,
            "textures": textures,
            "materials": materials,
            "models": models,
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
/// The models the client spawns as buildings, and as the map's objectives.
const BUILDINGS: [&str; 16] = [
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
    "lantern_of_sight",
    "outpost",
    "xp_shrine",
    "fountain_of_life",
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

/// The models of the trees `entities` place, each once, sorted: the trees the client draws.
pub fn tree_models(entities: &[Value]) -> Vec<String> {
    let placed = placements(entities);
    let models: BTreeSet<&str> = placed
        .iter()
        .map(|p| p.model.as_str())
        .filter(|model| TREES.iter().any(|t| model.starts_with(t)))
        .collect();
    models.into_iter().map(str::to_owned).collect()
}

/// Where the model of the tree `model` goes in the cache, and so where the client reads it from;
/// none for a name that is not a plain model name (the map's file is the install's, but the
/// cache is read back as a file of untrusted text).
pub fn tree_model_path(model: &str) -> Option<String> {
    let plain = !model.is_empty()
        && model.len() <= 64
        && model.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    plain.then(|| format!("models/tree/{model}/{model}.glb"))
}

/// Every building `entities` place, once per model and 32-unit cell:
/// `model dota_x dota_y yaw_deg skin scale_x scale_y scale_z`. The skin is the model's material
/// group the map draws it with (the Lotus Pool of the Dire's corner is skin 1), 0 where the
/// map names none; the scales are the map's own, per axis.
pub fn buildings_txt(entities: &[Value]) -> String {
    let mut out = String::from(
        "# model dota_x dota_y yaw_deg skin scale_x scale_y scale_z (bota = dota + 9216 on x and y)\n",
    );
    let mut seen = HashSet::new();
    for (entity, p) in entities
        .iter()
        .filter_map(|e| Some((e, placements(std::slice::from_ref(e)).pop()?)))
    {
        let key = (
            p.model.clone(),
            cell(p.origin[0], 32.0),
            cell(p.origin[1], 32.0),
        );
        if BUILDINGS.iter().any(|b| p.model.starts_with(b)) && seen.insert(key) {
            let [x, y, _] = p.origin;
            let [sx, sy, sz] = scales(entity);
            out += &format!(
                "{} {x:.1} {y:.1} {:.1} {} {sx:.3} {sy:.3} {sz:.3}\n",
                p.model,
                p.yaw,
                skin(entity)
            );
        }
    }
    out
}

/// The material group an entity draws its model with: its `skin`, 0 where it names none or
/// a name rather than a number ("default").
fn skin(entity: &Value) -> u32 {
    entity
        .get("skin")
        .and_then(|s| match s.as_str() {
            Some(s) => s.parse().ok(),
            None => s.as_i64().and_then(|s| u32::try_from(s).ok()),
        })
        .unwrap_or(0)
}

/// An entity's scale on each axis, 1 where unset or not a finite positive number.
fn scales(entity: &Value) -> [f64; 3] {
    let list = entity
        .get("scales")
        .and_then(Value::as_array)
        .unwrap_or_default();
    std::array::from_fn(|i| {
        list.get(i)
            .and_then(Value::as_f64)
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0)
    })
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

/// A voice line's lip sync as the client reads it (`src/face.rs`): a header, then `phoneme
/// <start> <end> <code>`, `emphasis <time> <value>`, `morph <ease in> <ease out> <name
/// hash>...` and its `key <time> <value>...` lines.
pub fn lipsync_txt(lipsync: &heatel::sound::Lipsync) -> String {
    let mut text = String::from("dbota-lipsync 1\n");
    for p in &lipsync.phonemes {
        text.push_str(&format!("phoneme {} {} {}\n", p.start, p.end, p.code));
    }
    for (t, v) in &lipsync.emphasis {
        text.push_str(&format!("emphasis {t} {v}\n"));
    }
    if let Some(m) = &lipsync.morph {
        text.push_str(&format!("morph {} {}", m.ease_in, m.ease_out));
        m.names.iter().for_each(|n| text.push_str(&format!(" {n}")));
        text.push('\n');
        for (t, row) in m.times.iter().zip(&m.values) {
            text.push_str(&format!("key {t}"));
            row.iter().for_each(|v| text.push_str(&format!(" {v}")));
            text.push('\n');
        }
    }
    text
}

/// The phoneme table as the client reads it: a header, `keys <controller>...`, then `phoneme
/// <code> <setting>...` per phoneme.
pub fn phonemes_txt(table: &heatel::face::PhonemeTable) -> String {
    let mut text = format!("dbota-phonemes 1\nkeys {}\n", table.keys.join(" "));
    for p in &table.phonemes {
        text.push_str(&format!("phoneme {}", p.code));
        p.settings
            .iter()
            .for_each(|s| text.push_str(&format!(" {s}")));
        text.push('\n');
    }
    text
}

/// A facial scene as the client reads it: a header, `loop <from> <to>` when it loops, and per
/// flex animation event `event <start> <end>`, its `ramp <x> <y> <slope in> <slope out>...`
/// when it has one, and its `track <controller> <min> <max> <time> <value>...` lines. A track
/// whose controller name is not one word is left out.
pub fn expression_txt(scene: &heatel::face::Scene) -> String {
    let mut text = String::from("dbota-expression 1\n");
    if let Some((from, to)) = scene.looping {
        text.push_str(&format!("loop {from} {to}\n"));
    }
    for event in &scene.events {
        text.push_str(&format!("event {} {}\n", event.start, event.end));
        if !event.ramp.is_empty() {
            text.push_str("ramp");
            for k in &event.ramp {
                text.push_str(&format!(" {} {} {} {}", k.x, k.y, k.slope_in, k.slope_out));
            }
            text.push('\n');
        }
        let plain = |c: char| c.is_ascii_alphanumeric() || c == '_';
        for track in event.tracks.iter().filter(|t| t.name.chars().all(plain)) {
            text.push_str(&format!("track {} {} {}", track.name, track.min, track.max));
            for (t, v) in &track.samples {
                text.push_str(&format!(" {t} {v}"));
            }
            text.push('\n');
        }
    }
    text
}

/// Most ambient particle systems `ambient.txt` lists: the map places about a thousand.
const MAX_AMBIENT: usize = 4096;
/// The highest control point a map particle sets (Dota's systems have 64).
const MAX_CONTROL_POINT: i64 = 63;
/// The longest effect file stem: it names the effect's cache file.
const MAX_STEM: usize = 64;

/// The file stem of a map particle's effect (`particles/x/y.vpcf` is `y`), if it is a plain
/// name: the stem names the effect's cache file (`particles/map/y.json`), and the cache is read
/// back as a file of untrusted text.
pub fn effect_stem(effect: &str) -> Option<&str> {
    let path = effect.strip_prefix("particles/")?.strip_suffix(".vpcf")?;
    let stem = path.rsplit('/').next()?;
    let plain = !stem.is_empty()
        && stem.len() <= MAX_STEM
        && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    plain.then_some(stem)
}

/// Three finite numbers (a non-finite one is 0), or none for anything but three numbers.
fn vector(value: &Value) -> Option<[f64; 3]> {
    let [x, y, z] = value.as_array()? else {
        return None;
    };
    let number = |v: &Value| Some(v.as_f64().filter(|f| f.is_finite()).unwrap_or(0.0));
    Some([number(x)?, number(y)?, number(z)?])
}

/// `ambient.txt` from the entities of the map's default lump, and the distinct effects it
/// names (`particles/.../x.vpcf`, sorted). One line per particle system the map starts and
/// Dota ever shows: `effect dota_x dota_y dota_z pitch yaw roll day night fog [cp<n> x y z]`,
/// where `day`, `night` and `fog` are 0 or 1 (whether it shows by day, by night and in the
/// fog of war) and `cp<n>` is the one control point the map gives it a vector for. Two effects
/// of one file stem would share a cache file: the first is kept.
pub fn ambient_txt(entities: &[Value]) -> (String, Vec<String>) {
    let mut out = String::from(
        "# effect dota_x dota_y dota_z pitch yaw roll day night fog [cp<n> x y z] (bota = dota + 9216 on x and y)\n",
    );
    let mut effects: BTreeMap<&str, &str> = BTreeMap::new();
    let mut lines = 0;
    for entity in entities {
        if lines == MAX_AMBIENT {
            break;
        }
        let Some((effect, stem, line)) = ambient_line(entity) else {
            continue;
        };
        if *effects.entry(stem).or_insert(effect) != effect {
            continue;
        }
        out += &line;
        out.push('\n');
        lines += 1;
    }
    let mut paths: Vec<String> = effects.values().map(|p| (*p).to_owned()).collect();
    paths.sort();
    (out, paths)
}

/// The effect, its stem and the `ambient.txt` line of one entity, if it is a particle system
/// the map starts and Dota shows.
fn ambient_line(entity: &Value) -> Option<(&str, &str, String)> {
    let class = entity.get("classname").and_then(Value::as_str)?;
    if !matches!(class, "dota_world_particle_system" | "info_particle_system") {
        return None;
    }
    if entity.get("start_active").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let effect = entity.get("effect_name").and_then(Value::as_str)?;
    let stem = effect_stem(effect)?;
    let shown = |key: &str| u8::from(entity.get(key).and_then(Value::as_str) != Some("0"));
    let (day, night) = (shown("showatday"), shown("showatnight"));
    if day == 0 && night == 0 {
        return None;
    }
    let fog = entity
        .get("show_in_fog")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let [x, y, z] = vector(entity.get("origin")?)?;
    let [pitch, yaw, roll] = entity.get("angles").and_then(vector).unwrap_or_default();
    let mut line = format!(
        "{stem} {x:.1} {y:.1} {z:.1} {pitch:.1} {yaw:.1} {roll:.1} {day} {night} {}",
        u8::from(fog)
    );
    let point = entity.get("data_cp").and_then(Value::as_i64);
    let value = entity.get("data_cp_value").and_then(vector);
    if let (Some(n), Some([a, b, c])) = (point, value)
        && (0..=MAX_CONTROL_POINT).contains(&n)
    {
        line += &format!(" cp{n} {a:.3} {b:.3} {c:.3}");
    }
    Some((effect, stem, line))
}

/// Most soundscape definitions kept, over all the files.
const MAX_SCAPES: usize = 512;
/// Most entries one soundscape has, nested soundscapes' included.
const MAX_ENTRIES: usize = 64;
/// Most waves one random entry picks from.
const MAX_WAVES: usize = 16;
/// Deepest `playsoundscape` nesting followed.
const MAX_NESTING: usize = 4;
/// Most keys looked at while one soundscape is flattened, so that nested soundscapes which
/// play nothing cannot multiply.
const MAX_WORK: usize = 1024;
/// Most soundscape files the manifest lists, and most env_soundscape entities written.
const MAX_SCRIPTS: usize = 16;
const MAX_PLACED: usize = 64;
/// Deepest block nesting and most tokens of a soundscape file.
const MAX_SCRIPT_DEPTH: usize = 8;
const MAX_SCRIPT_TOKENS: usize = 1 << 18;
/// Seconds a soundscape fades in for when it names none.
const DEFAULT_FADE: f64 = 3.0;

/// The keys of a KeyValues block in file order. A soundscape repeats keys (`playlooping`,
/// `wave`), which heatel's tree would merge or drop.
type Items = Vec<(String, Node)>;

enum Node {
    Text(String),
    Block(Items),
}

/// The blocks and keys of KeyValues text `text`, all of them in order; none for text that
/// closes more blocks than it opens, leaves one open, nests deeper than [`MAX_SCRIPT_DEPTH`]
/// or holds more than [`MAX_SCRIPT_TOKENS`] tokens. A block without a key is read under an
/// empty one (Valve's manifest names its block unquoted, which the tokenizer skips).
fn read_script(text: &str) -> Option<Items> {
    let mut stack: Vec<(String, Items)> = vec![(String::new(), Vec::new())];
    let mut key: Option<&str> = None;
    for (at, token) in tokens(text).enumerate() {
        if at == MAX_SCRIPT_TOKENS {
            return None;
        }
        match token {
            Token::Text(t) => match key.take() {
                Some(k) => stack
                    .last_mut()?
                    .1
                    .push((k.to_owned(), Node::Text(t.to_owned()))),
                None => key = Some(t),
            },
            Token::Open => {
                if stack.len() > MAX_SCRIPT_DEPTH {
                    return None;
                }
                stack.push((key.take().unwrap_or_default().to_owned(), Vec::new()));
            }
            Token::Close => {
                if stack.len() < 2 {
                    return None;
                }
                let (k, items) = stack.pop()?;
                stack.last_mut()?.1.push((k, Node::Block(items)));
            }
        }
    }
    if stack.len() != 1 {
        return None;
    }
    stack.pop().map(|(_, items)| items)
}

/// The first text `key` (any case) holds in `items`.
fn field<'a>(items: &'a Items, key: &str) -> Option<&'a str> {
    items.iter().find_map(|(k, node)| match node {
        Node::Text(t) if k.eq_ignore_ascii_case(key) => Some(t.as_str()),
        _ => None,
    })
}

/// The blocks `key` (any case) names in `items`, in order.
fn blocks<'a>(items: &'a Items, key: &'a str) -> impl Iterator<Item = &'a Items> {
    items.iter().filter_map(move |(k, node)| match node {
        Node::Block(b) if k.eq_ignore_ascii_case(key) => Some(b),
        _ => None,
    })
}

/// The files `scripts/soundscapes_manifest.txt` lists (`"file" "scripts/x.txt"`), in order; a
/// commented line is not listed.
pub fn soundscape_files(manifest: &str) -> Vec<String> {
    let Some(root) = read_script(manifest) else {
        return Vec::new();
    };
    root.iter()
        .filter_map(|(_, node)| match node {
            Node::Block(items) => Some(items),
            Node::Text(_) => None,
        })
        .flat_map(|items| items.iter())
        .filter_map(|(k, node)| match node {
            Node::Text(path) if k.eq_ignore_ascii_case("file") => Some(path),
            _ => None,
        })
        .filter(|path| path.starts_with("scripts/") && safe_path(path))
        .take(MAX_SCRIPTS)
        .cloned()
        .collect()
}

/// A sound a soundscape plays, as one flattened soundscape has it.
enum Entry {
    Loop {
        volume: f64,
        pitch: f64,
        origin: Option<[f64; 3]>,
        level: u32,
        wave: String,
    },
    Random {
        volume: (f64, f64),
        pitch: (f64, f64),
        time: (f64, f64),
        origin: Option<[f64; 3]>,
        level: u32,
        waves: Vec<String>,
    },
}

/// The cache file of the sound `vsnd` (`sounds/x.vsnd`): the same path as a WAV, which is what
/// Valve ships ambient sounds as (the import refuses a wave that converts to anything else).
pub fn wave_cache_path(vsnd: &str) -> String {
    format!("{}.wav", vsnd.strip_suffix(".vsnd").unwrap_or(vsnd))
}

/// A wave a script names, as the package holds it: Valve's paths ignore case, the package's are
/// lower case. None for anything but a plain `sounds/.../x.vsnd` path.
fn wave_path(wave: &str) -> Option<String> {
    let path = wave.trim().to_ascii_lowercase();
    let plain = path.starts_with("sounds/") && path.ends_with(".vsnd");
    (plain && safe_path(&path)).then_some(path)
}

/// `"a"` or `"a, b"` as an ordered pair of finite numbers that are not negative.
fn range(text: &str) -> Option<(f64, f64)> {
    let mut parts = text.split(',').map(|p| {
        p.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
    });
    let first = parts.next()??;
    let second = match parts.next() {
        Some(second) => second?,
        None => first,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((first.min(second), first.max(second)))
}

/// The range under `key`, or `default` where it is absent; none where it is no range.
fn range_of(items: &Items, key: &str, default: (f64, f64)) -> Option<(f64, f64)> {
    field(items, key).map_or(Some(default), range)
}

/// `"-7350, -6825, 389;"` as a position; none for anything else, which plays everywhere.
fn origin(items: &Items) -> Option<[f64; 3]> {
    let text = field(items, "origin")?.trim().trim_end_matches(';');
    let numbers: Vec<f64> = text
        .split(',')
        .map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<_>>()?;
    numbers.try_into().ok()
}

/// `SNDLVL_81dB` as 81; 0 where there is none.
fn sound_level(items: &Items) -> u32 {
    let level = field(items, "soundlevel").unwrap_or_default().trim();
    let digits = level.to_ascii_uppercase();
    let digits = digits.strip_prefix("SNDLVL_").unwrap_or(&digits);
    digits
        .trim_end_matches("DB")
        .parse::<u32>()
        .map_or(0, |db| db.min(200))
}

/// The `playlooping` entry in `body`, its volume times `mult`.
fn looping(body: &Items, mult: f64) -> Option<Entry> {
    Some(Entry::Loop {
        volume: range_of(body, "volume", (1.0, 1.0))?.0 * mult,
        pitch: range_of(body, "pitch", (100.0, 100.0))?.0,
        origin: origin(body),
        level: sound_level(body),
        wave: wave_path(field(body, "wave")?)?,
    })
}

/// The `playrandom` entry in `body`, its volume times `mult`.
fn random(body: &Items, mult: f64) -> Option<Entry> {
    let (low, high) = range_of(body, "volume", (1.0, 1.0))?;
    let waves: Vec<String> = blocks(body, "rndwave")
        .flat_map(|b| b.iter())
        .filter_map(|(k, node)| match node {
            Node::Text(w) if k.eq_ignore_ascii_case("wave") => wave_path(w),
            _ => None,
        })
        .take(MAX_WAVES)
        .collect();
    if waves.is_empty() {
        return None;
    }
    Some(Entry::Random {
        volume: (low * mult, high * mult),
        pitch: range_of(body, "pitch", (100.0, 100.0))?,
        time: range(field(body, "time")?)?,
        origin: origin(body),
        level: sound_level(body),
        waves,
    })
}

/// The entries of a soundscape with the soundscapes it plays, flattened.
struct Flatten<'a> {
    defs: &'a HashMap<String, &'a Items>,
    /// The soundscapes being flattened, outermost first.
    trail: Vec<String>,
    entries: Vec<Entry>,
    work: usize,
}

impl Flatten<'_> {
    /// Adds the entries of soundscape `name`, volumes times `volume`; a soundscape already on
    /// the trail, or one nested deeper than [`MAX_NESTING`], adds none.
    fn walk(&mut self, name: &str, volume: f64) {
        let name = name.to_ascii_lowercase();
        let Some(items) = self.defs.get(&name).copied() else {
            return;
        };
        if self.trail.len() > MAX_NESTING || self.trail.contains(&name) {
            return;
        }
        self.trail.push(name);
        for (kind, node) in items.iter().take(MAX_ENTRIES) {
            self.work += 1;
            if self.work > MAX_WORK || self.entries.len() >= MAX_ENTRIES {
                break;
            }
            let Node::Block(body) = node else {
                continue;
            };
            match kind.to_ascii_lowercase().as_str() {
                "playlooping" => self.entries.extend(looping(body, volume)),
                "playrandom" => self.entries.extend(random(body, volume)),
                "playsoundscape" => {
                    let inner = field(body, "name").unwrap_or_default();
                    let scale = field(body, "volume").and_then(range).map_or(1.0, |r| r.0);
                    self.walk(inner, volume * scale);
                }
                _ => {}
            }
        }
        self.trail.pop();
    }
}

/// The soundscape definitions of the parsed files by name lower-cased, the first definition of
/// a name winning, at most [`MAX_SCAPES`].
fn definitions(files: &[Items]) -> HashMap<String, &Items> {
    let mut defs = HashMap::new();
    for (name, node) in files.iter().flatten() {
        if let Node::Block(items) = node
            && defs.len() < MAX_SCAPES
        {
            defs.entry(name.to_ascii_lowercase()).or_insert(items);
        }
    }
    defs
}

/// A soundscape name the client may match: plain characters only, bounded.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
}

/// An env_soundscape that is on from the start: its name, position and radius.
fn placed_scape(entity: &Value) -> Option<(&str, [f64; 3], f64)> {
    if entity.get("classname").and_then(Value::as_str) != Some("env_soundscape") {
        return None;
    }
    if entity.get("StartDisabled").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let name = entity.get("soundscape").and_then(Value::as_str)?;
    let radius = entity.get("radius")?.as_f64().filter(|r| r.is_finite())?;
    let origin = vector(entity.get("origin")?)?;
    plain_name(name).then_some((name, origin, radius.max(0.0)))
}

/// `soundscapes.txt` from the entities of the map's default lump and the texts of the
/// soundscape files (the first definition of a name wins), and the distinct waves it plays
/// (`sounds/.../x.vsnd`, lower case, sorted). For each env_soundscape that starts on:
/// `scape <name> <dota_x> <dota_y> <dota_z> <radius> <fade_seconds>`, then, once per name, the
/// entries of the soundscape of that name with every soundscape it plays inside it (their
/// volumes times the playing entry's): `loop <name> <volume> <pitch_percent> <x,y,z|-> <db>
/// <wave>` and `random <name> <vol_min>,<vol_max> <pitch_min>,<pitch_max> <time_min>,<time_max>
/// <x,y,z|-> <db> <wave>...`. `-` plays everywhere; a wave is its cache file (a `.wav`, see
/// [`wave_cache_path`]).
pub fn soundscapes_txt(entities: &[Value], scripts: &[&str]) -> (String, Vec<String>) {
    let files: Vec<Items> = scripts
        .iter()
        .take(MAX_SCRIPTS)
        .filter_map(|s| read_script(s))
        .collect();
    let defs = definitions(&files);
    let mut out = String::from(SCAPE_HEADER);
    let mut waves = BTreeSet::new();
    let mut written = HashSet::new();
    for (name, [x, y, z], radius) in entities.iter().filter_map(placed_scape).take(MAX_PLACED) {
        let Some(items) = defs.get(&name.to_ascii_lowercase()) else {
            continue;
        };
        let fade = field(items, "fadetime")
            .and_then(range)
            .map_or(DEFAULT_FADE, |r| r.0);
        out += &format!("scape {name} {x:.1} {y:.1} {z:.1} {radius:.2} {fade:.2}\n");
        if !written.insert(name) {
            continue;
        }
        let mut flat = Flatten {
            defs: &defs,
            trail: Vec::new(),
            entries: Vec::new(),
            work: 0,
        };
        flat.walk(name, 1.0);
        for entry in &flat.entries {
            out += &entry_line(name, entry, &mut waves);
        }
    }
    (out, waves.into_iter().collect())
}

const SCAPE_HEADER: &str = "\
# scape <name> dota_x dota_y dota_z radius fade_seconds: a soundscape the map places (bota = dota + 9216 on x and y)
# loop <name> volume pitch_percent x,y,z|- soundlevel_db wave: a sound it keeps playing; - plays everywhere
# random <name> vol_min,vol_max pitch_min,pitch_max time_min,time_max x,y,z|- soundlevel_db wave...: a sound picked from the waves, again after a time in seconds
";

/// The line of `entry` of soundscape `name`; the waves it plays are added to `waves`.
fn entry_line(name: &str, entry: &Entry, waves: &mut BTreeSet<String>) -> String {
    let place = |origin: &Option<[f64; 3]>| match origin {
        Some([x, y, z]) => format!("{x:.1},{y:.1},{z:.1}"),
        None => "-".to_owned(),
    };
    match entry {
        Entry::Loop {
            volume,
            pitch,
            origin,
            level,
            wave,
        } => {
            waves.insert(wave.clone());
            let wave = wave_cache_path(wave);
            format!(
                "loop {name} {volume:.2} {pitch:.2} {} {level} {wave}\n",
                place(origin)
            )
        }
        Entry::Random {
            volume,
            pitch,
            time,
            origin,
            level,
            waves: picks,
        } => {
            waves.extend(picks.iter().cloned());
            let files: Vec<String> = picks.iter().map(|w| wave_cache_path(w)).collect();
            format!(
                "random {name} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {} {level} {}\n",
                volume.0,
                volume.1,
                pitch.0,
                pitch.1,
                time.0,
                time.1,
                place(origin),
                files.join(" ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use heatel::kv3::{self, Value};
    use heatel::portraits::framings;
    use serde_json::{Value as Json, json};

    use super::{ambient_txt, background_png, portraits_txt, soundscape_files, soundscapes_txt};

    /// `json` as KV3 values: heatel builds them only by decoding, so it is encoded as the first
    /// binary KV3 (stored, strings in a table, root first) and read back.
    fn kv3_values(json: &Json) -> Value {
        fn put(json: &Json, strings: &mut Vec<String>, out: &mut Vec<u8>) {
            fn string(s: &str, strings: &mut Vec<String>, out: &mut Vec<u8>) {
                out.extend((strings.len() as i32).to_le_bytes());
                strings.push(s.to_owned());
            }
            match json {
                Json::Null => out.push(1),
                Json::Bool(b) => out.push(if *b { 13 } else { 14 }),
                Json::Number(n) if n.is_f64() => {
                    out.push(5);
                    out.extend(n.as_f64().unwrap_or_default().to_le_bytes());
                }
                Json::Number(n) => {
                    out.push(3);
                    out.extend(n.as_i64().unwrap_or_default().to_le_bytes());
                }
                Json::String(s) => {
                    out.push(6);
                    string(s, strings, out);
                }
                Json::Array(items) => {
                    out.push(8);
                    out.extend((items.len() as i32).to_le_bytes());
                    for item in items {
                        put(item, strings, out);
                    }
                }
                Json::Object(members) => {
                    out.push(9);
                    out.extend((members.len() as i32).to_le_bytes());
                    for (key, value) in members {
                        string(key, strings, out);
                        put(value, strings, out);
                    }
                }
            }
        }
        let (mut strings, mut body) = (Vec::new(), Vec::new());
        put(json, &mut strings, &mut body);
        let mut bytes = 0x0356_4B56u32.to_le_bytes().to_vec();
        bytes.extend([0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40]);
        bytes.extend([0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7, 0x14]);
        bytes.extend([0; 16]);
        bytes.extend((strings.len() as u32).to_le_bytes());
        for s in &strings {
            bytes.extend(s.as_bytes());
            bytes.push(0);
        }
        bytes.extend(body);
        bytes.extend(u32::MAX.to_le_bytes());
        kv3::decode(&bytes).unwrap().root
    }

    fn entities(json: &Json) -> Vec<Value> {
        kv3_values(json).as_array().unwrap().to_vec()
    }

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

    /// The lines the client's ambient reader takes: a night-only effect, a control point, and
    /// none for what Dota never shows, does not start, or has no plain name.
    #[test]
    fn ambient_lines_list_what_the_map_shows() {
        let fx = "particles/world_environmental_fx";
        let at = json!([6656.0, -4096.0, 320.0]);
        let fireflies = json!({ "classname": "dota_world_particle_system", "origin": at,
            "effect_name": format!("{fx}/fireflies.vpcf"), "angles": [0.0, 0.0, 0.0],
            "showatday": "0", "showatnight": "1", "show_in_fog": false, "start_active": true });
        let river = json!({ "classname": "info_particle_system", "origin": [160.1415, -4667.4326, 144.0],
            "effect_name": format!("{fx}/map_riverflow_surface.vpcf"), "angles": [0.0, 341.875, 0.0],
            "data_cp": 10, "data_cp_value": [1.0, 0.0, 0.0] });
        let never = json!({ "classname": "dota_world_particle_system", "origin": at,
            "effect_name": format!("{fx}/never.vpcf"), "showatday": "0", "showatnight": "0" });
        let idle = json!({ "classname": "info_particle_system", "origin": at,
            "effect_name": format!("{fx}/idle.vpcf"), "start_active": false });
        let odd = json!({ "classname": "info_particle_system", "origin": at,
            "effect_name": format!("{fx}/bad name.vpcf") });
        let (text, effects) = ambient_txt(&entities(&json!([fireflies, never, river, idle, odd])));
        let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(
            lines,
            [
                "fireflies 6656.0 -4096.0 320.0 0.0 0.0 0.0 0 1 0",
                "map_riverflow_surface 160.1 -4667.4 144.0 0.0 341.9 0.0 1 1 1 cp10 1.000 0.000 0.000",
            ]
        );
        assert_eq!(
            effects,
            [
                format!("{fx}/fireflies.vpcf"),
                format!("{fx}/map_riverflow_surface.vpcf")
            ]
        );
    }

    const SCAPES: &str = r#"
// the day
"default_day"
{
    "fadetime" "2"
    "playlooping"
    {
        "volume" ".2"  "pitch" "100"  "soundlevel" "SNDLVL_81dB"
        "wave" "sounds/ambient/soundscapes/Spawn_L.vsnd"
        "origin" "-7350, -6825, 389;"
    }
    //"playlooping"
    //{
    //    "volume" "1"
    //    "wave" "sounds/ambient/soundscapes/commented.vsnd"
    //}
    "playsoundscape" { "name" "weather" "volume" "0.5" }
}
"weather"
{
    "playlooping" { "volume" "0.6" "pitch" "100" "wave" "sounds/ambient/soundscapes/day_lp.vsnd" }
    "playrandom"
    {
        "volume" "0.7, 1.0"  "pitch" "95, 105"  "time" "6, 9"  "position" "random"
        "rndwave"
        {
            "wave" "sounds/ambient/soundscapes/b1.vsnd"
            "wave" "sounds/ambient/soundscapes/b2.vsnd"
            "wave" "sounds/ambient/soundscapes/b3.vsnd"
        }
    }
    "playsoundscape" { "name" "default_day" "volume" "1" }
}"#;

    /// The lines the client's soundscape reader takes: a positional loop, a nested soundscape
    /// whose volume multiplies, a random entry keeping every wave in order, a commented entry
    /// that is off, and a soundscape that plays its parent, which ends.
    #[test]
    fn soundscape_lines_flatten_what_a_scape_plays() {
        let scape = |name: &str, off: bool| {
            json!({ "classname": "env_soundscape",
            "soundscape": name, "origin": [280.0, -144.0, 649.0], "radius": 16000,
            "StartDisabled": off })
        };
        let placed = entities(&json!([
            scape("default_day", false),
            scape("default_night", true)
        ]));
        let (text, waves) = soundscapes_txt(&placed, &[SCAPES]);
        let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        let dir = "sounds/ambient/soundscapes";
        assert_eq!(
            lines,
            [
                "scape default_day 280.0 -144.0 649.0 16000.00 2.00".to_owned(),
                format!("loop default_day 0.20 100.00 -7350.0,-6825.0,389.0 81 {dir}/spawn_l.wav"),
                format!("loop default_day 0.30 100.00 - 0 {dir}/day_lp.wav"),
                format!(
                    "random default_day 0.35,0.50 95.00,105.00 6.00,9.00 - 0 \
                     {dir}/b1.wav {dir}/b2.wav {dir}/b3.wav"
                ),
            ]
        );
        let names = ["b1", "b2", "b3", "day_lp", "spawn_l"];
        assert_eq!(waves, names.map(|n| format!("{dir}/{n}.vsnd")));
        let manifest =
            "soundscapes_manifest\n{\n\"file\" \"scripts/a.txt\"\n//\"file\" \"scripts/b.txt\"\n}";
        assert_eq!(soundscape_files(manifest), ["scripts/a.txt"]);
    }
}
