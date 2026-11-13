//! Golden test for the sound decoder: every sound of the manifest, read from the local Dota 2
//! install, against the file of the reference cache (see `common`). It needs both, so it is
//! ignored by default:
//!
//! ```sh
//! cargo test --release -p dbota_import --test golden_sounds -- --ignored --nocapture
//! ```
//!
//! `DBOTA_DOTA_PATH` (a `dota 2 beta` folder) and `DBOTA_REFERENCE` (the exported cache) override
//! where they are looked for.

mod common;

use std::path::{Path, PathBuf};

use heatel::face::PhonemeTable;
use heatel::sound::{Lipsync, Sound, SoundFormat};
use heatel::{DirSource, Vpk, find_game_dir};

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
        let sound = Sound::parse(&bytes, &mut heatel::Budget::default())
            .unwrap_or_else(|e| panic!("{vpk}: {e}"));
        assert_eq!(
            Some(sound.extension()),
            out.rsplit('.').next(),
            "{vpk}: kind"
        );
    }
    for (vpk, out) in held {
        let bytes = pak.read_file(&src, &vpk).unwrap();
        let sound = Sound::parse(&bytes, &mut heatel::Budget::default())
            .unwrap_or_else(|e| panic!("{vpk}: {e}"));
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
            other => panic!("{vpk}: {other:?}, a sound format this test does not know"),
        }
    }
    println!("sounds: {wavs} WAV and {mp3s} MP3 files equal to the exported ones");
    assert!(wavs > 0 && mp3s > 0);
}

/// Every hero voice line of the manifest carries its lip sync, in either layout of the vsnd
/// (the old binary sentence, the newer KV3 one with its morph curves): phonemes within the
/// line, each one the phoneme table knows.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn hero_voice_lines_carry_their_lip_sync() {
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref()).expect("Dota 2 not found");
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let table = PhonemeTable::parse(
        &pak.read_file(&src, "expressions/heroes/phonemes.txt")
            .unwrap(),
    )
    .unwrap();
    let known: Vec<u32> = table.phonemes.iter().map(|p| p.code).collect();
    let (mut lines, mut curves) = (0, 0);
    // The heroes' move and attack lines, as the package holds them (the client reads them from
    // each hero's voice pack, not from the manifest). Other lines may hold a phoneme a little
    // outside the sound (Pudge's `pud_ability_devour_07` at -0.086 s, a cosmetic's past its end).
    let lines_of = |entry: &heatel::vpk::Entry<'_>| {
        let path = entry.path();
        ["pudge/pud", "nevermore/nev"]
            .iter()
            .flat_map(|hero| ["move", "attack"].map(|line| format!("sounds/vo/{hero}_{line}_")))
            .any(|prefix| path.starts_with(&prefix))
            && path.ends_with(".vsnd_c")
    };
    for entry in pak.entries().filter(|entry| lines_of(entry)) {
        let vpk = entry.path();
        let bytes = pak.read_file(&src, &vpk).unwrap();
        let duration = Sound::parse(&bytes, &mut heatel::Budget::default())
            .unwrap()
            .duration();
        let lipsync = Lipsync::parse(&bytes, &mut heatel::Budget::default())
            .unwrap_or_else(|e| panic!("{vpk}: {e}"));
        assert!(!lipsync.phonemes.is_empty(), "{vpk}: no phonemes");
        for p in &lipsync.phonemes {
            assert!(
                p.start >= 0.0 && p.end <= duration + 0.05,
                "{vpk}: {p:?} past {duration}"
            );
            assert!(
                known.contains(&p.code),
                "{vpk}: phoneme {} not in the table",
                p.code
            );
        }
        lines += 1;
        curves += usize::from(lipsync.morph.is_some());
    }
    println!("voice lines: {lines} with phonemes, {curves} with morph curves");
    assert!(lines >= 48 && curves > 0);
}

/// The real importer, over a real WAV of the install, refuses to write it under an `.mp3` name and
/// says so by name; the same sound under its right name is written.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_importer_refuses_a_wav_it_is_asked_to_name_mp3() {
    use dbota_import::import::{Flow, Plan, Sink, run};
    #[derive(Default)]
    struct Memory(std::collections::BTreeMap<String, usize>);
    impl Sink for Memory {
        fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
            self.0.insert(path.to_owned(), data.len());
            Ok(())
        }
    }
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref()).expect("Dota 2 not found");
    let plan = Plan::parse(
        "sound a sounds/ambient/mega_horn_dire_2d.vsnd_c sounds/mega/wrong.mp3 event=MegaCreeps.Radiant\n\
         sound b sounds/ambient/mega_horn_dire_2d.vsnd_c sounds/mega/right.wav event=MegaCreeps.Radiant\n",
        "",
    )
    .expect("a plan");
    let mut sink = Memory::default();
    let report = run(DirSource::new(game), &plan, &mut sink, &mut |_| {
        Flow::Continue
    })
    .expect("the import runs");
    assert!(
        !sink.0.contains_key("sounds/mega/wrong.mp3"),
        "{:?}",
        sink.0
    );
    assert!(sink.0.contains_key("sounds/mega/right.wav"), "{:?}", sink.0);
    assert!(
        report
            .failed
            .iter()
            .any(|(_, why)| why
                == "sound: the sound is a wav, not the cache's sounds/mega/wrong.mp3"),
        "{:?}",
        report.failed
    );
}

/// The map's placements as the real importer makes them from the committed manifest: the soundscape
/// list and the sounds imported leave out the Ancients' loops the world line omits, and keep the
/// spawn loops beside them.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_manifests_omitted_soundscape_sounds_are_not_imported() {
    use dbota_import::import::{Flow, Item, Plan, Sink, run};
    #[derive(Default)]
    struct Memory(std::collections::BTreeMap<String, Vec<u8>>);
    impl Sink for Memory {
        fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
            self.0.insert(path.to_owned(), data.to_vec());
            Ok(())
        }
    }
    let dota_path = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let game = find_game_dir(dota_path.as_deref()).expect("Dota 2 not found");
    let mut plan = Plan::parse(
        &std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap(),
        &std::fs::read_to_string(repo().join("assets-manifest/ids.txt")).unwrap(),
    )
    .expect("the committed manifest");
    plan.items
        .retain(|item| matches!(item, Item::Placements { .. }));
    assert_eq!(plan.items.len(), 1, "the map's placements");
    let mut sink = Memory::default();
    run(DirSource::new(game), &plan, &mut sink, &mut |_| {
        Flow::Continue
    })
    .expect("the placements import");
    let list = String::from_utf8(sink.0["map/mid/soundscapes.txt"].clone()).unwrap();
    for omitted in ["radiant_ancient_lp_01", "dire_ancient_lp_01"] {
        assert!(!list.contains(omitted), "{omitted} is listed");
        assert!(
            !sink.0.keys().any(|path| path.contains(omitted)),
            "{omitted} is imported"
        );
    }
    for kept in ["radiant_spawn_loop_02", "dire_spawn_loop_01"] {
        assert!(list.contains(kept), "{kept} is no longer listed");
        assert!(
            sink.0.keys().any(|path| path.contains(kept)),
            "{kept} is no longer imported"
        );
    }
}
