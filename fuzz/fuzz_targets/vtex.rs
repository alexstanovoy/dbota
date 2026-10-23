//! A `vtex_c` texture and every way the importer and the client take it: its first sprite
//! frame, the KTX2 file, the RGBA8 pixels, the PNG files, and the images packed from its
//! channels. No input panics.
#![no_main]

use dbota_assets::{Encoding, Pack, Texture};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(texture) = Texture::parse(data) else {
        return;
    };
    let frame = texture.first_frame();
    for texture in [&texture, &frame] {
        let _ = texture.to_ktx2(Encoding::Srgb);
        let _ = texture.to_png();
        let Ok(mut pixels) = texture.to_rgba8() else {
            continue;
        };
        for pack in [Pack::Channel(1), Pack::Orm, Pack::Rgb] {
            pack.apply(&mut pixels);
        }
        if let Ok(packed) = Texture::from_rgba8(texture.width(), texture.height(), pixels) {
            let _ = packed.to_png();
        }
    }
    let _ = texture.first_frame_png();
});
