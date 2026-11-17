//! Markdown into the block tree, with pulldown-cmark. Offsets are
//! characters of the source as read.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::{Block, BlockKind, Style, Styled, Text, Tree};

/// The tree of a markdown file.
pub fn parse(source: &str) -> Tree {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let mut builder = Builder::new(source);
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        builder.event(event, range);
    }
    Tree {
        blocks: builder.blocks,
    }
}

/// Byte offsets to character offsets.
struct Chars {
    /// The byte where each character starts.
    starts: Vec<usize>,
}

impl Chars {
    fn new(source: &str) -> Self {
        Self {
            starts: source.char_indices().map(|(b, _)| b).collect(),
        }
    }

    fn at(&self, byte: usize) -> usize {
        self.starts.partition_point(|&b| b < byte)
    }

    fn range(&self, bytes: Range<usize>) -> Range<usize> {
        self.at(bytes.start)..self.at(bytes.end)
    }
}

/// What a block holding text is.
enum LeafKind {
    Paragraph,
    Heading(u8),
}

/// The block whose text is being read.
struct Leaf {
    kind: LeafKind,
    span: Range<usize>,
    text: Text,
    /// Styles not yet closed: the style, where it starts in the drawn
    /// text, and its span.
    open: Vec<(Style, usize, Range<usize>)>,
    /// Inside an image: its source, the alt text so far, and how many
    /// images inside its alt text are open.
    image: Option<(String, String, usize)>,
}

impl Leaf {
    /// Draws `drawn` from `source`, styled `style` over the whole
    /// construct `span`.
    fn push_styled(&mut self, drawn: &str, source: Range<usize>, style: Style, span: Range<usize>) {
        let start = self.text.string.len();
        self.text.push(drawn, source);
        self.text.styles.push(Styled {
            style,
            drawn: start..self.text.string.len(),
            span,
        });
    }
}

struct Builder<'a> {
    source: &'a str,
    chars: Chars,
    blocks: Vec<Block>,
    leaf: Option<Leaf>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: Chars::new(source),
            blocks: Vec::new(),
            leaf: None,
        }
    }

    fn event(&mut self, event: Event, range: Range<usize>) {
        match event {
            Event::Start(Tag::Paragraph) => self.open_leaf(LeafKind::Paragraph, range),
            Event::Start(Tag::Heading { level, .. }) => {
                self.open_leaf(LeafKind::Heading(heading_level(level)), range)
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => self.close_leaf(),
            event => self.inline(event, range),
        }
    }

    fn open_leaf(&mut self, kind: LeafKind, range: Range<usize>) {
        self.leaf = Some(Leaf {
            kind,
            span: self.chars.range(range),
            text: Text::default(),
            open: Vec::new(),
            image: None,
        });
    }

    fn close_leaf(&mut self) {
        let Some(leaf) = self.leaf.take() else {
            return;
        };
        let kind = match leaf.kind {
            LeafKind::Paragraph => BlockKind::Paragraph(leaf.text),
            LeafKind::Heading(level) => BlockKind::Heading {
                level,
                text: leaf.text,
            },
        };
        self.blocks.push(Block {
            kind,
            span: leaf.span,
        });
    }

    fn inline(&mut self, event: Event, range: Range<usize>) {
        let Some(leaf) = self.leaf.as_mut() else {
            return;
        };
        let chars = &self.chars;
        if let Some((src, alt, nested)) = leaf.image.as_mut() {
            match event {
                Event::Text(s) | Event::Code(s) => alt.push_str(&s),
                // An image in the alt text is part of it.
                Event::Start(Tag::Image { .. }) => *nested += 1,
                Event::End(TagEnd::Image) if *nested > 0 => *nested -= 1,
                Event::End(TagEnd::Image) => {
                    let style = Style::Image {
                        src: std::mem::take(src),
                        alt: std::mem::take(alt),
                    };
                    leaf.image = None;
                    let span = chars.range(range);
                    leaf.push_styled("\u{FFFC}", span.clone(), style, span);
                }
                _ => {}
            }
            return;
        }
        match event {
            Event::Text(s) => leaf.text.push(&s, chars.range(range)),
            Event::SoftBreak => leaf.text.push(" ", chars.range(range)),
            Event::HardBreak => leaf.text.push("\n", chars.range(range)),
            Event::Code(s) => {
                let inside = code_inside(self.source, range.clone());
                leaf.push_styled(&s, chars.range(inside), Style::Code, chars.range(range));
            }
            Event::FootnoteReference(label) => {
                // Inside `[^` and `]`.
                let inside = range.start + 2..range.end - 1;
                let style = Style::FootnoteRef {
                    label: label.to_string(),
                };
                leaf.push_styled(&label, chars.range(inside), style, chars.range(range));
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                leaf.image = Some((dest_url.to_string(), String::new(), 0));
            }
            Event::Start(tag) => {
                let style = match tag {
                    Tag::Emphasis => Style::Emphasis,
                    Tag::Strong => Style::Strong,
                    Tag::Strikethrough => Style::Strikethrough,
                    Tag::Link { dest_url, .. } => Style::Link {
                        dest: dest_url.to_string(),
                    },
                    _ => return,
                };
                let start = leaf.text.string.len();
                leaf.open.push((style, start, chars.range(range)));
            }
            Event::End(
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link,
            ) => {
                if let Some((style, start, span)) = leaf.open.pop() {
                    let drawn = start..leaf.text.string.len();
                    leaf.text.styles.push(Styled { style, drawn, span });
                }
            }
            _ => {}
        }
    }
}

/// A code span's content (bytes): inside its backtick runs, without the
/// one padding space or line ending on each side that CommonMark strips
/// when both sides have one and the content isn't all spaces.
fn code_inside(source: &str, range: Range<usize>) -> Range<usize> {
    let written = &source[range.clone()];
    let open = written.len() - written.trim_start_matches('`').len();
    let close = written.len() - written.trim_end_matches('`').len();
    let (mut start, mut end) = (range.start + open, range.end - close);
    let content = source.get(start..end).unwrap_or_default();
    let pad = |c: char| c == ' ' || c == '\n';
    if content.len() >= 2
        && content.starts_with(pad)
        && content.ends_with(pad)
        && !content.chars().all(pad)
    {
        start += 1;
        end -= 1;
    }
    start..end.max(start)
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, BlockKind, Style, Styled, Text};

    /// The first block's text.
    fn para(source: &str) -> Text {
        match parse(source).blocks.into_iter().next().map(|b| b.kind) {
            Some(BlockKind::Paragraph(text)) => text,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn a_heading_has_its_level_text_and_whole_span() {
        let tree = parse("## Hi there\n");
        let Block {
            kind: BlockKind::Heading { level, text },
            span,
        } = &tree.blocks[0]
        else {
            panic!("{tree:?}")
        };
        assert_eq!((*level, span.clone()), (2, 0..12));
        assert_eq!(text.string, "Hi there");
        assert_eq!(text.source_of(0), 3);
    }

    #[test]
    fn emphasis_is_a_style_over_the_drawn_text_with_its_markers_in_the_span() {
        let t = para("a *bc* d");
        assert_eq!(t.string, "a bc d");
        assert_eq!(
            t.styles,
            vec![Styled {
                style: Style::Emphasis,
                drawn: 2..4,
                span: 2..6
            }]
        );
        assert_eq!(t.drawn_of(3), Some(2));
        assert_eq!(t.drawn_of(5), Some(4)); // the closing `*`: just after "bc"
    }

    #[test]
    fn escapes_entities_and_smart_punctuation_map_back() {
        let t = para("\\*x &amp; \"q\" a--b");
        assert_eq!(t.string, "*x & “q” a–b");
        assert_eq!(t.source_of(0), 1); // `*`, after the backslash
        assert_eq!(t.source_of(t.string.find('&').unwrap()), 4);
        assert_eq!(t.drawn_of(1), Some(0));
    }

    #[test]
    fn breaks_are_drawn_as_space_and_newline() {
        let t = para("a\nb  \nc");
        assert_eq!(t.string, "a b\nc");
    }

    #[test]
    fn code_spans_footnote_refs_and_images_draw_their_inside() {
        // A footnote reference is one only when the footnote is defined.
        let t = para("`co de` x[^n] ![alt *e*](i.png)\n\n[^n]: N.\n");
        assert!(t.string.starts_with("co de x"));
        assert_eq!(t.source_of(0), 1);
        assert!(
            t.styles
                .iter()
                .any(|s| s.style == Style::FootnoteRef { label: "n".into() })
        );
        let img = t
            .styles
            .iter()
            .find(|s| matches!(s.style, Style::Image { .. }))
            .unwrap();
        assert_eq!(
            img.style,
            Style::Image {
                src: "i.png".into(),
                alt: "alt e".into()
            }
        );
        assert_eq!(&t.string[img.drawn.clone()], "\u{FFFC}");
        assert_eq!(t.source_of(img.drawn.start), img.span.start);
    }

    #[test]
    fn a_code_span_of_backticks_is_drawn_from_its_inside() {
        let t = para("`` ` ``");
        assert_eq!(t.string, "`");
        assert_eq!(t.source_of(0), 3);
        let t = para("``` `` ```");
        assert_eq!(t.string, "``");
        assert_eq!(t.source_of(0), 4);
        // The padding space on each side is stripped, and a line ending
        // inside is drawn as a space.
        let t = para("`` a\nb ``");
        assert_eq!(t.string, "a b");
        assert_eq!(t.source_of(0), 3);
        assert_eq!(t.source_of(2), 5);
    }

    #[test]
    fn an_image_in_an_images_alt_text_is_part_of_it() {
        let t = para("![a ![b](c) d](e) z");
        assert_eq!(t.string, "\u{FFFC} z");
        assert_eq!(
            t.styles,
            vec![Styled {
                style: Style::Image {
                    src: "e".into(),
                    alt: "a b d".into()
                },
                drawn: 0..3,
                span: 0..17
            }]
        );
    }

    #[test]
    fn inline_html_is_not_drawn() {
        assert_eq!(para("a<br>b").string, "ab");
    }

    #[test]
    fn offsets_are_characters_not_bytes() {
        let t = para("é 😀 *ü*");
        let em = &t.styles[0];
        assert_eq!(em.span, 4..7);
        assert_eq!(t.source_of(em.drawn.start), 5);
    }

    #[test]
    fn crlf_soft_breaks_are_one_space_over_two_characters() {
        let t = para("a\r\nb");
        assert_eq!(t.string, "a b");
        assert_eq!(t.source_of(2), 3);
    }

    #[test]
    fn nested_styles_overlap_with_their_own_spans() {
        let t = para("***x*** *a **[l](u)***");
        let styles: Vec<_> = t
            .styles
            .iter()
            .map(|s| (&s.style, s.drawn.clone()))
            .collect();
        assert!(styles.contains(&(&Style::Emphasis, 0..1)), "{styles:?}");
        assert!(styles.contains(&(&Style::Strong, 0..1)), "{styles:?}");
        assert!(
            styles.contains(&(&Style::Link { dest: "u".into() }, 4..5)),
            "{styles:?}"
        );
    }
}
