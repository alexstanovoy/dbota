//! The cache's small text files as the client reads them: the map's trees and buildings, its
//! lighting and the portrait framings. Every number read is finite. No input panics.
#![no_main]

#[path = "../../src/cache_text.rs"]
mod cache_text;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let finite = |n: &[f32]| assert!(n.iter().all(|n| n.is_finite()), "{n:?}");
    for t in cache_text::trees(text, 16_384) {
        finite(&[t.x, t.y, t.yaw, t.scale]);
    }
    for b in cache_text::buildings(text, 1024) {
        finite(&[b.x, b.y, b.yaw]);
    }
    if let Some(l) = cache_text::lighting(text) {
        finite(l.sun.as_flattened());
        finite(l.ambient.as_flattened());
        assert!(l.turn.is_finite() && l.turn > 0.0);
    }
    for a in cache_text::ambient(text, 4096) {
        finite(&a.at);
        finite(&a.angles);
        if let Some((cp, value)) = a.data {
            assert!(cp <= cache_text::MAX_CONTROL_POINT);
            finite(&value);
        }
    }
    for line in text.lines() {
        if let Some(f) = cache_text::framing(line) {
            finite(&f.numbers);
            if let Some((_, tints)) = f.background {
                finite(&tints);
            }
        }
    }
});
