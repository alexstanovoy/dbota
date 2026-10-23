use crate::error::{Result, malformed};

/// A bounds-checked little-endian reader over a byte slice; `what` names the format in errors.
#[derive(Clone, Debug)]
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8], what: &'static str) -> Self {
        Self { data, pos: 0, what }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| {
                malformed(
                    self.what,
                    format!(
                        "{len} bytes at {} run past the end ({})",
                        self.pos,
                        self.data.len()
                    ),
                )
            })?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn i32(&mut self) -> Result<i32> {
        self.array().map(i32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        self.array().map(u64::from_le_bytes)
    }

    /// A `u32` offset counted from its own field: the position it points at (which a 32-bit
    /// target may not reach).
    pub(crate) fn offset32(&mut self) -> Result<usize> {
        let field = self.pos;
        let delta = self.u32()?;
        field
            .checked_add(delta as usize)
            .ok_or_else(|| malformed(self.what, format!("offset {delta} at {field} overflows")))
    }

    /// A count or size field that must not be negative.
    pub(crate) fn len32(&mut self) -> Result<usize> {
        let value = self.i32()?;
        usize::try_from(value).map_err(|_| malformed(self.what, format!("negative size {value}")))
    }

    /// The bytes up to the next NUL; the NUL is consumed, not returned.
    pub(crate) fn cstr(&mut self) -> Result<&'a [u8]> {
        let rest = self.rest();
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| malformed(self.what, format!("unterminated string at {}", self.pos)))?;
        self.pos += len + 1;
        Ok(&rest[..len])
    }

    pub(crate) fn align(&mut self, to: usize) -> Result<()> {
        let pad = self.pos.next_multiple_of(to) - self.pos;
        self.take(pad).map(drop)
    }
}
