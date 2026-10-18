//! Reads Valve's Source 2 formats straight from a local Dota 2 install: VPK packages, compiled
//! resource containers and binary KV3, through a [`Source`] that is a folder on disk natively
//! and the user's picked files in a browser. Every parser is bounded: the input is an untrusted
//! local file, and a malformed one is an [`Error`], never a panic or an unbounded allocation.

mod budget;
mod bytes;
mod error;
pub mod kv3;
#[cfg(any(unix, windows))]
mod locate;
mod ntro;
mod resource;
mod source;
mod vpk;

pub use error::{Error, Result};
#[cfg(any(unix, windows))]
pub use locate::find_game_dir;
pub use resource::{Block, ExternalRef, Resource};
#[cfg(any(unix, windows))]
pub use source::DirSource;
pub use source::{Slice, SliceSource, Source};
pub use vpk::{Entry, Vpk};
