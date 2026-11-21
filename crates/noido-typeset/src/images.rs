//! The pictures of a book: where to find them and how big they are drawn.

use std::collections::HashMap;

use noido_engine::PdfBytes;

/// Points to a CSS pixel.
const POINT: f32 = 0.75;

/// One image of a book, by the order its size was first read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImageId(u32);

#[cfg(test)]
impl ImageId {
    pub(crate) fn test(n: u32) -> Self {
        ImageId(n)
    }
}

/// An image and its size in pixels.
type Sized = (ImageId, (u32, u32));

/// The images of a book. Sizes are read while it is laid out, the first
/// time each is asked for.
pub(crate) struct Images {
    /// The EPUB's bytes; none for markdown, which has no images.
    bytes: Option<PdfBytes>,
    /// By path in the archive; `None` for an image that cannot be read.
    known: HashMap<String, Option<Sized>>,
    next: u32,
}

impl Images {
    /// No images: markdown's.
    pub(crate) fn none() -> Self {
        Images {
            bytes: None,
            known: HashMap::new(),
            next: 0,
        }
    }

    /// The images of the EPUB made of `bytes`.
    pub(crate) fn of_epub(bytes: PdfBytes) -> Self {
        Images {
            bytes: Some(bytes),
            ..Self::none()
        }
    }

    /// The image at `path` in the archive and its size in pixels; `None`
    /// when there is none, or its header cannot be read.
    pub(crate) fn size(&mut self, path: &str) -> Option<Sized> {
        if let Some(known) = self.known.get(path) {
            return *known;
        }
        let found = self
            .bytes
            .as_ref()
            .and_then(|bytes| noido_epub::image_size(bytes, path))
            .map(|size| {
                self.next += 1;
                (ImageId(self.next - 1), size)
            });
        self.known.insert(path.to_string(), found);
        found
    }
}

/// An image's size in points: its pixels as CSS pixels, unless the
/// `<img>` gave a width or a height (one alone keeps the aspect). `None`
/// when a side is 0.
pub(crate) fn natural(
    px: (u32, u32),
    width: Option<u32>,
    height: Option<u32>,
) -> Option<(f32, f32)> {
    if px.0 == 0 || px.1 == 0 {
        return None;
    }
    let (w, h) = (px.0 as f32, px.1 as f32);
    let (w, h) = match (width, height) {
        (Some(width), Some(height)) => (width as f32, height as f32),
        (Some(width), None) => (width as f32, width as f32 * h / w),
        (None, Some(height)) => (height as f32 * w / h, height as f32),
        (None, None) => (w, h),
    };
    Some((w * POINT, h * POINT))
}

/// `size` shrunk to fit within `max`, the aspect kept; enlarged to fill
/// it when `enlarge`.
pub(crate) fn fit(size: (f32, f32), max: (f32, f32), enlarge: bool) -> (f32, f32) {
    let (w, h) = size;
    let (across, down) = (max.0 / w, max.1 / h);
    if across.min(down) >= 1.0 && !enlarge {
        return size;
    }
    // The side that limits is the maximum itself, not a product that
    // might miss it by a rounding.
    if across <= down {
        (max.0, h * max.0 / w)
    } else {
        (w * max.1 / h, max.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_is_three_quarters_of_a_pixel() {
        assert_eq!(natural((800, 100), None, None), Some((600.0, 75.0)));
    }

    #[test]
    fn one_attribute_keeps_the_aspect() {
        assert_eq!(natural((20, 20), Some(40), None), Some((30.0, 30.0)));
    }

    #[test]
    fn a_zero_side_has_no_size() {
        assert_eq!(natural((0, 5), None, None), None);
    }

    #[test]
    fn fit_shrinks_and_never_enlarges() {
        assert_eq!(fit((600.0, 75.0), (270.0, 434.0), false), (270.0, 33.75));
        assert_eq!(fit((15.0, 15.0), (270.0, 434.0), false), (15.0, 15.0));
    }
}
