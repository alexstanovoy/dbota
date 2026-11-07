//! Compact service images and user-picked thumbnails reject hostile bytes before allocation.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    for (limit, dimension) in [(512, 128), (256 * 1024, 4096)] {
        if let Ok(pixels) = dbota_import::thumbnail::decode(bytes, limit, dimension) {
            assert!(pixels.width() <= dimension && pixels.height() <= dimension);
            assert_eq!(
                pixels.rgba().len(),
                pixels.width() as usize * pixels.height() as usize * 4
            );
            let square = pixels.square(128).expect("a bounded output size");
            assert_eq!((square.width(), square.height()), (128, 128));
        }
    }
});
