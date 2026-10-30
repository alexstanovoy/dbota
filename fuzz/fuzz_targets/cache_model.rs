//! The cache's glTF and GLB files as the client vets them before Bevy's loader: any bytes,
//! read as a model. No input panics.
#![no_main]

#[path = "../../src/gltf_file.rs"]
mod gltf_file;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = gltf_file::check(data);
});
