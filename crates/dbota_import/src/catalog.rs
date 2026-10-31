//! The one reader of `assets-manifest/manifest.txt` and `ids.txt`.
//!
//! Every line is typed, its `key=value` fields are checked against the keys its kind knows, its
//! numbers are read as numbers, and the ids of `ids.txt` are checked against bota's registries
//! ([`IdSpace`]). A typo is an error that names the line, never a line that is silently
//! skipped. The importer's plan, the client's manifest, the golden tests and `dbota import
//! --verify` all read this.

use std::collections::BTreeSet;
use std::fmt;

/// A line that cannot be read: where and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineError {
    /// The file: `manifest.txt` or `ids.txt`.
    pub file: &'static str,
    /// 1-based.
    pub line: usize,
    pub why: String,
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} line {}: {}", self.file, self.line, self.why)
    }
}

/// A model line (`model <logical> <vmdl> <glb>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub logical: String,
    pub src: String,
    pub out: String,
    /// The exported animations; a model with any has its skeleton.
    pub anims: Vec<String>,
    /// Only these render meshes are kept.
    pub meshes: Option<Vec<String>>,
    /// The model this one is worn on (`attach=`).
    pub attach: Option<String>,
    /// The skin (material group) it is drawn with.
    pub skin: Option<String>,
    pub model_scale: Option<f32>,
}

/// A clip line (`clip <model> <logical> <glTF animation> <seconds>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub model: String,
    pub logical: String,
    pub name: String,
    pub duration: f32,
    /// The attack or cast point, in seconds at base speed.
    pub point: Option<f32>,
}

/// A unit line (`unit <npc_units name> <model logical>|none`).
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub unit: String,
    /// None for `none`: drawn as nothing.
    pub model: Option<String>,
    pub model_scale: Option<f32>,
    /// The particle (logical name) its attack's missile wears.
    pub attack: Option<String>,
}

/// Which converter reads a plain `<kind> <logical> <src> <out>` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Texture,
    Particle,
    Expression,
    Phonemes,
}

/// A line that converts one file of the install into one of the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub kind: FileKind,
    pub logical: String,
    pub src: String,
    pub out: String,
}

/// A sound line (`sound <logical> <vsnd> <out> event=<name>`); variants share the logical name.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub logical: String,
    pub src: String,
    pub out: String,
    pub event: String,
}

/// The world line: the map cut to a band, its terrain layers, trees and lighting, and the
/// ambient particle systems and soundscapes the map places.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub logical: String,
    pub src: String,
    pub out: String,
    pub vpk: String,
    pub half_width: f64,
    pub v_min: f64,
    pub v_max: f64,
    pub layers: String,
    pub trees: Option<String>,
    pub lighting: Option<String>,
    /// Where the map's ambient particle systems are listed (`ambient=`).
    pub ambient: Option<String>,
    /// Where the map's soundscapes are listed (`soundscapes=`).
    pub soundscapes: Option<String>,
}

/// The portrait framings line.
#[derive(Clone, Debug, PartialEq)]
pub struct Portraits {
    pub logical: String,
    pub src: String,
    pub out: String,
}

/// The shop line.
#[derive(Clone, Debug, PartialEq)]
pub struct Shop {
    pub logical: String,
    pub src: String,
    pub out: String,
    /// The localization file holding the shop's words.
    pub strings: String,
}

/// A line of the manifest.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Model(Model),
    Clip(Clip),
    Unit(Unit),
    File(File),
    Sound(Sound),
    World(World),
    Portraits(Portraits),
    Shop(Shop),
    /// An interface text: a localization file and a key.
    Label {
        src: String,
        key: String,
    },
    /// A colour a stylesheet defines: the stylesheet and the define's name.
    Style {
        src: String,
        name: String,
    },
    /// An effect bound to a fact of the game: the words after `fx`, read by the client.
    Fx(Vec<String>),
    /// An item drawn by its state.
    State(ItemState),
}

/// A state line (`state item.<name> <state> icon=<texture> [text=<label>]`): an item in a state,
/// the icon it is drawn with then and the label key of the install's words for that state.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemState {
    /// The item's Dota name (`power_treads`).
    pub item: String,
    pub when: StateWhen,
    /// The logical name of the icon's texture.
    pub icon: String,
    /// The label key of the words for the state.
    pub text: Option<String>,
}

/// A state an item can be in, as the wire tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateWhen {
    /// It keeps a rune of this kind (`rune=<rune art>`).
    Rune(bota_proto::RuneKind),
    /// It is set to this attribute (`mode=strength|agility|intelligence`).
    Mode(bota_proto::Attribute),
    /// It plants a Sentry Ward, or an Observer Ward (`ward=sentry|observer`).
    Ward { sentry: bool },
    /// It has this many charges left (`charges=<n>`).
    Charges(u8),
}

/// The name Dota's art gives a kind of rune: its model, glow, sound, Bottle icon and the hero
/// lines about it are named after it. Amplify Damage keeps Double Damage's art, the Bounty
/// Rune is `goldxp`'s model under the name bounty.
pub const fn rune_art(kind: bota_proto::RuneKind) -> &'static str {
    use bota_proto::RuneKind;
    match kind {
        RuneKind::Water => "water",
        RuneKind::Bounty => "bounty",
        RuneKind::Haste => "haste",
        RuneKind::AmplifyDamage => "doubledamage",
        RuneKind::Arcane => "arcane",
        RuneKind::Regeneration => "regeneration",
        RuneKind::Illusion => "illusion",
        RuneKind::Invisibility => "invisibility",
        RuneKind::Shield => "shield",
    }
}

fn state_line(rest: &[&str]) -> Result<ItemState, String> {
    use bota_proto::{Attribute, RuneKind};
    let [subject, state, fields @ ..] = rest else {
        return Err("state takes an item and a state".to_owned());
    };
    let item = subject
        .strip_prefix("item.")
        .ok_or_else(|| format!("'{subject}' is no item.<name>"))?;
    let when = match state.split_once('=') {
        Some(("rune", art)) => StateWhen::Rune(
            RuneKind::ALL
                .into_iter()
                .find(|k| rune_art(*k) == art)
                .ok_or_else(|| format!("no rune '{art}'"))?,
        ),
        Some(("mode", "strength")) => StateWhen::Mode(Attribute::Strength),
        Some(("mode", "agility")) => StateWhen::Mode(Attribute::Agility),
        Some(("mode", "intelligence")) => StateWhen::Mode(Attribute::Intelligence),
        Some(("ward", "sentry")) => StateWhen::Ward { sentry: true },
        Some(("ward", "observer")) => StateWhen::Ward { sentry: false },
        Some(("charges", n)) => StateWhen::Charges(
            n.parse()
                .map_err(|_| format!("charges '{n}' is no count"))?,
        ),
        _ => return Err(format!("no state '{state}'")),
    };
    let f = Fields::of(fields, &["icon", "text"])?;
    f.exactly::<0>("state's fields")?;
    Ok(ItemState {
        item: item.to_owned(),
        when,
        icon: f.need("icon")?.to_owned(),
        text: f.get("text").map(str::to_owned),
    })
}

/// A line of `ids.txt`: our id of a kind and the Dota name it stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Id {
    Item {
        id: u16,
        name: String,
    },
    Ability {
        id: u16,
        name: String,
    },
    Hero {
        id: u16,
        name: String,
    },
    /// An effect and the logical name of the icon Dota shows for it on the buff bar.
    Effect {
        id: u16,
        icon: String,
    },
    /// The Dota modifier an effect is.
    Modifier {
        effect: u16,
        name: String,
    },
    /// The Dota entry whose text describes an item or an ability, where the icon's name is not it.
    Text {
        of: TextOf,
        id: u16,
        key: String,
    },
}

/// What a `text` line describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOf {
    Item,
    Ability,
}

impl TextOf {
    /// The kind as `ids.txt` spells it.
    pub fn kind(self) -> &'static str {
        match self {
            TextOf::Item => "item",
            TextOf::Ability => "ability",
        }
    }
}

/// Which ids bota has: every position of its registries, a retired entry's included (its id is
/// never reused, and an old replay may still name it).
#[derive(Clone, Copy, Debug)]
pub struct IdSpace;

impl IdSpace {
    fn has(kind: &str, id: u16) -> bool {
        let id = usize::from(id);
        match kind {
            "item" => id < bota_core::ITEMS.len(),
            "ability" => id < bota_core::ABILITIES.len(),
            "hero" => id < bota_core::HEROES.len(),
            "effect" => id < bota_core::EFFECTS.len(),
            _ => false,
        }
    }
}

/// The manifest and the ids, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub lines: Vec<Line>,
    pub ids: Vec<Id>,
}

/// Reads the manifest and the ids; every line that cannot be read is reported.
pub fn parse(manifest: &str, ids: &str) -> Result<Catalog, Vec<LineError>> {
    let mut catalog = Catalog::default();
    let mut errors = Vec::new();
    for (at, text) in manifest.lines().enumerate() {
        let Some(words) = words(text) else { continue };
        match manifest_line(&words) {
            Ok(line) => catalog.lines.push(line),
            Err(why) => errors.push(LineError {
                file: "manifest.txt",
                line: at + 1,
                why,
            }),
        }
    }
    for (at, text) in ids.lines().enumerate() {
        let Some(words) = words(text) else { continue };
        match id_line(&words) {
            Ok(id) => catalog.ids.push(id),
            Err(why) => errors.push(LineError {
                file: "ids.txt",
                line: at + 1,
                why,
            }),
        }
    }
    errors.extend(cross_checks(&catalog));
    if errors.is_empty() {
        Ok(catalog)
    } else {
        Err(errors)
    }
}

/// The words of a line without its `#` comment; None for a line with none.
fn words(text: &str) -> Option<Vec<&str>> {
    let words: Vec<&str> = text.split('#').next()?.split_whitespace().collect();
    (!words.is_empty()).then_some(words)
}

/// The fields of a line: its positional words and its `key=value` words, the keys checked.
struct Fields<'a> {
    positional: Vec<&'a str>,
    keys: Vec<(&'a str, &'a str)>,
}

impl<'a> Fields<'a> {
    fn of(words: &[&'a str], allowed: &[&str]) -> Result<Fields<'a>, String> {
        let mut fields = Fields {
            positional: Vec::new(),
            keys: Vec::new(),
        };
        for word in words {
            match word.split_once('=') {
                Some((key, value)) => {
                    if !allowed.contains(&key) {
                        return Err(format!("no field '{key}='"));
                    }
                    if fields.keys.iter().any(|(k, _)| *k == key) {
                        return Err(format!("'{key}=' twice"));
                    }
                    if value.is_empty() {
                        return Err(format!("'{key}=' has no value"));
                    }
                    fields.keys.push((key, value));
                }
                None => fields.positional.push(word),
            }
        }
        Ok(fields)
    }

    /// Exactly `N` positional words.
    fn exactly<const N: usize>(&self, what: &str) -> Result<[&'a str; N], String> {
        self.positional
            .clone()
            .try_into()
            .map_err(|_| format!("{what} takes {N} words, not {}", self.positional.len()))
    }

    fn get(&self, key: &str) -> Option<&'a str> {
        self.keys.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    fn need(&self, key: &str) -> Result<&'a str, String> {
        self.get(key).ok_or_else(|| format!("no '{key}='"))
    }

    fn list(&self, key: &str) -> Option<Vec<String>> {
        self.get(key)
            .map(|v| v.split(',').map(str::to_owned).collect())
    }

    fn float(&self, key: &str) -> Result<Option<f32>, String> {
        self.get(key)
            .map(|v| number(key, v).map(|n| n as f32))
            .transpose()
    }
}

fn number(what: &str, text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{what}: '{text}' is no number"))
}

fn manifest_line(words: &[&str]) -> Result<Line, String> {
    let [kind, rest @ ..] = words else {
        return Err("empty".to_owned());
    };
    let own = |w: &str| w.to_owned();
    match *kind {
        "model" => {
            let f = Fields::of(rest, &["anims", "meshes", "attach", "skin", "model_scale"])?;
            let [logical, src, out] = f.exactly::<3>("model")?;
            Ok(Line::Model(Model {
                logical: own(logical),
                src: own(src),
                out: own(out),
                anims: f.list("anims").unwrap_or_default(),
                meshes: f.list("meshes"),
                attach: f.get("attach").map(own),
                skin: f.get("skin").map(own),
                model_scale: f.float("model_scale")?,
            }))
        }
        "clip" => {
            let f = Fields::of(rest, &["point"])?;
            let [model, logical, name, duration] = f.exactly::<4>("clip")?;
            Ok(Line::Clip(Clip {
                model: own(model),
                logical: own(logical),
                name: own(name),
                duration: number("duration", duration)? as f32,
                point: f.float("point")?,
            }))
        }
        "unit" => {
            let f = Fields::of(rest, &["model_scale", "attack"])?;
            let [unit, model] = f.exactly::<2>("unit")?;
            Ok(Line::Unit(Unit {
                unit: own(unit),
                model: (model != "none").then(|| own(model)),
                model_scale: f.float("model_scale")?,
                attack: f.get("attack").map(own),
            }))
        }
        "texture" | "particle" | "expression" | "phonemes" => {
            let f = Fields::of(rest, &[])?;
            let [logical, src, out] = f.exactly::<3>(kind)?;
            let kind = match *kind {
                "texture" => FileKind::Texture,
                "particle" => FileKind::Particle,
                "expression" => FileKind::Expression,
                _ => FileKind::Phonemes,
            };
            Ok(Line::File(File {
                kind,
                logical: own(logical),
                src: own(src),
                out: own(out),
            }))
        }
        "sound" => {
            let f = Fields::of(rest, &["event"])?;
            let [logical, src, out] = f.exactly::<3>("sound")?;
            Ok(Line::Sound(Sound {
                logical: own(logical),
                src: own(src),
                out: own(out),
                event: own(f.need("event")?),
            }))
        }
        "world" => {
            let f = Fields::of(
                rest,
                &[
                    "vpk",
                    "half_width",
                    "v_min",
                    "v_max",
                    "layers",
                    "trees",
                    "lighting",
                    "ambient",
                    "soundscapes",
                ],
            )?;
            let [logical, src, out] = f.exactly::<3>("world")?;
            Ok(Line::World(World {
                logical: own(logical),
                src: own(src),
                out: own(out),
                vpk: own(f.need("vpk")?),
                half_width: number("half_width", f.need("half_width")?)?,
                v_min: number("v_min", f.need("v_min")?)?,
                v_max: number("v_max", f.need("v_max")?)?,
                layers: own(f.need("layers")?),
                trees: f.get("trees").map(own),
                lighting: f.get("lighting").map(own),
                ambient: f.get("ambient").map(own),
                soundscapes: f.get("soundscapes").map(own),
            }))
        }
        "portraits" => {
            let f = Fields::of(rest, &[])?;
            let [logical, src, out] = f.exactly::<3>("portraits")?;
            Ok(Line::Portraits(Portraits {
                logical: own(logical),
                src: own(src),
                out: own(out),
            }))
        }
        "shop" => {
            let f = Fields::of(rest, &["strings"])?;
            let [logical, src, out] = f.exactly::<3>("shop")?;
            Ok(Line::Shop(Shop {
                logical: own(logical),
                src: own(src),
                out: own(out),
                strings: own(f.need("strings")?),
            }))
        }
        "label" | "style" => {
            let f = Fields::of(rest, &[])?;
            let [src, key] = f.exactly::<2>(kind)?;
            Ok(if *kind == "label" {
                Line::Label {
                    src: own(src),
                    key: own(key),
                }
            } else {
                Line::Style {
                    src: own(src),
                    name: own(key),
                }
            })
        }
        "fx" => Ok(Line::Fx(rest.iter().map(|w| own(w)).collect())),
        "state" => state_line(rest).map(Line::State),
        other => Err(format!("no line kind '{other}'")),
    }
}

fn id_line(words: &[&str]) -> Result<Id, String> {
    let id = |text: &str, kind: &str| -> Result<u16, String> {
        let id: u16 = text.parse().map_err(|_| format!("'{text}' is no id"))?;
        if IdSpace::has(kind, id) {
            Ok(id)
        } else {
            Err(format!("bota has no {kind} {id}"))
        }
    };
    match words {
        ["item", n, name] => Ok(Id::Item {
            id: id(n, "item")?,
            name: (*name).to_owned(),
        }),
        ["ability", n, name] => Ok(Id::Ability {
            id: id(n, "ability")?,
            name: (*name).to_owned(),
        }),
        ["hero", n, name] => Ok(Id::Hero {
            id: id(n, "hero")?,
            name: (*name).to_owned(),
        }),
        ["effect", n, icon] => Ok(Id::Effect {
            id: id(n, "effect")?,
            icon: (*icon).to_owned(),
        }),
        ["modifier", n, name] => Ok(Id::Modifier {
            effect: id(n, "effect")?,
            name: (*name).to_owned(),
        }),
        ["text", of @ ("item" | "ability"), n, key] => Ok(Id::Text {
            of: if *of == "item" {
                TextOf::Item
            } else {
                TextOf::Ability
            },
            id: id(n, of)?,
            key: (*key).to_owned(),
        }),
        [kind, ..] => Err(format!("cannot read an '{kind}' line of this shape")),
        [] => Err("empty".to_owned()),
    }
}

/// What no single line shows: a clip of a model nobody lists, a unit drawn with a model that is
/// not there, an attached model whose parent is not listed, a state drawn with a texture or
/// told in a label that is not listed or of an item ids.txt does not name, an id named twice.
fn cross_checks(catalog: &Catalog) -> Vec<LineError> {
    let textures: BTreeSet<&str> = catalog
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::File(f) if f.kind == FileKind::Texture => Some(f.logical.as_str()),
            _ => None,
        })
        .collect();
    let labels: BTreeSet<&str> = catalog
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Label { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let items: BTreeSet<&str> = catalog
        .ids
        .iter()
        .filter_map(|id| match id {
            Id::Item { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let models: BTreeSet<&str> = catalog
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Model(m) => Some(m.logical.as_str()),
            _ => None,
        })
        .collect();
    let mut errors = Vec::new();
    let mut fail = |why: String| {
        errors.push(LineError {
            file: "manifest.txt",
            line: 0,
            why,
        });
    };
    for line in &catalog.lines {
        match line {
            Line::Clip(c) if !models.contains(c.model.as_str()) => {
                fail(format!("clip {}: no model {}", c.logical, c.model));
            }
            Line::Unit(Unit {
                unit,
                model: Some(model),
                ..
            }) if !models.contains(model.as_str()) => {
                fail(format!("unit {unit}: no model {model}"));
            }
            Line::Model(Model {
                logical,
                attach: Some(parent),
                ..
            }) if !models.contains(parent.as_str()) => {
                fail(format!("model {logical}: attach={parent} is no model"));
            }
            Line::State(st) => {
                if !items.contains(st.item.as_str()) {
                    fail(format!("state item.{}: no item of that name", st.item));
                }
                if !textures.contains(st.icon.as_str()) {
                    fail(format!(
                        "state item.{}: icon={} is no texture",
                        st.item, st.icon
                    ));
                }
                if let Some(text) = st.text.as_deref().filter(|t| !labels.contains(t)) {
                    fail(format!("state item.{}: text={text} is no label", st.item));
                }
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    for id in &catalog.ids {
        let key = match id {
            Id::Item { id, .. } => ("item", *id),
            Id::Ability { id, .. } => ("ability", *id),
            Id::Hero { id, .. } => ("hero", *id),
            Id::Effect { id, .. } => ("effect", *id),
            Id::Modifier { effect, .. } => ("modifier", *effect),
            Id::Text { of, id, .. } => (
                match of {
                    TextOf::Item => "text item",
                    TextOf::Ability => "text ability",
                },
                *id,
            ),
        };
        if !seen.insert(key) {
            errors.push(LineError {
                file: "ids.txt",
                line: 0,
                why: format!("{} {} is named twice", key.0, key.1),
            });
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = include_str!("../../../assets-manifest/manifest.txt");
    const IDS: &str = include_str!("../../../assets-manifest/ids.txt");

    /// The committed files read with no error, and an injected typo is told with its line.
    #[test]
    fn the_committed_files_parse_and_a_typo_does_not() {
        let catalog = parse(MANIFEST, IDS).unwrap_or_else(|errors| {
            panic!(
                "{}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        assert!(catalog.lines.len() > 1000 && catalog.ids.len() > 200);
        for (typo, why) in [
            ("texure a a.vtex_c a.png\n", "no line kind 'texure'"),
            ("model m m.vmdl_c m.glb atach=p\n", "no field 'atach='"),
            ("sound s s.vsnd_c s.wav\n", "no 'event='"),
            ("shop s s.txt s.json\n", "no 'strings='"),
            ("clip nobody idle idle 1.0\n", "no model nobody"),
            (
                "state item.bottle mode=wisdom icon=icon.item.bottle\n",
                "no state 'mode=wisdom'",
            ),
            (
                "state item.bottle charges=1 icon=icon.item.nothing\n",
                "is no texture",
            ),
        ] {
            let errors = parse(&format!("{MANIFEST}{typo}"), IDS).expect_err(typo);
            assert!(
                errors.iter().any(|e| e.why.contains(why)),
                "{typo}: {errors:?}"
            );
        }
        let errors = parse(MANIFEST, &format!("{IDS}item 60000 nothing\n")).expect_err("an id");
        assert!(errors[0].why.contains("bota has no item"), "{errors:?}");
    }
}
