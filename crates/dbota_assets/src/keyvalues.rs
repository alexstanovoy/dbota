//! KeyValues text as Valve's scripts and localization files write it: quoted keys and values,
//! braces, `//` and `/* */` comments. Every reader is bounded: the text is an untrusted file.

use std::collections::HashMap;

use crate::error::{Result, malformed};

/// Deepest nesting of a script's blocks.
const MAX_DEPTH: usize = 64;

/// A file as text, without a byte-order mark.
pub(crate) fn text(bytes: &[u8]) -> Result<&str> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| malformed("keyvalues", e.to_string()))
}

/// A KeyValues node: a string, or a block of keys in file order (a repeated key keeps its
/// first value; repeated blocks merge into the first).
#[derive(Debug)]
pub(crate) enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    pub(crate) fn get(&self, key: &str) -> Option<&Kv> {
        self.block()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub(crate) fn block(&self) -> Option<&[(String, Kv)]> {
        match self {
            Self::Block(items) => Some(items),
            Self::Text(_) => None,
        }
    }
}

pub(crate) enum Token<'a> {
    Text(&'a str),
    Open,
    Close,
}

/// The quoted strings and braces of KeyValues text; comments and anything else are skipped.
pub(crate) fn tokens(text: &str) -> Vec<Token<'_>> {
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
pub(crate) fn parse_kv(text: &str) -> Result<Kv> {
    // Each open block: its items, and its key and place in its parent (none for a dropped one).
    let mut stack: Vec<Open> = vec![(Vec::new(), None)];
    let mut key: Option<String> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(malformed("keyvalues", "blocks nested too deep"));
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

/// The localization's `"key" "value"` lines, keys lower-cased, the first of a key kept.
pub(crate) fn localization_table(text: &str) -> HashMap<String, String> {
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
