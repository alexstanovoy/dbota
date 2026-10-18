//! A WAV or MP3 file of the asset cache, decoded as the client's audio
//! decodes it: no input panics or takes the memory.
#![no_main]

use libfuzzer_sys::fuzz_target;

/// Samples played at most: enough to cross a few frames of any stream.
const SAMPLES: usize = 1 << 16;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoder) = rodio::Decoder::new(std::io::Cursor::new(data.to_vec())) {
        decoder.take(SAMPLES).for_each(drop);
    }
});
