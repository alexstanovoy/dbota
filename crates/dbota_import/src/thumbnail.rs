//! Bounded PNG/JPEG thumbnails of the pictures a person chooses.
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops};
use std::io::Cursor;

/// The most bytes of a picture file [`decode`] reads.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
impl Pixels {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Pixels from 8-bit RGBA data; `None` when the data is not exactly `width` by `height`
    /// pixels, or either is zero or past [`MAX_SIDE`].
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let sides = 1..=MAX_SIDE;
        if !sides.contains(&width) || !sides.contains(&height) {
            return None;
        }
        (rgba.len() == width as usize * height as usize * 4).then_some(Self {
            width,
            height,
            rgba,
        })
    }

    /// The centre square of the picture, scaled to `side` by `side` with a Lanczos filter, which
    /// keeps the sharpness of a large picture brought down. Colours are weighted by their
    /// opacity while they are mixed, so the hidden colour of a transparent pixel does not tint
    /// the edge of what is visible. A picture that already is `side` square is returned as it is.
    pub fn square(&self, side: u32) -> Result<Self, String> {
        if side == 0 || side > MAX_SIDE {
            return Err("Thumbnail dimensions exceed the limit".to_owned());
        }
        let edge = self.width.min(self.height);
        let (left, top) = ((self.width - edge) / 2, (self.height - edge) / 2);
        let row = edge as usize * 4;
        let centre = (0..edge as usize).flat_map(|y| {
            let start = ((top as usize + y) * self.width as usize + left as usize) * 4;
            self.rgba[start..start + row].as_chunks::<4>().0.iter()
        });
        if edge == side {
            return Ok(Self {
                width: side,
                height: side,
                rgba: centre.flatten().copied().collect(),
            });
        }
        let mut weighted = Vec::with_capacity(row * edge as usize);
        for pixel in centre {
            let alpha = u32::from(pixel[3]);
            weighted.extend(
                pixel[..3]
                    .iter()
                    .map(|c| ((u32::from(*c) * alpha + 127) / 255) as u8),
            );
            weighted.push(pixel[3]);
        }
        let source =
            image::RgbaImage::from_raw(edge, edge, weighted).expect("exactly edge squared pixels");
        let resized = imageops::resize(&source, side, side, imageops::FilterType::Lanczos3);
        let mut rgba = resized.into_raw();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for color in &mut pixel[..3] {
                // The filter overshoots near edges: a weighted colour cannot exceed its opacity.
                let weighted = u32::from(*color).min(alpha);
                *color = ((weighted * 255 + alpha / 2).checked_div(alpha)).unwrap_or(0) as u8;
            }
        }
        Ok(Self {
            width: side,
            height: side,
            rgba,
        })
    }

    /// A square picture brought down by a whole power of two to `side`, each level the box of the
    /// one above (see [`halve`]); `None` when it is not such a picture.
    pub fn shrunk(&self, side: u32) -> Option<Self> {
        if self.width != self.height
            || side == 0
            || side > self.width
            || !self.width.is_multiple_of(side)
        {
            return None;
        }
        if !(self.width / side).is_power_of_two() {
            return None;
        }
        let mut level = self.rgba.clone();
        let mut edge = self.width as usize;
        while edge > side as usize {
            level = halve(&level, (edge, edge), (edge / 2, edge / 2));
            edge /= 2;
        }
        Self::from_rgba(side, side, level)
    }
}

/// The largest side of a picture here, in pixels.
pub const MAX_SIDE: u32 = 4096;

/// The next, smaller level of a picture: each texel the box of the texels it covers, weighted by
/// their opacity so what is clear does not tint what is not.
pub fn halve(level: &[u8], (w, h): (usize, usize), (nw, nh): (usize, usize)) -> Vec<u8> {
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = (2 * x, 2 * y);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let texels = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            let (mut sum, mut alpha) = ([0u32; 3], 0u32);
            for (tx, ty) in texels {
                let t = &level[(ty * w + tx) * 4..(ty * w + tx) * 4 + 4];
                let a = u32::from(t[3]);
                alpha += a;
                for c in 0..3 {
                    sum[c] += u32::from(t[c]) * a;
                }
            }
            for c in sum {
                out.push(c.checked_div(alpha).unwrap_or(0) as u8);
            }
            out.push(((alpha + 2) / 4) as u8);
        }
    }
    out
}

pub fn decode(bytes: &[u8], max_bytes: usize, max_dimension: u32) -> Result<Pixels, String> {
    if max_bytes > MAX_INPUT_BYTES || max_dimension == 0 || max_dimension > MAX_SIDE {
        return Err("Image dimension or byte limit is invalid".to_owned());
    }
    if bytes.len() > max_bytes {
        return Err(format!("Choose an image of at most {max_bytes} bytes."));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "The image could not be read".to_owned())?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err("Choose a PNG or JPEG image.".to_owned());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    limits.max_alloc = Some(u64::from(max_dimension) * u64::from(max_dimension) * 4 + (1 << 20));
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    if image.width() == 0 || image.height() == 0 {
        return Err("The image is empty.".to_owned());
    }
    let image = image.into_rgba8();
    Ok(Pixels {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn decode_error(error: image::ImageError) -> String {
    match error {
        image::ImageError::Limits(_) => {
            "Image dimensions or decoded data exceed the limits; use a smaller image.".to_owned()
        }
        _ => "The image could not be decoded; choose a PNG or JPEG image.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&pixel.repeat((width * height) as usize))
                .unwrap();
        }
        bytes
    }
    #[test]
    fn a_compact_service_png_decodes_and_makes_a_square_preview() {
        let bytes = png(1, 1, [20, 90, 180, 255]);
        assert!(bytes.len() <= 512);
        let pixels = decode(&bytes, 512, 128).expect("a valid compact avatar");
        let square = pixels.square(128).unwrap();
        assert_eq!((square.width(), square.height()), (128, 128));
        assert!(
            square
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [20, 90, 180, 255])
        );
    }
    #[test]
    fn byte_small_images_with_excessive_dimensions_are_rejected_before_decoding() {
        let bytes = png(8192, 1, [0, 0, 0, 255]);
        assert!(bytes.len() < 256 * 1024);
        let error = decode(&bytes, 256 * 1024, 4096).unwrap_err();
        assert!(error.contains("dimension"), "{error}");
    }
    #[test]
    fn the_service_image_budget_is_checked_before_the_codec() {
        let error = decode(&vec![0; 513], 512, 128).unwrap_err();
        assert!(error.contains("512"), "{error}");
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;
    #[test]
    fn a_wide_upload_crops_its_center_before_resizing() {
        let pixels = Pixels {
            width: 3,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
        };
        let cropped = pixels.square(2).unwrap();
        assert_eq!(
            cropped.rgba(),
            &[
                0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255
            ]
        );
    }
    #[test]
    fn jpeg_uploads_use_the_same_bounded_crop_path() {
        let source = image::RgbImage::from_pixel(3, 1, image::Rgb([20, 90, 180]));
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode_image(&DynamicImage::ImageRgb8(source))
            .unwrap();
        let pixels = decode(&bytes, 256 * 1024, 4096)
            .unwrap()
            .square(128)
            .unwrap();
        assert_eq!(pixels.rgba().len(), 128 * 128 * 4);
        assert!(
            pixels
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255 && pixel[2] > pixel[1] && pixel[1] > pixel[0])
        );
        assert!(decode(&bytes[..20], 256 * 1024, 4096).is_err());
    }
}

#[cfg(test)]
mod square_tests {
    use super::*;

    fn pixels(width: u32, height: u32, texel: impl Fn(u32, u32) -> [u8; 4]) -> Pixels {
        let rgba = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| texel(x, y))
            .collect();
        Pixels::from_rgba(width, height, rgba).expect("matching size")
    }

    #[test]
    fn a_picture_that_already_is_the_side_square_comes_out_unchanged() {
        let picture = pixels(64, 64, |x, y| [x as u8 * 3, y as u8 * 3, 7, 200]);
        assert_eq!(picture.square(64).expect("same size"), picture);
    }

    #[test]
    fn the_centre_square_of_a_tall_picture_is_kept() {
        let picture = pixels(4, 8, |_, y| {
            if (2..6).contains(&y) {
                [9, 9, 9, 255]
            } else {
                [1, 1, 1, 255]
            }
        });
        let square = picture.square(4).expect("square");
        assert!(
            square
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [9, 9, 9, 255])
        );
    }

    #[test]
    fn a_transparent_half_does_not_tint_the_visible_half_when_the_picture_shrinks() {
        // Opaque red on the left, fully transparent (and blue underneath) on the right.
        let picture = pixels(64, 64, |x, _| {
            if x < 32 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 0]
            }
        });
        let small = picture.square(8).expect("shrunk");
        for texel in small.rgba().as_chunks::<4>().0 {
            if texel[3] > 0 {
                assert!(
                    texel[2] <= 2,
                    "a visible texel took blue from what is not seen: {texel:?}"
                );
            }
        }
        let column = |x: usize| small.rgba()[x * 4..x * 4 + 4].to_vec();
        assert_eq!(column(0), [255, 0, 0, 255], "the opaque side is as it was");
        assert_eq!(column(7)[3], 0, "the clear side is clear");
    }

    #[test]
    fn a_large_picture_brought_down_keeps_a_sharp_edge() {
        let picture = pixels(512, 512, |x, _| {
            if x < 256 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let small = picture.square(32).expect("shrunk");
        let at = |x: usize| small.rgba()[x * 4];
        assert!(at(14) < 8 && at(17) > 247, "{} {}", at(14), at(17));
    }

    #[test]
    fn shrinking_by_whole_levels_averages_the_covered_texels() {
        let picture = pixels(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                [200, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let shrunk = picture.shrunk(2).expect("one level");
        assert!(
            shrunk
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [100, 0, 0, 255])
        );
        assert!(
            picture.shrunk(3).is_none()
                && picture.shrunk(8).is_none()
                && picture.shrunk(0).is_none()
        );
        assert_eq!(picture.shrunk(4).expect("itself"), picture);
    }

    #[test]
    fn pixels_must_match_their_declared_size() {
        assert!(Pixels::from_rgba(2, 2, vec![0; 15]).is_none());
        assert!(Pixels::from_rgba(0, 2, Vec::new()).is_none());
        assert!(Pixels::from_rgba(MAX_SIDE + 1, 1, vec![0; (MAX_SIDE as usize + 1) * 4]).is_none());
        assert!(Pixels::from_rgba(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn a_picked_file_beyond_the_byte_limit_is_refused_by_its_size() {
        let error = decode(&vec![0; MAX_INPUT_BYTES + 1], MAX_INPUT_BYTES, 4096).unwrap_err();
        assert!(error.contains("16777216"), "{error}");
    }
}
