//! A replay the client refuses (of another wire, cut short, garbage, a
//! Zstandard bomb) ends it cleanly, however early it is refused: an error
//! said once and exit code 1, never a crash, run after run.
//!
//! It opens a window and draws, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it on a display.

use std::path::{Path, PathBuf};
use std::process::Command;

use bota_proto::{ReplayHeader, ReplayRecord, encode_frame_to_vec};

/// Runs of each file: the crash this guards against came about one run in two.
const RUNS: usize = 5;

fn header(fingerprint: u64) -> Vec<u8> {
    encode_frame_to_vec(&ReplayRecord::Header(ReplayHeader::new(fingerprint))).expect("a header")
}

/// A Zstandard frame of RLE blocks, 128 KiB of one byte each: some 40 KiB
/// that would unpack to over a gibibyte.
fn zstd_bomb() -> Vec<u8> {
    let mut bomb = vec![0x28, 0xB5, 0x2F, 0xFD, 0x00, 9 << 3];
    let blocks = 10_000;
    for at in 0..blocks {
        let last = u32::from(at + 1 == blocks);
        let header = ((128 * 1024) << 3) | (1 << 1) | last;
        bomb.extend_from_slice(&header.to_le_bytes()[..3]);
        bomb.push(7);
    }
    bomb
}

fn refused(dir: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut truncated = header(bota_core::WIRE_FINGERPRINT);
    truncated.extend_from_slice(&1000u32.to_le_bytes());
    truncated.extend_from_slice(&[0; 10]);
    let mut other_wire = header(!bota_core::WIRE_FINGERPRINT);
    other_wire.extend_from_slice(&truncated[truncated.len() - 14..]);
    let files = [
        ("another wire", other_wire),
        ("cut short", truncated),
        (
            "garbage",
            (0..4096u32)
                .map(|at| (at.wrapping_mul(2_654_435_761) >> 13) as u8)
                .collect(),
        ),
        ("a zstd bomb", zstd_bomb()),
    ];
    files
        .into_iter()
        .map(|(what, bytes)| {
            let path = dir.join(format!("{}.brp", what.replace(' ', "-")));
            std::fs::write(&path, bytes).expect("a test replay");
            (what, path)
        })
        .collect()
}

#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn a_refused_replay_ends_the_client_cleanly_every_time() {
    let dir = std::env::temp_dir().join(format!("dbota-refused-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a test directory");
    for (what, path) in refused(&dir) {
        for run in 0..RUNS {
            let output = Command::new(env!("CARGO_BIN_EXE_dbota"))
                .arg("--replay")
                .arg(&path)
                .args(["--pov", "0", "--screenshot-at", "4000", "--screenshot-dir"])
                .arg(&dir)
                .args(["--mute", "--no-vsync"])
                .output()
                .expect("dbota runs");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{what}, run {run}: {:?}\n{stderr}",
                output.status
            );
            assert!(
                stderr.contains("the replay cannot be read"),
                "{what}, run {run}: {stderr}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
