//! Golden test for the sound decoder: every sound of the manifest, read from the local Dota 2
//! install, against the file of the reference cache (see `common`). It needs both, so it is
//! ignored by default:
//!
//! ```sh
//! cargo test --release -p dbota_assets --test golden_sounds -- --ignored --nocapture
//! ```
//!
//! `DBOTA_DOTA_PATH` (a `dota 2 beta` folder) and `DBOTA_REFERENCE` (the exported cache) override
//! where they are looked for.

mod common;

use std::path::{Path, PathBuf};

use dbota_assets::{DirSource, Sound, SoundFormat, Vpk, find_game_dir};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The `(vpk path, cache path)` of every `sound` line of the manifest.
fn manifest_sounds() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap();
    text.lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split('#').next()?.split_whitespace().collect();
            match words.as_slice() {
                ["sound", _, src, out, ..] => Some(((*src).to_owned(), (*out).to_owned())),
                _ => None,
            }
        })
        .collect()
}

/// Manifest lines as `(vpk path, cache path)`.
type Lines = Vec<(String, String)>;

/// The lines whose file the reference holds, and the rest. A line added to the manifest after the
/// reference was exported has nothing to be compared with: it is listed, and only read (exporting
/// it with the same Source2Viewer-CLI 20 into the reference has it compared too). The reference
/// must hold some.
fn in_reference(lines: Lines, reference: &Path, what: &str) -> (Lines, Lines) {
    let (held, added): (Vec<_>, Vec<_>) = lines
        .into_iter()
        .partition(|(_, out)| reference.join(out).is_file());
    assert!(
        !held.is_empty(),
        "the reference at {} holds none of the manifest's {what}",
        reference.display()
    );
    if !added.is_empty() {
        let outs: Vec<&str> = added.iter().map(|(_, out)| out.as_str()).collect();
        println!("{what} added to the manifest since the reference, only read: {outs:?}");
    }
    (held, added)
}

/// Rate, channels and playing time of an MP3 stream, counted frame by frame.
fn mp3_stats(data: &[u8]) -> (u32, u16, f64) {
    const V1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let mut at = 0;
    if data.starts_with(b"ID3") {
        at = 10
            + data[6..10]
                .iter()
                .fold(0usize, |n, &b| n << 7 | usize::from(b & 0x7F));
    }
    let (mut rate, mut channels, mut samples) = (0, 0, 0u64);
    while at + 4 <= data.len() && data[at] == 0xFF && data[at + 1] & 0xE0 == 0xE0 {
        let h = &data[at..at + 4];
        let v1 = (h[1] >> 3) & 3 == 3;
        let sample_rate = [44100, 48000, 32000][usize::from((h[2] >> 2) & 3)]
            / match (h[1] >> 3) & 3 {
                3 => 1,
                2 => 2,
                _ => 4,
            };
        let bitrate = 1000 * if v1 { V1 } else { V2 }[usize::from(h[2] >> 4)];
        let pad = usize::from((h[2] >> 1) & 1);
        let len = if v1 {
            144 * bitrate / sample_rate
        } else {
            72 * bitrate / sample_rate
        };
        (rate, channels) = (sample_rate, if h[3] >> 6 == 3 { 1 } else { 2 });
        samples += if v1 { 1152 } else { 576 };
        at += len as usize + pad;
    }
    assert!(rate > 0, "no frames");
    (rate, channels, samples as f64 / f64::from(rate))
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn manifest_sounds_match_the_exported_files() {
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref()).expect("Dota 2 not found");
    let cache = common::reference();
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let (mut wavs, mut mp3s) = (0, 0);
    let (held, added) = in_reference(manifest_sounds(), &cache, "sounds");
    for (vpk, out) in &added {
        let bytes = pak.read_file(&src, vpk).unwrap();
        let sound = Sound::parse(&bytes).unwrap_or_else(|e| panic!("{vpk}: {e}"));
        assert_eq!(
            Some(sound.extension()),
            out.rsplit('.').next(),
            "{vpk}: kind"
        );
    }
    for (vpk, out) in held {
        let bytes = pak.read_file(&src, &vpk).unwrap();
        let sound = Sound::parse(&bytes).unwrap_or_else(|e| panic!("{vpk}: {e}"));
        let old = std::fs::read(cache.join(&out)).unwrap_or_else(|e| panic!("{out}: {e}"));
        assert_eq!(
            Some(sound.extension()),
            out.rsplit('.').next(),
            "{vpk}: kind"
        );
        // The exported file is the audio itself: the MP3 untouched, the PCM behind a header.
        assert!(
            sound.file()[..] == old[..],
            "{vpk}: file differs from {out}"
        );
        match sound.format() {
            SoundFormat::Wav => {
                wavs += 1;
                let u16at = |at: usize| u16::from_le_bytes([old[at], old[at + 1]]);
                let u32at = |at: usize| u32::from_le_bytes(old[at..at + 4].try_into().unwrap());
                assert_eq!(
                    (u32at(24), u16at(22), u16at(34)),
                    (
                        sound.sample_rate(),
                        sound.channels(),
                        sound.bits_per_sample()
                    ),
                    "{vpk}"
                );
                let frames = u64::from(u32at(40)) / u64::from(u16at(32));
                assert_eq!(frames, sound.samples(), "{vpk}: samples");
                let seconds = frames as f64 / f64::from(sound.sample_rate());
                assert!(
                    (seconds - f64::from(sound.duration())).abs() < 1e-3,
                    "{vpk}: duration"
                );
            }
            SoundFormat::Mp3 => {
                mp3s += 1;
                let (rate, channels, seconds) = mp3_stats(&old);
                assert_eq!(
                    (rate, channels),
                    (sound.sample_rate(), sound.channels()),
                    "{vpk}"
                );
                // The stream's frames cover the sound plus the encoder's padding.
                let declared = f64::from(sound.duration());
                assert!(
                    (seconds - declared).abs() < 0.12,
                    "{vpk}: {seconds:.3} s of frames, {declared:.3} s declared"
                );
            }
        }
    }
    println!("sounds: {wavs} WAV and {mp3s} MP3 files equal to the exported ones");
    assert!(wavs > 0 && mp3s > 0);
}
