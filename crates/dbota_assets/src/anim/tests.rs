//! Segments are the one part of an animation the golden files cannot damage (a flipped bit in
//! a compressed block fails decompression first): any bytes are an error or a pose, never a
//! panic or a read out of bounds.

use super::segment::{Attribute, BonePose, Segment};

const CODECS: [&str; 11] = [
    "CCompressedStaticFullVector3",
    "CCompressedStaticVector3",
    "CCompressedStaticQuaternion",
    "CCompressedStaticFloat",
    "CCompressedFullVector3",
    "CCompressedDeltaVector3",
    "CCompressedAnimVector3",
    "CCompressedAnimQuaternion",
    "CCompressedFullQuaternion",
    "CCompressedFullFloat",
    "CCompressedFullBool",
];

#[test]
fn any_segment_bytes_are_an_error_or_a_pose() {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut below = move |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    let attributes = [
        Some(Attribute::Position),
        Some(Attribute::Angle),
        Some(Attribute::Scale),
        Some(Attribute::Data),
        None,
    ];
    let rest = BonePose {
        position: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
    };
    let (mut posed, mut failed) = (0, 0);
    for round in 0..50_000 {
        let mut bytes: Vec<u8> = (0..below(96)).map(|_| below(256) as u8).collect();
        if bytes.len() >= 8 && round % 2 == 0 {
            // A plausible header: a known codec, a few elements with small ids.
            let elements = below(6);
            bytes[..6].copy_from_slice(&[below(12) as u8, 0, 0, 0, elements as u8, 0]);
            for i in 0..elements {
                if let Some(id) = bytes.get_mut(8 + 2 * i..10 + 2 * i) {
                    id.copy_from_slice(&[below(8) as u8, 0]);
                }
            }
        }
        let remap: Vec<Option<i64>> = (0..below(6))
            .map(|_| (below(3) > 0).then(|| below(8) as i64))
            .collect();
        let mut pose = vec![rest; below(6)];
        let before = pose.clone();
        let attribute = attributes[below(attributes.len())];
        if let Ok(Some(segment)) = Segment::parse(&bytes, &CODECS, attribute, &remap) {
            for frame in [0, 1, below(16), usize::MAX] {
                match segment.read(frame, &mut pose) {
                    Ok(()) => posed += usize::from(pose != before),
                    Err(_) => failed += 1,
                }
            }
        }
    }
    assert!(
        posed > 1000 && failed > 1000,
        "{posed} posed, {failed} failed"
    );
}
