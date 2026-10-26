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

/// A point on a page in PDF points, origin top-left, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A glyph's outline on the page. Corners in order: start-bottom, end-bottom,
/// end-top, start-top, along the writing direction; bottom is the descent
/// side. A quad rather than a rectangle, so rotated text keeps its shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [Point; 4],
}

impl Quad {
    /// The top-left and bottom-right corners of the smallest upright
    /// rectangle holding the quad.
    pub fn bounds(&self) -> (Point, Point) {
        let mut min = self.corners[0];
        let mut max = self.corners[0];
        for c in &self.corners[1..] {
            min.x = min.x.min(c.x);
            min.y = min.y.min(c.y);
            max.x = max.x.max(c.x);
            max.y = max.y.max(c.y);
        }
        (min, max)
    }
}

/// One glyph drawn on a page, in content-stream order.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// Its Unicode text: possibly several characters (a ligature), empty
    /// when the font doesn't say.
    pub text: String,
    /// Where its baseline starts.
    pub origin: Point,
    /// The font size: the em, in points.
    pub size: f32,
    pub quad: Quad,
    /// Drawn invisibly, as OCR text layers are.
    pub invisible: bool,
}

/// An upright rectangle on the page, in PDF points from its top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A link on a page: where it is and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The areas that follow it: one, or several for a link over wrapped
    /// lines.
    pub areas: Vec<Rect>,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// A place in this document.
    Internal(Destination),
    /// A URI, as the document gives it.
    Web(String),
}

/// A place in the document: a page, and optionally a point on it in page
/// points. A missing coordinate means the destination doesn't say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub page: usize,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_bounds_hold_every_corner() {
        let p = |x, y| Point { x, y };
        let quad = Quad {
            corners: [p(2.0, 5.0), p(4.0, 1.0), p(3.0, 0.0), p(1.0, 4.0)],
        };
        assert_eq!(quad.bounds(), (p(1.0, 0.0), p(4.0, 5.0)));
    }

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

/// What a document says about itself. Fields it doesn't give are `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    /// The application the document was made in.
    pub creator: Option<String>,
    /// The application that made the PDF.
    pub producer: Option<String>,
    pub created: Option<Date>,
    pub modified: Option<Date>,
    /// "1.7", "2.0".
    pub version: Option<String>,
    /// Marked as tagged (structured for accessibility).
    pub tagged: bool,
    pub has_outline: bool,
    /// Each font once, by name.
    pub fonts: Vec<Font>,
}

/// A date as the document writes it. `utc_offset` is in minutes, `None`
/// when the date doesn't say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub utc_offset: Option<i16>,
}

/// A font the document uses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Font {
    /// As the document names it: "ABCDEF+MinionPro-It".
    pub name: String,
    /// Whether the document carries the font; if not, it only names it and
    /// the reader's system stands in.
    pub embedded: bool,
}
