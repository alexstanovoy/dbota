//! Zip files as a prepared cache's container: a reader that trusts nothing in the archive and a
//! writer for `dbota export-cache`.
//!
//! Only what a cache needs is read: one disk, no zip64 (a cache is under 4 GB), no encryption, no
//! links, members stored or deflated. Every size is bounded before a byte is inflated, a member
//! is inflated into no more than its declared size and its CRC-32 must match, and no two members
//! overlap, so reading all of them costs no more than the archive is long.

use std::io::{self, Write};

use heatel::Slice;

use crate::pack::{Container, Entry, MAX_FILE, MAX_FILES, PackError, shown, without_root};

const END_SIGNATURE: [u8; 4] = *b"PK\x05\x06";
const DIRECTORY_SIGNATURE: [u8; 4] = *b"PK\x01\x02";
const LOCAL_SIGNATURE: [u8; 4] = *b"PK\x03\x04";
const END_LEN: usize = 22;
const DIRECTORY_LEN: usize = 46;
const LOCAL_LEN: usize = 30;
/// An archive's comment is at most this long (a 16-bit length), so its end record is that close
/// to its end.
const MAX_COMMENT: u64 = 65_535;
/// Most members listed, folders included.
const MAX_MEMBERS: usize = 4 * MAX_FILES;
/// Longest central directory read.
const MAX_DIRECTORY: u64 = 8 << 20;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// A deflated member is no longer than its file and this much more (stored blocks, Huffman codes
/// longer than the bytes they code): a longer one is not worth reading.
const fn packed_limit(size: u64) -> u64 {
    size + (size >> 3) + 64
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn invalid(why: impl Into<String>) -> PackError {
    PackError::Invalid(format!("the zip {}", why.into()))
}

fn read(slice: &impl Slice, offset: u64, len: u64) -> Result<Vec<u8>, PackError> {
    let end = offset.checked_add(len).filter(|end| *end <= slice.size());
    let (Some(_), Ok(len)) = (end, usize::try_from(len)) else {
        return Err(invalid("is shorter than it says"));
    };
    slice
        .read_at(offset, len)
        .map_err(|e| PackError::Io(format!("cannot read the zip: {e}")))
}

/// A file member of the archive, as its central directory describes it.
struct Member {
    /// The name as the archive has it (the entry's path is this without a root folder).
    name: String,
    size: u64,
    method: u16,
    crc: u32,
    packed: u64,
    local: u64,
}

/// A zip read through a [`Slice`] (a file, a browser's `File`, bytes).
pub struct ZipContainer<S> {
    slice: S,
    entries: Vec<Entry>,
    members: Vec<Member>,
    /// Where the central directory starts: the members' data end before it.
    directory: u64,
}

impl<S: Slice> ZipContainer<S> {
    /// Reads the archive's directory; no member is read yet.
    pub fn open(slice: S) -> Result<Self, PackError> {
        let (count, offset, size) = end_of_directory(&slice)?;
        let directory = read(&slice, offset, size)?;
        let mut members = parse_directory(&directory, count)?;
        check_layout(&mut members, offset)?;
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        let cut = without_root(&names);
        let entries = members
            .iter()
            .map(|m| Entry {
                path: m.name[cut..].to_owned(),
                size: m.size,
            })
            .collect();
        Ok(Self {
            slice,
            entries,
            members,
            directory: offset,
        })
    }

    fn inflate(&self, member: &Member, entry: &Entry, data: Vec<u8>) -> Result<Vec<u8>, PackError> {
        if member.method == STORED {
            return Ok(data);
        }
        if entry.size == 0 {
            return Ok(Vec::new());
        }
        let size =
            usize::try_from(entry.size).map_err(|_| PackError::TooBig(shown(&entry.path)))?;
        miniz_oxide::inflate::decompress_to_vec_with_limit(&data, size).map_err(|e| {
            invalid(format!(
                "member {} does not inflate: {:?}",
                shown(&entry.path),
                e.status
            ))
        })
    }
}

/// The directory's place and the members' count, from the end-of-directory record.
fn end_of_directory(slice: &impl Slice) -> Result<(usize, u64, u64), PackError> {
    let size = slice.size();
    if size < END_LEN as u64 {
        return Err(invalid("is too short to be a zip file"));
    }
    let tail_len = size.min(END_LEN as u64 + MAX_COMMENT);
    let tail = read(slice, size - tail_len, tail_len)?;
    // The record is the last thing in the file, its comment (whose length it holds) after it.
    let at = (0..=tail.len() - END_LEN)
        .rev()
        .find(|&i| {
            tail[i..i + 4] == END_SIGNATURE
                && usize::from(le16(&tail, i + 20)) == tail.len() - END_LEN - i
        })
        .ok_or_else(|| invalid("has no end-of-directory record: it is not a zip file"))?;
    let record = &tail[at..];
    let (this_disk, directory_disk) = (le16(record, 4), le16(record, 6));
    let (on_disk, count) = (le16(record, 8), le16(record, 10));
    let (size_of_directory, offset) = (le32(record, 12), le32(record, 16));
    if this_disk != 0 || directory_disk != 0 || on_disk != count {
        return Err(invalid("is split over several files"));
    }
    if count == u16::MAX || size_of_directory == u32::MAX || offset == u32::MAX {
        return Err(invalid("is a zip64 archive; a cache is smaller than that"));
    }
    let (size_of_directory, offset) = (u64::from(size_of_directory), u64::from(offset));
    let record_at = size - (tail.len() - at) as u64;
    if size_of_directory > MAX_DIRECTORY || usize::from(count) > MAX_MEMBERS {
        return Err(PackError::TooBig(
            "the zip lists too many members".to_owned(),
        ));
    }
    if offset + size_of_directory > record_at {
        return Err(invalid("has a directory outside of itself"));
    }
    Ok((usize::from(count), offset, size_of_directory))
}

/// The file members of the central directory (folders and macOS's `__MACOSX` copies left out).
fn parse_directory(directory: &[u8], count: usize) -> Result<Vec<Member>, PackError> {
    let (mut at, mut files) = (0, Vec::new());
    for _ in 0..count {
        if directory.len() < at + DIRECTORY_LEN || directory[at..at + 4] != DIRECTORY_SIGNATURE {
            return Err(invalid("has a broken directory"));
        }
        let h = &directory[at..];
        let (flags, method) = (le16(h, 8), le16(h, 10));
        let (crc, packed, size) = (le32(h, 16), le32(h, 20), le32(h, 24));
        let (name_len, extra_len, comment_len) = (
            usize::from(le16(h, 28)),
            usize::from(le16(h, 30)),
            usize::from(le16(h, 32)),
        );
        let (disk, external, local) = (le16(h, 34), le32(h, 38), le32(h, 42));
        let next = at + DIRECTORY_LEN + name_len + extra_len + comment_len;
        if directory.len() < next {
            return Err(invalid("has a broken directory"));
        }
        let name = &directory[at + DIRECTORY_LEN..at + DIRECTORY_LEN + name_len];
        let made_on_unix = le16(h, 4) >> 8 == 3;
        at = next;
        let name =
            std::str::from_utf8(name).map_err(|_| invalid("names a file in no known encoding"))?;
        if name.ends_with('/') || name.starts_with("__MACOSX/") {
            continue;
        }
        if flags & 0x41 != 0 {
            return Err(invalid(format!("encrypts {}", shown(name))));
        }
        if method != STORED && method != DEFLATED {
            return Err(invalid(format!(
                "packs {} with method {method}; only stored and deflated files are read",
                shown(name)
            )));
        }
        if disk != 0 || [packed, size, local].contains(&u32::MAX) {
            return Err(invalid(format!(
                "describes {} in a way a cache never is",
                shown(name)
            )));
        }
        // A link is not a file: never followed, never taken.
        if made_on_unix && !matches!((external >> 16) & 0o170_000, 0 | 0o100_000) {
            return Err(PackError::Unsafe(shown(name)));
        }
        if u64::from(size) > MAX_FILE || u64::from(packed) > packed_limit(u64::from(size)) {
            return Err(PackError::TooBig(format!(
                "{} is {size} bytes",
                shown(name)
            )));
        }
        if method == STORED && packed != size {
            return Err(invalid(format!("stores {} with two sizes", shown(name))));
        }
        if files.len() >= MAX_FILES + 2 {
            return Err(PackError::TooBig(format!(
                "it has more than {} files, a cache has at most {MAX_FILES}",
                MAX_FILES + 2
            )));
        }
        files.push(Member {
            name: name.to_owned(),
            size: u64::from(size),
            method,
            crc,
            packed: u64::from(packed),
            local: u64::from(local),
        });
    }
    if at != directory.len() {
        return Err(invalid("has more in its directory than it counts"));
    }
    Ok(files)
}

/// The members' data lie one after the other, before the directory: none inside another, so
/// reading them all reads the archive once at most.
fn check_layout(members: &mut [Member], directory: u64) -> Result<(), PackError> {
    members.sort_by_key(|m| m.local);
    let mut free = 0;
    for member in members.iter() {
        let name = shown(&member.name);
        if member.local < free {
            return Err(invalid(format!("has {name} inside another file")));
        }
        free = member.local + (LOCAL_LEN + member.name.len()) as u64 + member.packed;
        if free > directory {
            return Err(invalid(format!("has {name} past its end")));
        }
    }
    Ok(())
}

impl<S: Slice> Container for ZipContainer<S> {
    fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn read(&self, at: usize) -> Result<Vec<u8>, PackError> {
        let (member, entry) = (&self.members[at], &self.entries[at]);
        let header = read(&self.slice, member.local, LOCAL_LEN as u64)?;
        if header[..4] != LOCAL_SIGNATURE {
            return Err(invalid(format!("has no file {}", shown(&entry.path))));
        }
        let (name_len, extra_len) = (u64::from(le16(&header, 26)), u64::from(le16(&header, 28)));
        let name = read(&self.slice, member.local + LOCAL_LEN as u64, name_len)?;
        if name != member.name.as_bytes() {
            return Err(invalid(format!(
                "names {} twice, differently",
                shown(&entry.path)
            )));
        }
        let start = member.local + LOCAL_LEN as u64 + name_len + extra_len;
        if start + member.packed > self.directory {
            return Err(invalid(format!("has {} past its end", shown(&entry.path))));
        }
        let data = read(&self.slice, start, member.packed)?;
        let bytes = self.inflate(member, entry, data)?;
        if bytes.len() as u64 != entry.size || crc32fast::hash(&bytes) != member.crc {
            return Err(invalid(format!("holds {} damaged", shown(&entry.path))));
        }
        Ok(bytes)
    }
}

/// Writes a zip: one member after the other, then the directory. Names are the cache's own
/// plain paths; the times are fixed (1980-01-01), so the same cache is the same archive.
pub struct ZipWriter<W> {
    out: W,
    written: u64,
    directory: Vec<u8>,
    count: u16,
}

/// 1980-01-01, the first day a zip can date.
const DOS_DATE: u16 = (1 << 5) | 1;

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            directory: Vec::new(),
            count: 0,
        }
    }

    /// Adds `data` as the file `name`, deflated when that makes it noticeably shorter.
    pub fn add(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let too_big = |what: &str| io::Error::other(format!("{what} is too big for a zip file"));
        let size = u32::try_from(data.len()).map_err(|_| too_big(name))?;
        let deflated = miniz_oxide::deflate::compress_to_vec(data, 6);
        let (method, packed) = if deflated.len() < data.len() - data.len() / 16 {
            (DEFLATED, &deflated[..])
        } else {
            (STORED, data)
        };
        let local = u32::try_from(self.written).map_err(|_| too_big("the cache"))?;
        let packed_len = u32::try_from(packed.len()).map_err(|_| too_big(name))?;
        let name_len = u16::try_from(name.len()).map_err(|_| too_big(name))?;
        let count = self
            .count
            .checked_add(1)
            .ok_or_else(|| too_big("the cache"))?;
        let record = Record {
            method,
            crc: crc32fast::hash(data),
            packed: packed_len,
            size,
            name_len,
        };
        let mut header = Vec::with_capacity(LOCAL_LEN + name.len());
        header.extend_from_slice(&LOCAL_SIGNATURE);
        record.put(&mut header, false);
        header.extend_from_slice(name.as_bytes());
        self.out.write_all(&header)?;
        self.out.write_all(packed)?;
        self.written += (header.len() + packed.len()) as u64;
        self.directory.extend_from_slice(&DIRECTORY_SIGNATURE);
        record.put(&mut self.directory, true);
        self.directory.extend_from_slice(&local.to_le_bytes());
        self.directory.extend_from_slice(name.as_bytes());
        self.count = count;
        Ok(())
    }

    /// Writes the directory and gives the writer back.
    pub fn finish(mut self) -> io::Result<W> {
        let offset = u32::try_from(self.written)
            .map_err(|_| io::Error::other("the cache is too big for a zip file"))?;
        let size = u32::try_from(self.directory.len())
            .map_err(|_| io::Error::other("the directory is too big for a zip file"))?;
        let mut end = Vec::with_capacity(END_LEN);
        end.extend_from_slice(&END_SIGNATURE);
        end.extend_from_slice(&[0; 4]);
        end.extend_from_slice(&self.count.to_le_bytes());
        end.extend_from_slice(&self.count.to_le_bytes());
        end.extend_from_slice(&size.to_le_bytes());
        end.extend_from_slice(&offset.to_le_bytes());
        end.extend_from_slice(&[0; 2]);
        self.out.write_all(&self.directory)?;
        self.out.write_all(&end)?;
        Ok(self.out)
    }
}

/// What a local header and a directory record have in common, in the order they hold it.
struct Record {
    method: u16,
    crc: u32,
    packed: u32,
    size: u32,
    name_len: u16,
}

impl Record {
    /// From the "version needed" field to the extra field's length; a directory record has
    /// "version made by" before and a comment, disk, attributes after (then the local offset).
    fn put(&self, out: &mut Vec<u8>, directory: bool) {
        if directory {
            // Made by version 2.0 on MS-DOS.
            out.extend_from_slice(&[20, 0]);
        }
        // Version 2.0 needed, no flags.
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&self.method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.packed.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.name_len.to_le_bytes());
        // No extra field.
        out.extend_from_slice(&0u16.to_le_bytes());
        if directory {
            // No comment, disk 0, no attributes.
            out.extend_from_slice(&[0; 2 + 2 + 2 + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_of(name: &str, data: &[u8]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Vec::new());
        zip.add(name, data).expect("a member");
        zip.finish().expect("the directory")
    }

    /// Where the member's directory record has its uncompressed size.
    fn size_field(zip: &[u8]) -> usize {
        zip.windows(4)
            .position(|w| w == DIRECTORY_SIGNATURE)
            .expect("a directory record")
            + 24
    }

    /// A member that inflates into more than its directory says (a zip bomb states a small size)
    /// is stopped at the stated size, not inflated whole.
    #[test]
    fn a_member_that_inflates_past_its_stated_size_is_refused() {
        let mut zip = zip_of("hud/a.png", &vec![0; 8 << 20]);
        let at = size_field(&zip);
        zip[at..at + 4].copy_from_slice(&1000u32.to_le_bytes());
        let container = ZipContainer::open(zip).expect("a directory");

        let why = container.read(0).expect_err("a bomb");

        assert!(why.to_string().contains("does not inflate"), "{why}");
    }

    #[test]
    fn a_member_stated_past_the_file_limit_is_refused_before_it_is_read() {
        let mut zip = zip_of("hud/a.png", b"abc");
        let at = size_field(&zip);
        zip[at..at + 4].copy_from_slice(&u32::try_from(MAX_FILE + 1).expect("fits").to_le_bytes());

        let why = ZipContainer::open(zip).err().expect("too big");

        assert!(matches!(why, PackError::TooBig(_)), "{why}");
    }

    /// A stored member whose bytes changed fails its CRC.
    #[test]
    fn a_damaged_member_is_refused() {
        let mut zip = zip_of("hud/a.png", &(0..255).collect::<Vec<u8>>());
        zip[LOCAL_LEN + "hud/a.png".len() + 3] ^= 1;
        let container = ZipContainer::open(zip).expect("a directory");

        let why = container.read(0).expect_err("damaged");

        assert!(why.to_string().contains("holds hud/a.png damaged"), "{why}");
    }
}
