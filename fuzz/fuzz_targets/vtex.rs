//! A `vtex_c` texture and every way the client takes it: its first sprite
//! frame, the KTX2 file and the RGBA8 pixels. No input panics.
#![no_main]

use dbota_assets::{Encoding, Texture};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(texture) = Texture::parse(data) else {
        return;
    };
    let frame = texture.first_frame();
    for texture in [&texture, &frame] {
        let _ = texture.to_ktx2(Encoding::Srgb);
        let _ = texture.to_rgba8();
    }
});
