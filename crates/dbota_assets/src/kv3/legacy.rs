//! The first binary KV3 (`VKV\x03`): an encoding GUID, then one stream holding the string table
//! and the values inline.

use super::decode::{buffer, lz4, node, string};
use super::value::{Document, Object, Value};
use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};

const ENCODING_BLOCK_COMPRESSED: [u8; 16] = [
    0x46, 0x1A, 0x79, 0x95, 0xBC, 0x95, 0x6C, 0x4F, 0xA7, 0x0B, 0x05, 0xBC, 0xA1, 0xB7, 0xDF, 0xD2,
];
const ENCODING_LZ4: [u8; 16] = [
    0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1, 0x19,
];
/// Stored as is: `binary`, and `binary_zstd`/`binary_auto`, which this header never compressed.
const ENCODINGS_STORED: [[u8; 16]; 3] = [
    [
        0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7,
        0x14,
    ],
    [
        0x00, 0x0A, 0x62, 0x6F, 0xF0, 0xFE, 0x05, 0x43, 0xA3, 0x5F, 0x04, 0x23, 0x46, 0xB1, 0xDB,
        0x29,
    ],
    [
        0xE6, 0x09, 0xB1, 0x6E, 0x85, 0x6B, 0x83, 0x45, 0xA3, 0x12, 0x70, 0x3A, 0x6E, 0x04, 0x06,
        0x8C,
    ],
];
const TRAILER: u32 = 0xFFFF_FFFF;

pub(crate) fn decode(bytes: &[u8]) -> Result<Document> {
    let mut c = Cursor::new(bytes, "KV3");
    c.u32()?;
    let encoding: [u8; 16] = c.array()?;
    let format = c.array()?;
    let data = if encoding == ENCODING_BLOCK_COMPRESSED {
        block_decompress(&mut c)?
    } else if encoding == ENCODING_LZ4 {
        let len = c.u32()? as usize;
        lz4(c.rest(), len)?
    } else if ENCODINGS_STORED.contains(&encoding) {
        c.rest().to_vec()
    } else {
        return Err(malformed("KV3", "unknown encoding"));
    };
    let mut c = Cursor::new(&data, "KV3");
    let count = c.u32()? as usize;
    let mut strings = Vec::with_capacity(count.min(c.remaining()));
    for _ in 0..count {
        strings.push(String::from_utf8_lossy(c.cstr()?).into_owned());
    }
    let mut decoder = Decoder {
        c,
        strings,
        budget: Budget::new("KV3"),
    };
    let ty = decoder.read_type()?;
    let root = decoder.value(ty, 0)?;
    if decoder.c.u32()? != TRAILER {
        return Err(malformed("KV3", "no trailer"));
    }
    Ok(Document { format, root })
}

/// Valve's block compression: 16 flag bits per group; a set bit is a back reference of
/// 12 bits offset and 4 bits length, a clear one a literal byte.
fn block_decompress(c: &mut Cursor) -> Result<Vec<u8>> {
    let size = c.u32()?;
    // The top bit marks data stored uncompressed.
    if size & 0x8000_0000 != 0 {
        return Ok(c.take((size & 0x7FFF_FFFF) as usize)?.to_vec());
    }
    // A 16-bit back reference makes 18 bytes at most, and every 16 of them take a 16-bit mask.
    if size as usize / 9 > c.remaining() {
        return Err(malformed(
            "KV3 block compression",
            format!("{} bytes cannot expand to {size}", c.remaining()),
        ));
    }
    let mut out = buffer(size as usize)?;
    let (mut at, mut mask, mut bits) = (0usize, 0u16, 0);
    while at < out.len() {
        if bits == 0 {
            mask = c.u16()?;
            bits = 16;
        }
        if mask & 1 != 0 {
            let token = c.u16()? as usize;
            let back = (token >> 4) + 1;
            let len = ((token & 0xF) + 3).min(out.len() - at);
            let from = at
                .checked_sub(back)
                .ok_or_else(|| malformed("KV3 block compression", "reference before the start"))?;
            // Byte by byte: a reference may overlap the bytes it produces.
            for i in 0..len {
                out[at + i] = out[from + i];
            }
            at += len;
        } else {
            out[at] = c.u8()?;
            at += 1;
        }
        mask >>= 1;
        bits -= 1;
    }
    Ok(out)
}

struct Decoder<'a> {
    c: Cursor<'a>,
    strings: Vec<String>,
    budget: Budget,
}

impl Decoder<'_> {
    fn read_type(&mut self) -> Result<u8> {
        let byte = self.c.u8()?;
        if byte & 0x80 != 0 && self.c.u8()? & 0x80 != 0 {
            return Err(malformed("KV3", "unknown flag"));
        }
        Ok(byte & 0x7F)
    }

    fn value(&mut self, ty: u8, depth: usize) -> Result<Value> {
        self.budget.node(depth)?;
        let c = &mut self.c;
        Ok(match ty {
            node::NULL => Value::Null,
            node::BOOLEAN => Value::Bool(c.u8()? != 0),
            node::BOOLEAN_TRUE => Value::Bool(true),
            node::BOOLEAN_FALSE => Value::Bool(false),
            node::INT64_ZERO => Value::Int(0),
            node::INT64_ONE => Value::Int(1),
            node::INT64 => Value::Int(c.u64()? as i64),
            node::UINT64 => Value::UInt(c.u64()?),
            node::INT32 => Value::Int(i64::from(c.i32()?)),
            node::UINT32 => Value::UInt(u64::from(c.u32()?)),
            node::DOUBLE => Value::Float(f64::from_bits(c.u64()?)),
            node::DOUBLE_ZERO => Value::Float(0.0),
            node::DOUBLE_ONE => Value::Float(1.0),
            node::STRING => {
                let id = c.i32()?;
                Value::String(string(&self.strings, id, &mut self.budget)?)
            }
            node::BINARY_BLOB => {
                let len = c.len32()?;
                let bytes = c.take(len)?;
                self.budget.bytes(len)?;
                Value::Blob(bytes.to_vec())
            }
            node::ARRAY => {
                let len = c.len32()?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    let ty = self.read_type()?;
                    items.push(self.value(ty, depth + 1)?);
                }
                Value::Array(items)
            }
            node::ARRAY_TYPED => {
                let len = c.len32()?;
                let element = self.read_type()?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.value(element, depth + 1)?);
                }
                Value::Array(items)
            }
            node::OBJECT => self.object(depth)?,
            _ => return Err(malformed("KV3", format!("unknown node type {ty}"))),
        })
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        let len = self.c.len32()?;
        let mut object = Object::with_capacity(len.min(4096));
        for _ in 0..len {
            let id = self.c.i32()?;
            let key = string(&self.strings, id, &mut self.budget)?;
            let ty = self.read_type()?;
            let value = self.value(ty, depth + 1)?;
            object.push(key, value);
        }
        Ok(Value::Object(object))
    }
}
