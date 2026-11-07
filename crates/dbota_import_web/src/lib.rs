//! Browser bindings for the importer. The client links only `dbota_import`'s Rust library,
//! so building another client graphics API cannot replace this module's Wasm artifact.

#[cfg(target_arch = "wasm32")]
pub mod web;
