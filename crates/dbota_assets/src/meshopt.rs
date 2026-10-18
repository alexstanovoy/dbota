//! meshoptimizer's vertex and index codecs, decode only. Valve stores the vertex and index
//! buffers of its models compressed with them. The input is an untrusted local file: every read
//! is checked, a malformed buffer is an [`Error`](crate::Error), and the output is bounded.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the model reader that calls the decoders comes next"
    )
)]

use crate::error::{Result, malformed};

#[cfg(test)]
mod tests;

/// Largest output one call may allocate.
const MAX_OUTPUT: usize = 256 << 20;

const VERTEX_HEADER: u8 = 0xa0;
const INDEX_HEADER: u8 = 0xe0;
const SEQUENCE_HEADER: u8 = 0xd0;
/// Newest codec version of each stream that the decoders understand.
const MAX_VERSION: u8 = 1;

const VERTEX_BLOCK_BYTES: usize = 8192;
const VERTEX_BLOCK_MAX: usize = 256;
const GROUP: usize = 16;
/// Bytes one group may read: a 16-byte literal group, or 8 header bytes plus 16 exceptions.
const GROUP_READ_LIMIT: usize = 24;
const BITS_V0: [u8; 4] = [0, 2, 4, 8];
const BITS_V1: [u8; 5] = [0, 1, 2, 4, 8];

fn bad(detail: impl Into<String>) -> crate::Error {
    malformed("meshopt", detail)
}

/// Allocates `count * size` zeroed bytes, refusing more than [`MAX_OUTPUT`].
fn output(count: usize, size: usize) -> Result<Vec<u8>> {
    match count.checked_mul(size) {
        Some(n) if n <= MAX_OUTPUT => Ok(vec![0; n]),
        _ => Err(bad(format!(
            "{count} elements of {size} bytes exceed {MAX_OUTPUT} bytes"
        ))),
    }
}

/// Splits `n` bytes off the front of `data`.
fn take<'a>(data: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if data.len() < n {
        return Err(bad("data ends early"));
    }
    let (head, rest) = data.split_at(n);
    *data = rest;
    Ok(head)
}

/// `count` vertices of `stride` bytes each, decoded from meshopt's vertex codec.
pub(crate) fn decode_vertex_buffer(data: &[u8], count: usize, stride: usize) -> Result<Vec<u8>> {
    if stride == 0 || stride > 256 || !stride.is_multiple_of(4) {
        return Err(bad(format!(
            "vertex stride {stride} is not a multiple of 4 up to 256"
        )));
    }
    let mut out = output(count, stride)?;
    let (&header, mut data) = data
        .split_first()
        .ok_or_else(|| bad("empty vertex buffer"))?;
    let version = header & 0x0f;
    if header & 0xf0 != VERTEX_HEADER || version > MAX_VERSION {
        return Err(bad(format!("vertex buffer header {header:#04x}")));
    }
    let tail_size = stride + if version == 0 { 0 } else { stride / 4 };
    let tail_pad = tail_size.max(if version == 0 { 32 } else { 24 });
    if data.len() < tail_pad {
        return Err(bad("vertex buffer is too short for its tail"));
    }
    let tail = &data[data.len() - tail_size..];
    let mut last = tail[..stride].to_vec();
    let channels = &tail[stride..];
    let block_len = ((VERTEX_BLOCK_BYTES / stride) & !(GROUP - 1)).min(VERTEX_BLOCK_MAX);
    for block in out.chunks_mut(block_len * stride) {
        let n = block.len() / stride;
        decode_block(&mut data, block, n, &mut last, channels, version)?;
    }
    if data.len() != tail_pad {
        return Err(bad("vertex buffer has bytes between its blocks and tail"));
    }
    Ok(out)
}

/// Decodes one block of `count` (1..=256) vertices into `out`, and leaves its last vertex in
/// `last` as the base of the next block.
fn decode_block(
    data: &mut &[u8],
    out: &mut [u8],
    count: usize,
    last: &mut [u8],
    channels: &[u8],
    version: u8,
) -> Result<()> {
    let stride = last.len();
    let aligned = count.next_multiple_of(GROUP);
    let control = take(data, if version == 0 { 0 } else { stride / 4 })?;
    let mut lanes = [0u8; VERTEX_BLOCK_MAX * 4];
    for k in (0..stride).step_by(4) {
        let ctrl_byte = control.get(k / 4).copied().unwrap_or(0);
        for j in 0..4 {
            let lane = &mut lanes[j * count..];
            match (ctrl_byte >> (j * 2)) & 3 {
                3 => lane[..count].copy_from_slice(take(data, count)?),
                2 => lane[..count].fill(0),
                ctrl => {
                    let bits = if version == 0 {
                        &BITS_V0[..]
                    } else {
                        &BITS_V1[usize::from(ctrl)..]
                    };
                    decode_bytes(data, &mut lane[..aligned], bits)?;
                }
            }
        }
        let channel = channels.get(k / 4).copied().unwrap_or(0);
        let width = match channel & 3 {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => return Err(bad("invalid vertex channel type")),
        };
        let rot = (32 - u32::from(channel >> 4)) & 31;
        let base = &last[k..k + 4];
        undelta(&lanes, &mut out[k..], count, stride, base, (width, rot));
    }
    last.copy_from_slice(&out[(count - 1) * stride..count * stride]);
    Ok(())
}

/// Decodes the byte groups of one lane (`buf`, a multiple of 16 bytes); each group's header
/// picks one of `bits` as its width.
fn decode_bytes(data: &mut &[u8], buf: &mut [u8], bits: &[u8]) -> Result<()> {
    let groups = buf.len() / GROUP;
    let header = take(data, groups.div_ceil(4))?;
    for (i, out) in buf.as_chunks_mut::<GROUP>().0.iter_mut().enumerate() {
        if data.len() < GROUP_READ_LIMIT {
            return Err(bad("vertex data ends inside a byte group"));
        }
        let bitsk = usize::from((header[i / 4] >> ((i % 4) * 2)) & 3);
        let width = bits
            .get(bitsk)
            .copied()
            .ok_or_else(|| bad("bad group width"))?;
        let used = decode_group(data, out, u32::from(width));
        *data = &data[used..];
    }
    Ok(())
}

/// Decodes one group of 16 values of `bits` bits (0, 1, 2, 4 or 8) from `data` (at least
/// [`GROUP_READ_LIMIT`] bytes) and returns the bytes it used.
fn decode_group(data: &[u8], out: &mut [u8], bits: u32) -> usize {
    match bits {
        0 => {
            out.fill(0);
            return 0;
        }
        8 => {
            out.copy_from_slice(&data[..GROUP]);
            return GROUP;
        }
        _ => {}
    }
    // Values below the all-ones escape sit in the header; escapes take a byte after it.
    let header_len = GROUP * bits as usize / 8;
    let per_byte = 8 / bits as usize;
    let escape = (1u8 << bits) - 1;
    let mut next = header_len;
    for (i, value) in out.iter_mut().enumerate() {
        let mut byte = data[i / per_byte];
        if bits == 1 {
            // the 1-bit groups are stored with their bits in reverse order
            byte = byte.reverse_bits();
        }
        let enc = (byte << ((i % per_byte) as u32 * bits)) >> (8 - bits);
        *value = if enc == escape {
            next += 1;
            data[next - 1]
        } else {
            enc
        };
    }
    next
}

fn unzigzag(v: u32) -> u32 {
    (v & 1).wrapping_neg() ^ (v >> 1)
}

/// Undoes the per-lane delta coding: `lanes` holds the byte planes of the block, and the
/// vertices go to every `stride` bytes of `out` from its first byte, 4 bytes per call as
/// `width`-byte values (`width`, `rot`).
fn undelta(
    lanes: &[u8],
    out: &mut [u8],
    count: usize,
    stride: usize,
    base: &[u8],
    (width, rot): (usize, u32),
) {
    let mask = u32::MAX >> (32 - 8 * width);
    for start in (0..4).step_by(width) {
        let mut p = base[start..start + width]
            .iter()
            .rev()
            .fold(0u32, |acc, &b| acc << 8 | u32::from(b));
        for i in 0..count {
            let v = (0..width).rev().fold(0u32, |acc, j| {
                acc << 8 | u32::from(lanes[(start + j) * count + i])
            });
            p = if width == 4 {
                v.rotate_left(rot) ^ p
            } else {
                unzigzag(v).wrapping_add(p) & mask
            };
            for j in 0..width {
                out[i * stride + start + j] = (p >> (8 * j)) as u8;
            }
        }
    }
}

/// `count` indices of `index_size` (2 or 4) bytes each, decoded from meshopt's index codec.
pub(crate) fn decode_index_buffer(data: &[u8], count: usize, index_size: usize) -> Result<Vec<u8>> {
    if index_size != 2 && index_size != 4 {
        return Err(bad(format!("index size {index_size} is not 2 or 4")));
    }
    let header = *data.first().ok_or_else(|| bad("empty index buffer"))?;
    if header & 0x0f > MAX_VERSION {
        return Err(bad(format!("index buffer header {header:#04x}")));
    }
    match header & 0xf0 {
        INDEX_HEADER => decode_triangles(data, count, index_size),
        SEQUENCE_HEADER => decode_sequence(data, count, index_size),
        _ => Err(bad(format!("index buffer header {header:#04x}"))),
    }
}

/// Reads the variable-length integers and codeaux bytes of the index data stream.
struct Stream<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Stream<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| bad("index data ends early"))?;
        self.pos += 1;
        Ok(b)
    }

    /// A 7-bits-per-byte integer of at most 5 bytes.
    fn vbyte(&mut self) -> Result<u32> {
        let mut result = 0u32;
        for i in 0..5 {
            let b = self.byte()?;
            result |= u32::from(b & 127) << (7 * i);
            if b < 128 {
                break;
            }
        }
        Ok(result)
    }

    /// A zigzag delta from `last`.
    fn index(&mut self, last: u32) -> Result<u32> {
        let v = self.vbyte()?;
        Ok(last.wrapping_add((v >> 1) ^ (v & 1).wrapping_neg()))
    }
}

fn put_index(out: &mut [u8], v: u32) {
    if out.len() == 2 {
        out.copy_from_slice(&(v as u16).to_le_bytes());
    } else {
        out.copy_from_slice(&v.to_le_bytes());
    }
}

/// The two FIFOs and counters a triangle list is rebuilt from.
struct Triangles {
    edges: [(u32, u32); 16],
    vertices: [u32; 16],
    edge_at: usize,
    vertex_at: usize,
    next: u32,
    last: u32,
}

impl Triangles {
    fn push_edge(&mut self, a: u32, b: u32) {
        self.edges[self.edge_at] = (a, b);
        self.edge_at = (self.edge_at + 1) & 15;
    }

    fn push_vertex(&mut self, v: u32, advance: bool) {
        self.vertices[self.vertex_at] = v;
        self.vertex_at = (self.vertex_at + usize::from(advance)) & 15;
    }

    fn recent(&self, back: usize) -> u32 {
        self.vertices[self.vertex_at.wrapping_sub(back) & 15]
    }

    /// A triangle that continues an edge of the FIFO with one more vertex.
    fn edge_triangle(&mut self, code: u8, data: &mut Stream, fec_max: u8) -> Result<[u32; 3]> {
        let (a, b) = self.edges[self.edge_at.wrapping_sub(1 + usize::from(code >> 4)) & 15];
        let fec = code & 15;
        let c = if fec < fec_max {
            let c = if fec == 0 {
                self.next
            } else {
                self.recent(1 + usize::from(fec))
            };
            self.next += u32::from(fec == 0);
            self.push_vertex(c, fec == 0);
            c
        } else {
            // 13 and 14 are the deltas -1 and +1 from the last free index
            let c = if fec == 15 {
                data.index(self.last)?
            } else {
                self.last.wrapping_add_signed(i32::from(fec) * 2 - 27)
            };
            self.last = c;
            self.push_vertex(c, true);
            c
        };
        self.push_edge(c, b);
        self.push_edge(a, c);
        Ok([a, b, c])
    }

    /// A triangle whose second and third vertex come from the codeaux table.
    fn table_triangle(&mut self, aux: u8) -> [u32; 3] {
        let (feb, fec) = (usize::from(aux >> 4), usize::from(aux & 15));
        let a = self.next;
        self.next += 1;
        let b = if feb == 0 {
            self.next
        } else {
            self.recent(feb)
        };
        self.next += u32::from(feb == 0);
        let c = if fec == 0 {
            self.next
        } else {
            self.recent(fec)
        };
        self.next += u32::from(fec == 0);
        self.push_vertex(a, true);
        self.push_vertex(b, feb == 0);
        self.push_vertex(c, fec == 0);
        self.finish_edges(a, b, c);
        [a, b, c]
    }

    /// A triangle with an explicit codeaux byte (and, with `code` 0xff, a free first vertex).
    fn free_triangle(&mut self, code: u8, data: &mut Stream) -> Result<[u32; 3]> {
        let aux = data.byte()?;
        let (fea, feb, fec) = (if code == 0xfe { 0 } else { 15 }, aux >> 4, aux & 15);
        if aux == 0 {
            self.next = 0;
        }
        let vertex = |s: &mut Self, f: u8, free: bool| {
            if f == 0 {
                s.next += 1;
                s.next - 1
            } else if free {
                0
            } else {
                s.recent(usize::from(f))
            }
        };
        let mut a = vertex(self, fea, fea == 15);
        let mut b = vertex(self, feb, feb == 15);
        let mut c = vertex(self, fec, fec == 15);
        for (f, v) in [(fea, &mut a), (feb, &mut b), (fec, &mut c)] {
            if f == 15 {
                *v = data.index(self.last)?;
                self.last = *v;
            }
        }
        self.push_vertex(a, true);
        self.push_vertex(b, feb == 0 || feb == 15);
        self.push_vertex(c, fec == 0 || fec == 15);
        self.finish_edges(a, b, c);
        Ok([a, b, c])
    }

    fn finish_edges(&mut self, a: u32, b: u32, c: u32) {
        self.push_edge(b, a);
        self.push_edge(c, b);
        self.push_edge(a, c);
    }
}

/// An index buffer of triangles: one code byte per triangle, then the data stream, then the
/// 16-byte codeaux table.
fn decode_triangles(buffer: &[u8], count: usize, index_size: usize) -> Result<Vec<u8>> {
    if !count.is_multiple_of(3) {
        return Err(bad(format!("{count} indices are not whole triangles")));
    }
    let data_at = 1 + count / 3;
    if buffer.len() < data_at + 16 {
        return Err(bad("index buffer is too short"));
    }
    let mut out = output(count, index_size)?;
    let (codes, data) = buffer[1..].split_at(data_at - 1);
    let safe_end = data.len() - 16;
    let table = &data[safe_end..];
    let mut state = Triangles {
        edges: [(u32::MAX, u32::MAX); 16],
        vertices: [u32::MAX; 16],
        edge_at: 0,
        vertex_at: 0,
        next: 0,
        last: 0,
    };
    let mut stream = Stream { data, pos: 0 };
    let fec_max = if buffer[0] & 0x0f >= 1 { 13 } else { 15 };
    for (&code, tri) in codes.iter().zip(out.chunks_exact_mut(3 * index_size)) {
        let corners = if code < 0xf0 {
            if stream.pos > safe_end {
                return Err(bad("index data is truncated"));
            }
            state.edge_triangle(code, &mut stream, fec_max)?
        } else if code < 0xfe {
            state.table_triangle(table[usize::from(code & 15)])
        } else {
            if stream.pos > safe_end {
                return Err(bad("index data is truncated"));
            }
            state.free_triangle(code, &mut stream)?
        };
        for (v, dst) in corners.into_iter().zip(tri.chunks_exact_mut(index_size)) {
            put_index(dst, v);
        }
    }
    if stream.pos != safe_end {
        return Err(bad("index data is malformed"));
    }
    Ok(out)
}

/// An index sequence: one delta per index against one of two running baselines, then 4 bytes.
fn decode_sequence(buffer: &[u8], count: usize, index_size: usize) -> Result<Vec<u8>> {
    if buffer.len() < count.saturating_add(5) {
        return Err(bad("index sequence is too short"));
    }
    let mut out = output(count, index_size)?;
    let data = &buffer[1..];
    let safe_end = data.len() - 4;
    let mut stream = Stream { data, pos: 0 };
    let mut last = [0u32; 2];
    for dst in out.chunks_exact_mut(index_size) {
        if stream.pos >= safe_end {
            return Err(bad("index sequence is truncated"));
        }
        let v = stream.vbyte()?;
        let base = &mut last[(v & 1) as usize];
        let delta = (v >> 2) ^ ((v >> 1) & 1).wrapping_neg();
        *base = base.wrapping_add(delta);
        put_index(dst, *base);
    }
    if stream.pos != safe_end {
        return Err(bad("index sequence is malformed"));
    }
    Ok(out)
}
