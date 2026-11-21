//! A block's text, flat: what is drawn, its styles, and where each drawn
//! character came from in the source.

use std::ops::Range;

/// The most characters a one-to-one segment holds, so a lookup counts
/// at most this many.
const MAX_RUN: usize = 256;

/// Text as the typesetter shapes it: one string, styles over ranges of
/// it, and segments mapping it back to the source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    /// What is drawn.
    pub string: String,
    pub styles: Vec<Styled>,
    /// In order, covering `string` without gaps.
    pub segments: Vec<Segment>,
}

/// A style over a range of drawn text.
#[derive(Clone, Debug, PartialEq)]
pub struct Styled {
    pub style: Style,
    /// Bytes of [`Text::string`].
    pub drawn: Range<usize>,
    /// The whole construct in the source, markers included (`*so*`).
    pub span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    Emphasis,
    Strong,
    Strikethrough,
    Code,
    Link {
        dest: String,
    },
    /// A reference to a footnote, drawn as its label.
    FootnoteRef {
        label: String,
    },
    /// Drawn smaller and raised; EPUB's `sup`.
    Superscript,
    /// Drawn smaller and lowered; EPUB's `sub`.
    Subscript,
    /// An image, drawn as one object character (U+FFFC) standing for
    /// the whole `![alt](src)`.
    Image {
        src: String,
        alt: String,
        /// From a plain-number `width` attribute; `None` otherwise.
        width: Option<u32>,
        /// From a plain-number `height` attribute; `None` otherwise.
        height: Option<u32>,
    },
    /// A task list item's box, drawn as one object character (U+FFFC)
    /// standing for `[ ]` or `[x]`.
    TaskBox {
        checked: bool,
    },
}

/// A piece of drawn text and the source it came from.
///
/// One-to-one when its drawn and source character counts are equal:
/// drawn character `i` is then source character `source.start + i`.
/// Otherwise (an entity, a dash from `--`, a hard break) every drawn
/// character maps to `source.start`.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// Where it starts in [`Text::string`], in bytes.
    pub drawn: usize,
    /// Its source, in characters of the part.
    pub source: Range<usize>,
}

impl Text {
    /// Appends `drawn`, which came from `source`.
    pub fn push(&mut self, drawn: &str, source: Range<usize>) {
        if drawn.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last() {
            debug_assert!(
                last.source.end <= source.start,
                "text must come in source order: {source:?} after {:?}",
                last.source
            );
        }
        if drawn.chars().count() != source.len() {
            self.segments.push(Segment {
                drawn: self.string.len(),
                source,
            });
            self.string.push_str(drawn);
            return;
        }
        // One-to-one: fill the last segment if it continues this one, then
        // cut the rest into runs of at most `MAX_RUN` characters.
        let mut rest = drawn;
        let mut at = source.start;
        if let Some(i) = self.segments.len().checked_sub(1) {
            let last = &self.segments[i];
            let room = MAX_RUN.saturating_sub(last.source.len());
            if last.source.end == at && room > 0 && self.one_to_one(i) {
                let (taken, n) = split_chars(rest, room);
                self.string.push_str(taken);
                self.segments[i].source.end += n;
                at += n;
                rest = &rest[taken.len()..];
            }
        }
        while !rest.is_empty() {
            let (taken, n) = split_chars(rest, MAX_RUN);
            self.segments.push(Segment {
                drawn: self.string.len(),
                source: at..at + n,
            });
            self.string.push_str(taken);
            at += n;
            rest = &rest[taken.len()..];
        }
    }

    /// Where segment `i` ends in the string.
    fn drawn_end(&self, i: usize) -> usize {
        self.segments
            .get(i + 1)
            .map_or(self.string.len(), |next| next.drawn)
    }

    fn one_to_one(&self, i: usize) -> bool {
        let segment = &self.segments[i];
        self.string[segment.drawn..self.drawn_end(i)]
            .chars()
            .count()
            == segment.source.len()
    }

    /// The source character drawn byte `drawn` came from; the end of the
    /// string gives the end of the last segment's source.
    pub fn source_of(&self, drawn: usize) -> usize {
        let Some(last) = self.segments.last() else {
            return 0;
        };
        if drawn >= self.string.len() {
            return last.source.end;
        }
        let i = self.segments.partition_point(|s| s.drawn <= drawn) - 1;
        let segment = &self.segments[i];
        if self.one_to_one(i) {
            segment.source.start + self.string[segment.drawn..drawn].chars().count()
        } else {
            segment.source.start
        }
    }

    /// Where source character `source` is drawn: `None` for a marker.
    /// The end of a segment's source gives the end of its drawn text.
    pub fn drawn_of(&self, source: usize) -> Option<usize> {
        let i = self
            .segments
            .partition_point(|s| s.source.start <= source)
            .checked_sub(1)?;
        let segment = &self.segments[i];
        if source < segment.source.end {
            if !self.one_to_one(i) {
                return Some(segment.drawn);
            }
            let skip = source - segment.source.start;
            let string = &self.string[segment.drawn..];
            return Some(
                segment.drawn
                    + string
                        .char_indices()
                        .nth(skip)
                        .map_or(string.len(), |(b, _)| b),
            );
        }
        (source == segment.source.end).then(|| self.drawn_end(i))
    }
}

/// The first `max` characters of `s` (or all of it), and how many that is.
fn split_chars(s: &str, max: usize) -> (&str, usize) {
    match s.char_indices().nth(max) {
        Some((byte, _)) => (&s[..byte], max),
        None => (s, s.chars().count()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_to_one_text_maps_each_character() {
        let mut t = Text::default();
        t.push("héllo", 10..15);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.source_of(0), 10);
        assert_eq!(t.source_of(3), 12); // 'l', after the two-byte 'é'
        assert_eq!(t.source_of(t.string.len()), 15);
        assert_eq!(t.drawn_of(12), Some(3));
        assert_eq!(t.drawn_of(15), Some(t.string.len()));
    }

    #[test]
    fn contiguous_pieces_are_one_segment() {
        let mut t = Text::default();
        t.push("ab", 0..2);
        t.push("cd", 2..4);
        assert_eq!(
            t.segments,
            vec![Segment {
                drawn: 0,
                source: 0..4
            }]
        );
    }

    #[test]
    fn a_gap_in_the_source_starts_a_segment_and_is_a_marker() {
        let mut t = Text::default();
        t.push("A ", 0..2);
        t.push("*b", 3..5); // `\*b`: the backslash at 2 is a gap
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.source_of(2), 3);
        assert_eq!(t.drawn_of(2), Some(2)); // end of "A "
        assert_eq!(t.drawn_of(3), Some(2));
    }

    #[test]
    fn a_piece_of_another_length_maps_to_its_start() {
        let mut t = Text::default();
        t.push("x ", 0..2);
        t.push("&", 2..7); // `&amp;`
        t.push(" y", 7..9);
        assert_eq!(t.source_of(2), 2);
        assert_eq!(t.source_of(3), 7);
        assert_eq!(t.drawn_of(4), Some(2)); // inside the entity
        assert_eq!(t.drawn_of(7), Some(3));
    }

    #[test]
    fn a_source_character_outside_every_segment_has_no_drawn_place() {
        let mut t = Text::default();
        t.push("a", 1..2);
        t.push("b", 4..5);
        assert_eq!(t.drawn_of(0), None);
        assert_eq!(t.drawn_of(3), None);
    }

    #[test]
    fn long_runs_are_cut_so_a_lookup_counts_few_characters() {
        let mut t = Text::default();
        let line = "é".repeat(100);
        for i in 0..20 {
            t.push(&line, i * 100..(i + 1) * 100);
        }
        t.push(&"x".repeat(1000), 2000..3000);
        assert!(
            t.segments.iter().all(|s| s.source.len() <= MAX_RUN),
            "{:?}",
            t.segments
        );
        for (i, (byte, _)) in t.string.char_indices().enumerate() {
            assert_eq!(t.source_of(byte), i);
            assert_eq!(t.drawn_of(i), Some(byte));
        }
    }
}
