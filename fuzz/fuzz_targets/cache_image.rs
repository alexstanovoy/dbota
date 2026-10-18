//! A PNG or KTX2 file of the asset cache, decoded as the client's image
//! loader decodes it once the cache reader has let it through: no input
//! panics or takes the memory.
#![no_main]

use bevy_asset::RenderAssetUsages;
use bevy_image::{CompressedImageFormats, Image, ImageSampler, ImageType};
use libfuzzer_sys::fuzz_target;

#[path = "../../src/texture_file.rs"]
mod texture_file;

fuzz_target!(|data: &[u8]| {
    let head = &data[..data.len().min(texture_file::HEAD_BYTES)];
    if let Some(Err(_)) = texture_file::decoded_size(head) {
        return;
    }
    // A file's name, not its bytes, picks the decoder.
    for extension in ["png", "ktx2"] {
        let _ = Image::from_buffer(
            data,
            ImageType::Extension(extension),
            CompressedImageFormats::all(),
            true,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        );
    }
});
