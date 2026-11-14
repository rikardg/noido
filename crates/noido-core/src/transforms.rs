//! What the reader does to a page's look without touching the file: a
//! page's rotation and scale, and the document's scale-to-fit toggle.
//! Kept per document, by page index, as pins are.

use std::collections::BTreeMap;

pub use noido_engine::Rotation;
use noido_engine::{Point, Quad, Rect, Size};

/// How the reader shows one page: turned, then scaled. The default is
/// the page as the PDF shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageTransform {
    pub rotation: Rotation,
    /// Multiplies the turned page's size; 1.0 leaves it as it is.
    pub scale: f32,
}

impl Default for PageTransform {
    fn default() -> Self {
        Self {
            rotation: Rotation::R0,
            scale: 1.0,
        }
    }
}

impl PageTransform {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// The shown size of a page whose engine (unturned, unscaled) size
    /// is `engine`.
    pub fn size(self, engine: Size) -> Size {
        let s = self.rotation.size(engine);
        Size {
            width: s.width * self.scale,
            height: s.height * self.scale,
        }
    }

    pub fn point(self, p: Point, engine: Size) -> Point {
        let p = self.rotation.point(p, engine);
        Point {
            x: p.x * self.scale,
            y: p.y * self.scale,
        }
    }

    pub fn rect(self, r: Rect, engine: Size) -> Rect {
        let r = self.rotation.rect(r, engine);
        Rect {
            x: r.x * self.scale,
            y: r.y * self.scale,
            width: r.width * self.scale,
            height: r.height * self.scale,
        }
    }

    pub fn quad(self, q: Quad, engine: Size) -> Quad {
        let q = self.rotation.quad(q, engine);
        Quad {
            corners: q.corners.map(|c| Point {
                x: c.x * self.scale,
                y: c.y * self.scale,
            }),
        }
    }

    /// `p`, a point on the page shown with `from`, on the same page
    /// shown with `to`.
    pub fn remap(p: Point, from: PageTransform, to: PageTransform, engine: Size) -> Point {
        let p = Point {
            x: p.x / from.scale,
            y: p.y / from.scale,
        };
        let p = Rotation::remap(p, from.rotation, to.rotation, engine);
        Point {
            x: p.x * to.scale,
            y: p.y * to.scale,
        }
    }
}

/// The factor each page of shown (turned, unscaled) `sizes` is scaled
/// by to fit the median page: the reference box is the median width and
/// the median height, each on its own (index `n / 2` of the sorted
/// values). Pages only shrink; a page with a zero side gets 1.
pub fn fit_scales(sizes: &[Size]) -> Vec<f32> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let median = |side: fn(&Size) -> f32| {
        let mut values: Vec<f32> = sizes.iter().map(side).collect();
        values.sort_by(f32::total_cmp);
        values[values.len() / 2]
    };
    let (box_w, box_h) = (median(|s| s.width), median(|s| s.height));
    if box_w <= 0.0 || box_h <= 0.0 {
        return vec![1.0; sizes.len()];
    }
    sizes
        .iter()
        .map(|s| {
            if s.width <= 0.0 || s.height <= 0.0 {
                1.0
            } else {
                (box_w / s.width).min(box_h / s.height).min(1.0)
            }
        })
        .collect()
}

/// Each page's transform; pages without one are as the PDF shows them.
/// Also whether the document is scaled to fit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageTransforms {
    pages: BTreeMap<usize, PageTransform>,
    scale_to_fit: bool,
}

impl PageTransforms {
    /// The transforms `records` gives; identities are left out.
    pub fn from_records(records: impl IntoIterator<Item = (usize, PageTransform)>) -> Self {
        let mut transforms = Self::default();
        for (page, transform) in records {
            transforms.set(page, transform);
        }
        transforms
    }

    pub fn get(&self, page: usize) -> PageTransform {
        self.pages.get(&page).copied().unwrap_or_default()
    }

    /// Sets `page`'s transform; the identity removes it.
    pub fn set(&mut self, page: usize, transform: PageTransform) {
        if transform.is_identity() {
            self.pages.remove(&page);
        } else {
            self.pages.insert(page, transform);
        }
    }

    /// The pages with a transform, in page order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, PageTransform)> + '_ {
        self.pages
            .iter()
            .map(|(&page, &transform)| (page, transform))
    }

    pub fn scale_to_fit(&self) -> bool {
        self.scale_to_fit
    }

    /// Does not refit; the caller does that with the page sizes.
    pub fn set_scale_to_fit(&mut self, on: bool) {
        self.scale_to_fit = on;
    }

    /// Sets every page's scale, keeping its rotation: to the fit scales
    /// of the turned `engine_sizes` when scale to fit is on, else to 1.
    pub fn refit(&mut self, engine_sizes: &[Size]) {
        let scales = if self.scale_to_fit {
            let shown: Vec<Size> = engine_sizes
                .iter()
                .enumerate()
                .map(|(page, &s)| self.get(page).rotation.size(s))
                .collect();
            fit_scales(&shown)
        } else {
            vec![1.0; engine_sizes.len()]
        };
        for (page, scale) in scales.into_iter().enumerate() {
            let mut transform = self.get(page);
            transform.scale = scale;
            self.set(page, transform);
        }
        // Pages past the sizes given keep no scale either.
        let past: Vec<usize> = self
            .pages
            .range(engine_sizes.len()..)
            .map(|(&p, _)| p)
            .collect();
        for page in past {
            let mut transform = self.get(page);
            transform.scale = 1.0;
            self.set(page, transform);
        }
    }

    /// Drops transforms at or past `page_count`.
    pub fn retain_below(&mut self, page_count: usize) {
        self.pages.retain(|&page, _| page < page_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn turned(rotation: Rotation) -> PageTransform {
        PageTransform {
            rotation,
            ..PageTransform::default()
        }
    }

    fn sz(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn fit_scales_odd_count_uses_the_middle() {
        let sizes = [sz(600.0, 800.0), sz(600.0, 800.0), sz(300.0, 400.0)];
        assert_eq!(fit_scales(&sizes), vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn fit_scales_even_count_takes_the_larger_middle() {
        let sizes = [
            sz(600.0, 800.0),
            sz(600.0, 800.0),
            sz(800.0, 600.0),
            sz(800.0, 600.0),
        ];
        assert_eq!(fit_scales(&sizes), vec![1.0; 4]);
    }

    #[test]
    fn fit_scales_shrinks_a_large_page() {
        let mut sizes = vec![sz(600.0, 800.0); 4];
        sizes.insert(2, sz(1200.0, 1600.0));
        assert_eq!(fit_scales(&sizes), vec![1.0, 1.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn fit_scales_landscape_among_portrait() {
        let mut sizes = vec![sz(595.0, 842.0); 4];
        sizes.push(sz(842.0, 595.0));
        let scales = fit_scales(&sizes);
        assert_eq!(&scales[..4], &[1.0; 4]);
        assert!(close(scales[4], 595.0 / 842.0));
    }

    #[test]
    fn fit_scales_zero_size_page_is_left_alone() {
        let sizes = [sz(600.0, 800.0), sz(0.0, 800.0), sz(600.0, 0.0)];
        assert_eq!(fit_scales(&sizes), vec![1.0; 3]);
    }

    #[test]
    fn fit_scales_a_zero_reference_box_leaves_every_page_alone() {
        let sizes = [sz(0.0, 0.0), sz(0.0, 0.0), sz(595.0, 842.0)];
        assert_eq!(fit_scales(&sizes), vec![1.0; 3]);
    }

    #[test]
    fn fit_scales_empty() {
        assert!(fit_scales(&[]).is_empty());
    }

    #[test]
    fn refit_counts_a_turned_page_by_its_turned_shape() {
        // Four portrait pages; the fifth is landscape on file but turned
        // upright, so it is not shrunk.
        let mut sizes = vec![sz(600.0, 800.0); 4];
        sizes.push(sz(800.0, 600.0));
        let mut transforms = PageTransforms::from_records([(4, turned(Rotation::R90))]);
        transforms.set_scale_to_fit(true);
        transforms.refit(&sizes);
        assert_eq!(transforms.get(4), turned(Rotation::R90));
        assert_eq!(transforms, {
            let mut t = PageTransforms::from_records([(4, turned(Rotation::R90))]);
            t.set_scale_to_fit(true);
            t
        });
    }

    #[test]
    fn refit_scales_the_big_page_and_keeps_its_rotation() {
        let mut sizes = vec![sz(600.0, 800.0); 4];
        sizes.push(sz(1600.0, 1200.0));
        let mut transforms = PageTransforms::from_records([(4, turned(Rotation::R90))]);
        transforms.set_scale_to_fit(true);
        transforms.refit(&sizes);
        assert_eq!(transforms.get(4).rotation, Rotation::R90);
        assert!(close(transforms.get(4).scale, 0.5));
        assert_eq!(transforms.get(0), PageTransform::default());
    }

    #[test]
    fn refit_off_resets_scales_and_drops_identities() {
        let mut transforms = PageTransforms::default();
        transforms.set(
            1,
            PageTransform {
                rotation: Rotation::R0,
                scale: 0.5,
            },
        );
        transforms.set(
            2,
            PageTransform {
                rotation: Rotation::R90,
                scale: 0.5,
            },
        );
        transforms.refit(&[sz(600.0, 800.0); 3]);
        assert_eq!(
            transforms.iter().collect::<Vec<_>>(),
            vec![(2, turned(Rotation::R90))]
        );
    }

    #[test]
    fn scale_to_fit_is_part_of_equality() {
        let mut on = PageTransforms::default();
        on.set_scale_to_fit(true);
        assert!(on.scale_to_fit());
        assert_ne!(on, PageTransforms::default());
    }

    #[test]
    fn size_and_point_turn_then_scale() {
        let t = PageTransform {
            rotation: Rotation::R90,
            scale: 0.5,
        };
        let engine = sz(600.0, 800.0);
        assert_eq!(t.size(engine), sz(400.0, 300.0));
        let p = t.point(Point { x: 100.0, y: 200.0 }, engine);
        assert!(close(p.x, (800.0 - 200.0) * 0.5) && close(p.y, 50.0));
        let r = t.rect(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 200.0,
            },
            engine,
        );
        // Turned: x 600..800, y 0..100; then halved.
        assert!(close(r.x, 300.0) && close(r.y, 0.0));
        assert!(close(r.width, 100.0) && close(r.height, 50.0));
    }

    #[test]
    fn quad_turns_then_scales() {
        let t = PageTransform {
            rotation: Rotation::R90,
            scale: 0.5,
        };
        let engine = sz(600.0, 800.0);
        let q = Quad {
            corners: [
                Point { x: 0.0, y: 0.0 },
                Point { x: 100.0, y: 0.0 },
                Point { x: 100.0, y: 200.0 },
                Point { x: 0.0, y: 200.0 },
            ],
        };
        // R90 sends (x, y) to (800 - y, x); then halved.
        let expected = [(400.0, 0.0), (400.0, 50.0), (300.0, 50.0), (300.0, 0.0)];
        let turned = t.quad(q, engine);
        for (c, (x, y)) in turned.corners.iter().zip(expected) {
            assert!(close(c.x, x) && close(c.y, y), "{c:?} vs ({x}, {y})");
        }
    }

    #[test]
    fn remap_between_transforms_round_trips() {
        let engine = sz(600.0, 800.0);
        let a = PageTransform::default();
        let b = PageTransform {
            rotation: Rotation::R90,
            scale: 0.5,
        };
        let p = Point { x: 123.0, y: 321.0 };
        let there = PageTransform::remap(p, a, b, engine);
        assert_eq!(there, b.point(p, engine));
        let back = PageTransform::remap(there, b, a, engine);
        assert!(close(back.x, p.x) && close(back.y, p.y));
    }

    #[test]
    fn identity_is_not_kept() {
        let mut transforms = PageTransforms::default();
        transforms.set(3, turned(Rotation::R90));
        assert_eq!(transforms.get(3), turned(Rotation::R90));
        transforms.set(3, PageTransform::default());
        assert_eq!(transforms, PageTransforms::default());
        assert_eq!(transforms.get(3), PageTransform::default());
        assert!(PageTransform::default().is_identity());
    }

    #[test]
    fn retain_below_drops_past_the_end() {
        let mut transforms =
            PageTransforms::from_records([(1, turned(Rotation::R90)), (5, turned(Rotation::R180))]);
        transforms.retain_below(5);
        assert_eq!(
            transforms.iter().collect::<Vec<_>>(),
            vec![(1, turned(Rotation::R90))]
        );
    }

    #[test]
    fn iter_is_in_page_order() {
        let transforms = PageTransforms::from_records([
            (7, turned(Rotation::R270)),
            (2, turned(Rotation::R90)),
            (4, PageTransform::default()),
        ]);
        assert_eq!(
            transforms.iter().collect::<Vec<_>>(),
            vec![(2, turned(Rotation::R90)), (7, turned(Rotation::R270))]
        );
    }
}
