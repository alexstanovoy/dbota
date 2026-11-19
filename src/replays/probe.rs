//! Checks a kept file's first frame before the browser offers to play it.
use bota_client_core::{UnpackedReplay, ZSTD_MAGIC};
use bota_proto::{LEN_PREFIX, ReplayHeader, ReplayRecord};
use std::io::{self, Cursor, Read};

/// A Zstandard block is at most 128 KiB. Leave space for its frame and block headers.
#[cfg(target_arch = "wasm32")]
pub const MAX_PREFIX: usize = (128 << 10) + 64;
const INVALID: &str = "The replay header is missing or damaged.";

pub(super) fn problem(bytes: Vec<u8>) -> Result<(), &'static str> {
    let compressed = bytes.starts_with(&ZSTD_MAGIC);
    let input = Cursor::new(bytes);
    if compressed {
        check_header(&mut UnpackedReplay::new(input))
    } else {
        check_header(&mut input.into_inner().as_slice())
    }
}

fn check_header(input: &mut impl Read) -> Result<(), &'static str> {
    let mut length = [0; LEN_PREFIX];
    let mut header = [0; 32];
    input.read_exact(&mut length).map_err(read_problem)?;
    let count = u32::from_le_bytes(length) as usize;
    let payload = header.get_mut(..count).ok_or(INVALID)?;
    input.read_exact(payload).map_err(read_problem)?;
    match bota_proto::decode_payload(payload) {
        Ok(ReplayRecord::Header(header)) if header.magic == ReplayHeader::MAGIC => {
            if bota_core::admits_wire(header.wire_fingerprint) {
                Ok(())
            } else {
                Err("The replay was recorded by another version of the game.")
            }
        }
        _ => Err(INVALID),
    }
}

fn read_problem(_error: io::Error) -> &'static str {
    INVALID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_replay_probe_checks_real_compressed_headers_and_rejects_broken_files() {
        let (replay, _) = crate::postgame::recorded_match_for_menu();
        let bytes = std::fs::read(replay).unwrap();
        assert_eq!(
            problem(bytes.into_iter().take((128 << 10) + 64).collect()),
            Ok(())
        );
        assert_eq!(problem(vec![7; 2048]), Err(INVALID));
        let other = ReplayRecord::Header(ReplayHeader::new(0));
        assert_eq!(
            problem(bota_proto::encode_frame_to_vec(&other).unwrap()),
            Err("The replay was recorded by another version of the game.")
        );
    }
}
