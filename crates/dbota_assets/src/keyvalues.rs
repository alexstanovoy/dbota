//! KeyValues text as Valve's scripts and localization files write it: quoted keys and values,
//! braces, `//` and `/* */` comments. Every reader is bounded: the text is an untrusted file.

use std::collections::HashMap;

use crate::error::{Result, malformed};

/// Deepest nesting of a script's blocks.
const MAX_DEPTH: usize = 64;

/// Largest file read; the largest, the localization, is ~1.8 MB.
const MAX_FILE: usize = 16 << 20;

/// A file as text, without a byte-order mark; one larger than any of them is an error.
pub(crate) fn text(bytes: &[u8]) -> Result<&str> {
    if bytes.len() > MAX_FILE {
        return Err(malformed("keyvalues", format!("{} bytes", bytes.len())));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| malformed("keyvalues", e.to_string()))
}

/// A KeyValues node: a string, or a block of keys in file order (a repeated key keeps its
/// first value; repeated blocks merge into the first).
#[derive(Debug)]
pub(crate) enum Kv {
    Text(String),
    Block(Block),
}

/// A block's items in file order, each key's first place found by hash: a script may hold
/// any number of keys, and each is looked up as it is read.
#[derive(Debug, Default)]
pub(crate) struct Block {
    items: Vec<(String, Kv)>,
    first: HashMap<String, usize>,
}

impl Block {
    fn position(&self, key: &str) -> Option<usize> {
        self.first.get(key).copied()
    }

    fn push(&mut self, key: String, value: Kv) {
        self.first.entry(key.clone()).or_insert(self.items.len());
        self.items.push((key, value));
    }
}

impl Kv {
    pub(crate) fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Self::Block(block) => block.position(key).map(|at| &block.items[at].1),
            Self::Text(_) => None,
        }
    }

    pub(crate) fn block(&self) -> Option<&[(String, Kv)]> {
        match self {
            Self::Block(block) => Some(&block.items),
            Self::Text(_) => None,
        }
    }
}

pub(crate) enum Token<'a> {
    Text(&'a str),
    Open,
    Close,
}

/// The quoted strings and braces of KeyValues text, one at a time; comments and anything else
/// are skipped.
pub(crate) struct Tokens<'a> {
    text: &'a str,
    at: usize,
    /// Where a `*/` was last searched for and none found: none follows any later `/*` either.
    no_comment_end: Option<usize>,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let bytes = self.text.as_bytes();
        while self.at < bytes.len() {
            let i = self.at;
            self.at += 1;
            match bytes[i] {
                b'"' => {
                    if let Some(end) = quoted_end(bytes, i + 1) {
                        self.at = end + 1;
                        return Some(Token::Text(&self.text[i + 1..end]));
                    }
                }
                b'{' => return Some(Token::Open),
                b'}' => return Some(Token::Close),
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    self.at = bytes[i..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |n| i + n);
                }
                b'/' if bytes.get(i + 1) == Some(&b'*') && self.no_comment_end.is_none() => {
                    match self.text[i + 2..].find("*/") {
                        Some(n) => self.at = i + n + 4,
                        None => self.no_comment_end = Some(i),
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// The tokens of `text`, read as they are asked for.
pub(crate) fn tokens(text: &str) -> Tokens<'_> {
    Tokens {
        text,
        at: 0,
        no_comment_end: None,
    }
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
    let mut stack: Vec<Open> = vec![(Block::default(), None)];
    let mut key: Option<String> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(malformed("keyvalues", "blocks nested too deep"));
                }
                let top = &mut stack.last_mut().expect("the root stays").0;
                let open = match key.take() {
                    Some(k) => match top.position(&k) {
                        None => Some((k, top.items.len(), Block::default())),
                        Some(at) => match &mut top.items[at].1 {
                            Kv::Block(block) => Some((k, at, std::mem::take(block))),
                            Kv::Text(_) => None,
                        },
                    },
                    None => None,
                };
                match open {
                    Some((k, at, block)) => stack.push((block, Some((k, at)))),
                    None => stack.push((Block::default(), None)),
                }
            }
            Token::Close => close(&mut stack),
            Token::Text(s) => match key.take() {
                None => key = Some(s.to_owned()),
                Some(k) => {
                    let top = &mut stack.last_mut().expect("the root stays").0;
                    if top.position(&k).is_none() {
                        top.push(k, Kv::Text(s.to_owned()));
                    }
                }
            },
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    Ok(Kv::Block(
        stack.pop().map(|(block, _)| block).unwrap_or_default(),
    ))
}

/// An open block: its items so far, and its key and place in its parent.
type Open = (Block, Option<(String, usize)>);

/// Closes the innermost open block, putting it back in its place (the root stays open): where
/// it was taken from when reopened, else after its parent's items (none came while it was open).
fn close(stack: &mut Vec<Open>) {
    if stack.len() < 2 {
        return;
    }
    let (block, place) = stack.pop().expect("more than the root");
    let Some((key, at)) = place else {
        return;
    };
    let parent = &mut stack.last_mut().expect("the root stays").0;
    match parent.items.get_mut(at) {
        Some((k, slot)) if *k == key => *slot = Kv::Block(block),
        _ => parent.push(key, Kv::Block(block)),
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
