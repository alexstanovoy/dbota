//! A binary `VBIB` (or `MBUF`) block of a mesh: its vertex buffers decoded (raw, zstd and
//! meshopt) and converted to glTF's attributes, skinned or not, and its index buffers. No
//! input panics.
#![no_main]

use dbota_assets::fuzzing::vbib;
use libfuzzer_sys::fuzz_target;

/// A remapping table as a mesh's model gives one.
const JOINTS: [u16; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fuzz_target!(|data: &[u8]| {
    let Some((&weights, block)) = data.split_last() else {
        return;
    };
    let weights = usize::from(weights & 7);
    let joints = (weights > 0).then_some(&JOINTS[..]);
    let _ = vbib(block, joints, weights);
});
