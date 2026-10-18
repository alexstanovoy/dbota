//! Binary KV3 versions 1 to 5 (`\x0n3VK`): values split into lanes by width, a string table,
//! a type stream and binary blobs, optionally LZ4 or zstd compressed.

use super::decode::{LZ4_RATIO, buffer, lz4, node, string, zstd};
use super::value::{Document, Object, Value};
use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};

const TRAILER: u32 = 0xFFEE_DD00;
/// The only LZ4 frame size writers use for chained blob frames.
const LZ4_FRAME: u16 = 16384;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Compression {
    None,
    Lz4,
    Zstd,
}

/// Lane sizes (in values) of the 1-, 2-, 4- and 8-byte lanes.
type Counts = [usize; 4];

struct Header {
    version: u32,
    format: [u8; 16],
    compression: Compression,
    counts: Counts,
    /// Bytes of string table plus type stream (versions 2-4), or of the type stream (5).
    types_len: usize,
    total_len: usize,
    blobs: usize,
    blobs_len: usize,
    /// Versions before 5 hold everything in buffer 1; buffer 2 holds the values in version 5.
    buffer1: (usize, usize),
    buffer2: (usize, usize),
    compressed_total: usize,
    counts2: Counts,
    objects2: usize,
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Document> {
    let mut c = Cursor::new(bytes, "KV3");
    let version = c.u32()? & 0xFF;
    let h = Header::read(&mut c, version)?;
    let (buffer1, early_blobs) = inflate_buffer1(&h, &mut c)?;
    let buffer2 = if version >= 5 {
        inflate(&h, &mut c, h.buffer2)?
    } else {
        Vec::new()
    };
    let mut layout = Layout::new(&h, &buffer1, &buffer2)?;
    let mut blobs = Vec::new();
    if h.blobs > 0 {
        blobs = match early_blobs {
            Some(blobs) => blobs,
            None => inflate_blobs(&h, &mut c, &mut layout)?,
        };
        if c.u32()? != TRAILER {
            return Err(malformed("KV3", "no trailer after the blobs"));
        }
    }
    let mut decoder = Decoder {
        version,
        layout,
        blobs: Cursor::new(&blobs, "KV3 blobs"),
        budget: Budget::new("KV3"),
    };
    let ty = decoder.read_type()?;
    let root = decoder.value(ty, false, 0)?;
    Ok(Document {
        format: h.format,
        root,
    })
}

impl Header {
    fn read(c: &mut Cursor, version: u32) -> Result<Self> {
        let format = c.array()?;
        let compression = match c.u32()? {
            0 => Compression::None,
            1 => Compression::Lz4,
            2 if version >= 2 => Compression::Zstd,
            other => return Err(malformed("KV3", format!("compression method {other}"))),
        };
        if version == 1 {
            let counts = [c.len32()?, 0, c.len32()?, c.len32()?];
            let total_len = c.len32()?;
            let compressed = c.remaining();
            return Ok(Self::simple(
                version,
                format,
                compression,
                counts,
                total_len,
                compressed,
            ));
        }
        let dictionary = c.u16()?;
        let frame = c.u16()?;
        let expected_frame = if compression == Compression::Lz4 {
            LZ4_FRAME
        } else {
            0
        };
        if dictionary != 0 || frame != expected_frame {
            return Err(malformed(
                "KV3",
                format!("dictionary {dictionary}, frame size {frame}"),
            ));
        }
        let (b1, b4, b8) = (c.len32()?, c.len32()?, c.len32()?);
        let types_len = c.len32()?;
        // Object and array counts: allocation hints only.
        c.take(4)?;
        let total_len = c.len32()?;
        let compressed_total = c.len32()?;
        let blobs = c.len32()?;
        let blobs_len = c.len32()?;
        let mut b2 = 0;
        if version >= 4 {
            b2 = c.len32()?;
            // The LZ4 frame-size list's length: the list ends its buffer anyway.
            c.take(4)?;
        }
        let mut h = Self::simple(
            version,
            format,
            compression,
            [b1, b2, b4, b8],
            total_len,
            compressed_total,
        );
        h.types_len = types_len;
        h.blobs = blobs;
        h.blobs_len = blobs_len;
        if version >= 5 {
            h.read_v5(c)?;
        }
        Ok(h)
    }

    fn simple(
        version: u32,
        format: [u8; 16],
        compression: Compression,
        counts: Counts,
        total_len: usize,
        compressed_total: usize,
    ) -> Self {
        Self {
            version,
            format,
            compression,
            counts,
            types_len: 0,
            total_len,
            blobs: 0,
            blobs_len: 0,
            buffer1: (total_len, compressed_total),
            buffer2: (0, 0),
            compressed_total,
            counts2: [0; 4],
            objects2: 0,
        }
    }

    fn read_v5(&mut self, c: &mut Cursor) -> Result<()> {
        self.buffer1 = (c.len32()?, c.len32()?);
        self.buffer2 = (c.len32()?, c.len32()?);
        self.counts2 = [c.len32()?, c.len32()?, c.len32()?, c.len32()?];
        let _nodes = c.len32()?;
        self.objects2 = c.len32()?;
        let _arrays = c.len32()?;
        let _elements = c.len32()?;
        Ok(())
    }
}

/// Buffer 1, and the blobs when zstd before version 5 compressed them in the same run.
fn inflate_buffer1(h: &Header, c: &mut Cursor) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    if h.version < 5 && h.compression == Compression::Zstd && h.blobs > 0 {
        let (len, compressed) = h.buffer1;
        let mut out = zstd(c.take(compressed)?, len.saturating_add(h.blobs_len))?;
        let blobs = out.split_off(len);
        return Ok((out, Some(blobs)));
    }
    Ok((inflate(h, c, h.buffer1)?, None))
}

/// One buffer of (uncompressed length, compressed length).
fn inflate(h: &Header, c: &mut Cursor, (len, compressed): (usize, usize)) -> Result<Vec<u8>> {
    match h.compression {
        Compression::None => Ok(c.take(len)?.to_vec()),
        Compression::Lz4 => lz4(c.take(compressed)?, len),
        Compression::Zstd => zstd(c.take(compressed)?, len),
    }
}

/// The blobs, which follow the buffers in the block.
fn inflate_blobs(h: &Header, c: &mut Cursor, layout: &mut Layout) -> Result<Vec<u8>> {
    match h.compression {
        Compression::None => Ok(c.take(h.blobs_len)?.to_vec()),
        Compression::Zstd => {
            let compressed = h
                .compressed_total
                .checked_sub(h.buffer1.1 + h.buffer2.1)
                .ok_or_else(|| malformed("KV3", "blob sizes exceed the total"))?;
            zstd(c.take(compressed)?, h.blobs_len)
        }
        Compression::Lz4 => {
            if h.blobs_len / LZ4_RATIO > c.remaining() {
                return Err(malformed(
                    "KV3 LZ4 blob",
                    "more blob bytes than the frames hold",
                ));
            }
            let mut out = buffer(h.blobs_len)?;
            lz4_chained(c, layout, &mut out)?;
            Ok(out)
        }
    }
}

/// Blobs cut into LZ4 frames of at most 16 KiB, a frame never spanning two blobs; each frame
/// may reference the 64 KiB decoded before it, across blobs.
fn lz4_chained(c: &mut Cursor, layout: &mut Layout, out: &mut [u8]) -> Result<()> {
    let mut lengths = layout.blob_lengths.clone();
    let mut at = 0usize;
    while lengths.remaining() > 0 {
        let end = at
            .checked_add(lengths.len32()?)
            .filter(|&end| end <= out.len())
            .ok_or_else(|| malformed("KV3 blobs", "blob lengths exceed the blob bytes"))?;
        while at < end {
            let frame = c.take(layout.frame_sizes.u16()? as usize)?;
            let (done, rest) = out.split_at_mut(at);
            let dict = &done[done.len().saturating_sub(64 << 10)..];
            let n = lz4_flex::block::decompress_into_with_dict(frame, &mut rest[..end - at], dict)
                .map_err(|e| malformed("KV3 LZ4 blob", e.to_string()))?;
            if n == 0 {
                return Err(malformed("KV3 LZ4 blob", "an empty frame"));
            }
            at += n;
        }
    }
    Ok(())
}

/// Lanes of 1-, 2-, 4- and 8-byte values.
#[derive(Clone)]
struct Lanes<'a> {
    b1: Cursor<'a>,
    b2: Cursor<'a>,
    b4: Cursor<'a>,
    b8: Cursor<'a>,
}

impl<'a> Lanes<'a> {
    /// Splits the lanes off `c`, each aligned to its width when present; before version 5 the
    /// end is aligned to 8 even when the 8-byte lane is empty.
    fn split(c: &mut Cursor<'a>, [n1, n2, n4, n8]: Counts, version: u32) -> Result<Self> {
        let mut lane = |n: usize, width: usize| -> Result<Cursor<'a>> {
            if n > 0 {
                c.align(width)?;
            }
            let len = n
                .checked_mul(width)
                .ok_or_else(|| malformed("KV3", "lane size overflows"))?;
            Ok(Cursor::new(c.take(len)?, "KV3 values"))
        };
        let lanes = Self {
            b1: lane(n1, 1)?,
            b2: lane(n2, 2)?,
            b4: lane(n4, 4)?,
            b8: lane(n8, 8)?,
        };
        if n8 == 0 && version < 5 {
            c.align(8)?;
        }
        Ok(lanes)
    }
}

/// The decompressed buffers cut into their streams.
struct Layout<'a> {
    strings: Vec<String>,
    types: Cursor<'a>,
    main: Lanes<'a>,
    /// Version 5's buffer 1 lanes, read by auxiliary-buffer typed arrays; before version 5
    /// those read the main lanes.
    aux: Option<Lanes<'a>>,
    object_lengths: Cursor<'a>,
    blob_lengths: Cursor<'a>,
    frame_sizes: Cursor<'a>,
}

fn strings_from(c: &mut Cursor, count: usize) -> Result<Vec<String>> {
    let mut strings = Vec::with_capacity(count.min(c.remaining()));
    for _ in 0..count {
        strings.push(String::from_utf8_lossy(c.cstr()?).into_owned());
    }
    Ok(strings)
}

impl<'a> Layout<'a> {
    fn new(h: &Header, buffer1: &'a [u8], buffer2: &'a [u8]) -> Result<Self> {
        if h.version >= 5 {
            return Self::new_v5(h, buffer1, buffer2);
        }
        let mut c = Cursor::new(buffer1, "KV3 buffer");
        let mut main = Lanes::split(&mut c, h.counts, h.version)?;
        let count = main.b4.len32()?;
        let strings_start = c.pos();
        let strings = strings_from(&mut c, count)?;
        let types_len = if h.version == 1 {
            h.total_len.checked_sub(c.pos() + 4)
        } else {
            (h.types_len + strings_start).checked_sub(c.pos())
        }
        .ok_or_else(|| malformed("KV3", "type stream size"))?;
        let types = Cursor::new(c.take(types_len)?, "KV3 types");
        let mut layout = Self {
            strings,
            types,
            main,
            aux: None,
            object_lengths: Cursor::new(&[], "KV3 object lengths"),
            blob_lengths: Cursor::new(&[], "KV3 blob lengths"),
            frame_sizes: Cursor::new(&[], "KV3 frame sizes"),
        };
        layout.tail(h, c)?;
        Ok(layout)
    }

    fn new_v5(h: &Header, buffer1: &'a [u8], buffer2: &'a [u8]) -> Result<Self> {
        let mut c1 = Cursor::new(buffer1, "KV3 buffer 1");
        let mut aux = Lanes::split(&mut c1, h.counts, h.version)?;
        let count = aux.b4.len32()?;
        let strings = strings_from(&mut aux.b1, count)?;
        let mut c = Cursor::new(buffer2, "KV3 buffer 2");
        let object_lengths =
            Cursor::new(c.take(h.objects2.saturating_mul(4))?, "KV3 object lengths");
        let main = Lanes::split(&mut c, h.counts2, h.version)?;
        let types = Cursor::new(c.take(h.types_len)?, "KV3 types");
        let mut layout = Self {
            strings,
            types,
            main,
            aux: Some(aux),
            object_lengths,
            blob_lengths: Cursor::new(&[], "KV3 blob lengths"),
            frame_sizes: Cursor::new(&[], "KV3 frame sizes"),
        };
        layout.tail(h, c)?;
        Ok(layout)
    }

    /// After the type stream: the trailer, or the blob lengths, the trailer and LZ4 frame sizes.
    fn tail(&mut self, h: &Header, mut c: Cursor<'a>) -> Result<()> {
        if h.blobs > 0 {
            let lengths = c.take(h.blobs.saturating_mul(4))?;
            self.blob_lengths = Cursor::new(lengths, "KV3 blob lengths");
        }
        if c.u32()? != TRAILER {
            return Err(malformed("KV3", "no trailer after the type stream"));
        }
        self.frame_sizes = Cursor::new(c.rest(), "KV3 frame sizes");
        Ok(())
    }
}

struct Decoder<'a> {
    version: u32,
    layout: Layout<'a>,
    blobs: Cursor<'a>,
    budget: Budget,
}

impl<'a> Decoder<'a> {
    /// A type byte, skipping the flag byte (and a reserved one) that may follow it.
    fn read_type(&mut self) -> Result<u8> {
        let types = &mut self.layout.types;
        let byte = types.u8()?;
        if self.version >= 3 {
            if byte & 0x80 != 0 && !(1..=7).contains(&types.u8()?) {
                return Err(malformed("KV3", "unknown flag"));
            }
            if byte & 0x40 != 0 {
                types.u8()?;
            }
            return Ok(byte & 0x3F);
        }
        if byte & 0x80 != 0 && types.u8()? & 0x80 != 0 {
            return Err(malformed("KV3", "unknown flag"));
        }
        Ok(byte & 0x7F)
    }

    fn lanes(&mut self, aux: bool) -> &mut Lanes<'a> {
        match &mut self.layout.aux {
            Some(lanes) if aux => lanes,
            _ => &mut self.layout.main,
        }
    }

    /// A value of type `ty`; its primitive bytes come from the auxiliary lanes when `aux`.
    fn value(&mut self, ty: u8, aux: bool, depth: usize) -> Result<Value> {
        self.budget.node(depth)?;
        Ok(match ty {
            node::NULL => Value::Null,
            node::BOOLEAN_TRUE => Value::Bool(true),
            node::BOOLEAN_FALSE => Value::Bool(false),
            node::INT64_ZERO => Value::Int(0),
            node::INT64_ONE => Value::Int(1),
            node::DOUBLE_ZERO => Value::Float(0.0),
            node::DOUBLE_ONE => Value::Float(1.0),
            node::UINT32_ZERO => Value::UInt(0),
            node::UINT32_ONE => Value::UInt(1),
            node::BOOLEAN => Value::Bool(self.lanes(aux).b1.u8()? != 0),
            node::INT8 => Value::Int(i64::from(self.lanes(aux).b1.u8()? as i8)),
            node::UINT8 => Value::UInt(u64::from(self.lanes(aux).b1.u8()?)),
            node::INT16 => Value::Int(i64::from(self.lanes(aux).b2.u16()? as i16)),
            node::UINT16 => Value::UInt(u64::from(self.lanes(aux).b2.u16()?)),
            node::INT32 => Value::Int(i64::from(self.lanes(aux).b4.i32()?)),
            node::UINT32 => Value::UInt(u64::from(self.lanes(aux).b4.u32()?)),
            node::FLOAT => Value::Float(f64::from(f32::from_bits(self.lanes(aux).b4.u32()?))),
            node::INT64 => Value::Int(self.lanes(aux).b8.u64()? as i64),
            node::UINT64 => Value::UInt(self.lanes(aux).b8.u64()?),
            node::DOUBLE => Value::Float(f64::from_bits(self.lanes(aux).b8.u64()?)),
            node::STRING => {
                let id = self.layout.main.b4.i32()?;
                Value::String(string(&self.layout.strings, id, &mut self.budget)?)
            }
            node::BINARY_BLOB => Value::Blob(self.blob()?),
            node::ARRAY => self.array(depth)?,
            node::ARRAY_TYPED
            | node::ARRAY_TYPE_BYTE_LENGTH
            | node::ARRAY_TYPE_AUXILIARY_BUFFER => self.typed_array(ty, depth)?,
            node::OBJECT => self.object(depth)?,
            _ => return Err(malformed("KV3", format!("unknown node type {ty}"))),
        })
    }

    fn blob(&mut self) -> Result<Vec<u8>> {
        let bytes = if self.version < 2 {
            let len = self.layout.main.b4.len32()?;
            self.layout.main.b1.take(len)?
        } else {
            let len = self.layout.blob_lengths.len32()?;
            self.blobs.take(len)?
        };
        self.budget.bytes(bytes.len())?;
        Ok(bytes.to_vec())
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        let len = self.layout.main.b4.len32()?;
        let mut items = Vec::with_capacity(len.min(4096));
        for _ in 0..len {
            let ty = self.read_type()?;
            items.push(self.value(ty, false, depth + 1)?);
        }
        Ok(Value::Array(items))
    }

    fn typed_array(&mut self, ty: u8, depth: usize) -> Result<Value> {
        let len = if ty == node::ARRAY_TYPED {
            self.layout.main.b4.len32()?
        } else {
            usize::from(self.layout.main.b1.u8()?)
        };
        if len == 0 && self.version >= 2 {
            return Err(malformed("KV3", "an empty typed array"));
        }
        let element = self.read_type()?;
        let aux = ty == node::ARRAY_TYPE_AUXILIARY_BUFFER;
        let mut items = Vec::with_capacity(len.min(4096));
        for _ in 0..len {
            items.push(self.value(element, aux, depth + 1)?);
        }
        Ok(Value::Array(items))
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        let len = if self.version >= 5 {
            self.layout.object_lengths.len32()?
        } else {
            self.layout.main.b4.len32()?
        };
        let mut object = Object::with_capacity(len.min(4096));
        for _ in 0..len {
            let ty = self.read_type()?;
            let id = self.layout.main.b4.i32()?;
            let key = string(&self.layout.strings, id, &mut self.budget)?;
            let value = self.value(ty, false, depth + 1)?;
            object.push(key, value);
        }
        Ok(Value::Object(object))
    }
}
