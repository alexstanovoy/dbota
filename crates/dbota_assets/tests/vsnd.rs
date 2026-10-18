//! Sounds built here and read back: the `vsnd_c` layouts and the files made of them, without a
//! Dota 2 install. The golden test compares with the real files.

use dbota_assets::{Sound, SoundFormat};

/// A `vsnd_c` with the older binary `DATA` block: `packed` first word, sample count, duration,
/// audio size, then the audio.
fn vsnd(version: u16, packed: u32, samples: u32, seconds: f32, audio: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    for word in [
        packed,
        u32::MAX,
        samples,
        seconds.to_bits(),
        0,
        0,
        0,
        audio.len() as u32,
    ] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    data.extend_from_slice(&[0; 8]);
    let file_size = 28 + data.len();
    let mut out = (file_size as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&12u16.to_le_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    for word in [8, 1] {
        out.extend_from_slice(&u32::to_le_bytes(word));
    }
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(audio);
    out
}

/// The first word of version 1: file type, bits, channels, rate.
fn packed_v1(file_type: u32, bits: u32, channels: u32, rate: u32) -> u32 {
    file_type | bits << 2 | channels << 7 | rate << 14
}

/// The first word of version 4: rate, format, channels.
fn packed_v4(rate: u32, format: u32, channels: u32) -> u32 {
    rate | format << 16 | channels << 24
}

const MP3_FRAME: [u8; 4] = [0xFF, 0xFB, 0x50, 0xC0]; // MPEG 1 layer III, 128 kbit/s, 44.1 kHz, mono

#[test]
fn pcm_comes_out_behind_a_canonical_wav_header() {
    let samples: Vec<u8> = (0..16).collect();
    for file in [
        vsnd(1, packed_v1(1, 16, 2, 44100), 4, 4.0 / 44100.0, &samples),
        vsnd(4, packed_v4(44100, 0, 2), 4, 4.0 / 44100.0, &samples),
    ] {
        let sound = Sound::parse(&file).unwrap();
        assert_eq!(sound.format(), SoundFormat::Wav);
        assert_eq!(
            (
                sound.sample_rate(),
                sound.channels(),
                sound.bits_per_sample(),
                sound.samples()
            ),
            (44100, 2, 16, 4)
        );
        let wav = sound.file();
        let mut expected = b"RIFF".to_vec();
        expected.extend_from_slice(&52u32.to_le_bytes());
        expected.extend_from_slice(b"WAVEfmt ");
        expected.extend_from_slice(&16u32.to_le_bytes());
        expected.extend_from_slice(&[1, 0, 2, 0]);
        expected.extend_from_slice(&44100u32.to_le_bytes());
        expected.extend_from_slice(&(44100u32 * 4).to_le_bytes());
        expected.extend_from_slice(&[4, 0, 16, 0]);
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&16u32.to_le_bytes());
        expected.extend_from_slice(&samples);
        assert_eq!(&wav[..], &expected[..]);
    }
}

#[test]
fn an_mp3_is_kept_as_it_is_and_tells_its_rate_and_channels_from_its_frames() {
    let mut stream = b"ID3\x04\0\0\0\0\0\x05abcde".to_vec();
    stream.extend_from_slice(&MP3_FRAME);
    stream.extend_from_slice(&[0; 20]);
    for file in [
        vsnd(1, packed_v1(2, 0, 0, 0), 1152, 0.026, &stream),
        vsnd(4, packed_v4(0, 2, 0), 1152, 0.026, &stream),
    ] {
        let sound = Sound::parse(&file).unwrap();
        assert_eq!(
            (sound.format(), sound.extension()),
            (SoundFormat::Mp3, "mp3")
        );
        assert_eq!((sound.sample_rate(), sound.channels()), (44100, 1));
        assert_eq!(&sound.file()[..], &stream[..]);
    }
}

#[test]
fn damaged_sounds_are_errors_not_panics() {
    let good = vsnd(1, packed_v1(1, 8, 1, 22050), 8, 8.0 / 22050.0, &[7; 8]);
    assert!(Sound::parse(&good).is_ok());
    for len in 0..good.len() {
        assert!(Sound::parse(&good[..len]).is_err(), "cut at {len}");
    }
    for (what, bad) in [
        ("adpcm", vsnd(4, packed_v4(44100, 3, 1), 8, 0.1, &[0; 8])),
        (
            "file type",
            vsnd(1, packed_v1(3, 16, 1, 44100), 8, 0.1, &[0; 8]),
        ),
        (
            "12 bits",
            vsnd(1, packed_v1(1, 12, 1, 44100), 8, 0.1, &[0; 8]),
        ),
        (
            "no channels",
            vsnd(4, packed_v4(44100, 0, 0), 8, 0.1, &[0; 8]),
        ),
        ("no rate", vsnd(4, packed_v4(0, 0, 1), 8, 0.1, &[0; 8])),
        (
            "half a sample",
            vsnd(4, packed_v4(44100, 0, 2), 8, 0.1, &[0; 6]),
        ),
        (
            "no mp3 frame",
            vsnd(4, packed_v4(0, 2, 0), 8, 0.1, &[0; 64]),
        ),
    ] {
        assert!(Sound::parse(&bad).is_err(), "{what}");
    }
    let mp3 = vsnd(4, packed_v4(0, 2, 0), 1152, 0.026, &MP3_FRAME);
    for at in 0..mp3.len().min(good.len()) {
        for file in [&good, &mp3] {
            let mut flipped = file.clone();
            if let Some(byte) = flipped.get_mut(at) {
                *byte ^= 0xFF;
            }
            if let Ok(sound) = Sound::parse(&flipped) {
                let _ = sound.file();
            }
        }
    }
}
