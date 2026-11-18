//! Why an asset cache is refused: the errors of the checks its index and its files pass
//! ([`crate::index`]) before the client reads anything of it, natively and in the browser.

use std::fmt;

pub use crate::index::{MAX_FILE, MAX_FILES, MAX_TOTAL, size_limit};

/// Why an asset cache is refused.
#[derive(Debug, PartialEq, Eq)]
pub enum PackError {
    /// A file's name that must not be in a cache (shown made printable).
    Unsafe(String),
    /// It is not a cache, or a broken one.
    Invalid(String),
    /// More than a cache is.
    TooBig(String),
    /// A file could not be read or written.
    Io(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsafe(path) => write!(f, "The asset cache holds a file it must not: {path}"),
            Self::Invalid(why) => write!(f, "This is not a usable asset cache: {why}"),
            Self::TooBig(what) => write!(f, "This is too big for an asset cache: {what}"),
            Self::Io(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for PackError {}

/// `text` as printable ASCII, cut: it comes from a cache file that may be anything.
pub(crate) fn shown(text: &str) -> String {
    const SHOWN: usize = 64;
    let mut out: String = text
        .chars()
        .take(SHOWN)
        .map(|c| if c.is_ascii_graphic() { c } else { '?' })
        .collect();
    if text.chars().count() > SHOWN {
        out.push_str("...");
    }
    out
}
