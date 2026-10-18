use std::borrow::Cow;
use std::hash::BuildHasher;

use foldhash::fast::RandomState;
use hashbrown::HashTable;

use crate::bytes::Cursor;
use crate::error::{Error, Result, malformed};
use crate::source::Source;

const SIGNATURE: u32 = 0x55AA_1234;
/// `archive_index` of an entry whose bytes follow the directory tree in the directory file itself.
const IN_DIR_FILE: u16 = 0x7FFF;
const TERMINATOR: u16 = 0xFFFF;
const RECORD_LEN: usize = 18;

/// A VPK (v1 or v2) package's directory: `pak01_dir.vpk` with its `pak01_NNN.vpk` parts, or a
/// single-file package such as `maps/dota.vpk`.
///
/// The tree stays as read, ~21 MB for `pak01_dir.vpk`; the index adds 16 bytes per file, so its
/// 388 000 entries cost no allocation each.
pub struct Vpk {
    dir_path: String,
    /// `pak01` of `pak01_dir.vpk`: the parts are `pak01_000.vpk`, ...; `None` for a single file.
    part_prefix: Option<String>,
    /// Where entries stored in the directory file start: right after the tree.
    data_start: u64,
    tree: Box<[u8]>,
    files: Vec<Record>,
    index: Index,
    hasher: RandomState,
}

/// Entries by the hash of (directory, name, extension): (hash, index into `files`).
type Index = HashTable<(u64, u32)>;

/// Offsets into the tree of an entry's NUL-terminated strings and of its 18-byte record.
#[derive(Clone, Copy)]
struct Record {
    ext: u32,
    dir: u32,
    name: u32,
    info: u32,
}

/// One file of a [`Vpk`].
#[derive(Clone, Copy)]
pub struct Entry<'a> {
    vpk: &'a Vpk,
    record: Record,
}

impl Vpk {
    /// Reads the header and directory tree of the package at `dir_path` in `src`.
    pub fn open(src: &(impl Source + ?Sized), dir_path: &str) -> Result<Self> {
        let size = src.size(dir_path)?;
        let head = src.read_at(dir_path, 0, size.min(28) as usize)?;
        let mut header = Cursor::new(&head, "VPK header");
        let signature = header.u32()?;
        let version = header.u32()?;
        let tree_len = header.u32()? as usize;
        if signature != SIGNATURE {
            return Err(malformed(
                "VPK header",
                format!("{dir_path}: signature {signature:08x}"),
            ));
        }
        let header_len = match version {
            1 => 12,
            2 => 28,
            _ => {
                return Err(malformed(
                    "VPK header",
                    format!("{dir_path}: version {version}"),
                ));
            }
        };
        let tree = src.read_at(dir_path, header_len, tree_len)?;
        let part_prefix = dir_path.strip_suffix("_dir.vpk").map(str::to_owned);
        let hasher = RandomState::default();
        let (files, index) = index_tree(&tree, &hasher)?;
        Ok(Self {
            dir_path: dir_path.to_owned(),
            part_prefix,
            data_start: header_len + tree_len as u64,
            tree: tree.into_boxed_slice(),
            files,
            index,
            hasher,
        })
    }

    pub fn dir_path(&self) -> &str {
        &self.dir_path
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The entry at `path` (`models/heroes/pudge/pudge.vmdl_c`), compared byte for byte; a `\`
    /// separates directories too, as in the world's node references.
    pub fn get(&self, path: &str) -> Option<Entry<'_>> {
        let path: Cow<str> = if path.contains('\\') {
            path.replace('\\', "/").into()
        } else {
            path.into()
        };
        let path = path.as_ref();
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
        let (name, ext) = file.rsplit_once('.').unwrap_or((file, ""));
        let hash = self.hasher.hash_one((dir, name, ext));
        let &(_, i) = self.index.find(hash, |&(h, i)| {
            let entry = self.entry(i);
            h == hash && entry.name() == name && entry.dir() == dir && entry.ext() == ext
        })?;
        Some(self.entry(i))
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = Entry<'_>> {
        self.files.iter().map(|&record| Entry { vpk: self, record })
    }

    fn entry(&self, i: u32) -> Entry<'_> {
        Entry {
            vpk: self,
            record: self.files[i as usize],
        }
    }

    /// The whole file at `path`, its CRC checked.
    pub fn read_file(&self, src: &(impl Source + ?Sized), path: &str) -> Result<Vec<u8>> {
        let entry = self
            .get(path)
            .ok_or_else(|| Error::NotFound(format!("{}:{path}", self.dir_path)))?;
        self.read(src, entry)
    }

    /// The entry's bytes (preload, then the archive part's), its CRC checked.
    pub fn read(&self, src: &(impl Source + ?Sized), entry: Entry<'_>) -> Result<Vec<u8>> {
        let len = entry.archive_len() as usize;
        let tail = if len == 0 {
            Vec::new()
        } else {
            let (part, offset) = self.location(entry)?;
            src.read_at(&part, offset, len)?
        };
        let preload = entry.preload();
        let data = if preload.is_empty() {
            tail
        } else {
            [preload, &tail].concat()
        };
        let actual = crc32fast::hash(&data);
        if actual != entry.crc() {
            return Err(Error::Crc {
                path: format!("{}:{}", self.dir_path, entry.path()),
                expected: entry.crc(),
                actual,
            });
        }
        Ok(data)
    }

    /// The file and offset holding the entry's archive bytes.
    fn location(&self, entry: Entry<'_>) -> Result<(String, u64)> {
        let archive = entry.archive_index();
        if archive == IN_DIR_FILE {
            return Ok((
                self.dir_path.clone(),
                self.data_start + u64::from(entry.archive_offset()),
            ));
        }
        let prefix = self.part_prefix.as_ref().ok_or_else(|| {
            malformed(
                "VPK entry",
                format!("{}: part {archive} of a single-file package", entry.path()),
            )
        })?;
        Ok((
            format!("{prefix}_{archive:03}.vpk"),
            u64::from(entry.archive_offset()),
        ))
    }
}

/// Walks the tree (extension → directory → name → record), checking every bound.
fn index_tree(tree: &[u8], hasher: &RandomState) -> Result<(Vec<Record>, Index)> {
    let mut files = Vec::new();
    let mut index = HashTable::new();
    let mut c = Cursor::new(tree, "VPK directory");
    loop {
        let ext_at = c.pos() as u32;
        let ext = c.cstr()?;
        if ext.is_empty() {
            return Ok((files, index));
        }
        let ext = text(ext)?;
        loop {
            let dir_at = c.pos() as u32;
            let dir = c.cstr()?;
            if dir.is_empty() {
                break;
            }
            let dir = text(dir)?;
            loop {
                let name_at = c.pos() as u32;
                let name = c.cstr()?;
                if name.is_empty() {
                    break;
                }
                let info = c.pos() as u32;
                let mut record = Cursor::new(c.take(RECORD_LEN)?, "VPK entry");
                record.take(4)?;
                let preload = record.u16()? as usize;
                record.take(10)?;
                if record.u16()? != TERMINATOR {
                    return Err(malformed("VPK entry", format!("no terminator at {info}")));
                }
                c.take(preload)?;
                let hash = hasher.hash_one((dir, text(name)?, ext));
                index.insert_unique(hash, (hash, files.len() as u32), |&(h, _)| h);
                files.push(Record {
                    ext: ext_at,
                    dir: dir_at,
                    name: name_at,
                    info,
                });
            }
        }
    }
}

/// A tree string; the tree writes an empty directory or extension as a single space.
fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| malformed("VPK directory", "a name is not UTF-8"))
        .map(|s| if s == " " { "" } else { s })
}

impl<'a> Entry<'a> {
    fn string(&self, offset: u32) -> &'a str {
        let rest = &self.vpk.tree[offset as usize..];
        let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        // index_tree checked every string is UTF-8 and NUL-terminated.
        text(&rest[..len]).unwrap_or_default()
    }

    fn field(&self, at: usize, len: usize) -> &'a [u8] {
        let start = self.record.info as usize + at;
        &self.vpk.tree[start..start + len]
    }

    pub fn dir(&self) -> &'a str {
        self.string(self.record.dir)
    }

    pub fn name(&self) -> &'a str {
        self.string(self.record.name)
    }

    pub fn ext(&self) -> &'a str {
        self.string(self.record.ext)
    }

    /// `dir/name.ext`, as [`Vpk::get`] takes it.
    pub fn path(&self) -> String {
        let (dir, name, ext) = (self.dir(), self.name(), self.ext());
        let mut path = String::with_capacity(dir.len() + name.len() + ext.len() + 2);
        if !dir.is_empty() {
            path.push_str(dir);
            path.push('/');
        }
        path.push_str(name);
        if !ext.is_empty() {
            path.push('.');
            path.push_str(ext);
        }
        path
    }

    fn u16_at(&self, at: usize) -> u16 {
        let b = self.field(at, 2);
        u16::from_le_bytes([b[0], b[1]])
    }

    fn u32_at(&self, at: usize) -> u32 {
        let b = self.field(at, 4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    pub fn crc(&self) -> u32 {
        self.u32_at(0)
    }

    /// The first bytes of the file, stored in the directory tree.
    pub fn preload(&self) -> &'a [u8] {
        self.field(RECORD_LEN, self.u16_at(4) as usize)
    }

    pub fn archive_index(&self) -> u16 {
        self.u16_at(6)
    }

    pub fn archive_offset(&self) -> u32 {
        self.u32_at(8)
    }

    /// Bytes stored in the archive part, after the preload.
    pub fn archive_len(&self) -> u32 {
        self.u32_at(12)
    }

    /// The whole file's size.
    pub fn size(&self) -> u64 {
        self.preload().len() as u64 + u64::from(self.archive_len())
    }
}
