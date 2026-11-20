//! dbota's asset importer: reads what the manifest (`assets-manifest/`) lists out of a local
//! Dota 2 install with heatel's readers and writes dbota's asset cache. The same code runs
//! natively (`dbota import`, [`cache_dir`]) and in a browser worker (`dbota_import_web`).

pub mod browser;
#[cfg(any(unix, windows))]
pub mod cache_dir;
pub mod catalog;
pub mod import;
pub mod index;
pub mod pack;
pub mod png;
pub mod soften;
pub mod source;
pub mod text;
#[cfg(any(unix, windows))]
pub mod verify;
pub mod voice;

pub use source::ADDONS;
pub mod thumbnail;
