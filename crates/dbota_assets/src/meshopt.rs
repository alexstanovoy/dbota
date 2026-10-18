//! meshoptimizer's vertex and index codecs, decode only. Valve stores the vertex and index
//! buffers of its models compressed with them. The input is an untrusted local file: every read
//! is checked, a malformed buffer is an [`Error`](crate::Error), and the output is bounded.

use meshoptimizer_rs::codec;

use crate::error::{Result, malformed};

#[cfg(test)]
mod tests;

/// Largest output one call may allocate.
const MAX_OUTPUT: usize = 256 << 20;

const INDEX_HEADER: u8 = 0xe0;
const SEQUENCE_HEADER: u8 = 0xd0;

fn bad(detail: impl Into<String>) -> crate::Error {
    malformed("meshopt", detail)
}

fn check(count: usize, size: usize) -> Result<()> {
    match count.checked_mul(size) {
        Some(n) if n <= MAX_OUTPUT => Ok(()),
        _ => Err(bad(format!(
            "{count} elements of {size} bytes exceed {MAX_OUTPUT} bytes"
        ))),
    }
}

/// `count` vertices of `stride` bytes each, decoded from meshopt's vertex codec.
pub(crate) fn decode_vertex_buffer(data: &[u8], count: usize, stride: usize) -> Result<Vec<u8>> {
    if stride == 0 || stride > 256 || !stride.is_multiple_of(4) {
        return Err(bad(format!(
            "vertex stride {stride} is not a multiple of 4 up to 256"
        )));
    }
    check(count, stride)?;
    codec::decode_vertex_buffer(
        count,
        stride,
        data,
        &mut meshoptimizer_rs::Workspace::default(),
    )
    .map_err(|e| bad(e.to_string()))
}

/// `count` indices of `index_size` (2 or 4) bytes each, decoded from meshopt's index codec.
pub(crate) fn decode_index_buffer(data: &[u8], count: usize, index_size: usize) -> Result<Vec<u8>> {
    if index_size != 2 && index_size != 4 {
        return Err(bad(format!("index size {index_size} is not 2 or 4")));
    }
    check(count, index_size)?;
    let header = *data.first().ok_or_else(|| bad("empty index buffer"))?;
    let mut ws = meshoptimizer_rs::Workspace::default();
    match header & 0xf0 {
        INDEX_HEADER => codec::decode_index_buffer(count, index_size, data, &mut ws),
        SEQUENCE_HEADER => codec::decode_index_sequence(count, index_size, data, &mut ws),
        _ => return Err(bad(format!("index buffer header {header:#04x}"))),
    }
    .map_err(|e| bad(e.to_string()))
}
