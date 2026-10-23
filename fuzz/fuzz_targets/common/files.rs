//! An install made of the fuzzer's input. The input splits into files at [`SEPARATOR`], each a
//! line with its path then its bytes: the first file goes where the target puts it, the others
//! under their own paths. A file whose path line is empty stands for the next path the files so
//! far name (a KV3 string of one of their blocks, or a path-like word of a text) that no file
//! holds. The files are packed into an in-memory VPK, so the code under test reads them through
//! the same index, CRC and bounds as a real install's. `seeds.py` writes such inputs from a real
//! install.
#![allow(dead_code)]

use std::collections::{HashSet, VecDeque};

use dbota_assets::kv3::{self, Value};
use dbota_assets::{Resource, SliceSource, Vpk};

/// Splits the fuzzer's input into files.
pub const SEPARATOR: &[u8] = b"\n%%dbota-file%%\n";
/// Most files an input becomes.
pub const MAX_FILES: usize = 24;
/// Most strings looked at per file.
const MAX_STRINGS: usize = 4096;
/// What a named path may end in to be given a file.
const EXTENSIONS: [&str; 9] = [
    ".vmdl", ".vmat", ".vtex", ".vwnod", ".vents", ".vwrld", ".vsnd", ".txt", "_c",
];

/// The input's files, in order: each its path (empty for none) and bytes.
pub fn split(data: &[u8]) -> Vec<(&str, &[u8])> {
    let mut chunks = Vec::new();
    let mut rest = data;
    while chunks.len() + 1 < MAX_FILES {
        match find(rest, SEPARATOR) {
            Some(at) => {
                chunks.push(&rest[..at]);
                rest = &rest[at + SEPARATOR.len()..];
            }
            None => break,
        }
    }
    chunks.push(rest);
    chunks
        .into_iter()
        .map(|chunk| {
            let (path, bytes) = match chunk.iter().position(|&b| b == b'\n') {
                Some(at) => (&chunk[..at], &chunk[at + 1..]),
                None => (&chunk[..0], chunk),
            };
            (std::str::from_utf8(path).unwrap_or_default(), bytes)
        })
        .collect()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The compiled file a resource name stands for (`x.vmat` is `x.vmat_c`), `/`-separated.
pub fn compiled(name: &str) -> String {
    let name = name.replace('\\', "/");
    if name.ends_with("_c") || name.ends_with(".txt") {
        name
    } else {
        format!("{name}_c")
    }
}

/// The paths a file names: strings of its KV3 blocks (a world node is named by its prefix,
/// without extension), and path-like words of its text.
pub fn named(bytes: &[u8]) -> Vec<String> {
    let mut strings = strings(bytes);
    if strings.is_empty() {
        words(bytes, &mut strings);
    }
    let mut out = Vec::new();
    for s in strings
        .iter()
        .filter(|s| s.contains('/') || s.contains('\\'))
    {
        if EXTENSIONS.iter().any(|e| s.ends_with(e)) {
            out.push(compiled(s));
        } else if !s.contains('.') {
            out.push(format!("{}.vwnod_c", s.replace('\\', "/")));
        }
    }
    out
}

/// The strings of a resource: its external references, its KV3 blocks' and its `DATA`'s.
pub fn strings(bytes: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    if let Ok(resource) = Resource::parse(bytes) {
        if let Ok(refs) = resource.external_refs() {
            strings.extend(refs.into_iter().map(|r| r.name));
        }
        for block in resource.blocks() {
            if let Ok(doc) = kv3::decode(resource.bytes(block)) {
                collect(&doc.root, &mut strings);
            }
        }
        if let Some(Ok(data)) = resource.data() {
            collect(&data, &mut strings);
        }
    }
    strings
}

fn collect(value: &Value, out: &mut Vec<String>) {
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        if out.len() >= MAX_STRINGS {
            return;
        }
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(items) => stack.extend(items),
            Value::Object(o) => stack.extend(o.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
}

fn words(bytes: &[u8], out: &mut Vec<String>) {
    let path_byte = |b: &u8| b.is_ascii_alphanumeric() || b"_-./\\".contains(b);
    for word in bytes.split(|b| !path_byte(b)) {
        if out.len() >= MAX_STRINGS {
            return;
        }
        if word.len() > 4
            && let Ok(word) = std::str::from_utf8(word)
        {
            out.push(word.to_owned());
        }
    }
}

/// The files of the input: the first at `root` (or at each of `roots`, the input's files in
/// order), the named ones at their paths, and the unnamed rest at the paths the files name
/// that none holds, in the order they are found.
pub fn resolve(roots: &[&str], chunks: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut held = HashSet::new();
    let mut unnamed = VecDeque::new();
    for (i, &(path, bytes)) in chunks.iter().enumerate() {
        let path = roots.get(i).copied().unwrap_or(path);
        if path.is_empty() {
            unnamed.push_back(bytes);
        } else if packable(path) && held.insert(path.to_owned()) {
            out.push((path.to_owned(), bytes.to_vec()));
        }
    }
    let mut at = 0;
    while at < out.len() && !unnamed.is_empty() {
        for path in named(&out[at].1) {
            if packable(&path) && !held.contains(&path) {
                let Some(bytes) = unnamed.pop_front() else {
                    break;
                };
                held.insert(path.clone());
                out.push((path, bytes.to_vec()));
            }
        }
        at += 1;
    }
    out
}

/// Whether a VPK's tree can hold `path`: a file name, no NUL.
fn packable(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let name = file.rsplit_once('.').map_or(file, |(name, _)| name);
    !name.is_empty() && !path.contains('\0') && !path.split('/').any(|p| p == " ")
}

/// A VPK (version 1) holding `files` after its directory tree.
pub fn vpk(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut tree = Vec::new();
    let mut data = Vec::new();
    for (path, bytes) in files {
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
        let (name, ext) = file.rsplit_once('.').unwrap_or((file, ""));
        let blank = |s: &str| {
            if s.is_empty() {
                " ".to_owned()
            } else {
                s.to_owned()
            }
        };
        for part in [blank(ext), blank(dir), name.to_owned()] {
            tree.extend_from_slice(part.as_bytes());
            tree.push(0);
        }
        tree.extend_from_slice(&crc32fast::hash(bytes).to_le_bytes());
        tree.extend_from_slice(&0u16.to_le_bytes());
        tree.extend_from_slice(&0x7FFFu16.to_le_bytes());
        tree.extend_from_slice(&(data.len() as u32).to_le_bytes());
        tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
        // Each file its own extension and directory: no grouping to get wrong.
        tree.extend_from_slice(&[0, 0]);
        data.extend_from_slice(bytes);
    }
    tree.push(0);
    let mut out = Vec::with_capacity(12 + tree.len() + data.len());
    out.extend_from_slice(&0x55AA_1234u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    out.extend_from_slice(&tree);
    out.extend_from_slice(&data);
    out
}

/// Files by path, as a package holds them.
pub type Files = [(String, Vec<u8>)];

/// An install with each package of `packages` (its path, its files) packed.
pub fn install(packages: &[(&str, &Files)]) -> SliceSource<Vec<u8>> {
    let mut src = SliceSource::default();
    for (path, files) in packages {
        src.insert(*path, vpk(files));
    }
    src
}

/// The package at `path` of `src`.
pub fn open(src: &SliceSource<Vec<u8>>, path: &str) -> Option<Vpk> {
    Vpk::open(src, path).ok()
}
