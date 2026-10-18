//! The tooltip texts of the items and abilities the client names: their names, descriptions,
//! lore, notes, number labels and Dota's own numbers, read out of the install's localization and
//! item/ability scripts (plain KeyValues text in the package) into the cache's
//! `text/tooltips.json`.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value as Json, json};

use crate::error::{Result, malformed};

/// Deepest nesting of a script's blocks.
const MAX_DEPTH: usize = 64;
const LOCALIZATION: &str = "resource/localization/abilities_english.txt";
const PREFIX: &str = "dota_tooltip_ability_";
const VARIABLE: &str = "dota_ability_variable_";

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

/// A file as text, without a byte-order mark.
fn text(bytes: &[u8]) -> Result<&str> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| malformed("tooltips", e.to_string()))
}

/// A KeyValues node: a string, or a block of keys in file order (a repeated key keeps its
/// first value; repeated blocks merge into the first).
#[derive(Debug)]
enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        self.block()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn block(&self) -> Option<&[(String, Kv)]> {
        match self {
            Self::Block(items) => Some(items),
            Self::Text(_) => None,
        }
    }
}

enum Token<'a> {
    Text(&'a str),
    Open,
    Close,
}

/// The quoted strings and braces of KeyValues text; comments and anything else are skipped.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                if let Some(end) = quoted_end(bytes, i + 1) {
                    out.push(Token::Text(&text[i + 1..end]));
                    i = end + 1;
                    continue;
                }
            }
            b'{' => out.push(Token::Open),
            b'}' => out.push(Token::Close),
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = bytes[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                if let Some(n) = text[i + 2..].find("*/") {
                    i += n + 4;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Where the quoted string whose text starts at `from` closes (a backslash escapes the next
/// byte, a newline excepted).
fn quoted_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some(i),
            b'\\' if bytes.get(i + 1).is_some_and(|&b| b != b'\n') => i += 2,
            b'\\' => return None,
            _ => i += 1,
        }
    }
    None
}

/// KeyValues text as a tree. Keys and values are quoted strings; a block without a key, or
/// whose key already holds a string, is read and dropped; a repeated block key reopens the
/// first block of that key in its place.
fn parse_kv(text: &str) -> Result<Kv> {
    // Each open block: its items, and its key and place in its parent (none for a dropped one).
    let mut stack: Vec<Open> = vec![(Vec::new(), None)];
    let mut key: Option<String> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(malformed("tooltips", "blocks nested too deep"));
                }
                let top = &mut stack.last_mut().expect("the root stays").0;
                let open = match key.take() {
                    Some(k) => match top.iter().position(|(x, _)| *x == k) {
                        None => Some((k, top.len(), Vec::new())),
                        Some(at) => match &mut top[at].1 {
                            Kv::Block(items) => Some((k, at, std::mem::take(items))),
                            Kv::Text(_) => None,
                        },
                    },
                    None => None,
                };
                match open {
                    Some((k, at, items)) => stack.push((items, Some((k, at)))),
                    None => stack.push((Vec::new(), None)),
                }
            }
            Token::Close => close(&mut stack),
            Token::Text(s) => match key.take() {
                None => key = Some(s.to_owned()),
                Some(k) => {
                    let top = &mut stack.last_mut().expect("the root stays").0;
                    if !top.iter().any(|(x, _)| *x == k) {
                        top.push((k, Kv::Text(s.to_owned())));
                    }
                }
            },
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    Ok(Kv::Block(
        stack.pop().map(|(items, _)| items).unwrap_or_default(),
    ))
}

/// An open block: its items so far, and its key and place in its parent.
type Open = (Vec<(String, Kv)>, Option<(String, usize)>);

/// Closes the innermost open block, putting it back in its place (the root stays open).
fn close(stack: &mut Vec<Open>) {
    if stack.len() < 2 {
        return;
    }
    let (items, place) = stack.pop().expect("more than the root");
    let Some((key, at)) = place else {
        return;
    };
    let parent = &mut stack.last_mut().expect("the root stays").0;
    match parent.get_mut(at) {
        Some((k, slot)) if *k == key => *slot = Kv::Block(items),
        _ => parent.insert(at.min(parent.len()), (key, Kv::Block(items))),
    }
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

/// The localization's `"key" "value"` lines, keys lower-cased, the first of a key kept.
fn localization_table(text: &str) -> HashMap<String, String> {
    let bytes = text.as_bytes();
    let mut table = HashMap::new();
    // Each try starts at a line start.
    let mut at = 0;
    while at < bytes.len() {
        let from = match localization_pair(text, at) {
            Some((key, value, end)) => {
                let value = value.replace("\\\"", "\"").replace("\\n", "\n");
                table.entry(key.to_lowercase()).or_insert(value);
                end
            }
            None => at,
        };
        match bytes[from..].iter().position(|&b| b == b'\n') {
            Some(n) => at = from + n + 1,
            None => break,
        }
    }
    table
}

/// A `"key" "value"` pair starting at line start `at` (whitespace may come first): the key, the
/// raw value and where the match ends.
fn localization_pair(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    let bytes = text.as_bytes();
    let space = |i: usize| bytes.get(i).is_some_and(|b| b.is_ascii_whitespace());
    let mut i = at;
    while space(i) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let key_start = i + 1;
    let key_end = key_start + bytes[key_start..].iter().position(|&b| b == b'"')?;
    if key_end == key_start {
        return None;
    }
    i = key_end + 1;
    if !space(i) {
        return None;
    }
    while space(i) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let end = quoted_end(bytes, i + 1)?;
    Some((&text[key_start..key_end], &text[i + 1..end], end + 1))
}
