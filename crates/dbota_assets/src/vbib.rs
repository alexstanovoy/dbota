//! Vertex and index buffers of Source 2 meshes: the descriptions a model's `CTRL` block (or an
//! older binary `VBIB`/`MBUF` block) gives, their data (raw, zstd and/or meshopt compressed), and
//! the attribute formats Dota's meshes use.

use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::meshopt;
use crate::resource::Resource;

/// Largest decoded buffer accepted.
const MAX_BUFFER: usize = 256 << 20;
/// Binary buffer size flags: meshopt when clear, zstd when set; the rest is the stride.
const FLAG_NOT_MESHOPT: u32 = 0x400_0000;
const FLAG_ZSTD: u32 = 0x800_0000;
const STRIDE_MASK: u32 = 0x3FF_FFFF;

// DXGI formats of vertex attributes.
pub(crate) const R32G32B32A32_FLOAT: u32 = 2;
pub(crate) const R32G32B32_FLOAT: u32 = 6;
pub(crate) const R16G16B16A16_FLOAT: u32 = 10;
pub(crate) const R16G16B16A16_SINT: u32 = 14;
pub(crate) const R32G32_FLOAT: u32 = 16;
pub(crate) const R8G8B8A8_UNORM: u32 = 28;
pub(crate) const R8G8B8A8_UINT: u32 = 30;
pub(crate) const R16G16_FLOAT: u32 = 34;
pub(crate) const R16G16_UNORM: u32 = 35;
pub(crate) const R16G16_SNORM: u32 = 37;
pub(crate) const R16G16_SINT: u32 = 38;
pub(crate) const R32_UINT: u32 = 42;

/// Components per vertex of an attribute stored as `format`, for the formats
/// [`Buffer::floats`] reads.
pub(crate) fn components(format: u32) -> Option<usize> {
    match format {
        R32G32B32A32_FLOAT | R16G16B16A16_FLOAT | R8G8B8A8_UNORM => Some(4),
        R32G32B32_FLOAT => Some(3),
        R32G32_FLOAT | R16G16_FLOAT | R16G16_UNORM | R16G16_SNORM => Some(2),
        _ => None,
    }
}

/// One vertex or index buffer, decoded.
#[derive(Clone, Debug)]
pub struct Buffer {
    pub count: usize,
    pub stride: usize,
    pub fields: Vec<Field>,
    pub data: Vec<u8>,
}

/// One vertex attribute of a [`Buffer`]; `semantic` is upper case (`POSITION`, `TEXCOORD`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub semantic: String,
    pub index: u32,
    pub format: u32,
    pub offset: usize,
}

/// Reads one vertex's attribute from its bytes.
type Read<T> = fn(&[u8]) -> T;
/// Normals, and tangents with the bitangent sign in w when the format has them.
type NormalsTangents = (Vec<[f32; 3]>, Option<Vec<[f32; 4]>>);

/// How a buffer's bytes are stored.
struct Packing {
    meshopt: bool,
    zstd: bool,
}

fn decode(
    stored: &[u8],
    count: usize,
    stride: usize,
    vertex: bool,
    packing: Packing,
) -> Result<Vec<u8>> {
    let len = count
        .checked_mul(stride)
        .filter(|&len| len <= MAX_BUFFER)
        .ok_or_else(|| malformed("mesh buffer", format!("{count} × {stride} bytes")))?;
    let unzipped;
    let mut bytes = stored;
    if packing.zstd {
        unzipped = kv3::zstd_to_vec(stored, len)?;
        bytes = &unzipped;
    }
    if !packing.meshopt {
        if bytes.len() != len {
            return Err(malformed(
                "mesh buffer",
                format!("{} bytes, expected {len}", bytes.len()),
            ));
        }
        return Ok(bytes.to_vec());
    }
    if vertex {
        meshopt::decode_vertex_buffer(bytes, count, stride)
    } else {
        meshopt::decode_index_buffer(bytes, count, stride)
    }
}

/// The buffer a `CTRL` description (`m_vertexBuffers[i]` or `m_indexBuffers[i]`) names.
pub(crate) fn from_ctrl(resource: &Resource, desc: &Value, vertex: bool) -> Result<Buffer> {
    let number = |key: &str| {
        desc.get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| malformed("mesh buffer", format!("no {key}")))
    };
    let count = number("m_nElementCount")? as usize;
    let stride = number("m_nElementSizeInBytes")? as usize;
    let flag = |key: &str| desc.get(key).and_then(Value::as_bool).unwrap_or(false);
    let block = number("m_nBlockIndex")? as usize;
    let block = resource
        .blocks()
        .get(block)
        .ok_or_else(|| malformed("mesh buffer", format!("no block {block}")))?;
    let packing = Packing {
        meshopt: flag("m_bMeshoptCompressed"),
        zstd: flag("m_bCompressedZSTD"),
    };
    let data = decode(resource.bytes(block), count, stride, vertex, packing)?;
    let fields = desc
        .get("m_inputLayoutFields")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(field_from_kv3)
        .collect::<Result<_>>()?;
    Ok(Buffer {
        count,
        stride,
        fields,
        data,
    })
}

fn field_from_kv3(field: &Value) -> Result<Field> {
    let semantic = match field.get("m_pSemanticName") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Blob(bytes)) => String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_owned(),
        _ => return Err(malformed("mesh buffer", "a field without a semantic")),
    };
    let number = |key: &str| field.get(key).and_then(Value::as_u64).unwrap_or(0);
    Ok(Field {
        semantic: semantic.to_uppercase(),
        index: number("m_nSemanticIndex") as u32,
        format: number("m_Format") as u32,
        offset: number("m_nOffset") as usize,
    })
}

/// The vertex and index buffers of a binary `VBIB` (or `MBUF`) block.
pub(crate) fn from_vbib(block: &[u8]) -> Result<(Vec<Buffer>, Vec<Buffer>)> {
    let mut c = Cursor::new(block, "VBIB");
    let vertex_at = c.u32()? as usize;
    let vertex_count = c.u32()? as usize;
    let index_at = 8 + c.u32()? as usize;
    let index_count = c.u32()? as usize;
    let list = |at: usize, count: usize, vertex: bool| -> Result<Vec<Buffer>> {
        if count > block.len() / 24 {
            return Err(malformed("VBIB", format!("{count} buffers")));
        }
        (0..count)
            .map(|i| binary_buffer(block, at + i * 24, vertex))
            .collect()
    };
    Ok((
        list(vertex_at, vertex_count, true)?,
        list(index_at, index_count, false)?,
    ))
}

/// A cursor at `at` of `block`.
fn seek(block: &[u8], at: usize) -> Result<Cursor<'_>> {
    let mut c = Cursor::new(block, "VBIB");
    c.take(at)?;
    Ok(c)
}

/// One 24-byte buffer header: count, stride and flags, then (offset, count) of its fields and
/// (offset, size) of its data, each offset counted from its own field.
fn binary_buffer(block: &[u8], at: usize, vertex: bool) -> Result<Buffer> {
    let mut c = seek(block, at)?;
    let count = c.u32()? as usize;
    let size = c.u32()?;
    let fields_at = c.pos() + c.u32()? as usize;
    let field_count = c.u32()? as usize;
    let data_at = c.pos() + c.u32()? as usize;
    let stored = c.u32()? as usize;
    let stride = (size & STRIDE_MASK) as usize;
    if field_count > block.len() / 56 {
        return Err(malformed("VBIB", format!("{field_count} fields")));
    }
    let fields = (0..field_count)
        .map(|i| binary_field(block, fields_at + i * 56))
        .collect::<Result<_>>()?;
    let stored = seek(block, data_at)?.take(stored)?;
    let compressed = count.saturating_mul(stride) > stored.len();
    let packing = Packing {
        meshopt: compressed && size & FLAG_NOT_MESHOPT == 0,
        zstd: compressed && size & FLAG_ZSTD != 0,
    };
    let data = decode(stored, count, stride, vertex, packing)?;
    Ok(Buffer {
        count,
        stride,
        fields,
        data,
    })
}

/// A 56-byte field: a 32-byte name, semantic index, format, offset, then slot data.
fn binary_field(block: &[u8], at: usize) -> Result<Field> {
    let mut c = seek(block, at)?;
    let name = c.take(32)?;
    let end = name.iter().position(|&b| b == 0).unwrap_or(32);
    Ok(Field {
        semantic: String::from_utf8_lossy(&name[..end]).to_uppercase(),
        index: c.u32()?,
        format: c.u32()?,
        offset: c.u32()? as usize,
    })
}

impl Buffer {
    /// The bytes of attribute `field` of every vertex, `width` bytes each.
    fn column<'a>(&'a self, field: &Field, width: usize) -> Result<impl Iterator<Item = &'a [u8]>> {
        if field.offset + width > self.stride || self.data.len() < self.count * self.stride {
            return Err(malformed(
                "mesh buffer",
                format!("{} runs past its vertex", field.semantic),
            ));
        }
        let offset = field.offset;
        Ok(self
            .data
            .chunks_exact(self.stride)
            .take(self.count)
            .map(move |vertex| &vertex[offset..offset + width]))
    }

    pub fn field(&self, semantic: &str, index: u32) -> Option<&Field> {
        self.fields
            .iter()
            .find(|f| f.semantic == semantic && f.index == index)
    }

    /// Up to four float components of `field` (unused ones 0), whatever its format.
    pub fn floats(&self, field: &Field) -> Result<Vec<[f32; 4]>> {
        let (width, read): (usize, Read<[f32; 4]>) = match field.format {
            R32G32B32A32_FLOAT => (16, |b| {
                [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8), f32_at(b, 12)]
            }),
            R32G32B32_FLOAT => (12, |b| [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8), 0.0]),
            R32G32_FLOAT => (8, |b| [f32_at(b, 0), f32_at(b, 4), 0.0, 0.0]),
            R16G16B16A16_FLOAT => (8, |b| {
                [half_at(b, 0), half_at(b, 2), half_at(b, 4), half_at(b, 6)]
            }),
            R16G16_FLOAT => (4, |b| [half_at(b, 0), half_at(b, 2), 0.0, 0.0]),
            R16G16_UNORM => (4, |b| {
                [
                    u16_at(b, 0) as f32 / 65535.0,
                    u16_at(b, 2) as f32 / 65535.0,
                    0.0,
                    0.0,
                ]
            }),
            R16G16_SNORM => (4, |b| {
                [
                    u16_at(b, 0) as i16 as f32 / 32767.0,
                    u16_at(b, 2) as i16 as f32 / 32767.0,
                    0.0,
                    0.0,
                ]
            }),
            R8G8B8A8_UNORM => (4, |b| {
                b[..4]
                    .iter()
                    .map(|&v| v as f32 / 255.0)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap_or_default()
            }),
            other => {
                return Err(malformed(
                    "mesh buffer",
                    format!("{} format {other}", field.semantic),
                ));
            }
        };
        Ok(self.column(field, width)?.map(read).collect())
    }

    /// Normals and tangents (w = bitangent sign), unpacked from the two compressed forms or read
    /// as plain floats (then without tangents).
    pub fn normals_tangents(&self, field: &Field) -> Result<NormalsTangents> {
        match field.format {
            R32G32B32_FLOAT => {
                let normals = self
                    .floats(field)?
                    .into_iter()
                    .map(|v| [v[0], v[1], v[2]])
                    .collect();
                Ok((normals, None))
            }
            R32_UINT => {
                let (normals, tangents) = self
                    .column(field, 4)?
                    .map(|b| unpack_frame(u32_at(b, 0)))
                    .unzip();
                Ok((normals, Some(tangents)))
            }
            R8G8B8A8_UNORM => {
                let (normals, tangents) = self
                    .column(field, 4)?
                    .map(|b| (unpack_normal(b[0], b[1]), unpack_tangent(b[2], b[3])))
                    .unzip();
                Ok((normals, Some(tangents)))
            }
            other => Err(malformed("mesh buffer", format!("NORMAL format {other}"))),
        }
    }

    /// Four bone indices per vertex (into the mesh's remapping table).
    pub fn joints(&self, field: &Field) -> Result<Vec<[u16; 4]>> {
        let (width, read): (usize, Read<[u16; 4]>) = match field.format {
            R8G8B8A8_UINT => (4, |b| [b[0].into(), b[1].into(), b[2].into(), b[3].into()]),
            R16G16B16A16_SINT => (8, |b| {
                [u16_at(b, 0), u16_at(b, 2), u16_at(b, 4), u16_at(b, 6)]
            }),
            // Two bones: the second repeats in the unused slots, as Valve's reader does.
            R16G16_SINT => (4, |b| {
                [u16_at(b, 0), u16_at(b, 2), u16_at(b, 2), u16_at(b, 2)]
            }),
            other => {
                return Err(malformed(
                    "mesh buffer",
                    format!("BLENDINDICES format {other}"),
                ));
            }
        };
        Ok(self.column(field, width)?.map(read).collect())
    }

    /// The indices of an index buffer (2 or 4 bytes each).
    pub fn indices(&self) -> Result<Vec<u32>> {
        let read: fn(&[u8]) -> u32 = match self.stride {
            2 => |b| u16_at(b, 0).into(),
            4 => |b| u32_at(b, 0),
            other => return Err(malformed("mesh buffer", format!("{other}-byte indices"))),
        };
        if self.data.len() < self.count * self.stride {
            return Err(malformed("mesh buffer", "index data too short"));
        }
        Ok(self
            .data
            .chunks_exact(self.stride)
            .take(self.count)
            .map(read)
            .collect())
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

/// An IEEE half float.
fn half_at(b: &[u8], at: usize) -> f32 {
    let h = u16_at(b, at);
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1F);
    let mantissa = f32::from(h & 0x3FF);
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 if mantissa == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
    }
}

/// The first compressed form: an octahedral-like normal in two bytes.
fn unpack_normal(x: u8, y: u8) -> [f32; 3] {
    let (mut x, mut y) = (f32::from(x) - 128.0, f32::from(y) - 128.0);
    let z_neg = if x < 0.0 { 1.0 } else { 0.0 };
    let t_neg = if y < 0.0 { 1.0 } else { 0.0 };
    let (z_sign, t_sign) = (1.0 - 2.0 * z_neg, 1.0 - 2.0 * t_neg);
    x = x * z_sign - z_neg - 64.0;
    y = y * t_sign - t_neg - 64.0;
    let x_neg = if x < 0.0 { 1.0 } else { 0.0 };
    let y_neg = if y < 0.0 { 1.0 } else { 0.0 };
    let (x_sign, y_sign) = (1.0 - 2.0 * x_neg, 1.0 - 2.0 * y_neg);
    x = (x * x_sign - x_neg) / 63.0;
    y = (y * y_sign - y_neg) / 63.0;
    let z = 1.0 - x - y;
    let inv = 1.0 / (x * x + y * y + z * z).sqrt();
    [x * inv * x_sign, y * inv * y_sign, z * inv * z_sign]
}

fn unpack_tangent(x: u8, y: u8) -> [f32; 4] {
    let [a, b, c] = unpack_normal(x, y);
    [a, b, c, if y < 128 { -1.0 } else { 1.0 }]
}

/// The second compressed form: a 32-bit tangent frame (10+10 bits of normal, 11 bits of
/// tangent angle around it, 1 bit of bitangent sign).
fn unpack_frame(packed: u32) -> ([f32; 3], [f32; 4]) {
    let sign = if packed & 1 == 0 { -1.0 } else { 1.0 };
    let angle = ((packed >> 1) & 0x7FF) as f32 / 2047.0 * std::f32::consts::TAU;
    let x = ((packed >> 12) & 0x3FF) as f32 / 1023.0 * 2.0 - 1.0;
    let y = ((packed >> 22) & 0x3FF) as f32 / 1023.0 * 2.0 - 1.0;
    let z = 1.0 - x.abs() - y.abs();
    let fold = (-z).clamp(0.0, 1.0);
    let x = x + if x >= 0.0 { -fold } else { fold };
    let y = y + if y >= 0.0 { -fold } else { fold };
    let n = normalize([x, y, z]);
    let t_sign = if n[2] >= 0.0 { 1.0 } else { -1.0 };
    let rcp = 1.0 / (t_sign + n[2]);
    let base = [
        -t_sign * (n[0] * n[0]) * rcp + 1.0,
        -t_sign * ((n[0] * n[1]) * rcp),
        -t_sign * n[0],
    ];
    let cross = [
        n[1] * base[2] - n[2] * base[1],
        n[2] * base[0] - n[0] * base[2],
        n[0] * base[1] - n[1] * base[0],
    ];
    let (s, c) = angle.sin_cos();
    let t = [0, 1, 2].map(|i| base[i] * c + cross[i] * s);
    (n, [t[0], t[1], t[2], sign])
}

pub(crate) fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 { v.map(|c| c / len) } else { v }
}
