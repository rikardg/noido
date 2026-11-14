//! What the reader does to a page's look without touching the file: for
//! now its rotation. Kept per document, by page index, as pins are.

use std::collections::BTreeMap;

pub use noido_engine::Rotation;

/// How the reader shows one page. The default is the page as the PDF
/// shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageTransform {
    pub rotation: Rotation,
}

impl PageTransform {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// Each page's transform; pages without one are as the PDF shows them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageTransforms(BTreeMap<usize, PageTransform>);

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
        self.0.get(&page).copied().unwrap_or_default()
    }

    /// Sets `page`'s transform; the identity removes it.
    pub fn set(&mut self, page: usize, transform: PageTransform) {
        if transform.is_identity() {
            self.0.remove(&page);
        } else {
            self.0.insert(page, transform);
        }
    }

    /// The pages with a transform, in page order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, PageTransform)> + '_ {
        self.0.iter().map(|(&page, &transform)| (page, transform))
    }

    /// Drops transforms at or past `page_count`.
    pub fn retain_below(&mut self, page_count: usize) {
        self.0.retain(|&page, _| page < page_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turned(rotation: Rotation) -> PageTransform {
        PageTransform { rotation }
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
