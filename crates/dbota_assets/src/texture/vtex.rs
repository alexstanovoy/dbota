use std::io::Cursor as IoCursor;

use super::convert::Content;
use super::format::{PixelFormat, Stored};
use super::ktx2::{self, Encoding};
use super::sheet;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::resource::Resource;

const WHAT: &str = "vtex";
/// Largest side of a texture; the game's biggest is 4096.
const MAX_SIDE: u32 = 16384;
/// Most memory the decoded levels of one texture may take.
const MAX_BYTES: usize = 256 << 20;
/// Most extra data entries a header may list (the game's textures have up to three).
const MAX_EXTRAS: u32 = 16;
/// Flags of a texture that is not a plain 2D one: a cube map, a volume, an array.
const NOT_2D: u16 = 0x10 | 0x20 | 0x40;
/// LZ4 expands a byte at most 255 times, so a level cannot be larger than this per stored byte.
const LZ4_RATIO: usize = 255;

const EXTRA_SHEET: u32 = 2;
const EXTRA_COMPRESSED_MIPS: u32 = 4;

/// A 2D texture of a `vtex_c`: its mip levels in the pixel format they are stored in (block
/// formats stay compressed).
#[derive(Clone, Debug)]
pub struct Texture {
    width: u32,
    height: u32,
    format: PixelFormat,
    /// Largest level first; level `i` is `max(1, size >> i)` texels square.
    mips: Vec<Vec<u8>>,
    /// The first frame of the file's sprite sheet, `[u0, v0, u1, v1]` in 0..1, if it has one.
    rect: Option<[f32; 4]>,
}

impl Texture {
    /// Reads the texture of a `vtex_c` file: its header, the levels after the resource's own
    /// blocks (the smallest first in the file), LZ4-compressed levels expanded. Cube maps,
    /// volumes and arrays are errors, so are JPEG-coded textures.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let resource = Resource::parse(bytes)?;
        let data = resource
            .block(b"DATA")
            .ok_or_else(|| malformed(WHAT, "no DATA block"))?;
        let header = Header::parse(data)?;
        let (stored, width, height) = (header.stored, header.width, header.height);
        let sizes = header.level_sizes()?;
        let raw = resource.trailing();
        let mips = if matches!(stored, Stored::Png) {
            vec![decode_png(raw, width, height)?]
        } else {
            header.read_levels(raw, &sizes)?
        };
        let mut texture = Self {
            width,
            height,
            format: stored.decoded(),
            mips,
            rect: header.rect,
        };
        match stored {
            Stored::Bgra8 => {
                for px in texture
                    .mips
                    .iter_mut()
                    .flat_map(|mip| mip.as_chunks_mut::<4>().0.iter_mut())
                {
                    px.swap(0, 2);
                }
            }
            Stored::Plain(PixelFormat::Bc3) => texture.decode_content(Content::of(&resource))?,
            _ => {}
        }
        Ok(texture)
    }

    /// Decodes DXT5 levels that hold a normal or a YCoCg image to RGBA8.
    fn decode_content(&mut self, content: Content) -> Result<()> {
        if content == Content::Colour {
            return Ok(());
        }
        for (level, mip) in self.mips.iter_mut().enumerate() {
            let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
            *mip = content.decode_bc3(mip, w, h)?;
        }
        self.format = PixelFormat::Rgba8;
        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The mip levels, largest first.
    pub fn mips(&self) -> &[Vec<u8>] {
        &self.mips
    }

    /// Whether the file is a sprite sheet.
    pub fn is_sheet(&self) -> bool {
        self.rect.is_some()
    }

    /// Where the first frame sits in a sprite sheet: the texels `[x0, y0, x1, y1]` of the first
    /// sequence's first frame cropped to its content, widened to whole texels. Source2Viewer
    /// exports exactly these.
    pub fn first_frame_rect(&self) -> Option<[u32; 4]> {
        let rect = self.rect?;
        let low = |u: f32, side: u32| (u.clamp(0.0, 1.0) * side as f32).floor() as u32;
        let high = |u: f32, side: u32| (u.clamp(0.0, 1.0) * side as f32).ceil() as u32;
        Some([
            low(rect[0], self.width),
            low(rect[1], self.height),
            high(rect[2], self.width),
            high(rect[3], self.height),
        ])
    }

    /// The first frame of a sprite sheet ([`first_frame_rect`](Self::first_frame_rect)), cut out
    /// of every level it lines up with. Block formats stay compressed, so the cut starts at the
    /// block border before the frame (up to three texels more on the left and top); levels the
    /// frame does not line up with are dropped. A texture that is no sheet comes back whole.
    pub fn first_frame(&self) -> Self {
        let Some([fx0, fy0, x1, y1]) = self.first_frame_rect() else {
            return self.clone();
        };
        let (bw, bh, _) = self.format.block();
        let (x0, y0) = (fx0 / bw * bw, fy0 / bh * bh);
        if x1 <= x0 || y1 <= y0 {
            return Self {
                rect: None,
                ..self.clone()
            };
        }
        let mut mips = Vec::new();
        for (level, mip) in self.mips.iter().enumerate() {
            let mask = (1u32 << level) - 1;
            let aligned = [x0, y0, x1, y1].iter().all(|v| v & mask == 0);
            let (lx0, ly0) = (x0 >> level, y0 >> level);
            let (lx1, ly1) = (x1 >> level, y1 >> level);
            if !aligned || lx0 % bw != 0 || ly0 % bh != 0 || lx1 == lx0 || ly1 == ly0 {
                break;
            }
            let side = (self.width >> level).max(1);
            mips.push(self.cut(mip, side, [lx0, ly0, lx1, ly1]));
        }
        Self {
            width: x1 - x0,
            height: y1 - y0,
            format: self.format,
            mips,
            rect: None,
        }
    }

    /// The blocks of `rect` (`[x0, y0, x1, y1]` texels, `x0` and `y0` on block borders) out of a
    /// level `side` texels wide.
    fn cut(&self, level: &[u8], side: u32, rect: [u32; 4]) -> Vec<u8> {
        let (bw, bh, bytes) = self.format.block();
        let stride = side.div_ceil(bw) as usize * bytes;
        let (from, to) = ((rect[0] / bw) as usize, rect[2].div_ceil(bw) as usize);
        let rows = rect[1] / bh..rect[3].div_ceil(bh);
        let mut out = Vec::with_capacity(rows.len() * (to - from) * bytes);
        for row in rows {
            let start = row as usize * stride;
            out.extend_from_slice(&level[start + from * bytes..start + to * bytes]);
        }
        out
    }

    /// The KTX2 file of this texture. Block formats are written as they are; `encoding` says
    /// whether the colours are sRGB (see [`Encoding`]).
    pub fn to_ktx2(&self, encoding: Encoding) -> Vec<u8> {
        ktx2::write(self.width, self.height, self.format, encoding, &self.mips)
    }
}

/// The mip sizes a header lists as stored, and whether they are LZ4-compressed.
struct Compressed {
    lz4: bool,
    sizes: Vec<u32>,
}

struct Header {
    stored: Stored,
    width: u32,
    height: u32,
    mips: usize,
    compressed: Option<Compressed>,
    rect: Option<[f32; 4]>,
}

impl Header {
    fn parse(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data, WHAT);
        let version = c.u16()?;
        if version != 1 {
            return Err(malformed(WHAT, format!("version {version}, expected 1")));
        }
        let flags = c.u16()?;
        if flags & NOT_2D != 0 {
            return Err(malformed(WHAT, "a cube map, volume or array"));
        }
        c.take(16)?; // reflectivity
        let (width, height, depth) = (c.u16()?, c.u16()?, c.u16()?);
        let (format, mips) = (c.u8()?, c.u8()?);
        c.u32()?;
        let extras_at = c.pos() as u64;
        let extras_offset = c.u32()?;
        let extras = c.u32()?;
        if depth > 1 || width == 0 || height == 0 {
            return Err(malformed(
                WHAT,
                format!("{width} x {height} x {depth} is not a 2D texture"),
            ));
        }
        let (width, height) = (u32::from(width), u32::from(height));
        let longest = width.max(height);
        let (stored, mips) = (Stored::from_id(format)?, usize::from(mips));
        if longest > MAX_SIDE || mips == 0 || mips > (32 - longest.leading_zeros()) as usize {
            return Err(malformed(
                WHAT,
                format!("{mips} levels of {width} x {height} texels"),
            ));
        }
        let mut header = Self {
            stored,
            width,
            height,
            mips,
            compressed: None,
            rect: None,
        };
        header.read_extras(data, extras_at + u64::from(extras_offset), extras)?;
        Ok(header)
    }

    /// Reads the list of `count` extra data entries at `at`: (type, offset from the offset field,
    /// size).
    fn read_extras(&mut self, data: &[u8], at: u64, count: u32) -> Result<()> {
        if count > MAX_EXTRAS {
            return Err(malformed(WHAT, format!("{count} extra data entries")));
        }
        let start = usize::try_from(at).map_err(|_| malformed(WHAT, "extra data offset"))?;
        let mut list = Cursor::new(data.get(start..).unwrap_or_default(), WHAT);
        for _ in 0..count {
            let (kind, pos) = (list.u32()?, start + list.pos());
            let (offset, size) = (list.u32()? as usize, list.u32()? as usize);
            let payload = (pos + offset)
                .checked_add(size)
                .and_then(|end| data.get(pos + offset..end))
                .ok_or_else(|| malformed(WHAT, "extra data runs past the end"))?;
            match kind {
                EXTRA_SHEET => self.rect = sheet::first_rect(payload)?,
                EXTRA_COMPRESSED_MIPS => self.compressed = Some(self.compressed_mips(payload)?),
                _ => {}
            }
        }
        Ok(())
    }

    /// The payload of the compressed mip sizes entry: whether they are compressed, the offset of
    /// the sizes (always 8), their count and the sizes, one per level.
    fn compressed_mips(&self, payload: &[u8]) -> Result<Compressed> {
        let mut c = Cursor::new(payload, WHAT);
        let lz4 = match c.u32()? {
            0 => false,
            1 => true,
            other => return Err(malformed(WHAT, format!("compressed mips flag {other}"))),
        };
        let offset = c.u32()?;
        let count = c.u32()? as usize;
        if offset != 8 || count < self.mips || c.remaining() / 4 < count {
            return Err(malformed(WHAT, "compressed mip sizes"));
        }
        let sizes = (0..count).map(|_| c.u32()).collect::<Result<_>>()?;
        Ok(Compressed { lz4, sizes })
    }

    /// The decoded size of every level, largest first.
    fn level_sizes(&self) -> Result<Vec<usize>> {
        let format = self.stored.decoded();
        let mut total = 0usize;
        (0..self.mips)
            .map(|level| {
                let size = format
                    .level_size((self.width >> level).max(1), (self.height >> level).max(1))
                    .and_then(|size| {
                        total = total.checked_add(size)?;
                        (total <= MAX_BYTES).then_some(size)
                    });
                size.ok_or_else(|| {
                    malformed(WHAT, format!("decodes to more than {MAX_BYTES} bytes"))
                })
            })
            .collect()
    }

    /// The levels out of the bytes after the resource, which hold the smallest level first. Each
    /// is stored as listed in the compressed sizes entry, if any: LZ4 where that is smaller than
    /// the level itself.
    fn read_levels(&self, raw: &[u8], sizes: &[usize]) -> Result<Vec<Vec<u8>>> {
        let stored: Vec<usize> = match &self.compressed {
            Some(c) => c.sizes[..self.mips].iter().map(|&n| n as usize).collect(),
            None => sizes.to_vec(),
        };
        let mut levels = vec![Vec::new(); self.mips];
        let mut at = 0usize;
        for level in (0..self.mips).rev() {
            let (size, len) = (sizes[level], stored[level]);
            let part = at
                .checked_add(len)
                .and_then(|end| raw.get(at..end))
                .ok_or_else(|| malformed(WHAT, "level data runs past the end"))?;
            at += len;
            levels[level] = if len >= size {
                part[..size].to_vec()
            } else if self.compressed.as_ref().is_some_and(|c| c.lz4) && size / LZ4_RATIO <= len {
                let mut out = vec![0; size];
                lz4_flex::block::decompress_into(part, &mut out)
                    .map_err(|e| malformed(WHAT, format!("level {level}: {e}")))?;
                out
            } else {
                return Err(malformed(
                    WHAT,
                    format!("level {level} has {len} of its {size} bytes"),
                ));
            };
        }
        Ok(levels)
    }
}

/// The RGBA8 pixels of a PNG-coded level, which must be the texture's size.
fn decode_png(png: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let png_err = |e: png::DecodingError| malformed(WHAT, format!("png: {e}"));
    let mut decoder = png::Decoder::new(IoCursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(png_err)?;
    let info = reader.info();
    if (info.width, info.height) != (width, height) {
        return Err(malformed(
            WHAT,
            format!(
                "png of {} x {} in a {width} x {height} texture",
                info.width, info.height
            ),
        ));
    }
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| malformed(WHAT, "png size"))?
    ];
    let frame = reader.next_frame(&mut pixels).map_err(png_err)?;
    pixels.truncate(frame.buffer_size());
    let (colors, texels) = (frame.color_type, width as usize * height as usize);
    let rgba = match colors {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(malformed(WHAT, "png left indexed")),
    };
    if rgba.len() != texels * 4 {
        return Err(malformed(WHAT, "png pixels do not fill the texture"));
    }
    Ok(rgba)
}
