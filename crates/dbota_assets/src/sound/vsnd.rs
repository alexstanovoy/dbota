use std::borrow::Cow;

use super::{mp3, wav};
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

const WHAT: &str = "vsnd";
const MAX_RATE: u32 = 384_000;
const MAX_CHANNELS: u16 = 8;

/// The kind of audio file a sound is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundFormat {
    /// Uncompressed 8- or 16-bit samples.
    Wav,
    Mp3,
}

/// A sound of a `vsnd_c`, with the audio data it holds (borrowed from the file).
#[derive(Clone, Debug)]
pub struct Sound<'a> {
    format: SoundFormat,
    sample_rate: u32,
    channels: u16,
    bits: u16,
    samples: u64,
    duration: f32,
    data: &'a [u8],
}

impl<'a> Sound<'a> {
    /// Reads the sound of a `vsnd_c`: older files describe it in a small binary `DATA` block,
    /// newer ones in a KV3 `CTRL` block; the audio follows the resource either way. Only PCM and
    /// MP3 sounds are supported; others are errors.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let resource = Resource::parse(bytes)?;
        let audio = resource.trailing();
        let info = match resource.kv3(b"CTRL") {
            Some(ctrl) => Info::from_ctrl(&ctrl?.root)?,
            None => Info::from_data(
                resource
                    .block(b"DATA")
                    .ok_or_else(|| malformed(WHAT, "no DATA or CTRL block"))?,
                resource.version(),
            )?,
        };
        let data = audio.get(..info.size).ok_or_else(|| {
            malformed(
                WHAT,
                format!("{} audio bytes of {}", audio.len(), info.size),
            )
        })?;
        let (format, sample_rate, channels, bits) = match info.pcm_bits {
            Some(bits) => {
                let block = usize::from(info.channels) * usize::from(bits / 8);
                if info.channels == 0 || info.channels > MAX_CHANNELS || data.len() % block != 0 {
                    return Err(malformed(WHAT, "PCM channels or size"));
                }
                (SoundFormat::Wav, info.rate, info.channels, bits)
            }
            None => {
                let (rate, channels) = mp3::first_frame(data)?;
                (SoundFormat::Mp3, rate, channels, 0)
            }
        };
        if sample_rate == 0 || sample_rate > MAX_RATE {
            return Err(malformed(WHAT, format!("sample rate {sample_rate}")));
        }
        Ok(Self {
            format,
            sample_rate,
            channels,
            bits,
            samples: info.samples,
            duration: info.duration,
            data,
        })
    }

    pub fn format(&self) -> SoundFormat {
        self.format
    }

    /// `"wav"` or `"mp3"`: the extension of [`file`](Self::file).
    pub fn extension(&self) -> &'static str {
        match self.format {
            SoundFormat::Wav => "wav",
            SoundFormat::Mp3 => "mp3",
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Bits per sample of PCM sounds, 0 for MP3.
    pub fn bits_per_sample(&self) -> u16 {
        self.bits
    }

    /// Samples per channel, as the file declares.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Length in seconds, as the file declares.
    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// The audio file: a MP3 stream as it is stored, PCM samples behind a canonical WAV header.
    pub fn file(&self) -> Cow<'a, [u8]> {
        match self.format {
            SoundFormat::Mp3 => Cow::Borrowed(self.data),
            SoundFormat::Wav => {
                // The size field is 32 bits; a longer sound is cut at its last whole sample.
                let len = (self.data.len() as u64).min(u64::from(u32::MAX - 36)) as usize;
                let block = usize::from(self.channels * self.bits / 8);
                let len = len - len % block;
                let head = wav::header(self.sample_rate, self.channels, self.bits, len as u32);
                Cow::Owned([&head[..], &self.data[..len]].concat())
            }
        }
    }
}

/// What a `vsnd_c` says about its sound.
struct Info {
    /// Bits per sample when the audio is PCM; none for MP3.
    pcm_bits: Option<u16>,
    rate: u32,
    channels: u16,
    samples: u64,
    duration: f32,
    /// Bytes of audio after the resource.
    size: usize,
}

impl Info {
    /// The newer layout: `CTRL.m_vSound`.
    fn from_ctrl(root: &Value) -> Result<Self> {
        let sound = root
            .get("m_vSound")
            .ok_or_else(|| malformed(WHAT, "CTRL has no m_vSound"))?;
        let number = |key: &str| {
            sound
                .get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| malformed(WHAT, format!("no {key}")))
        };
        let pcm_bits = match sound.get("m_nFormat").and_then(Value::as_str) {
            Some("PCM16") => Some(16),
            Some("PCM8") => Some(8),
            Some("MP3") => None,
            other => return Err(malformed(WHAT, format!("unsupported format {other:?}"))),
        };
        Ok(Self {
            pcm_bits,
            rate: u32::try_from(number("m_nRate")?).map_err(|_| malformed(WHAT, "rate"))?,
            channels: u16::try_from(number("m_nChannels")?)
                .map_err(|_| malformed(WHAT, "channels"))?,
            samples: number("m_nSampleCount")?,
            duration: sound
                .get("m_flDuration")
                .and_then(Value::as_f64)
                .ok_or_else(|| malformed(WHAT, "no m_flDuration"))? as f32,
            size: usize::try_from(number("m_nStreamingSize")?)
                .map_err(|_| malformed(WHAT, "size"))?,
        })
    }

    /// The older layouts, a small binary `DATA` block: a first word that says the format (see
    /// below), a -1 or 0, the sample count, the duration (f32) and, at byte 28, the audio size.
    /// Resource version 4 starts with the rate (u16), the format (0 PCM16, 1 PCM8, 2 MP3) and
    /// the channels (one byte each); version 1 with a bit-packed word: file type (2 bits; 1 is
    /// PCM, 2 is MP3), bits per sample (5), channels (2), sample size (3), format (2), rate (17).
    /// MP3 files tell their rate and channels in the stream.
    fn from_data(data: &[u8], version: u16) -> Result<Self> {
        let mut c = Cursor::new(data, WHAT);
        let packed = c.u32()?;
        c.u32()?;
        let samples = u64::from(c.u32()?);
        let duration = f32::from_bits(c.u32()?);
        c.take(12)?;
        let size = c.len32()?;
        let field = |low: u32, width: u32| (packed >> low) & ((1 << width) - 1);
        let (pcm_bits, rate, channels) = if version >= 4 {
            let bits = match field(16, 8) {
                0 => Some(16),
                1 => Some(8),
                2 => None,
                other => return Err(malformed(WHAT, format!("audio format {other}"))),
            };
            (bits, field(0, 16), field(24, 8) as u16)
        } else {
            let bits = match field(0, 2) {
                1 => Some(field(2, 5) as u16),
                2 => None,
                other => return Err(malformed(WHAT, format!("audio file type {other}"))),
            };
            (bits, field(14, 17), field(7, 2) as u16)
        };
        if pcm_bits.is_some_and(|bits| bits != 8 && bits != 16) {
            return Err(malformed(WHAT, "PCM that is not 8 or 16 bits"));
        }
        Ok(Self {
            pcm_bits,
            rate,
            channels,
            samples,
            duration,
            size,
        })
    }
}
