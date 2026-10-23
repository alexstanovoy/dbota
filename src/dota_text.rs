//! The names and descriptions of items and abilities, as Dota writes them.
//!
//! They come from the asset cache (`text/tooltips.json`, which
//! `dbota import` builds out of the player's own Dota install) and
//! are never part of this repository. Without the cache there is no text
//! here at all, and the interface falls back to the simulator's own plain
//! names and numbers.

use std::{collections::HashMap, path::Path};

use bevy::prelude::*;
use serde_json::Value;

/// Where the text file lies in the cache.
const FILE: &str = "text/tooltips.json";
/// An upper bound on the file, so a broken cache cannot fill the memory.
const MAX_ENTRIES: usize = 4096;
/// Most aliases kept of one entry.
const MAX_ALIASES: usize = 16;

/// One item or ability, as Dota's files give it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub name: String,
    /// The description, with its markup and `%value%` placeholders.
    pub description: String,
    pub lore: String,
    /// The notes, which may hold placeholders as well.
    pub notes: Vec<String>,
    /// Dota's own numbers by lower-cased key, one per level (one for an item).
    pub values: HashMap<String, Vec<String>>,
    /// Dota's price, wait and mana, one per level, for the audit.
    pub price: Vec<String>,
    pub cooldown: Vec<String>,
    pub mana: Vec<String>,
    /// The labels of the numbers worth a line, in Dota's order: lower-cased
    /// key and label.
    pub labels: Vec<(String, String)>,
    /// The other names the shop's search finds an item by.
    pub aliases: Vec<String>,
}

/// Everything the text file of the cache holds, by Dota's name for each
/// (`item_blink`, `pudge_meat_hook`).
#[derive(Resource, Clone, Debug, Default)]
pub struct Texts {
    entries: HashMap<String, Entry>,
}

impl Texts {
    /// Reads the cache's text file; empty when there is none.
    pub fn load(root: Option<&Path>) -> Texts {
        let Some(bytes) = root.and_then(|root| crate::cachefs::read_small(root, FILE)) else {
            return Texts::default();
        };
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => Texts {
                entries: map
                    .iter()
                    .take(MAX_ENTRIES)
                    .filter_map(|(key, value)| Some((key.clone(), entry(value)?)))
                    .collect(),
            },
            Ok(_) => {
                warn!("{FILE}: not an object of entries");
                Texts::default()
            }
            Err(err) => {
                warn!("{FILE}: {err}");
                Texts::default()
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn entry(value: &Value) -> Option<Entry> {
    let text = |field: &str| {
        value
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let values = value
        .get("values")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, v)| Some((key.to_lowercase(), levels(v.as_str()?))))
                .collect()
        })
        .unwrap_or_default();
    let labels = value
        .get("labels")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    Some((
                        pair.first()?.as_str()?.to_lowercase(),
                        pair.get(1)?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Entry {
        name: value.get("name")?.as_str()?.to_string(),
        description: text("description"),
        lore: text("lore"),
        notes: value
            .get("notes")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|n| n.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        aliases: value
            .get("aliases")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .take(MAX_ALIASES)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        price: levels(&text("cost")),
        cooldown: levels(&text("cooldown")),
        mana: levels(&text("mana")),
        values,
        labels,
    })
}

/// A script value, one number per level.
pub fn levels(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(number).collect()
}

/// A number as Dota prints it: no trailing zeros, no leading ones.
pub fn number(raw: &str) -> String {
    match raw.parse::<f64>() {
        Ok(n) => plain(n),
        Err(_) => raw.to_string(),
    }
}

/// A number to at most two decimals with no trailing zeros.
pub fn plain(n: f64) -> String {
    let text = format!("{n:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_string()
}

/// A piece of Dota's markup.
#[derive(Clone, Debug, PartialEq)]
pub enum Token<'a> {
    Text(&'a str),
    /// A `%key%` placeholder, lower-cased.
    Value(String),
    /// `<br>` or a newline.
    Break,
    /// `<h1>`.
    HeaderOpen,
    /// `</h1>`.
    HeaderClose,
    /// `<b>` or `</b>`.
    Bold(bool),
    /// `<font color='#rrggbb'>` (the colour) or `</font>` (none).
    Font(Option<Color>),
}

/// Splits a string of Dota's markup (`<h1>`, `<br>`, `<b>`, `<font>`,
/// `%value%`, `%%`) into tokens. Other tags are dropped.
pub fn tokens(source: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = source;
    while !rest.is_empty() {
        let stop = rest.find(['<', '%', '\n']).unwrap_or(rest.len());
        if stop > 0 {
            out.push(Token::Text(&rest[..stop]));
            rest = &rest[stop..];
            continue;
        }
        if let Some(after) = rest.strip_prefix('\n') {
            out.push(Token::Break);
            rest = after;
        } else if let Some(after) = rest.strip_prefix("%%") {
            out.push(Token::Text("%"));
            rest = after;
        } else if rest.starts_with('%') {
            match rest[1..].find('%').map(|end| (&rest[1..1 + end], 2 + end)) {
                Some((key, len)) if is_key(key) => {
                    out.push(Token::Value(key.to_lowercase()));
                    rest = &rest[len..];
                }
                _ => {
                    out.push(Token::Text("%"));
                    rest = &rest[1..];
                }
            }
        } else {
            let Some(end) = rest.find('>') else {
                out.push(Token::Text(rest));
                break;
            };
            out.extend(tag(&rest[1..end]));
            rest = &rest[end + 1..];
        }
    }
    out
}

fn is_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn tag(inner: &str) -> Option<Token<'static>> {
    let name = inner.split_whitespace().next()?.to_ascii_lowercase();
    Some(match name.as_str() {
        "br" | "br/" | "/br" => Token::Break,
        "h1" => Token::HeaderOpen,
        "/h1" => Token::HeaderClose,
        "b" => Token::Bold(true),
        "/b" => Token::Bold(false),
        "/font" => Token::Font(None),
        "font" => Token::Font(font_color(inner)),
        _ => return None,
    })
}

fn font_color(inner: &str) -> Option<Color> {
    let at = inner.find("color")?;
    let hex = inner[at..].split(['\'', '"']).nth(1)?;
    Srgba::hex(hex).ok().map(Color::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_splits_into_headers_values_and_breaks() {
        let t = tokens("<h1>Active: A</h1> Go %range%%% far.<br>Next\nline");
        assert_eq!(
            t,
            vec![
                Token::HeaderOpen,
                Token::Text("Active: A"),
                Token::HeaderClose,
                Token::Text(" Go "),
                Token::Value("range".into()),
                Token::Text("%"),
                Token::Text(" far."),
                Token::Break,
                Token::Text("Next"),
                Token::Break,
                Token::Text("line"),
            ]
        );
    }

    #[test]
    fn numbers_lose_their_trailing_zeros() {
        assert_eq!(levels("3.0 00 -2.50 1600.0"), ["3", "0", "-2.5", "1600"]);
    }
}
