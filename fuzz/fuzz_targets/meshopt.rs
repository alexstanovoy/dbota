//! meshoptimizer's vertex and index codecs as the mesh buffers use them: the first four bytes
//! give the element count and size, the rest is the encoded buffer. No input panics or
//! decodes past its bound.
#![no_main]

use dbota_assets::fuzzing::{meshopt_indices, meshopt_vertices};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((head, encoded)) = data.split_first_chunk::<4>() else {
        return;
    };
    let count = usize::from(u16::from_le_bytes([head[0], head[1]])) << (head[3] & 7);
    let stride = usize::from(head[2]);
    if let Ok(out) = meshopt_vertices(encoded, count, stride) {
        assert_eq!(out.len(), count * stride);
    }
    for size in [2, 4] {
        if let Ok(out) = meshopt_indices(encoded, count, size) {
            assert_eq!(out.len(), count * size);
        }
    }
});
