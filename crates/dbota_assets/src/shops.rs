//! The shop's layout data: which items each of Dota's shop categories lists, in Dota's order
//! (`scripts/shops.txt`), and the shop's own words (its category, tab and button names) out of
//! the install's localization, into the cache's `text/shop.json`.

use serde_json::{Map, Value as Json, json};

use crate::error::{Result, malformed};
use crate::keyvalues::{Token, localization_table, text, tokens};

/// Most categories kept.
const MAX_SHOPS: usize = 64;
/// Most items kept of one category.
const MAX_ITEMS: usize = 128;
/// Most localization strings kept.
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
    let categories: Vec<Json> = categories(text(shops)?)?
        .into_iter()
        .map(|(name, items)| json!([name, items]))
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
    serde_json::to_vec(&json!({ "shops": categories, "strings": strings }))
        .map_err(|e| malformed("shops", e.to_string()))
}

/// The categories of `shops.txt`: the blocks under its root, each with the `item` values it
/// lists in order. A category named twice keeps its first list; deeper blocks are skipped.
fn categories(text: &str) -> Result<Vec<(String, Vec<String>)>> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut depth = 0usize;
    // The key read and waiting for its value or block.
    let mut key: Option<&str> = None;
    // The category whose block is open, by its index in `out`, if it is kept.
    let mut open: Option<usize> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                depth += 1;
                if depth == 2 {
                    open = key
                        .filter(|name| !out.iter().any(|(n, _)| n == name))
                        .filter(|_| out.len() < MAX_SHOPS)
                        .map(|name| {
                            out.push((name.to_owned(), Vec::new()));
                            out.len() - 1
                        });
                }
                key = None;
            }
            Token::Close => {
                if depth == 0 {
                    return Err(malformed("shops", "a block closes that never opened"));
                }
                if depth == 2 {
                    open = None;
                }
                depth -= 1;
                key = None;
            }
            Token::Text(word) => match key.take() {
                None => key = Some(word),
                Some(k) => {
                    if depth == 2
                        && k == "item"
                        && let Some(at) = open
                        && out[at].1.len() < MAX_ITEMS
                    {
                        out[at].1.push(word.to_owned());
                    }
                }
            },
        }
    }
    Ok(out)
}
