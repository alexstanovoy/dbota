//! What a sound file of the asset cache is decoded with. The importer writes WAV files and MP3
//! files, an MP3 perhaps after an ID3v2 tag; the cache is a folder anyone may write (or a
//! prepared one someone handed on), and rodio takes any bytes: what its WAV reader refuses goes
//! to symphonia, which searches the file for any format or tag it knows and reads an ID3v2 tag it
//! finds with allocations of what the tag says (gigabytes from 20 bytes) and panics on some. So a
//! file is decoded only as WAV when the WAV reader takes its header, or as MP3 from an MPEG audio
//! frame, which symphonia then takes at once without searching further: at the file's start, or
//! right after a tag whose size this reads itself, and symphonia is handed the frames alone.
//!
//! No Bevy: the fuzz targets read this file as their own module.

use std::io::Cursor;

/// An ID3v2 tag's header: `ID3`, the version, the flags and the size.
const TAG_HEADER: usize = 10;
/// The flag of an ID3v2.4 tag with a footer (another 10 bytes) after it.
const TAG_FOOTER: u8 = 0x10;

/// A sound file's bytes from where its audio starts.
pub struct Audio<B> {
    bytes: B,
    start: usize,
}

impl<B: AsRef<[u8]>> AsRef<[u8]> for Audio<B> {
    fn as_ref(&self) -> &[u8] {
        &self.bytes.as_ref()[self.start..]
    }
}

/// The decoder of a sound file's `bytes`, or why it is none.
pub fn decoder<B>(bytes: B) -> Result<rodio::Decoder<Cursor<Audio<B>>>, String>
where
    B: AsRef<[u8]> + Send + Sync + 'static,
{
    let file = bytes.as_ref();
    let start = if file.starts_with(b"RIFF") {
        hound::WavReader::new(Cursor::new(file)).map_err(|e| format!("a broken WAV file: {e}"))?;
        0
    } else {
        let start = tag_end(file)?;
        if !mpeg_frame(&file[start..]) {
            return Err("not a WAV or MP3 file as the importer writes them".to_owned());
        }
        start
    };
    let audio = Audio { bytes, start };
    rodio::Decoder::builder()
        .with_byte_len(audio.as_ref().len() as u64)
        .with_data(Cursor::new(audio))
        .build()
        .map_err(|e| e.to_string())
}

/// Where a leading ID3v2 tag of `file` ends (0 without one): its size is four bytes of seven
/// bits each, and it must end within the file.
fn tag_end(file: &[u8]) -> Result<usize, String> {
    let Some(header) = file
        .first_chunk::<TAG_HEADER>()
        .filter(|h| h.starts_with(b"ID3"))
    else {
        return Ok(0);
    };
    let size = &header[6..];
    if size.iter().any(|&b| b >= 0x80) {
        return Err("a broken ID3v2 tag".to_owned());
    }
    let size = size.iter().fold(0usize, |n, &b| (n << 7) | usize::from(b));
    let footer = if header[5] & TAG_FOOTER != 0 {
        TAG_HEADER
    } else {
        0
    };
    let end = TAG_HEADER + size + footer;
    if end > file.len() {
        return Err(format!(
            "an ID3v2 tag of {end} bytes, past the end of a file of {}",
            file.len()
        ));
    }
    Ok(end)
}

/// Whether `bytes` start with an MPEG audio frame's sync word and a version and layer that are
/// not reserved: the markers symphonia's MP3 reader is found by.
fn mpeg_frame(bytes: &[u8]) -> bool {
    let [0xFF, second, ..] = *bytes else {
        return false;
    };
    let (version, layer) = ((second >> 3) & 3, (second >> 1) & 3);
    second & 0xE0 == 0xE0 && version != 1 && layer != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A mono 16-bit WAV of `samples` silent samples.
    fn wav(samples: u32) -> Vec<u8> {
        let data = samples * 2;
        let mut file = b"RIFF".to_vec();
        file.extend((36 + data).to_le_bytes());
        file.extend(b"WAVEfmt ");
        file.extend(16u32.to_le_bytes());
        file.extend(1u16.to_le_bytes());
        file.extend(1u16.to_le_bytes());
        file.extend(22_050u32.to_le_bytes());
        file.extend(44_100u32.to_le_bytes());
        file.extend(2u16.to_le_bytes());
        file.extend(16u16.to_le_bytes());
        file.extend(b"data");
        file.extend(data.to_le_bytes());
        file.resize(file.len() + data as usize, 0);
        file
    }

    /// Silent MPEG-1 layer III frames (128 kbit/s, 44.1 kHz, 417 bytes each).
    fn mp3(frames: usize) -> Vec<u8> {
        let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
        frame.resize(417, 0);
        frame.repeat(frames)
    }

    /// An ID3v2 tag of `size` bytes past its header (zeros: padding), syncsafe as the format
    /// writes sizes.
    fn tag(size: u32) -> Vec<u8> {
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend((0..4).rev().map(|i| ((size >> (7 * i)) & 0x7F) as u8));
        tag.resize(10 + size as usize, 0);
        tag
    }

    /// The sounds the importer writes decode, an MP3 with a tag before its first frame too (an
    /// encoder may write one).
    #[test]
    fn a_wav_and_an_mp3_as_the_importer_writes_them_decode() {
        let wav = decoder(wav(1024)).expect("a WAV");
        assert_eq!(wav.count(), 1024);
        assert!(decoder(mp3(8)).is_ok());
        let tagged = [tag(300), mp3(8)].concat();
        let why = decoder(tagged).err();
        assert_eq!(why, None, "an MP3 after an ID3v2 tag");
    }

    /// Files that start with an ID3v2 tag, found by fuzzing: symphonia read the tag (rodio hands
    /// it every file its WAV reader refuses), panicking on the first and allocating 4 GiB for
    /// the others; any of them in the cache took the client down when it played. A tag is
    /// skipped here only when it ends within the file and a frame follows it, and symphonia is
    /// never handed one; any file of no format the importer writes is refused.
    #[test]
    fn a_file_other_than_a_wav_or_an_mp3_frame_is_refused_before_decoding() {
        let refused = [
            ("49443304c071aa54555555555555011c01ff", "a broken ID3v2 tag"),
            ("494433032431000000345452434bfeff00000000", "past the end"),
            ("4944330324bf000000345452434bff1f00002000", "past the end"),
        ]
        .map(|(file, why)| (hex(file), why))
        .into_iter()
        .chain([
            (
                [tag(16), b"junk".to_vec()].concat(),
                "not a WAV or MP3 file",
            ),
            ([tag(16), tag(16), mp3(2)].concat(), "not a WAV or MP3 file"),
            (b"junk".to_vec(), "not a WAV or MP3 file"),
            (Vec::new(), "not a WAV or MP3 file"),
        ]);
        for (file, expected) in refused {
            let why = decoder(file.clone()).err().expect("refused");
            assert!(why.contains(expected), "{file:x?}: {why}");
        }
        let mut broken_wav = wav(16);
        broken_wav[8..12].copy_from_slice(b"ID3\x04");
        let why = decoder(broken_wav).err().expect("refused");
        assert!(why.contains("a broken WAV file"), "{why}");
    }
}
