use std::io;

/// Everything that can go wrong reading Valve's files. Every input is an untrusted local file, so
/// a malformed one is an ordinary error, never a panic.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: no such file")]
    NotFound(String),
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("{path}: bytes {offset}..+{len} run past its end ({size} bytes)")]
    OutOfRange {
        path: String,
        offset: u64,
        len: usize,
        size: u64,
    },
    #[error("{what}: {detail}")]
    Malformed { what: &'static str, detail: String },
    #[error("{path}: CRC32 {actual:08x}, the VPK directory says {expected:08x}")]
    Crc {
        path: String,
        expected: u32,
        actual: u32,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) fn malformed(what: &'static str, detail: impl Into<String>) -> Error {
    Error::Malformed {
        what,
        detail: detail.into(),
    }
}
