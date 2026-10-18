//! One compressed segment of an `ANIM` block: a run of frames of some bones' positions,
//! rotations or scales, stored by one of Valve's `CCompressed*` codecs. Reading a frame writes
//! the values into a pose and leaves the bones the segment does not cover alone.

use crate::error::{Result, malformed};

/// What a channel of the decode key animates (`m_szVariableName`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attribute {
    Position,
    Angle,
    Scale,
    /// Flex controller values (`data`): read by no glTF channel.
    Data,
}

impl Attribute {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Position" => Self::Position,
            "Angle" => Self::Angle,
            "Scale" => Self::Scale,
            "data" => Self::Data,
            _ => return None,
        })
    }
}

/// A bone's local transform in Source's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BonePose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
}

/// The codecs Valve's animation compiler writes. The ones not listed (chars, ints, bools,
/// colours, 2D/4D vectors) animate nothing a skeleton shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Codec {
    StaticFullVector3,
    StaticVector3,
    StaticQuaternion,
    StaticFloat,
    FullVector3,
    DeltaVector3,
    AnimVector3,
    AnimQuaternion,
    FullQuaternion,
    FullFloat,
}

impl Codec {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "CCompressedStaticFullVector3" => Self::StaticFullVector3,
            "CCompressedStaticVector3" => Self::StaticVector3,
            "CCompressedStaticQuaternion" => Self::StaticQuaternion,
            "CCompressedStaticFloat" => Self::StaticFloat,
            "CCompressedFullVector3" => Self::FullVector3,
            "CCompressedDeltaVector3" => Self::DeltaVector3,
            "CCompressedAnimVector3" => Self::AnimVector3,
            "CCompressedAnimQuaternion" => Self::AnimQuaternion,
            "CCompressedFullQuaternion" => Self::FullQuaternion,
            "CCompressedFullFloat" => Self::FullFloat,
            _ => return None,
        })
    }

    /// Bytes per element and frame.
    fn size(self) -> usize {
        match self {
            Self::StaticFullVector3 | Self::FullVector3 => 12,
            Self::StaticVector3
            | Self::DeltaVector3
            | Self::AnimVector3
            | Self::StaticQuaternion
            | Self::AnimQuaternion => 6,
            Self::StaticFloat | Self::FullFloat => 4,
            Self::FullQuaternion => 16,
        }
    }

    fn is_static(self) -> bool {
        matches!(
            self,
            Self::StaticFullVector3
                | Self::StaticVector3
                | Self::StaticQuaternion
                | Self::StaticFloat
        )
    }
}

/// A decoded value of one element.
enum Sample {
    Vector([f32; 3]),
    Quaternion([f32; 4]),
    Float(f32),
}

/// A segment ready to read: its codec, the channel it animates, its data and which of its
/// elements land on which bone.
pub(crate) struct Segment<'a> {
    codec: Codec,
    attribute: Attribute,
    data: &'a [u8],
    elements: usize,
    /// (element position in the segment, bone index), in bone order.
    wanted: Vec<(usize, usize)>,
}

impl<'a> Segment<'a> {
    /// Parses a segment's container: an `i16` codec index into `codecs` (the block's
    /// `m_decoderArray` names), an `i16` cardinality, an `i16` element count, an `i16` length,
    /// the elements' ids (`i16` each), then the codec's data. `remap[bone]` is the bone's
    /// element id in the segment's channel. `None` for a codec or channel that animates nothing
    /// a skeleton shows.
    pub(crate) fn parse(
        container: &'a [u8],
        codecs: &[&str],
        attribute: Option<Attribute>,
        remap: &[Option<i64>],
    ) -> Result<Option<Self>> {
        let word = |i: usize| -> Result<i16> {
            container
                .get(i * 2..i * 2 + 2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .ok_or_else(|| malformed("animation", "a segment header runs past its end"))
        };
        let codec = usize::try_from(word(0)?)
            .ok()
            .and_then(|i| codecs.get(i))
            .ok_or_else(|| malformed("animation", "a segment names no codec"))?;
        let elements = usize::try_from(word(2)?)
            .map_err(|_| malformed("animation", "a segment has a negative element count"))?;
        let ids = (0..elements)
            .map(|i| word(4 + i))
            .collect::<Result<Vec<_>>>()?;
        let (Some(codec), Some(attribute)) = (Codec::from_name(codec), attribute) else {
            return Ok(None);
        };
        // Element id -> its first position, as Valve's reader looks it up.
        let mut first = std::collections::HashMap::with_capacity(ids.len());
        for (position, &id) in ids.iter().enumerate() {
            first.entry(i64::from(id)).or_insert(position);
        }
        let wanted = remap
            .iter()
            .enumerate()
            .filter_map(|(bone, id)| Some((*first.get(&(*id)?)?, bone)))
            .collect();
        Ok(Some(Self {
            codec,
            attribute,
            data: container.get(8 + elements * 2..).unwrap_or_default(),
            elements,
            wanted,
        }))
    }

    /// Element reads one frame takes: the work it costs.
    pub(crate) fn work(&self) -> usize {
        self.wanted.len()
    }

    /// Writes frame `frame` (counted from the segment's first) of every bone it covers into
    /// `pose`.
    pub(crate) fn read(&self, frame: usize, pose: &mut [BonePose]) -> Result<()> {
        let row = if self.codec.is_static() {
            0
        } else {
            frame
                .checked_mul(self.elements)
                .ok_or_else(|| malformed("animation", "a frame index overflows"))?
        };
        for &(element, bone) in &self.wanted {
            let index = row
                .checked_add(element)
                .ok_or_else(|| malformed("animation", "a frame index overflows"))?;
            let sample = self.sample(index, element)?;
            let Some(target) = pose.get_mut(bone) else {
                continue;
            };
            match (self.attribute, sample) {
                (Attribute::Position, Sample::Vector(v)) => target.position = v,
                (Attribute::Angle, Sample::Quaternion(q)) => target.rotation = q,
                (Attribute::Scale, Sample::Float(s)) => target.scale = s,
                _ => {}
            }
        }
        Ok(())
    }

    /// The value at `index` (frame row plus element); `element` indexes the delta codec's base.
    fn sample(&self, index: usize, element: usize) -> Result<Sample> {
        let size = self.codec.size();
        let at = |data: &'a [u8], index: usize, size: usize| -> Result<&'a [u8]> {
            index
                .checked_mul(size)
                .and_then(|start| data.get(start..start.checked_add(size)?))
                .ok_or_else(|| malformed("animation", "a segment reads past its data"))
        };
        Ok(match self.codec {
            Codec::StaticFullVector3 | Codec::FullVector3 => {
                Sample::Vector(floats(at(self.data, index, size)?))
            }
            Codec::StaticVector3 | Codec::AnimVector3 => {
                Sample::Vector(halves(at(self.data, index, size)?))
            }
            Codec::DeltaVector3 => {
                let bases = self.elements * 12;
                let base: [f32; 3] = floats(at(self.data, element, 12)?);
                let deltas = self.data.get(bases..).unwrap_or_default();
                let delta = halves(at(deltas, index, size)?);
                Sample::Vector([0, 1, 2].map(|i| base[i] + delta[i]))
            }
            Codec::StaticQuaternion | Codec::AnimQuaternion => {
                Sample::Quaternion(quaternion48(at(self.data, index, size)?))
            }
            Codec::FullQuaternion => Sample::Quaternion(floats(at(self.data, index, size)?)),
            Codec::StaticFloat | Codec::FullFloat => {
                Sample::Float(floats::<1>(at(self.data, index, size)?)[0])
            }
        })
    }
}

fn floats<const N: usize>(bytes: &[u8]) -> [f32; N] {
    std::array::from_fn(|i| f32::from_le_bytes(std::array::from_fn(|k| bytes[i * 4 + k])))
}

fn halves(bytes: &[u8]) -> [f32; 3] {
    std::array::from_fn(|i| half(u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]])))
}

/// An IEEE 754 half as `f32`, exactly.
pub(crate) fn half(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from(bits >> 10 & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let magnitude = match exponent {
        // Subnormal: mantissa × 2^-24, a normal f32.
        0 => (mantissa as f32 * f32::from_bits(0x3380_0000)).to_bits(),
        0x1f => 0xff << 23 | mantissa << 13,
        e => (e + 112) << 23 | mantissa << 13,
    };
    f32::from_bits(sign | magnitude)
}

/// Valve's 48-bit quaternion: three 15-bit components (bit 14 set: no bias) scaled to
/// ±1/√2, the largest one left out and rebuilt; the top bits of the first two words say where
/// it goes, the top bit of the third its sign.
fn quaternion48(b: &[u8]) -> [f32; 4] {
    const SCALE: f32 = std::f32::consts::FRAC_1_SQRT_2 / 16384.0;
    let component = |lo: u8, hi: u8| {
        let value = f32::from(u16::from(lo) | u16::from(hi & 63) << 8);
        if hi & 64 == 0 {
            SCALE * (value - 16384.0)
        } else {
            SCALE * value
        }
    };
    let (x, y, z) = (
        component(b[0], b[1]),
        component(b[2], b[3]),
        component(b[4], b[5]),
    );
    let mut w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    if b[5] & 128 != 0 {
        w = -w;
    }
    match (b[1] & 128 != 0, b[3] & 128 != 0) {
        (true, true) => [y, z, w, x],
        (true, false) => [z, w, x, y],
        (false, true) => [w, x, y, z],
        (false, false) => [x, y, z, w],
    }
}
