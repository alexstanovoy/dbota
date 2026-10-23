//! The tooltip texts of the items and abilities the client names: their names, the shop search's
//! aliases, descriptions, lore, notes, number labels and Dota's own numbers, read out of the
//! install's localization and item/ability scripts (plain KeyValues text in the package) into the
//! cache's `text/tooltips.json`; and the interface's labels (a stat's name, a line of the stats
//! tooltip) into `text/labels.json`.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value as Json, json};

use crate::error::{Result, malformed};
use crate::keyvalues::{Kv, localization_table, parse_kv, text};

const LOCALIZATION: &str = "resource/localization/abilities_english.txt";
const PREFIX: &str = "dota_tooltip_ability_";
const VARIABLE: &str = "dota_ability_variable_";
/// The localization's search aliases of an item: `DOTA_SearchAlias_Ability_<name>`.
const ALIAS: &str = "dota_searchalias_ability_";
/// Most aliases kept of one name.
const MAX_ALIASES: usize = 16;

/// What `ids.txt` asks texts of: Dota names (`item_<name>` for items) and the heroes whose
/// ability scripts hold them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub names: Vec<String>,
    pub heroes: Vec<String>,
}

impl Wanted {
    /// The names of `ids.txt`: items and abilities (not recipes), `text` entries, heroes.
    pub fn parse(ids: &str) -> Self {
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
        Self { names, heroes }
    }

    /// The install files the texts come from: the localization, then the scripts in the order
    /// their blocks are searched.
    pub fn sources(&self) -> Vec<String> {
        let mut heroes: Vec<String> = self
            .heroes
            .iter()
            .map(|h| format!("scripts/npc/heroes/npc_dota_hero_{h}.txt"))
            .collect();
        heroes.sort();
        heroes.dedup();
        let mut out = vec![
            LOCALIZATION.to_owned(),
            "scripts/npc/items.txt".to_owned(),
            "scripts/npc/npc_abilities.txt".to_owned(),
        ];
        out.extend(heroes);
        out
    }
}

/// `text/tooltips.json` from the localization and the scripts (the files of
/// [`Wanted::sources`], in that order).
pub fn tooltips_json(wanted: &Wanted, localization: &[u8], scripts: &[&[u8]]) -> Result<Vec<u8>> {
    let loc = localization_table(text(localization)?);
    let variables: HashMap<&str, &str> = loc
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(VARIABLE)?, v.as_str())))
        .collect();
    let mut found: HashMap<&str, &Kv> = HashMap::new();
    let trees = scripts
        .iter()
        .map(|s| parse_kv(text(s)?))
        .collect::<Result<Vec<_>>>()?;
    for tree in &trees {
        find_blocks(tree, &wanted.names, &mut found);
    }
    let mut out = BTreeMap::new();
    for name in &wanted.names {
        let Some(title) = loc.get(&format!("{PREFIX}{name}")) else {
            continue;
        };
        let block = found.get(name.as_str()).copied();
        out.insert(name.clone(), entry(name, title, block, &loc, &variables));
    }
    serde_json::to_vec(&out).map_err(|e| malformed("tooltips", e.to_string()))
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
    serde_json::to_vec(&out).map_err(|e| malformed("labels", e.to_string()))
}

/// One name's entry.
fn entry(
    name: &str,
    title: &str,
    block: Option<&Kv>,
    loc: &HashMap<String, String>,
    variables: &HashMap<&str, &str>,
) -> Json {
    let key = |suffix: &str| format!("{PREFIX}{name}{suffix}");
    let mut entry = Map::new();
    entry.insert("name".into(), json!(title));
    for (field, suffix) in [("description", "_description"), ("lore", "_lore")] {
        if let Some(text) = loc.get(&key(suffix)) {
            entry.insert(field.into(), json!(text));
        }
    }
    let notes: Vec<&String> = (0..)
        .map_while(|n| loc.get(&key(&format!("_note{n}"))))
        .collect();
    if !notes.is_empty() {
        entry.insert("notes".into(), json!(notes));
    }
    // The other names the shop's search finds it by, `;` between them.
    let aliases: Vec<&str> = loc
        .get(&format!("{ALIAS}{name}"))
        .map(|list| {
            list.split(';')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .take(MAX_ALIASES)
                .collect()
        })
        .unwrap_or_default();
    if !aliases.is_empty() {
        entry.insert("aliases".into(), json!(aliases));
    }
    let values: Vec<(&str, String)> = block
        .and_then(|b| b.get("AbilityValues"))
        .and_then(Kv::block)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str(), level_values(Some(v))?)))
        .collect();
    let value_map: Map<String, Json> = values
        .iter()
        .map(|(k, v)| ((*k).into(), json!(v)))
        .collect();
    entry.insert("values".into(), Json::Object(value_map));
    let fields = [
        ("cost", "ItemCost"),
        ("cooldown", "AbilityCooldown"),
        ("mana", "AbilityManaCost"),
        ("range", "AbilityCastRange"),
    ];
    for (field, script_key) in fields {
        let own = level_values(block.and_then(|b| b.get(script_key))).filter(|s| !s.is_empty());
        let from_values = || {
            values
                .iter()
                .find(|(k, _)| *k == script_key)
                .map(|(_, v)| v.clone())
        };
        if let Some(v) = own.or_else(from_values) {
            entry.insert(field.into(), json!(v));
        }
    }
    let labels: Vec<Json> = values
        .iter()
        .filter_map(|(k, _)| {
            let label = loc.get(&key(&format!("_{}", k.to_lowercase())))?;
            (!label.is_empty()).then(|| json!([k, substitute(label, variables)]))
        })
        .collect();
    if !labels.is_empty() {
        entry.insert("labels".into(), json!(labels));
    }
    Json::Object(entry)
}

/// A script value as the space-separated text Dota prints: a string, or a block's `value`.
fn level_values(v: Option<&Kv>) -> Option<String> {
    match v? {
        Kv::Text(s) => Some(s.trim().to_owned()),
        Kv::Block(_) => match v?.get("value")? {
            Kv::Text(s) => Some(s.trim().to_owned()),
            Kv::Block(_) => None,
        },
    }
}

/// `$name` replaced by the localization's variable of that name, where there is one.
fn substitute(label: &str, variables: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    let mut rest = label;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(tail.len());
        let word = &tail[..end];
        match variables.get(word) {
            Some(v) if !word.is_empty() => out.push_str(v),
            _ => {
                out.push('$');
                out.push_str(word);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The first block anywhere under `node` (depth first, in file order) for each wanted name
/// that holds an ability or item.
fn find_blocks<'a>(node: &'a Kv, wanted: &[String], found: &mut HashMap<&'a str, &'a Kv>) {
    let mut stack: Vec<(&'a str, &'a Kv)> = Vec::new();
    let push = |stack: &mut Vec<(&'a str, &'a Kv)>, node: &'a Kv| {
        stack.extend(
            node.block()
                .unwrap_or_default()
                .iter()
                .rev()
                .map(|(k, v)| (k.as_str(), v)),
        );
    };
    push(&mut stack, node);
    while let Some((k, v)) = stack.pop() {
        if v.block().is_none() {
            continue;
        }
        let holds = ["AbilityValues", "ItemCost", "AbilityBehavior"]
            .iter()
            .any(|key| v.get(key).is_some());
        if holds && !found.contains_key(k) && wanted.iter().any(|w| w == k) {
            found.insert(k, v);
        }
        push(&mut stack, v);
    }
}
