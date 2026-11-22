//! The block tree as a flat run of paragraphs, in source order. Blocks
//! Y3b will set properly (lists, tables, quotes, notes) get simple
//! stand-ins here.

use std::collections::HashMap;

use std::ops::Range;

use noido_blocks::{Block, BlockKind, Segment, Style, Styled, Text, Tree};

use crate::house::{LEADING, LINES, MEASURE};
use crate::images::{ImageId, fit};

/// How a paragraph is set.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Role {
    Body {
        indent: bool,
    },
    Heading(u8),
    Code,
    /// An image on a line of its own, `width` by `height` points; the
    /// text is its object character. `heading`: it is a heading's only
    /// content, and keeps the heading's rules.
    Figure {
        image: ImageId,
        width: f32,
        height: f32,
        heading: Option<u8>,
    },
}

/// One paragraph to set.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Para {
    pub role: Role,
    /// The block's own text, source map intact.
    pub text: Text,
    /// Drawn before the text, with no source: "• ", "3. ", a note's "1 ".
    pub prefix: Option<String>,
    /// A level-1 heading starts a page.
    pub new_page: bool,
    /// The label of the footnote this paragraph begins.
    pub note: Option<String>,
    /// The images drawn in its lines: the range of the drawn text, the
    /// image and its natural size in points.
    pub images: Vec<(Range<usize>, ImageId, (f32, f32))>,
}

/// An image drawn as a block of its own.
struct Figure {
    /// Its object character, in bytes of the text.
    at: Range<usize>,
    image: ImageId,
    /// Drawn size in points.
    size: (f32, f32),
    /// The paragraph holds nothing else.
    alone: bool,
}

/// The part of `text` in `range` (bytes of its string, on the edge of
/// a segment): its styles clipped to it, its segments kept, so that the
/// source map is the original's.
fn clip(text: &Text, range: Range<usize>) -> Text {
    let styles = text
        .styles
        .iter()
        .filter_map(|s| {
            let start = s.drawn.start.max(range.start);
            let end = s.drawn.end.min(range.end);
            (start < end).then(|| Styled {
                style: s.style.clone(),
                drawn: start - range.start..end - range.start,
                span: s.span.clone(),
            })
        })
        .collect();
    let mut segments: Vec<Segment> = Vec::new();
    for (i, segment) in text.segments.iter().enumerate() {
        let end = text
            .segments
            .get(i + 1)
            .map_or(text.string.len(), |n| n.drawn);
        if end <= range.start || segment.drawn >= range.end {
            continue;
        }
        segments.push(Segment {
            drawn: segment.drawn.saturating_sub(range.start),
            source: segment.source.clone(),
        });
    }
    Text {
        string: text.string[range].to_string(),
        styles,
        segments,
    }
}

/// Footnote label to its number: the order of its first reference.
/// Notes never referenced follow, in definition order.
pub(crate) struct Notes(pub HashMap<String, usize>);

/// Maps an image's `src` as written and its `width` and `height`
/// attributes to its id and natural size in points; `None` when it
/// cannot be drawn.
pub(crate) type Resolve<'a> =
    dyn FnMut(&str, Option<u32>, Option<u32>) -> Option<(ImageId, (f32, f32))> + 'a;

/// A resolver for a source without images (markdown).
pub(crate) fn no_images(
    _src: &str,
    _width: Option<u32>,
    _height: Option<u32>,
) -> Option<(ImageId, (f32, f32))> {
    None
}

pub(crate) fn paragraphs(tree: &Tree, resolve: &mut Resolve) -> (Vec<Para>, Notes) {
    let notes = number_notes(&tree.blocks);
    let mut out = Out {
        paras: Vec::new(),
        notes: &notes,
        resolve,
        indent: false,
    };
    out.blocks(&tree.blocks);
    (out.paras, notes)
}

struct Out<'a> {
    paras: Vec<Para>,
    notes: &'a Notes,
    resolve: &'a mut Resolve<'a>,
    /// Whether the next body paragraph is indented: not the first, nor
    /// the first after a heading.
    indent: bool,
}

impl Out<'_> {
    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.block(block);
        }
    }

    fn body(&mut self, text: &Text) {
        self.text_blocks(Role::Body { indent: false }, text, false);
    }

    /// A paragraph or a heading: as it is, or split around the images
    /// that are figures.
    fn text_blocks(&mut self, role: Role, text: &Text, new_page: bool) {
        let heading = match role {
            Role::Heading(level) => Some(level),
            _ => None,
        };
        let (figures, inline) = self.figures(text);
        if figures.is_empty() {
            self.paras.push(Para {
                role: match role {
                    Role::Body { .. } => Role::Body {
                        indent: self.indent,
                    },
                    other => other,
                },
                text: text.clone(),
                prefix: None,
                new_page,
                note: None,
                images: inline,
            });
            self.indent = heading.is_none();
            return;
        }
        let mut pieces = Vec::new();
        let mut at = 0;
        for figure in &figures {
            pieces.push((at..figure.at.start, None));
            pieces.push((figure.at.clone(), Some(figure)));
            at = figure.at.end;
        }
        pieces.push((at..text.string.len(), None));

        let first = self.paras.len();
        let mut ends_in_text = false;
        for (range, figure) in pieces {
            let alone = figures.len() == 1 && figure.is_some_and(|f| f.alone);
            let role = match figure {
                Some(f) => Role::Figure {
                    image: f.image,
                    width: f.size.0,
                    height: f.size.1,
                    heading: heading.filter(|_| alone),
                },
                None if text.string[range.clone()].trim().is_empty() => continue,
                None if self.paras.len() > first => match role.clone() {
                    Role::Body { .. } => Role::Body { indent: false },
                    other => other,
                },
                None => match role.clone() {
                    Role::Body { .. } => Role::Body {
                        indent: self.indent,
                    },
                    other => other,
                },
            };
            ends_in_text = figure.is_none();
            let images = inline
                .iter()
                .filter(|(at, ..)| range.start <= at.start && at.end <= range.end)
                .map(|(at, id, size)| (at.start - range.start..at.end - range.start, *id, *size))
                .collect();
            self.paras.push(Para {
                role,
                text: clip(text, range),
                prefix: None,
                new_page: new_page && self.paras.len() == first,
                note: None,
                images,
            });
        }
        self.indent = ends_in_text && heading.is_none();
    }

    /// The images of `text` that are figures, in order: the only drawn
    /// content of the paragraph, or taller than two lines; and the other
    /// images that can be drawn, which stay in the line.
    #[allow(clippy::type_complexity)]
    fn figures(&mut self, text: &Text) -> (Vec<Figure>, Vec<(Range<usize>, ImageId, (f32, f32))>) {
        let mut found: Vec<(Range<usize>, ImageId, (f32, f32))> = Vec::new();
        for styled in &text.styles {
            let Style::Image {
                src, width, height, ..
            } = &styled.style
            else {
                continue;
            };
            if text.string.get(styled.drawn.clone()) != Some("\u{fffc}") {
                continue;
            }
            if let Some((image, size)) = (self.resolve)(src, *width, *height) {
                found.push((styled.drawn.clone(), image, size));
            }
        }
        found.sort_by_key(|f| f.0.start);
        let alone = found.len() == 1 && {
            let (range, ..) = &found[0];
            text.string[..range.start].trim().is_empty()
                && text.string[range.end..].trim().is_empty()
        };
        let (large, small): (Vec<_>, Vec<_>) = found
            .into_iter()
            .partition(|(_, _, natural)| alone || natural.1 > 2.0 * LEADING);
        let figures = large
            .into_iter()
            .map(|(at, image, natural)| Figure {
                at,
                image,
                size: fit(natural, (MEASURE, LINES as f32 * LEADING), false),
                alone,
            })
            .collect();
        (figures, small)
    }

    /// The blocks of a container, the first paragraph carrying `prefix`.
    fn with_prefix(&mut self, blocks: &[Block], prefix: String) {
        let from = self.paras.len();
        self.blocks(blocks);
        if let Some(first) = self.paras.get_mut(from) {
            first.prefix = Some(prefix);
        }
    }

    fn block(&mut self, block: &Block) {
        match &block.kind {
            BlockKind::Heading { level, text } => {
                self.text_blocks(Role::Heading(*level), text, *level == 1);
            }
            BlockKind::Paragraph(text) => self.body(text),
            BlockKind::Code { text, .. } => {
                self.paras.push(Para {
                    role: Role::Code,
                    text: text.clone(),
                    prefix: None,
                    new_page: false,
                    note: None,
                    images: Vec::new(),
                });
                self.indent = false;
            }
            BlockKind::Quote(blocks) => {
                self.indent = false;
                self.blocks(blocks);
                self.indent = false;
            }
            BlockKind::List { start, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = match start {
                        Some(start) => format!("{}. ", start + i as u64),
                        None => "• ".to_string(),
                    };
                    let from = self.paras.len();
                    self.with_prefix(&item.blocks, marker);
                    // The marker marks an item: none of it is indented.
                    for para in &mut self.paras[from..] {
                        if let Role::Body { indent } = &mut para.role {
                            *indent = false;
                        }
                    }
                }
                self.indent = false;
            }
            BlockKind::Table { head, rows, .. } => {
                // Joining cells would break the source map: each is its
                // own paragraph, unindented.
                for cell in head.iter().chain(rows.iter().flatten()) {
                    self.paras.push(Para {
                        role: Role::Body { indent: false },
                        text: cell.text.clone(),
                        prefix: None,
                        new_page: false,
                        note: None,
                        images: Vec::new(),
                    });
                }
                self.indent = false;
            }
            BlockKind::Rule => {
                // A section break: what follows starts afresh.
                self.indent = false;
                self.body(&Text::default());
                self.indent = false;
            }
            BlockKind::Footnote { label, blocks } => {
                self.indent = false;
                let from = self.paras.len();
                match self.notes.0.get(label).map(|n| format!("{n} ")) {
                    Some(prefix) => self.with_prefix(blocks, prefix),
                    None => self.blocks(blocks),
                }
                if let Some(first) = self.paras.get_mut(from) {
                    first.note = Some(label.clone());
                }
                self.indent = false;
            }
            BlockKind::Html | BlockKind::FrontMatter => {}
        }
    }
}

/// Numbers footnotes by first reference, then the unreferenced by
/// definition.
fn number_notes(blocks: &[Block]) -> Notes {
    let mut refs = Vec::new();
    let mut defined = Vec::new();
    collect(blocks, &mut refs, &mut defined);
    let mut map = HashMap::new();
    for label in refs.into_iter().chain(defined) {
        let next = map.len() + 1;
        map.entry(label).or_insert(next);
    }
    Notes(map)
}

fn refs_in(text: &Text, refs: &mut Vec<String>) {
    for styled in &text.styles {
        if let Style::FootnoteRef { label } = &styled.style {
            refs.push(label.clone());
        }
    }
}

fn collect(blocks: &[Block], refs: &mut Vec<String>, defined: &mut Vec<String>) {
    for block in blocks {
        match &block.kind {
            BlockKind::Heading { text, .. } | BlockKind::Paragraph(text) => refs_in(text, refs),
            BlockKind::Table { head, rows, .. } => {
                for cell in head.iter().chain(rows.iter().flatten()) {
                    refs_in(&cell.text, refs);
                }
            }
            BlockKind::Quote(inner) => collect(inner, refs, defined),
            BlockKind::List { items, .. } => {
                for item in items {
                    collect(&item.blocks, refs, defined);
                }
            }
            BlockKind::Footnote {
                label,
                blocks: inner,
            } => {
                defined.push(label.clone());
                collect(inner, refs, defined);
            }
            BlockKind::Code { .. } | BlockKind::Rule | BlockKind::Html | BlockKind::FrontMatter => {
            }
        }
    }
}

/// What the front matter says about the book.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Front {
    pub title: Option<String>,
    pub author: Option<String>,
    pub lang: Option<String>,
}

/// Reads `title`, `author` and `lang` (or `language`; `lang` wins) from
/// the top-level `key: value` lines of the front matter block.
pub(crate) fn front_matter(tree: &Tree, source: &str) -> Front {
    let mut front = Front {
        title: None,
        author: None,
        lang: None,
    };
    let Some(block) = tree
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::FrontMatter))
    else {
        return front;
    };
    // Spans are in characters, not bytes.
    let yaml: String = source
        .chars()
        .skip(block.span.start)
        .take(block.span.len())
        .collect();
    let mut language = None;
    for line in yaml.lines() {
        if line.starts_with([' ', '\t', '-', '#']) {
            continue; // nested, a list item, or a comment
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "title" => front.title = Some(value),
            "author" => front.author = Some(value),
            "lang" => front.lang = Some(value),
            "language" => language = Some(value),
            _ => {}
        }
    }
    front.lang = front.lang.or(language);
    front
}

fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner.to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_blocks::markdown::parse;

    fn paras(md: &str) -> Vec<Para> {
        paragraphs(&parse(md), &mut no_images).0
    }

    #[test]
    fn paragraphs_come_in_source_order_with_book_indents() {
        let p = paras("# T\n\nOne.\n\nTwo.\n");
        let roles: Vec<_> = p.iter().map(|p| p.role.clone()).collect();
        assert_eq!(
            roles,
            [
                Role::Heading(1),
                Role::Body { indent: false },
                Role::Body { indent: true }
            ]
        );
        assert!(p[0].new_page);
        assert!(!p[1].new_page);
    }

    fn indents(md: &str) -> Vec<bool> {
        paras(md)
            .iter()
            .filter_map(|p| match p.role {
                Role::Body { indent } => Some(indent),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_paragraph_after_a_heading_is_unindented_and_levels_are_kept() {
        let p = paras("One.\n\n## H\n\nTwo.\n");
        assert_eq!(p[1].role, Role::Heading(2));
        assert!(!p[1].new_page);
        assert_eq!(indents("One.\n\n## H\n\nTwo.\n"), [false, false]);
    }

    #[test]
    fn notes_referenced_in_a_table_are_numbered_by_reference() {
        let md = "A.\n\n| a[^x] |\n|---|\n| b |\n\n[^y]: Y.\n\n[^x]: X.\n";
        let (_, notes) = paragraphs(&parse(md), &mut no_images);
        assert_eq!((notes.0["x"], notes.0["y"]), (1, 2));
    }

    #[test]
    fn a_paragraph_after_a_rule_is_unindented() {
        // One, (rule), Two, Three
        assert_eq!(
            indents("One.\n\n***\n\nTwo.\n\nThree.\n"),
            [false, false, false, true]
        );
    }

    #[test]
    fn a_paragraph_after_a_list_is_unindented() {
        // One, a, b, Two, Three: list items are unindented.
        assert_eq!(
            indents("One.\n\n- a\n- b\n\nTwo.\n\nThree.\n"),
            [false, false, false, false, true]
        );
    }

    #[test]
    fn a_paragraph_after_code_a_table_a_quote_or_a_note_is_unindented() {
        assert_eq!(indents("One.\n\n```\nx\n```\n\nTwo.\n"), [false, false]);
        assert_eq!(
            indents("One.\n\n| a |\n|---|\n| b |\n\nTwo.\n"),
            [false, false, false, false]
        );
        assert_eq!(indents("One.\n\n> Q.\n\nTwo.\n"), [false, false, false]);
        assert_eq!(
            indents("One[^x].\n\n[^x]: N.\n\nTwo.\n"),
            [false, false, false]
        );
    }

    #[test]
    fn paragraphs_inside_a_quote_follow_the_rule_among_themselves() {
        assert_eq!(indents("> A.\n>\n> B.\n"), [false, true]);
    }

    #[test]
    fn list_items_carry_their_markers() {
        let prefixes =
            |md| -> Vec<Option<String>> { paras(md).into_iter().map(|p| p.prefix).collect() };
        assert_eq!(
            prefixes("- a\n- b\n"),
            [Some("• ".into()), Some("• ".into())]
        );
        assert_eq!(
            prefixes("3. a\n4. b\n"),
            [Some("3. ".into()), Some("4. ".into())]
        );
    }

    #[test]
    fn a_footnote_stays_where_it_is_defined() {
        let (p, notes) = paragraphs(&parse("A[^x].\n\n[^x]: Note.\n\nB.\n"), &mut no_images);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].text.string, "Note.");
        assert_eq!(p[1].prefix.as_deref(), Some("1 "));
        assert_eq!(notes.0["x"], 1);
    }

    #[test]
    fn notes_are_numbered_by_first_reference_then_unreferenced() {
        let md = "A[^b] and [^a].\n\n[^a]: One.\n\n[^b]: Two.\n\n[^c]: Three.\n";
        let (_, notes) = paragraphs(&parse(md), &mut no_images);
        assert_eq!((notes.0["b"], notes.0["a"], notes.0["c"]), (1, 2, 3));
    }

    #[test]
    fn html_and_front_matter_are_not_set() {
        let p = paras("---\ntitle: x\n---\n\n<div>hi</div>\n\nText.\n");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text.string, "Text.");
    }

    #[test]
    fn a_table_cell_is_its_own_paragraph() {
        let p = paras("| a | b |\n|---|---|\n| c | d |\n");
        let cells: Vec<_> = p.iter().map(|p| p.text.string.as_str()).collect();
        assert_eq!(cells, ["a", "b", "c", "d"]);
    }

    #[test]
    fn code_and_rules_are_simple_paragraphs() {
        let p = paras("```\nx = 1\n```\n\n---\n");
        assert_eq!(p[0].role, Role::Code);
        assert_eq!(p[1].text, Text::default());
    }

    #[test]
    fn front_matter_gives_title_author_and_lang() {
        let md = "---\ntitle: \"On Liberty\"\nauthor: J. S. Mill\nlang: sv\n---\n\nText.\n";
        assert_eq!(
            front_matter(&parse(md), md),
            Front {
                title: Some("On Liberty".into()),
                author: Some("J. S. Mill".into()),
                lang: Some("sv".into())
            }
        );
    }

    #[test]
    fn front_matter_spans_are_characters() {
        let md = "---\ntitle: Åäö\nlanguage: sv\n---\n\nText.\n";
        let f = front_matter(&parse(md), md);
        assert_eq!(f.title.as_deref(), Some("Åäö"));
        assert_eq!(f.lang.as_deref(), Some("sv"));
    }

    #[test]
    fn lang_wins_over_language() {
        let md = "---\nlanguage: en\nlang: sv\n---\n\nText.\n";
        assert_eq!(front_matter(&parse(md), md).lang.as_deref(), Some("sv"));
    }

    #[test]
    fn front_matter_does_not_run_into_the_text_after_it() {
        // Many non-ASCII characters: a byte count would overshoot.
        let md = "---\ntitle: ÅÅÅÅÅÅÅÅÅÅÅÅ\n---\ntitle: Body\n";
        let f = front_matter(&parse(md), md);
        assert_eq!(f.title.as_deref(), Some("ÅÅÅÅÅÅÅÅÅÅÅÅ"));
    }

    #[test]
    fn indented_keys_are_not_read() {
        let md = "---\ntitle: T\nmeta:\n  title: Nested\n  lang: xx\n- author: A\n---\n\nText.\n";
        let f = front_matter(&parse(md), md);
        assert_eq!(f.title.as_deref(), Some("T"));
        assert_eq!(f.lang, None);
        assert_eq!(f.author, None);
    }

    #[test]
    fn nested_front_matter_is_ignored() {
        let md = "---\ntags:\n  - a\ntitle: T\n---\n\nText.\n";
        assert_eq!(front_matter(&parse(md), md).title.as_deref(), Some("T"));
    }

    /// Paragraphs of an XHTML body whose images all resolve, `side`
    /// points each way (the id is the image's number in the body).
    fn xhtml_paras(body: &str, side: f32) -> Vec<Para> {
        let xhtml = noido_blocks::xhtml::parse(
            format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head>\
                 <body>{body}</body></html>"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut next = 0;
        let mut resolve = |_: &str, _: Option<u32>, _: Option<u32>| {
            next += 1;
            Some((ImageId::test(next), (side, side)))
        };
        paragraphs(&xhtml.tree, &mut resolve).0
    }

    #[test]
    fn a_large_image_splits_its_paragraph_and_keeps_the_text_map() {
        let p = xhtml_paras("<p>Before <img src=\"a.png\" alt=\"a\"/> after.</p>", 100.0);
        assert_eq!(p.len(), 3, "{p:#?}");
        assert_eq!(p[0].text.string, "Before ");
        assert!(matches!(p[1].role, Role::Figure { heading: None, .. }));
        assert_eq!(p[1].text.string, "\u{fffc}");
        assert_eq!(p[2].text.string, " after.");
        assert_eq!(p[2].role, Role::Body { indent: false });
        // Source characters are the original's: "Before " is 0..7, and
        // " after." follows the image, which has no characters.
        assert_eq!(p[0].text.segments[0].source, 0..7);
        assert_eq!(p[2].text.segments[0].source, 7..14);
    }

    #[test]
    fn a_small_image_in_text_stays_in_its_paragraph() {
        let p = xhtml_paras("<p>Before <img src=\"a.png\" alt=\"a\"/> after.</p>", 10.0);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text.styles.len(), 1);
        assert_eq!(p[0].images, vec![(7..10, ImageId::test(1), (10.0, 10.0))]);
    }

    #[test]
    fn an_image_alone_is_a_figure_even_when_small() {
        let p = xhtml_paras(
            "<p><a href=\"#x\"><img src=\"a.png\" alt=\"a\"/></a></p>",
            10.0,
        );
        assert_eq!(p.len(), 1);
        assert!(matches!(p[0].role, Role::Figure { .. }));
        // The link that wraps it goes with it.
        let link = |s: &Styled| matches!(s.style, Style::Link { .. });
        assert!(p[0].text.styles.iter().any(link));
    }

    #[test]
    fn a_heading_holding_an_image_is_a_figure_that_keeps_its_rules() {
        let p = xhtml_paras("<h1><img src=\"a.png\" alt=\"Title\"/></h1>", 10.0);
        assert_eq!(p.len(), 1);
        assert!(matches!(
            p[0].role,
            Role::Figure {
                heading: Some(1),
                ..
            }
        ));
        assert!(p[0].new_page);
    }
}
