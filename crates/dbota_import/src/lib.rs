//! dbota's asset importer: reads what the manifest (`assets-manifest/`) lists out of a local
//! Dota 2 install with heatel's readers and writes dbota's asset cache. The same code runs
//! natively (`dbota import`, [`cache_dir`]) and in a browser worker ([`web`], wasm only). A cache
//! prepared elsewhere is taken the same way in both ([`pack`], [`zip`], [`pack_dir`]).

#[cfg(any(unix, windows))]
pub mod cache_dir;
pub mod catalog;
pub mod import;
pub mod pack;
#[cfg(any(unix, windows))]
pub mod pack_dir;
pub mod source;
pub mod text;
#[cfg(any(unix, windows))]
pub mod verify;
pub mod zip;

pub use source::ADDONS;
// The browser module's bindings, only in the importer's own module (web/build.sh): the client,
// which links this crate for the fingerprint, exports none of them.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;
