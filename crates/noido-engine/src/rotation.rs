//! Quarter turns of a page, clockwise, and how points, rectangles, glyph
//! quads and images on the page turn with it.

use crate::{Point, Quad, Rect, Rgba8Image, Size};

/// A clockwise turn of a page in 90° steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    /// Every turn, in the order a menu offers them.
    pub const ALL: [Rotation; 4] = [Self::R0, Self::R90, Self::R180, Self::R270];

    fn quarters(self) -> u8 {
        match self {
            Self::R0 => 0,
            Self::R90 => 1,
            Self::R180 => 2,
            Self::R270 => 3,
        }
    }

    fn from_quarters(quarters: u8) -> Rotation {
        Self::ALL[usize::from(quarters % 4)]
    }

    pub fn degrees(self) -> u16 {
        u16::from(self.quarters()) * 90
    }

    /// The turn of `degrees`: 0, 90, 180 or 270; anything else is `None`.
    pub fn from_degrees(degrees: u16) -> Option<Rotation> {
        (degrees.is_multiple_of(90) && degrees < 360)
            .then(|| Self::from_quarters((degrees / 90) as u8))
    }

    /// The turn that undoes this one.
    pub fn inverse(self) -> Rotation {
        Self::from_quarters(4 - self.quarters())
    }

    /// This turn followed by `next`.
    pub fn then(self, next: Rotation) -> Rotation {
        Self::from_quarters(self.quarters() + next.quarters())
    }

    /// The size of a page of unturned size `s` once turned.
    pub fn size(self, s: Size) -> Size {
        match self {
            Self::R0 | Self::R180 => s,
            Self::R90 | Self::R270 => Size {
                width: s.height,
                height: s.width,
            },
        }
    }

    /// Where `p`, on a page of unturned size `s` (origin top-left, y down),
    /// is once the page is turned. With a zero size it is a plain turn
    /// about the origin.
    pub fn point(self, p: Point, s: Size) -> Point {
        let (w, h) = (s.width, s.height);
        match self {
            Self::R0 => p,
            Self::R90 => Point { x: h - p.y, y: p.x },
            Self::R180 => Point {
                x: w - p.x,
                y: h - p.y,
            },
            Self::R270 => Point { x: p.y, y: w - p.x },
        }
    }

    /// The upright rectangle `r` covers once turned.
    pub fn rect(self, r: Rect, s: Size) -> Rect {
        let a = self.point(Point { x: r.x, y: r.y }, s);
        let b = self.point(
            Point {
                x: r.x + r.width,
                y: r.y + r.height,
            },
            s,
        );
        Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    /// `q` turned, corner by corner, so its writing direction turns too.
    pub fn quad(self, q: Quad, s: Size) -> Quad {
        Quad {
            corners: q.corners.map(|c| self.point(c, s)),
        }
    }

    /// `p`, a point on a page shown turned by `from`, on the same page
    /// shown turned by `to`; `unturned` is the page's size without a turn.
    pub fn remap(p: Point, from: Rotation, to: Rotation, unturned: Size) -> Point {
        let plain = from.inverse().point(p, from.size(unturned));
        to.point(plain, unturned)
    }
}

impl Rgba8Image {
    /// The image turned clockwise by `r`.
    pub fn turned(&self, r: Rotation) -> Rgba8Image {
        let (w, h) = (self.width() as usize, self.height() as usize);
        if r == Rotation::R0 {
            return self.clone();
        }
        let (tw, th) = match r {
            Rotation::R90 | Rotation::R270 => (h, w),
            _ => (w, h),
        };
        let source = self.pixels();
        let mut pixels = vec![0u8; source.len()];
        for y in 0..h {
            for x in 0..w {
                let (tx, ty) = match r {
                    Rotation::R90 => (h - 1 - y, x),
                    Rotation::R180 => (w - 1 - x, h - 1 - y),
                    _ => (y, w - 1 - x),
                };
                let from = (y * w + x) * 4;
                let to = (ty * tw + tx) * 4;
                pixels[to..to + 4].copy_from_slice(&source[from..from + 4]);
            }
        }
        Rgba8Image::from_premultiplied(tw as u32, th as u32, pixels)
            .expect("a turn keeps the pixel count")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, Rect, Size};

    const S: Size = Size {
        width: 100.0,
        height: 200.0,
    };
    const P: Point = Point { x: 10.0, y: 20.0 };

    #[test]
    fn point_follows_the_spec_formulas() {
        assert_eq!(Rotation::R0.point(P, S), P);
        assert_eq!(Rotation::R90.point(P, S), Point { x: 180.0, y: 10.0 });
        assert_eq!(Rotation::R180.point(P, S), Point { x: 90.0, y: 180.0 });
        assert_eq!(Rotation::R270.point(P, S), Point { x: 20.0, y: 90.0 });
    }

    #[test]
    fn inverse_undoes_every_turn() {
        for r in Rotation::ALL {
            assert_eq!(r.inverse().point(r.point(P, S), r.size(S)), P, "{r:?}");
        }
    }

    #[test]
    fn then_adds_turns() {
        assert_eq!(Rotation::R90.then(Rotation::R270), Rotation::R0);
        assert_eq!(Rotation::R90.then(Rotation::R90), Rotation::R180);
        assert_eq!(Rotation::R180.then(Rotation::R270), Rotation::R90);
    }

    #[test]
    fn size_swaps_on_quarter_turns() {
        let swapped = Size {
            width: 200.0,
            height: 100.0,
        };
        assert_eq!(Rotation::R0.size(S), S);
        assert_eq!(Rotation::R90.size(S), swapped);
        assert_eq!(Rotation::R180.size(S), S);
        assert_eq!(Rotation::R270.size(S), swapped);
    }

    #[test]
    fn degrees_round_trip_and_reject_others() {
        for r in Rotation::ALL {
            assert_eq!(Rotation::from_degrees(r.degrees()), Some(r));
        }
        assert_eq!(Rotation::R270.degrees(), 270);
        assert_eq!(Rotation::from_degrees(45), None);
        assert_eq!(Rotation::from_degrees(360), None);
    }

    #[test]
    fn remap_between_turns_equals_through_unturned() {
        for from in Rotation::ALL {
            for to in Rotation::ALL {
                let p = from.point(P, S);
                assert_eq!(Rotation::remap(p, from, to, S), to.point(P, S));
            }
        }
    }

    #[test]
    fn rect_turns_its_corners() {
        let r = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 5.0,
        };
        assert_eq!(
            Rotation::R90.rect(r, S),
            Rect {
                x: 175.0,
                y: 10.0,
                width: 5.0,
                height: 30.0,
            }
        );
    }

    #[test]
    fn quad_keeps_its_corner_order() {
        let q = crate::Quad {
            corners: [
                Point { x: 0.0, y: 10.0 },
                Point { x: 5.0, y: 10.0 },
                Point { x: 5.0, y: 0.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        let t = Rotation::R90.quad(q, S);
        for i in 0..4 {
            assert_eq!(t.corners[i], Rotation::R90.point(q.corners[i], S));
        }
    }

    #[test]
    fn turned_image_moves_the_corner_pixel() {
        const RED: [u8; 4] = [255, 0, 0, 255];
        let mut pixels = vec![0u8; 3 * 2 * 4];
        pixels[..4].copy_from_slice(&RED);
        let image = Rgba8Image::from_premultiplied(3, 2, pixels).unwrap();
        let r90 = image.turned(Rotation::R90);
        assert_eq!((r90.width(), r90.height()), (2, 3));
        assert_eq!(r90.pixel(1, 0), RED);
        let r180 = image.turned(Rotation::R180);
        assert_eq!((r180.width(), r180.height()), (3, 2));
        assert_eq!(r180.pixel(2, 1), RED);
        let r270 = image.turned(Rotation::R270);
        assert_eq!((r270.width(), r270.height()), (2, 3));
        assert_eq!(r270.pixel(0, 2), RED);
        assert_eq!(image.turned(Rotation::R0).pixel(0, 0), RED);
    }
}
