//! A service avatar is bytes a stranger chose: whatever is no avatar is refused, whatever is one
//! becomes exactly the pictures the menu draws, and no input is slow or large.
#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../../src/main_menu/avatar_image.rs"]
mod avatar_image;

fuzz_target!(|bytes: &[u8]| {
    let large = avatar_image::decode(bytes);
    let list = avatar_image::decode_for_list(bytes);
    assert_eq!(large.is_ok(), list.is_ok(), "the list picture is made of the large one");
    if let (Ok(large), Ok(list)) = (large, list) {
        assert_eq!((large.width(), large.height()), (avatar_image::SIDE, avatar_image::SIDE));
        assert_eq!(large.rgba().len(), (avatar_image::SIDE as usize).pow(2) * 4);
        assert_eq!((list.width(), list.height()), (avatar_image::LIST_SIDE, avatar_image::LIST_SIDE));
        assert_eq!(list.rgba().len(), (avatar_image::LIST_SIDE as usize).pow(2) * 4);
    }
});
