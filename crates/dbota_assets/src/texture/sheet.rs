use crate::bytes::Cursor;
use crate::error::{Result, malformed};

const WHAT: &str = "vtex sprite sheet";

/// The first image of the first frame of the first sequence of a sprite sheet, as texture
/// coordinates `[u0, v0, u1, v1]` (0..1, v down the image like the rows): the frame cropped to
/// its content. `None` when the sheet has no frame.
///
/// The sheet is a list of sequences (32 bytes each: id, flags, offset to the frames, frame count,
/// frame rate, offset to the name) and, per frame, a display time and a list of images of two
/// rectangles each: the frame cropped to its content, then the whole cell it was cut from.
pub(super) fn first_rect(data: &[u8]) -> Result<Option<[f32; 4]>> {
    let mut head = Cursor::new(data, WHAT);
    let _version = head.u32()?;
    if head.u32()? == 0 {
        return Ok(None);
    }
    let mut seq = at(data, 8 + 8)?;
    let frames = seq.u32()?;
    if seq.u32()? == 0 {
        return Ok(None);
    }
    let frame_pos = offset(16, frames)?;
    let mut frame = at(data, frame_pos)?;
    let _display_time = frame.u32()?;
    let images = frame.u32()?;
    if frame.u32()? == 0 {
        return Ok(None);
    }
    let mut image = at(data, offset(frame_pos + 4, images)?)?;
    let mut rect = [0.0; 4];
    for value in &mut rect {
        *value = f32::from_bits(image.u32()?);
        if !value.is_finite() {
            return Err(malformed(WHAT, "a texture coordinate is not a number"));
        }
    }
    Ok(Some(rect))
}

/// A reader of `data` from `pos`.
fn at(data: &[u8], pos: usize) -> Result<Cursor<'_>> {
    data.get(pos..)
        .map(|rest| Cursor::new(rest, WHAT))
        .ok_or_else(|| malformed(WHAT, format!("offset {pos} runs past the end")))
}

/// The position `delta` bytes after the field at `field`.
fn offset(field: usize, delta: u32) -> Result<usize> {
    field
        .checked_add(delta as usize)
        .ok_or_else(|| malformed(WHAT, "offset overflows"))
}
