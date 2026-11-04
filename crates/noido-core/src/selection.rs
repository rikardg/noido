//! Text selection across pages: carets in document order, placed from
//! points on the layout.

use std::ops::{Range, RangeInclusive};

use noido_text::PageText;

use crate::{DocPoint, Layout};

/// A caret in document order: before character `char` of `page`. The
/// boundary between pages k and k+1 is `Caret { page: k + 1, char: 0 }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Caret {
    pub page: usize,
    pub char: usize,
}

/// Where a point lands: a caret, or a point on a page whose text hasn't
/// arrived, kept to be placed when it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spot {
    At(Caret),
    Waiting(DocPoint),
}

/// What a page's text is, as the window knows it.
#[derive(Clone, Copy)]
pub enum PageTextState<'a> {
    Unknown,
    Known(&'a PageText),
}

/// Where `p` lands. Outside its page, the boundary before the page (above
/// it, or left of it) or after it (below or right of it).
pub fn spot_at(layout: &Layout, p: DocPoint, text: PageTextState) -> Spot {
    let before = Spot::At(Caret {
        page: p.page,
        char: 0,
    });
    let after = Spot::At(Caret {
        page: p.page + 1,
        char: 0,
    });
    let Some(size) = layout.page_size(p.page) else {
        return before;
    };
    let inside = (0.0..=size.width).contains(&p.x) && (0.0..=size.height).contains(&p.y);
    if !inside {
        return if p.y < 0.0 || (p.x < 0.0 && p.y <= size.height) {
            before
        } else {
            after
        };
    }
    place(p, text).unwrap_or(Spot::Waiting(p))
}

/// The caret at `p`, a point inside its page, once its text is known.
fn place(p: DocPoint, text: PageTextState) -> Option<Spot> {
    let PageTextState::Known(text) = text else {
        return None;
    };
    let char = text.caret_at(p.x, p.y).unwrap_or(0);
    Some(Spot::At(Caret { page: p.page, char }))
}

/// `spot` placed, if it was waiting for its page's text and that has come.
pub fn resolve<'a>(spot: Spot, text_of: impl Fn(usize) -> PageTextState<'a>) -> Spot {
    match spot {
        Spot::Waiting(p) => place(p, text_of(p.page)).unwrap_or(spot),
        Spot::At(_) => spot,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub anchor: Spot,
    pub focus: Spot,
}

impl Selection {
    /// Both ends placed: (start, end) in document order.
    pub fn carets(&self) -> Option<(Caret, Caret)> {
        let (Spot::At(a), Spot::At(b)) = (self.anchor, self.focus) else {
            return None;
        };
        Some((a.min(b), a.max(b)))
    }

    /// The selected characters on `page`, given its char count.
    pub fn on_page(&self, page: usize, char_count: usize) -> Option<Range<usize>> {
        let (start, end) = self.carets()?;
        if page < start.page || page > end.page {
            return None;
        }
        let from = if page == start.page { start.char } else { 0 };
        let to = if page == end.page {
            end.char
        } else {
            char_count
        };
        let range = from.min(char_count)..to.min(char_count);
        (!range.is_empty()).then_some(range)
    }

    /// The pages from start to end.
    pub fn pages(&self) -> Option<RangeInclusive<usize>> {
        let (start, end) = self.carets()?;
        Some(start.page..=end.page)
    }

    /// The pages holding selected characters: those from start to end,
    /// less the end's page when the end is at its start.
    /// `None` with no characters between equal ends.
    fn text_pages(&self) -> Option<RangeInclusive<usize>> {
        let (start, end) = self.carets()?;
        if start == end {
            return None;
        }
        let last = if end.char == 0 && end.page > start.page {
            end.page - 1
        } else {
            end.page
        };
        Some(start.page..=last)
    }

    /// Whether no character lies between the ends. Unplaced ends, or
    /// pages between them whose text is unknown, count as not empty.
    pub fn is_empty<'a>(&self, text_of: impl Fn(usize) -> PageTextState<'a>) -> bool {
        let Some(pages) = self.text_pages() else {
            // Equal ends hold nothing; unplaced ones may.
            return self.carets().is_some();
        };
        for page in pages {
            match text_of(page) {
                PageTextState::Unknown => return false,
                PageTextState::Known(text) => {
                    if self.on_page(page, text.char_count()).is_some() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Moves the end nearer to `spot` there (Shift+click); the other end
    /// becomes the anchor.
    pub fn extend_to(&mut self, spot: Spot) {
        let (Some((start, end)), Spot::At(to)) = (self.carets(), spot) else {
            self.focus = spot;
            return;
        };
        let nearer_start = if to <= start {
            true
        } else if to >= end {
            false
        } else if start.page == end.page {
            to.char - start.char <= end.char - to.char
        } else {
            to.page - start.page <= end.page - to.page
        };
        self.anchor = Spot::At(if nearer_start { end } else { start });
        self.focus = spot;
    }

    /// Places ends waiting for their page's text.
    pub fn resolve<'a>(&mut self, text_of: impl Fn(usize) -> PageTextState<'a>) {
        self.anchor = resolve(self.anchor, &text_of);
        self.focus = resolve(self.focus, &text_of);
    }

    /// The copied text; `None` while an end is unplaced or a page in
    /// range is unknown.
    pub fn text<'a>(&self, text_of: impl Fn(usize) -> PageTextState<'a>) -> Option<String> {
        let mut pieces = Vec::new();
        self.carets()?;
        for page in self.text_pages().into_iter().flatten() {
            let PageTextState::Known(text) = text_of(page) else {
                return None;
            };
            if let Some(range) = self.on_page(page, text.char_count()) {
                pieces.push((text, range));
            }
        }
        Some(noido_text::copy_text(&pieces))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_engine::Size;
    use noido_text::testing::page;

    fn layout(pages: usize) -> Layout {
        let mut layout = Layout::new(vec![
            Size {
                width: 600.0,
                height: 800.0
            };
            pages
        ]);
        layout.arrange(1.0, 1000.0, 1000.0);
        layout
    }

    fn at(page: usize, char: usize) -> Spot {
        Spot::At(Caret { page, char })
    }

    fn point(page: usize, x: f32, y: f32) -> DocPoint {
        DocPoint { page, x, y }
    }

    fn texts() -> Vec<PageText> {
        vec![
            PageText::new(page(&["the cat sat", "on the mat"])),
            PageText::new(page(&["a dog"])),
            PageText::new(page(&["one more page"])),
            PageText::new(page(&["last"])),
        ]
    }

    fn known<'a>(texts: &'a [PageText]) -> impl Fn(usize) -> PageTextState<'a> + 'a {
        |page| {
            texts
                .get(page)
                .map_or(PageTextState::Unknown, PageTextState::Known)
        }
    }

    fn selection(anchor: Spot, focus: Spot) -> Selection {
        Selection { anchor, focus }
    }

    #[test]
    fn spot_inside_page_with_text() {
        let t = texts();
        let spot = spot_at(
            &layout(4),
            point(0, 93.0, 97.0),
            PageTextState::Known(&t[0]),
        );
        assert_eq!(spot, at(0, 4));
    }

    #[test]
    fn spot_on_page_without_text() {
        let empty = PageText::new(Vec::new());
        let spot = spot_at(
            &layout(4),
            point(1, 300.0, 400.0),
            PageTextState::Known(&empty),
        );
        assert_eq!(spot, at(1, 0));
    }

    #[test]
    fn spot_waiting_without_text_known() {
        let p = point(1, 300.0, 400.0);
        assert_eq!(
            spot_at(&layout(4), p, PageTextState::Unknown),
            Spot::Waiting(p)
        );
    }

    #[test]
    fn spot_in_gap_is_boundary() {
        let l = layout(4);
        assert_eq!(
            spot_at(&l, point(0, 300.0, 805.0), PageTextState::Unknown),
            at(1, 0)
        );
        assert_eq!(
            spot_at(&l, point(1, 300.0, -5.0), PageTextState::Unknown),
            at(1, 0)
        );
    }

    #[test]
    fn spot_in_dual_gap() {
        let l = layout(4);
        assert_eq!(
            spot_at(&l, point(0, 605.0, 100.0), PageTextState::Unknown),
            at(1, 0)
        );
        assert_eq!(
            spot_at(&l, point(1, -5.0, 100.0), PageTextState::Unknown),
            at(1, 0)
        );
    }

    #[test]
    fn resolve_places_waiting() {
        let t = texts();
        let waiting = Spot::Waiting(point(0, 93.0, 97.0));
        assert_eq!(resolve(waiting, |_| PageTextState::Unknown), waiting);
        assert_eq!(resolve(waiting, known(&t)), at(0, 4));
    }

    #[test]
    fn order_across_pages() {
        let s = selection(at(2, 3), at(0, 1));
        assert_eq!(
            s.carets(),
            Some((Caret { page: 0, char: 1 }, Caret { page: 2, char: 3 }))
        );
        assert_eq!(s.pages(), Some(0..=2));
        assert_eq!(
            selection(Spot::Waiting(point(0, 0.0, 0.0)), at(0, 1)).carets(),
            None
        );
    }

    #[test]
    fn empty_from_gap_to_gap() {
        let t = vec![
            PageText::new(page(&["a"])),
            PageText::new(Vec::new()),
            PageText::new(page(&["b"])),
        ];
        assert!(selection(at(1, 0), at(2, 0)).is_empty(known(&t)));
        assert!(!selection(at(1, 0), at(2, 1)).is_empty(known(&t)));
        assert!(!selection(at(0, 0), at(2, 0)).is_empty(|_| PageTextState::Unknown));
    }

    #[test]
    fn equal_ends_are_empty_even_past_the_last_page() {
        let s = selection(at(4, 0), at(4, 0));
        assert!(s.is_empty(|_| PageTextState::Unknown));
        assert_eq!(s.text(|_| PageTextState::Unknown).as_deref(), Some(""));
    }

    #[test]
    fn on_page_ranges() {
        let t = texts();
        let s = selection(at(0, 4), at(2, 3));
        assert_eq!(s.on_page(0, t[0].char_count()), Some(4..t[0].char_count()));
        assert_eq!(s.on_page(1, t[1].char_count()), Some(0..t[1].char_count()));
        assert_eq!(s.on_page(2, t[2].char_count()), Some(0..3));
        assert_eq!(s.on_page(3, t[3].char_count()), None);
        assert_eq!(selection(at(0, 4), at(1, 0)).on_page(1, 5), None);
    }

    #[test]
    fn extend_from_either_end() {
        let mut s = selection(at(0, 10), at(0, 20));
        s.extend_to(at(0, 5));
        assert_eq!(
            s.carets(),
            Some((Caret { page: 0, char: 5 }, Caret { page: 0, char: 20 }))
        );
        let mut s = selection(at(0, 20), at(0, 10));
        s.extend_to(at(0, 25));
        assert_eq!(
            s.carets(),
            Some((Caret { page: 0, char: 10 }, Caret { page: 0, char: 25 }))
        );
        let mut s = selection(at(0, 10), at(0, 20));
        s.extend_to(at(0, 18));
        assert_eq!(
            s.carets(),
            Some((Caret { page: 0, char: 10 }, Caret { page: 0, char: 18 }))
        );
    }

    #[test]
    fn text_waits_for_unknown_pages() {
        let t = texts();
        let s = selection(at(0, 4), at(1, 5));
        assert_eq!(
            s.text(|p| if p == 1 {
                PageTextState::Unknown
            } else {
                PageTextState::Known(&t[p])
            }),
            None
        );
        assert_eq!(
            s.text(known(&t)).as_deref(),
            Some("cat sat on the mat a dog")
        );
        assert_eq!(
            selection(at(0, 4), at(1, 0))
                .text(|p| if p == 1 {
                    PageTextState::Unknown
                } else {
                    PageTextState::Known(&t[p])
                })
                .as_deref(),
            Some("cat sat on the mat")
        );
    }
}
