//! A `vsnd_c` sound and the audio file made of it: no input panics.
#![no_main]

use dbota_assets::Sound;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(sound) = Sound::parse(data) {
        let _ = sound.file();
    }
});
