//! User-picked pictures reject hostile bytes before allocation, and whatever decodes can be made
//! into the avatar's square.
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
            for side in [128, 256] {
                let square = pixels.square(side).expect("a bounded output size");
                assert_eq!((square.width(), square.height()), (side, side));
                assert_eq!(square.rgba().len(), side as usize * side as usize * 4);
            }
        }
    }
});
