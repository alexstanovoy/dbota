//! A service avatar as the client takes it. The bytes are a stranger's: they are read by the
//! avatar format's own decoder (exactly the avatar size, one bounded inflation, every checksum),
//! never by a general image decoder, and what comes out is the pictures the menu draws.
//!
//! Plain types only: the fuzz target takes this file as it is.
use dbota_import::thumbnail::Pixels;

/// The side of the large picture, as the service stores it.
pub const SIDE: u32 = radovid_proto::AVATAR_SIDE;
/// The side of the picture a list row keeps: twice the 32 pixels it is drawn at, and a sixteenth
/// of the memory of the large one.
pub const LIST_SIDE: u32 = 64;
const _: () = assert!(SIDE.is_multiple_of(LIST_SIDE) && (SIDE / LIST_SIDE).is_power_of_two());

/// The large picture of the avatar `png`, or why it is no avatar.
pub fn decode(png: &[u8]) -> Result<Pixels, String> {
    let image = radovid_proto::decode_avatar(png).map_err(|why| why.to_string())?;
    Pixels::from_rgba(SIDE, SIDE, image.to_rgba())
        .ok_or_else(|| "The decoded avatar is not the avatar size.".to_owned())
}

/// The picture a list row keeps of the avatar `png`.
pub fn decode_for_list(png: &[u8]) -> Result<Pixels, String> {
    let large = decode(png)?;
    large
        .shrunk(LIST_SIDE)
        .ok_or_else(|| "The decoded avatar cannot be brought to the list size.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(color: [u8; 4]) -> Vec<u8> {
        let rgba: Vec<u8> = (0..SIDE * SIDE).flat_map(|_| color).collect();
        radovid_proto::encode_avatar_rgba(&rgba).expect("a flat picture fits")
    }

    #[test]
    fn a_service_avatar_decodes_to_the_large_picture_and_a_quarter_size_list_picture() {
        let png = flat([20, 90, 180, 255]);
        let large = decode(&png).expect("a valid avatar");
        assert_eq!((large.width(), large.height()), (SIDE, SIDE));
        assert!(
            large
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [20, 90, 180, 255])
        );
        let list = decode_for_list(&png).expect("a valid avatar");
        assert_eq!((list.width(), list.height()), (LIST_SIDE, LIST_SIDE));
        assert!(
            list.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [20, 90, 180, 255])
        );
    }

    #[test]
    fn bytes_that_are_no_avatar_are_refused_with_the_formats_reason() {
        assert_eq!(decode(b"<svg/>").unwrap_err(), "avatar refused: not a PNG");
        let mut png = flat([1, 2, 3, 255]);
        png.truncate(png.len() - 3);
        assert!(decode(&png).is_err());
        // A solid 32 by 32 PNG, the size avatars had before 256.
        let small = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 32, 0, 0, 0, 32,
            8, 6, 0, 0, 0, 115, 122, 122, 244, 0, 0, 0, 47, 73, 68, 65, 84, 120, 218, 237, 206, 33,
            1, 0, 0, 8, 3, 48, 130, 145, 142, 136, 20, 130, 24, 55, 19, 243, 171, 158, 189, 164,
            18, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 72, 7, 30, 115, 205, 60,
            151, 190, 23, 53, 16, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        assert_eq!(
            decode(&small).unwrap_err(),
            "avatar refused: not exactly the avatar size",
            "the old 32 pixel size is no avatar any more"
        );
    }

    #[test]
    fn an_avatar_with_any_byte_changed_or_cut_short_is_refused_and_never_panics() {
        let png = flat([200, 100, 50, 255]);
        for at in (0..png.len()).step_by(7) {
            let mut changed = png.clone();
            changed[at] ^= 0x55;
            assert!(decode(&changed).is_err(), "byte {at} changed");
        }
        for end in (0..png.len()).step_by(11) {
            assert!(decode(&png[..end]).is_err(), "cut at {end}");
        }
    }
}
