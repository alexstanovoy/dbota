//! A hero's voice pack: one cache file holding its pruned response rules, the mixer lines of
//! its voice events, and the compressed sound and lip sync of every line those rules can say,
//! so that the client reads one file per hero and only its own hero's.
//!
//! The layout is a list of named records:
//!
//! ```text
//! MAGIC (8 bytes) | record count (u32 LE) | per record: name length (u16 LE), name, data length
//! (u32 LE), data
//! ```
//!
//! A pack is untrusted (a cache can be handed on): every length is bounded before anything is
//! taken, names are plain and unique, and the records must fill the file exactly.

use std::fmt;
use std::ops::Range;

/// The first bytes of a pack; the last byte is the layout's version. A pack of another layout
/// is refused by name, so a cache of another build that kept the same import revision fails
/// loudly instead of being half read.
pub const MAGIC: [u8; 8] = *b"DBVOICE\x01";
/// Most bytes of a pack, and so of one hero's resident voice: eight times the largest pack of
/// the installed heroes (Shadow Fiend's, 3.8 MB of 162 lines, measured on Dota 7.41f).
pub const MAX_PACK_BYTES: usize = 32 << 20;
/// Most records of a pack: two per line (its sound, its lip sync) and a few more; the largest
/// hero voice script of the install names 2,099 lines (Pudge's, before pruning).
pub const MAX_RECORDS: usize = 8192;
/// Longest record name.
pub const MAX_NAME: usize = 64;

/// Why a pack is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackError {
    /// Not a pack of this layout (another build's, or not a pack at all).
    NotAPack,
    /// More than any pack holds.
    TooBig(String),
    /// A broken pack.
    Broken(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPack => write!(
                f,
                "not a voice pack of this build (made by another dbota build of the same import \
                 revision?): import the assets again"
            ),
            Self::TooBig(what) => write!(f, "the voice pack is too big: {what}"),
            Self::Broken(why) => write!(f, "the voice pack is broken: {why}"),
        }
    }
}

impl std::error::Error for PackError {}

/// A record's name and where its data is in the pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub data: Range<usize>,
}

/// Whether a record name is a plain one: short, printable, with no path tricks.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('/')
        && !name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/'))
}

/// The pack of `records`, in their order. A record over the bounds is a bug of the importer,
/// which bounds what it packs: it is refused here too.
pub fn write<'a>(
    records: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Vec<u8>, PackError> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&[0; 4]);
    let mut count = 0usize;
    for (name, data) in records {
        if !plain_name(name) {
            return Err(PackError::Broken(format!("record name {name:?}")));
        }
        count += 1;
        let size = out.len() + 2 + name.len() + 4 + data.len();
        if count > MAX_RECORDS || size > MAX_PACK_BYTES {
            return Err(PackError::TooBig(format!(
                "{count} records, {size} bytes; at most {MAX_RECORDS} records, \
                 {MAX_PACK_BYTES} bytes"
            )));
        }
        let name_len = u16::try_from(name.len()).expect("a plain name is short");
        let data_len = u32::try_from(data.len()).expect("a record fits the pack's bound");
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(data);
    }
    let count = u32::try_from(count).expect("bounded by MAX_RECORDS");
    out[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&count.to_le_bytes());
    Ok(out)
}

/// Reads a little-endian number of `N` bytes at `*at`, moving past it.
fn take<const N: usize>(bytes: &[u8], at: &mut usize) -> Result<[u8; N], PackError> {
    let end = at
        .checked_add(N)
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| PackError::Broken("cut short".to_owned()))?;
    let mut out = [0; N];
    out.copy_from_slice(&bytes[*at..end]);
    *at = end;
    Ok(out)
}

/// The records of a pack, checked: the magic, the bounds, plain unique names, and records that
/// fill the file exactly.
pub fn read(bytes: &[u8]) -> Result<Vec<Record>, PackError> {
    if bytes.len() > MAX_PACK_BYTES {
        return Err(PackError::TooBig(format!(
            "{} bytes, at most {MAX_PACK_BYTES}",
            bytes.len()
        )));
    }
    if bytes.get(..MAGIC.len()) != Some(&MAGIC[..]) {
        return Err(PackError::NotAPack);
    }
    let mut at = MAGIC.len();
    let count = u32::from_le_bytes(take(bytes, &mut at)?) as usize;
    if count > MAX_RECORDS {
        return Err(PackError::TooBig(format!(
            "{count} records, at most {MAX_RECORDS}"
        )));
    }
    let mut records: Vec<Record> = Vec::with_capacity(count);
    let mut names = std::collections::HashSet::with_capacity(count);
    for _ in 0..count {
        let name_len = usize::from(u16::from_le_bytes(take(bytes, &mut at)?));
        if name_len > MAX_NAME {
            return Err(PackError::Broken(format!(
                "a record name of {name_len} bytes"
            )));
        }
        let name = bytes
            .get(at..at + name_len)
            .and_then(|name| std::str::from_utf8(name).ok())
            .filter(|name| plain_name(name))
            .ok_or_else(|| PackError::Broken("a record name that is not plain".to_owned()))?;
        at += name_len;
        if !names.insert(name) {
            return Err(PackError::Broken(format!("record {name} twice")));
        }
        let data_len = u32::from_le_bytes(take(bytes, &mut at)?) as usize;
        let end = at
            .checked_add(data_len)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| PackError::Broken(format!("record {name} cut short")))?;
        records.push(Record {
            name: name.to_owned(),
            data: at..end,
        });
        at = end;
    }
    if at != bytes.len() {
        return Err(PackError::Broken(format!(
            "{} bytes after the last record",
            bytes.len() - at
        )));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Vec<u8> {
        write([("rules.json", &b"{}"[..]), ("sound/0.mp3", &b"ID3"[..])]).unwrap()
    }

    #[test]
    fn a_pack_reads_back_its_records() {
        let bytes = pack();
        let records = read(&bytes).unwrap();
        let named: Vec<_> = records
            .iter()
            .map(|r| (r.name.as_str(), &bytes[r.data.clone()]))
            .collect();
        assert_eq!(
            named,
            [("rules.json", &b"{}"[..]), ("sound/0.mp3", &b"ID3"[..])]
        );
    }

    #[test]
    fn a_pack_of_another_layout_is_refused_by_name() {
        let mut bytes = pack();
        bytes[7] = 2;
        assert_eq!(read(&bytes), Err(PackError::NotAPack));
        assert!(
            PackError::NotAPack
                .to_string()
                .contains("import the assets again")
        );
    }

    #[test]
    fn a_cut_pack_is_refused_at_every_length() {
        let bytes = pack();
        for len in MAGIC.len()..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "{len}");
        }
    }

    #[test]
    fn trailing_bytes_and_repeated_names_are_refused() {
        let mut bytes = pack();
        bytes.push(0);
        assert_eq!(
            read(&bytes),
            Err(PackError::Broken(
                "1 bytes after the last record".to_owned()
            ))
        );
        let twice = write([("a", &b""[..]), ("a", &b""[..])]).unwrap();
        assert_eq!(
            read(&twice),
            Err(PackError::Broken("record a twice".to_owned()))
        );
    }

    #[test]
    fn a_record_count_past_the_bound_is_refused_before_reading() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(read(&bytes), Err(PackError::TooBig(_))));
    }

    #[test]
    fn names_with_path_tricks_are_refused() {
        for name in ["../x", "/x", "a//b", "a/./b", "a b", ""] {
            assert!(write([(name, &b""[..])]).is_err(), "{name}");
        }
    }
}
