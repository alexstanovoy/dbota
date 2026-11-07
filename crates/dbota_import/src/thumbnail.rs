//! Bounded PNG/JPEG thumbnails for user-selected images and compact service images.
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops};
use std::io::Cursor;

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
    pub fn square(&self, side: u32) -> Result<Self, String> {
        if side == 0 || side > 4096 {
            return Err("Thumbnail dimensions exceed the limit".to_owned());
        }
        let source = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("decoded RGBA dimensions");
        let edge = self.width.min(self.height);
        let cropped = imageops::crop_imm(
            &source,
            (self.width - edge) / 2,
            (self.height - edge) / 2,
            edge,
            edge,
        )
        .to_image();
        let resized = imageops::resize(&cropped, side, side, imageops::FilterType::Triangle);
        Ok(Self {
            width: side,
            height: side,
            rgba: resized.into_raw(),
        })
    }
}

pub fn decode(bytes: &[u8], max_bytes: usize, max_dimension: u32) -> Result<Pixels, String> {
    if max_bytes > 256 * 1024 || max_dimension == 0 || max_dimension > 4096 {
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
