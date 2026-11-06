//! A WAV or MP3 file of the asset cache, decoded as the client's audio
//! decodes it (`sound_file`): no input panics or takes the memory.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../src/sound_file.rs"]
mod sound_file;

/// Samples played at most: enough to cross a few frames of any stream.
const SAMPLES: usize = 1 << 16;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoder) = sound_file::decoder(data.to_vec()) {
        decoder.take(SAMPLES).for_each(drop);
    }
});
