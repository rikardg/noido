//! Markdown into the block tree, with pulldown-cmark. Offsets are
//! characters of the source as read.

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::{Align, Block, BlockKind, Cell, Item, Style, Styled, Text, Tree};

/// The tree of a markdown file.
pub fn parse(source: &str) -> Tree {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    // A byte order mark is parsed past, as a marker; offsets still count
    // it, as a character of the file as read.
    let (body, shift) = match source.strip_prefix('\u{FEFF}') {
        Some(body) => (body, 1),
        None => (source, 0),
    };
    let mut builder = Builder::new(body, shift);
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        builder.event(event, range);
    }
    Tree {
        blocks: builder.blocks,
    }
}

/// Byte offsets in the parsed text to character offsets in the file.
struct Chars {
    /// The byte where each character starts.
    starts: Vec<usize>,
    /// Characters of the file before the parsed text.
    shift: usize,
}

impl Chars {
    fn new(source: &str, shift: usize) -> Self {
        Self {
            starts: source.char_indices().map(|(b, _)| b).collect(),
            shift,
        }
    }

    fn at(&self, byte: usize) -> usize {
        self.shift + self.starts.partition_point(|&b| b < byte)
    }

    fn range(&self, bytes: Range<usize>) -> Range<usize> {
        self.at(bytes.start)..self.at(bytes.end)
    }
}

/// What a block holding text is.
enum LeafKind {
    Paragraph,
    /// A tight list item's text, which pulldown-cmark sends with no
    /// paragraph around it: a paragraph spanning its events.
    Bare,
    Heading(u8),
    Code(Option<String>),
    Cell,
}

/// A block holding blocks, still open.
enum Frame {
    Quote {
        span: Range<usize>,
        blocks: Vec<Block>,
    },
    List {
        span: Range<usize>,
        start: Option<u64>,
        items: Vec<Item>,
    },
    Item {
        span: Range<usize>,
        task: Option<bool>,
        blocks: Vec<Block>,
    },
    Footnote {
        span: Range<usize>,
        label: String,
        blocks: Vec<Block>,
    },
    Table {
        span: Range<usize>,
        align: Vec<Align>,
        head: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
        in_head: bool,
    },
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
    frames: Vec<Frame>,
    leaf: Option<Leaf>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str, shift: usize) -> Self {
        Self {
            source,
            chars: Chars::new(source, shift),
            blocks: Vec::new(),
            frames: Vec::new(),
            leaf: None,
        }
    }

    fn event(&mut self, event: Event, range: Range<usize>) {
        let span = self.chars.range(range.clone());
        match event {
            Event::Start(Tag::Paragraph) => self.open_leaf(LeafKind::Paragraph, span),
            Event::Start(Tag::Heading { level, .. }) => {
                self.open_leaf(LeafKind::Heading(heading_level(level)), span)
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) if !info.is_empty() => Some(info.to_string()),
                    _ => None,
                };
                self.open_leaf(LeafKind::Code(info), span)
            }
            Event::Start(Tag::TableCell) => self.open_leaf(LeafKind::Cell, span),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::TableCell,
            ) => self.close_leaf(),
            Event::Start(Tag::BlockQuote(_)) => self.open_frame(Frame::Quote {
                span,
                blocks: Vec::new(),
            }),
            Event::Start(Tag::List(start)) => self.open_frame(Frame::List {
                span,
                start,
                items: Vec::new(),
            }),
            Event::Start(Tag::Item) => self.open_frame(Frame::Item {
                span,
                task: None,
                blocks: Vec::new(),
            }),
            Event::Start(Tag::FootnoteDefinition(label)) => self.open_frame(Frame::Footnote {
                span,
                label: label.to_lowercase(),
                blocks: Vec::new(),
            }),
            Event::Start(Tag::Table(align)) => self.open_frame(Frame::Table {
                span,
                align: align.into_iter().map(alignment).collect(),
                head: Vec::new(),
                rows: Vec::new(),
                in_head: false,
            }),
            Event::Start(Tag::TableHead) => self.set_in_head(true),
            Event::End(TagEnd::TableHead) => self.set_in_head(false),
            Event::Start(Tag::TableRow) => {
                if let Some(Frame::Table { rows, .. }) = self.frames.last_mut() {
                    rows.push(Vec::new());
                }
            }
            Event::End(
                TagEnd::BlockQuote(_)
                | TagEnd::List(_)
                | TagEnd::Item
                | TagEnd::FootnoteDefinition
                | TagEnd::Table,
            ) => self.close_frame(),
            Event::Rule => self.block(BlockKind::Rule, span),
            Event::Start(Tag::HtmlBlock) => self.block(BlockKind::Html, span),
            Event::Start(Tag::MetadataBlock(_)) => self.block(BlockKind::FrontMatter, span),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item { task, .. }) = self.frames.last_mut() {
                    *task = Some(checked);
                }
                let space = self.source[range.end..]
                    .starts_with(' ')
                    .then(|| span.end..span.end + 1);
                let leaf = bare_leaf(&mut self.leaf, span.clone());
                leaf.push_styled("\u{FFFC}", span.clone(), Style::TaskBox { checked }, span);
                if let Some(space) = space {
                    leaf.text.push(" ", space);
                }
            }
            event => self.inline(event, range),
        }
    }

    fn open_leaf(&mut self, kind: LeafKind, span: Range<usize>) {
        self.close_leaf();
        self.leaf = Some(Leaf {
            kind,
            span,
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
            LeafKind::Paragraph | LeafKind::Bare => BlockKind::Paragraph(leaf.text),
            LeafKind::Heading(level) => BlockKind::Heading {
                level,
                text: leaf.text,
            },
            LeafKind::Code(info) => BlockKind::Code {
                info,
                text: leaf.text,
            },
            LeafKind::Cell => {
                if let Some(Frame::Table {
                    head,
                    rows,
                    in_head,
                    ..
                }) = self.frames.last_mut()
                {
                    let cell = Cell {
                        span: leaf.span,
                        text: leaf.text,
                    };
                    match rows.last_mut() {
                        Some(row) if !*in_head => row.push(cell),
                        _ => head.push(cell),
                    }
                }
                return;
            }
        };
        self.block(kind, leaf.span);
    }

    fn open_frame(&mut self, frame: Frame) {
        self.close_leaf();
        self.frames.push(frame);
    }

    fn set_in_head(&mut self, value: bool) {
        if let Some(Frame::Table { in_head, .. }) = self.frames.last_mut() {
            *in_head = value;
        }
    }

    fn close_frame(&mut self) {
        self.close_leaf();
        let Some(frame) = self.frames.pop() else {
            return;
        };
        match frame {
            Frame::Item { span, task, blocks } => {
                if let Some(Frame::List { items, .. }) = self.frames.last_mut() {
                    items.push(Item { span, task, blocks });
                }
            }
            Frame::Quote { span, blocks } => self.block(BlockKind::Quote(blocks), span),
            Frame::List { span, start, items } => {
                self.block(BlockKind::List { start, items }, span)
            }
            Frame::Footnote {
                span,
                label,
                blocks,
            } => self.block(BlockKind::Footnote { label, blocks }, span),
            Frame::Table {
                span,
                align,
                head,
                rows,
                ..
            } => self.block(BlockKind::Table { align, head, rows }, span),
        }
    }

    /// Adds a finished block to the innermost open container.
    fn block(&mut self, kind: BlockKind, span: Range<usize>) {
        self.close_leaf();
        let block = Block { kind, span };
        for frame in self.frames.iter_mut().rev() {
            match frame {
                Frame::Quote { blocks, .. }
                | Frame::Item { blocks, .. }
                | Frame::Footnote { blocks, .. } => {
                    blocks.push(block);
                    return;
                }
                Frame::List { .. } | Frame::Table { .. } => {}
            }
        }
        self.blocks.push(block);
    }

    fn inline(&mut self, event: Event, range: Range<usize>) {
        let span = self.chars.range(range.clone());
        let draws = matches!(
            event,
            Event::Text(_) | Event::Code(_) | Event::FootnoteReference(_) | Event::Start(_)
        );
        let in_item = matches!(self.frames.last(), Some(Frame::Item { .. }));
        if self.leaf.is_none() && !(draws && in_item) {
            return;
        }
        let leaf = bare_leaf(&mut self.leaf, span.clone());
        if matches!(leaf.kind, LeafKind::Bare) {
            leaf.span.end = leaf.span.end.max(span.end);
        }
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
                    label: label.to_lowercase(),
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

/// The leaf being read; in a tight list item, a bare one opened at
/// `span`.
fn bare_leaf(leaf: &mut Option<Leaf>, span: Range<usize>) -> &mut Leaf {
    leaf.get_or_insert_with(|| Leaf {
        kind: LeafKind::Bare,
        span,
        text: Text::default(),
        open: Vec::new(),
        image: None,
    })
}

fn alignment(align: Alignment) -> Align {
    match align {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
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
    use crate::{Align, Block, BlockKind, Style, Styled, Text};

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

    #[test]
    fn a_byte_order_mark_is_a_marker_and_offsets_count_it() {
        let tree = parse("\u{FEFF}# Héllo\n\nText");
        let BlockKind::Heading { text, .. } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(text.string, "Héllo");
        assert_eq!(tree.blocks[0].span.start, 1);
        assert_eq!(text.source_of(0), 3);
        let BlockKind::Paragraph(text) = &tree.blocks[1].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(text.source_of(0), 10);
        assert!(matches!(
            parse("\u{FEFF}---\nt: x\n---\n").blocks[0].kind,
            BlockKind::FrontMatter
        ));
    }

    #[test]
    fn footnote_labels_match_whatever_their_case() {
        let tree = parse("x[^N]\n\n[^n]: z\n");
        let BlockKind::Paragraph(t) = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(t.string, "xN");
        assert_eq!(t.styles[0].style, Style::FootnoteRef { label: "n".into() });
        assert!(matches!(&tree.blocks[1].kind, BlockKind::Footnote { label, .. } if label == "n"));
        let tree = parse("x[^n]\n\n[^N]: z\n");
        assert!(matches!(&tree.blocks[1].kind, BlockKind::Footnote { label, .. } if label == "n"));
    }

    #[test]
    fn a_task_list_item_has_its_box() {
        let tree = parse("- [x] done\n- two\n");
        let BlockKind::List { start: None, items } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(items[0].task, Some(true));
        assert_eq!(items[1].task, None);
        let BlockKind::Paragraph(t) = &items[0].blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(t.string, "\u{FFFC} done");
        assert_eq!(t.source_of(0), 2);
    }

    #[test]
    fn an_ordered_list_keeps_its_start() {
        let tree = parse("3. a\n4. b\n");
        let BlockKind::List { start, .. } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(*start, Some(3));
    }

    #[test]
    fn a_quote_holds_blocks_and_skips_its_markers() {
        let tree = parse("> one\n> two\n");
        let BlockKind::Quote(inner) = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        let BlockKind::Paragraph(t) = &inner[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(t.string, "one two");
        assert_eq!(t.source_of(4), 8);
        assert_eq!(t.drawn_of(7), None); // the space after the second `>`
    }

    #[test]
    fn a_fenced_code_block_keeps_its_info_and_text() {
        let tree = parse("```rust\nfn x() {}\n```\n");
        let BlockKind::Code { info, text } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(info.as_deref(), Some("rust"));
        assert_eq!(text.string, "fn x() {}\n");
        assert_eq!(text.source_of(0), 8);
    }

    #[test]
    fn a_table_has_alignment_head_and_rows() {
        let tree = parse("| a | b |\n|---|:-:|\n| c | d |\n");
        let BlockKind::Table { align, head, rows } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(align, &vec![Align::None, Align::Center]);
        assert_eq!(head[1].text.string, "b");
        assert_eq!(rows[0][0].text.source_of(0), 22);
    }

    #[test]
    fn rules_footnotes_html_and_front_matter_are_blocks() {
        let tree = parse("---\nt: x\n---\n\n***\n\n[^n]: Note.\n\n<div>h</div>\n");
        assert_eq!(tree.blocks.len(), 4, "{tree:?}");
        assert!(matches!(tree.blocks[0].kind, BlockKind::FrontMatter));
        assert_eq!(tree.blocks[0].span, 0..12);
        assert!(matches!(tree.blocks[1].kind, BlockKind::Rule));
        assert!(matches!(&tree.blocks[2].kind, BlockKind::Footnote { label, .. } if label == "n"));
        assert!(matches!(tree.blocks[3].kind, BlockKind::Html));
    }

    #[test]
    fn empty_and_unclosed_input_parses() {
        assert!(parse("").blocks.is_empty());
        assert!(parse("  \n\n").blocks.is_empty());
        assert_eq!(para("a *b").string, "a *b");
        assert!(matches!(
            parse("```\ncode").blocks[0].kind,
            BlockKind::Code { .. }
        ));
    }

    #[test]
    fn containers_nest_three_deep() {
        let tree = parse("- a\n  - > q *e*\n");
        let BlockKind::List { items, .. } = &tree.blocks[0].kind else {
            panic!("{tree:?}")
        };
        let BlockKind::List { items: inner, .. } = &items[0].blocks[1].kind else {
            panic!("{tree:?}")
        };
        let BlockKind::Quote(q) = &inner[0].blocks[0].kind else {
            panic!("{tree:?}")
        };
        let BlockKind::Paragraph(t) = &q[0].kind else {
            panic!("{tree:?}")
        };
        assert_eq!(t.string, "q e");
        assert_eq!(t.source_of(2), 13);
    }
}
