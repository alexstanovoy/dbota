//! dbota's asset importer: reads what the manifest (`assets-manifest/`) lists out of a local
//! Dota 2 install with heatel's readers and writes dbota's asset cache. The same code runs
//! natively (`dbota import`, [`cache_dir`]) and in a browser worker ([`web`], wasm only).

#[cfg(any(unix, windows))]
pub mod cache_dir;
pub mod import;
pub mod text;
#[cfg(target_arch = "wasm32")]
pub mod web;
