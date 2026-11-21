//! noido image support: sniffing sizes from headers, decoding with a pixel
//! cap, and resizing, shared by the comic engine and the typesetter.

use std::fmt;
use std::io::{Cursor, Read};

/// An image with more pixels than this is refused, so a zip bomb or a stray
/// poster scan cannot take a render thread down.
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;
/// An archive entry larger than this is refused: an image is decoded whole.
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// How much of an entry is read looking for an image header. A real header is
/// in the first few kilobytes; this only bounds a hostile or unusual entry.
pub const MAX_HEADER_BYTES: u64 = 8 * 1024 * 1024;

/// Why an image could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The decoder refused the data.
    Image(String),
    /// The image has this many pixels, over [`MAX_PIXELS`].
    TooManyPixels(u64),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image(message) => f.write_str(message),
            Self::TooManyPixels(pixels) => {
                write!(f, "{} megapixels, too large", pixels / 1_000_000)
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Reads from `entry` until an image's size parses, in growing steps (4 KB,
/// 64 KB, 4 MB, then up to [`MAX_HEADER_BYTES`] in all). Returns the bytes
/// read and the size, if one was found.
pub fn read_header(entry: &mut impl Read) -> (Vec<u8>, Option<(u32, u32)>) {
    let mut data = Vec::new();
    for more in [4_096u64, 64 * 1024, 4 * 1024 * 1024] {
        let before = data.len();
        if entry.take(more).read_to_end(&mut data).is_err() {
            break;
        }
        if let Some(size) = dimensions(&data) {
            return (data, Some(size));
        }
        // The entry ended: reading more would tell us nothing.
        if data.len() == before {
            break;
        }
    }
    let remaining = MAX_HEADER_BYTES.saturating_sub(data.len() as u64);
    if remaining > 0
        && entry.take(remaining).read_to_end(&mut data).is_ok()
        && let Some(size) = dimensions(&data)
    {
        return (data, Some(size));
    }
    (data, None)
}

/// The image's size from its header, without decoding the pixels.
pub fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Decodes `data` to premultiplied RGBA8. An image over [`MAX_PIXELS`] is
/// refused.
pub fn decode(data: &[u8]) -> Result<image::RgbaImage, DecodeError> {
    let decoded = image::load_from_memory(data).map_err(|e| DecodeError::Image(e.to_string()))?;
    let mut rgba = decoded.into_rgba8();
    let pixels = u64::from(rgba.width()) * u64::from(rgba.height());
    if pixels > MAX_PIXELS {
        return Err(DecodeError::TooManyPixels(pixels));
    }
    premultiply(&mut rgba);
    Ok(rgba)
}

/// `image` scaled to `width` x `height` with Catmull-Rom. Its alpha must
/// already be premultiplied.
pub fn resized(image: image::RgbaImage, width: u32, height: u32) -> image::RgbaImage {
    if image.width() == width && image.height() == height {
        return image;
    }
    let (source_width, source_height) = image.dimensions();
    let source = fast_image_resize::images::Image::from_vec_u8(
        source_width,
        source_height,
        image.into_raw(),
        fast_image_resize::PixelType::U8x4,
    )
    .expect("an RGBA image is the right size");
    let mut destination =
        fast_image_resize::images::Image::new(width, height, fast_image_resize::PixelType::U8x4);
    // The pixels are already premultiplied; convolving them as they are is
    // the right thing, so the resizer must not scale them by alpha again.
    let mut options = fast_image_resize::ResizeOptions::new().resize_alg(
        fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::CatmullRom),
    );
    options.mul_div_alpha = false;
    fast_image_resize::Resizer::new()
        .resize(&source, &mut destination, Some(&options))
        .expect("both images are RGBA");
    image::RgbaImage::from_raw(width, height, destination.into_vec())
        .expect("the resizer made the right size")
}

/// `image`'s pixels in the premultiplied form the engines carry.
pub fn premultiply(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha != 255 {
            for channel in &mut pixel.0[..3] {
                *channel = (u32::from(*channel) * alpha / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png_1x1(pixel: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba(pixel));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A 1x1 PNG whose IHDR claims `width` x `height`.
    fn png_claiming(width: u32, height: u32) -> Vec<u8> {
        let mut png = png_1x1([1, 2, 3, 255]);
        // Signature (8), length (4), "IHDR" (4), then the 13 data bytes.
        png[16..20].copy_from_slice(&width.to_be_bytes());
        png[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        png
    }

    #[test]
    fn read_header_stops_at_the_header_cap() {
        let mut reader = Cursor::new(vec![0u8; (MAX_HEADER_BYTES * 2) as usize]);
        let (data, size) = read_header(&mut reader);
        assert_eq!(size, None);
        assert_eq!(data.len() as u64, MAX_HEADER_BYTES);
        assert_eq!(reader.position(), MAX_HEADER_BYTES);
    }

    #[test]
    fn read_header_finds_the_size() {
        let mut reader = Cursor::new(png_1x1([0, 0, 0, 255]));
        assert_eq!(read_header(&mut reader).1, Some((1, 1)));
    }

    #[test]
    fn decode_premultiplies() {
        let image = decode(&png_1x1([200, 100, 50, 128])).unwrap();
        let pixel = image.get_pixel(0, 0).0;
        for (got, want) in pixel.into_iter().zip([100u8, 50, 25, 128]) {
            assert!(got.abs_diff(want) <= 1, "{pixel:?}");
        }
    }

    #[test]
    fn decode_refuses_too_many_pixels() {
        let result = decode(&png_claiming(8000, 8000));
        assert!(
            matches!(
                result,
                Err(DecodeError::TooManyPixels(_) | DecodeError::Image(_))
            ),
            "{result:?}"
        );
    }

    #[test]
    fn resized_keeps_a_solid_colour() {
        let solid = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let out = resized(solid, 2, 2);
        assert_eq!(out.dimensions(), (2, 2));
        for pixel in out.pixels() {
            assert_eq!(pixel.0, [10, 20, 30, 255]);
        }
    }
}
