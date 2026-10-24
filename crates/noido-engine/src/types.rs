/// A size in PDF points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// A rectangle in the pixel space of a page rendered at a given scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    /// The rectangle covering a whole page of `size` points rendered at `scale` pixels per point.
    pub fn full_page(size: Size, scale: f32) -> Self {
        Self {
            x: 0,
            y: 0,
            width: (size.width * scale).ceil() as u32,
            height: (size.height * scale).ceil() as u32,
        }
    }
}

/// A premultiplied RGBA8 image, row-major, without row padding.
#[derive(Clone, Debug)]
pub struct Rgba8Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Rgba8Image {
    /// Returns `None` unless `pixels` holds exactly `width * height` RGBA pixels.
    pub fn from_premultiplied(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        let expected = width as usize * height as usize * 4;
        (pixels.len() == expected).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixel at (`x`, `y`). Panics if out of bounds.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].try_into().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_rejects_wrong_length() {
        assert!(Rgba8Image::from_premultiplied(2, 2, vec![0; 15]).is_none());
        assert!(Rgba8Image::from_premultiplied(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn rgba_pixel_reads_row_major() {
        let image = Rgba8Image::from_premultiplied(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(image.pixel(1, 0), [5, 6, 7, 8]);
    }

    #[test]
    fn full_page_rect_rounds_up() {
        let letter = Size {
            width: 612.0,
            height: 792.0,
        };
        assert_eq!(
            PixelRect::full_page(letter, 0.5),
            PixelRect {
                x: 0,
                y: 0,
                width: 306,
                height: 396
            }
        );
        let odd = Size {
            width: 10.2,
            height: 10.0,
        };
        assert_eq!(
            PixelRect::full_page(odd, 1.0),
            PixelRect {
                x: 0,
                y: 0,
                width: 11,
                height: 10
            }
        );
    }
}
