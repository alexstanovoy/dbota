//! The cache's face files as the client reads them: the phoneme table, an idle expression, a
//! line's lip sync and a mesh's flex rig, then a face posed from them at a few times. No input
//! panics and every morph weight is a number from 0 to 1.
#![no_main]

#[path = "../../src/face.rs"]
mod face;

use std::collections::HashMap;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let table = face::Phonemes::parse(text).unwrap_or_default();
    let scene = face::Expression::parse(text);
    let line = face::Lipsync::parse(text);
    let rig = face::Rig::from_extras(text);
    let ranges: HashMap<u32, (f32, f32)> = rig
        .iter()
        .flat_map(|r| r.controllers.iter().map(|&(n, min, max)| (n, (min, max))))
        .collect();
    for t in [-1.0, 0.0, 0.05, 0.5, 1.7, 30.0] {
        let mut controls = face::Controls::new();
        if let Some(scene) = &scene {
            scene.apply(t, &mut controls);
        }
        if let Some(line) = &line {
            line.apply(t, &table, &ranges, &mut controls);
            let _ = line.end();
        }
        if let Some(rig) = &rig {
            let mut weights = vec![0.0; 300];
            rig.weights(&controls, &mut weights);
            assert!(weights.iter().all(|w| (0.0..=1.0).contains(w)), "{weights:?}");
        }
    }
});
