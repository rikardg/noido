//! XHTML (an EPUB chapter) into the block tree, with roxmltree. Offsets
//! are characters of the part: the NFC text content of `<body>`.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::ops::Range;

use roxmltree::{Document, Node, ParsingOptions};
use unicode_normalization::UnicodeNormalization;

use crate::entities::lookup;
use crate::{Align, Block, BlockKind, Cell, Item, Style, Styled, Text, Tree};

/// A chapter read into the block tree.
pub struct Xhtml {
    pub tree: Tree,
    /// Element id → the part character where its text begins, first
    /// occurrence wins, in document order.
    pub ids: Vec<(String, usize)>,
    /// The part's length in characters.
    pub chars: usize,
    /// The body, or a section directly in it, has `epub:type="cover"`.
    pub cover: bool,
    /// Some text node that would be drawn has a character that is not
    /// whitespace (`chars` counts whitespace too, for locations). Text
    /// in `svg` and `math` is counted but not drawn, so it does not
    /// count here.
    pub has_text: bool,
    /// An `<svg>` in the body holds an `<image>`.
    pub svg_image: bool,
}

/// A chapter file's bytes; `Err(())` when it isn't UTF-8 (a leading
/// BOM skipped) or isn't well-formed after entity replacement.
#[allow(clippy::result_unit_err)] // the only failure is "unreadable"
pub fn parse(bytes: &[u8]) -> Result<Xhtml, ()> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let source = std::str::from_utf8(bytes).map_err(|_| ())?;
    let source = replace_entities(source);
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(&source, options).map_err(|_| ())?;
    let mut reader = Reader {
        pos: 0,
        ids: Vec::new(),
        seen: HashSet::new(),
        has_text: false,
        svg_image: false,
    };
    let mut blocks = Vec::new();
    let mut cover = false;
    if let Some(body) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
    {
        cover = is_cover(body)
            || body
                .children()
                .any(|c| c.is_element() && c.tag_name().name() == "section" && is_cover(c));
        reader.note_id(body);
        reader.container(body, &mut blocks);
    }
    Ok(Xhtml {
        tree: Tree { blocks },
        ids: reader.ids,
        chars: reader.pos,
        cover,
        has_text: reader.has_text,
        svg_image: reader.svg_image,
    })
}

const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

/// Whether `node` has `epub:type` naming a cover.
fn is_cover(node: Node) -> bool {
    node.attribute((EPUB_NS, "type"))
        .is_some_and(|t| t.split_ascii_whitespace().any(|w| w == "cover"))
}

/// HTML's named entities, which XML doesn't know, as numeric references.
fn replace_entities(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        match lookup(&rest[..end]) {
            Some(c) if rest[end..].starts_with(';') => {
                let _ = write!(out, "&#{};", c as u32);
                rest = &rest[end + 1..];
            }
            _ => out.push('&'),
        }
    }
    out.push_str(rest);
    out
}

/// Elements whose text is not drawn.
fn skipped(name: &str) -> bool {
    matches!(name, "head" | "script" | "style" | "svg" | "math")
}

/// Elements whose text is not counted either: what is drawn later
/// (svg, math) must not shift the offsets after it.
fn uncounted(name: &str) -> bool {
    matches!(name, "head" | "script" | "style")
}

/// Elements read as the text of the block around them.
fn is_inline(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "big"
            | "br"
            | "cite"
            | "code"
            | "data"
            | "del"
            | "dfn"
            | "em"
            | "i"
            | "img"
            | "ins"
            | "kbd"
            | "label"
            | "mark"
            | "q"
            | "rp"
            | "rt"
            | "ruby"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strike"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "tt"
            | "u"
            | "var"
    )
}

/// Elements whose children are read as blocks, with a break around them.
fn flattens(name: &str) -> bool {
    matches!(
        name,
        "figure"
            | "figcaption"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "nav"
            | "main"
            | "div"
            | "body"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "address"
            | "details"
            | "summary"
            | "fieldset"
            | "form"
            | "center"
    )
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// The inline content of one block, as it is read.
#[derive(Default)]
struct Run {
    text: Text,
    /// Whitespace seen since the last drawn character: its first
    /// character, which a space is drawn for.
    pending: Option<Range<usize>>,
    /// Whitespace kept as written.
    raw: bool,
    /// The part characters the run covers.
    span: Option<Range<usize>>,
}

impl Run {
    fn raw() -> Self {
        Self {
            raw: true,
            ..Self::default()
        }
    }

    /// The run covers at least the position `at`.
    fn touch(&mut self, at: usize) {
        match &mut self.span {
            Some(span) => span.end = span.end.max(at),
            None => self.span = Some(at..at),
        }
    }

    /// Adds a text node's NFC text, which starts at part character `at`.
    fn add(&mut self, s: &str, at: usize) {
        let total = s.chars().count();
        match &mut self.span {
            Some(span) => span.end = at + total,
            None => self.span = Some(at..at + total),
        }
        if self.raw {
            self.text.push(s, at..at + total);
            return;
        }
        // The drawn piece being read: its byte and character start.
        let mut piece: Option<(usize, usize)> = None;
        for (i, (byte, c)) in s.char_indices().enumerate() {
            if is_space(c) {
                if let Some((b, from)) = piece.take() {
                    self.text.push(&s[b..byte], at + from..at + i);
                }
                if !self.text.string.is_empty()
                    && !self.text.string.ends_with('\n')
                    && self.pending.is_none()
                {
                    self.pending = Some(at + i..at + i + 1);
                }
            } else if piece.is_none() {
                if let Some(space) = self.pending.take() {
                    self.text.push(" ", space);
                }
                piece = Some((byte, i));
            }
        }
        if let Some((b, from)) = piece {
            self.text.push(&s[b..], at + from..at + total);
        }
    }
}

// Lookups `note_id` made among the ids so far, for the tests.
#[cfg(test)]
thread_local! {
    static ID_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Reader {
    /// The part characters read so far.
    pos: usize,
    /// Element ids and where their text begins, first occurrence wins.
    ids: Vec<(String, usize)>,
    /// The ids in `ids`, for the repeat check.
    seen: HashSet<String>,
    has_text: bool,
    svg_image: bool,
}

impl Reader {
    /// The blocks of `node`'s children.
    fn container(&mut self, node: Node, out: &mut Vec<Block>) {
        let mut run = Run::default();
        self.flow(node, &mut run, out);
        self.flush(&mut run, out);
    }

    fn flow(&mut self, node: Node, run: &mut Run, out: &mut Vec<Block>) {
        for child in node.children() {
            if child.is_text() {
                self.add_text(run, child.text().unwrap_or_default());
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let name = child.tag_name().name();
            if skipped(name) {
                self.count_child(child);
                continue;
            }
            if is_inline(name) {
                self.inline_element(child, run);
                continue;
            }
            self.note_id(child);
            if flattens(name) {
                self.flush(run, out);
                self.flow(child, run, out);
                self.flush(run, out);
            } else if let Some(kind) = self.block_name(name) {
                self.flush(run, out);
                let start = self.pos;
                if let Some(kind) = self.block(child, kind) {
                    out.push(Block {
                        kind,
                        span: start..self.pos,
                    });
                }
            } else {
                // An unknown element: its children, in the same flow.
                self.flow(child, run, out);
            }
        }
    }

    fn block_name<'a>(&self, name: &'a str) -> Option<&'a str> {
        matches!(
            name,
            "h1" | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "p"
                | "blockquote"
                | "ul"
                | "ol"
                | "pre"
                | "table"
                | "hr"
        )
        .then_some(name)
    }

    /// The block of a block element, its text counted.
    fn block(&mut self, node: Node, name: &str) -> Option<BlockKind> {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let text = self.leaf(node, Run::default())?;
                let level = name.as_bytes()[1] - b'0';
                Some(BlockKind::Heading { level, text })
            }
            "p" => self.leaf(node, Run::default()).map(BlockKind::Paragraph),
            "pre" => self
                .leaf(node, Run::raw())
                .map(|text| BlockKind::Code { info: None, text }),
            "blockquote" => {
                let mut blocks = Vec::new();
                self.container(node, &mut blocks);
                Some(BlockKind::Quote(blocks))
            }
            "ul" | "ol" => Some(self.list(node, name == "ol")),
            "table" => Some(self.table(node)),
            // "hr": nothing in it, but its (empty) text, if any, counts.
            _ => {
                self.count(node);
                Some(BlockKind::Rule)
            }
        }
    }

    /// A block's text; `None` when nothing is drawn.
    fn leaf(&mut self, node: Node, mut run: Run) -> Option<Text> {
        self.inline(node, &mut run);
        (!run.text.string.is_empty()).then_some(run.text)
    }

    fn list(&mut self, node: Node, ordered: bool) -> BlockKind {
        let start = ordered.then(|| {
            node.attribute("start")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1)
        });
        let mut items = Vec::new();
        for child in node.children() {
            if child.is_element() && child.tag_name().name() == "li" {
                self.note_id(child);
                let from = self.pos;
                let mut blocks = Vec::new();
                self.container(child, &mut blocks);
                items.push(Item {
                    span: from..self.pos,
                    task: None,
                    blocks,
                });
            } else {
                self.count_child(child);
            }
        }
        BlockKind::List { start, items }
    }

    fn table(&mut self, node: Node) -> BlockKind {
        // Each row, and whether it has a `th`.
        let mut rows: Vec<(Vec<Cell>, bool)> = Vec::new();
        self.rows(node, &mut rows);
        let mut rows = rows.into_iter();
        let mut head = Vec::new();
        let mut body = Vec::new();
        if let Some((first, has_th)) = rows.next() {
            if has_th {
                head = first;
            } else {
                body.push(first);
            }
        }
        body.extend(rows.map(|(cells, _)| cells));
        BlockKind::Table {
            align: vec![Align::None; head.len().max(body.iter().map(Vec::len).max().unwrap_or(0))],
            head,
            rows: body,
        }
    }

    fn rows(&mut self, node: Node, rows: &mut Vec<(Vec<Cell>, bool)>) {
        for child in node.children() {
            if !child.is_element() {
                self.count_child(child);
                continue;
            }
            self.note_id(child);
            match child.tag_name().name() {
                "tr" => {
                    let mut cells = Vec::new();
                    let mut has_th = false;
                    for cell in child.children() {
                        let name = cell.tag_name().name();
                        if cell.is_element() && matches!(name, "th" | "td") {
                            has_th |= name == "th";
                            self.note_id(cell);
                            let from = self.pos;
                            let mut run = Run::default();
                            self.inline(cell, &mut run);
                            cells.push(Cell {
                                span: from..self.pos,
                                text: run.text,
                            });
                        } else {
                            self.count_child(cell);
                        }
                    }
                    rows.push((cells, has_th));
                }
                "thead" | "tbody" | "tfoot" => self.rows(child, rows),
                _ => self.count_child(child),
            }
        }
    }

    /// Closes the run of loose inline content: a paragraph if it draws
    /// anything.
    fn flush(&mut self, run: &mut Run, out: &mut Vec<Block>) {
        let run = std::mem::take(run);
        if run.text.string.is_empty() {
            return;
        }
        out.push(Block {
            kind: BlockKind::Paragraph(run.text),
            span: run.span.unwrap_or(self.pos..self.pos),
        });
    }

    fn add_text(&mut self, run: &mut Run, s: &str) {
        let s: String = s.nfc().collect();
        self.has_text |= s.chars().any(|c| !c.is_whitespace());
        run.add(&s, self.pos);
        self.pos += s.chars().count();
    }

    /// `node`'s text, read into `run`.
    fn inline(&mut self, node: Node, run: &mut Run) {
        for child in node.children() {
            if child.is_text() {
                self.add_text(run, child.text().unwrap_or_default());
            } else if child.is_element() {
                if skipped(child.tag_name().name()) {
                    self.count_child(child);
                } else {
                    self.inline_element(child, run);
                }
            }
        }
    }

    /// Records `node`'s id (or an `a`'s legacy name) at the current
    /// position.
    fn note_id(&mut self, node: Node) {
        let id = node.attribute("id").or_else(|| {
            (node.tag_name().name() == "a")
                .then(|| node.attribute("name"))
                .flatten()
        });
        if let Some(id) = id
            && {
                #[cfg(test)]
                ID_LOOKUPS.with(|c| c.set(c.get() + 1));
                self.seen.insert(id.to_string())
            }
        {
            self.ids.push((id.to_string(), self.pos));
        }
    }

    /// An inline element: its id, its style over its text, `br` and
    /// `img` drawn where they stand.
    fn inline_element(&mut self, node: Node, run: &mut Run) {
        self.note_id(node);
        let at = self.pos;
        let name = node.tag_name().name();
        let style = match name {
            "br" => {
                run.pending = None;
                run.touch(at);
                run.text.push("\n", at..at);
                self.inline(node, run);
                return;
            }
            "img" => {
                run.touch(at);
                if let Some(space) = run.pending.take() {
                    run.text.push(" ", space);
                }
                let start = run.text.string.len();
                run.text.push("\u{fffc}", at..at);
                run.text.styles.push(Styled {
                    style: Style::Image {
                        src: node.attribute("src").unwrap_or_default().to_string(),
                        alt: node.attribute("alt").unwrap_or_default().to_string(),
                    },
                    drawn: start..run.text.string.len(),
                    span: at..at,
                });
                self.inline(node, run);
                return;
            }
            "i" | "em" | "cite" | "dfn" | "var" => Some(Style::Emphasis),
            "b" | "strong" => Some(Style::Strong),
            "s" | "strike" | "del" => Some(Style::Strikethrough),
            "code" | "kbd" | "samp" | "tt" => Some(Style::Code),
            "sup" => Some(Style::Superscript),
            "sub" => Some(Style::Subscript),
            "a" => node.attribute("href").map(|dest| Style::Link {
                dest: dest.to_string(),
            }),
            _ => None,
        };
        let Some(style) = style else {
            self.inline(node, run);
            return;
        };
        let had_pending = run.pending.is_some();
        let mut start = run.text.string.len();
        self.inline(node, run);
        // A space owed from before the element is drawn inside it, but
        // is not part of it.
        if had_pending && run.text.string.len() > start && run.pending.is_none() {
            start += 1;
        }
        let end = run.text.string.len();
        if end > start {
            run.text.styles.push(Styled {
                style,
                drawn: start..end,
                span: at..self.pos,
            });
        }
    }

    /// Counts the text under `node`'s children without drawing it.
    fn count(&mut self, node: Node) {
        for child in node.children() {
            self.count_child(child);
        }
    }

    fn count_child(&mut self, node: Node) {
        if node.is_text() {
            let text = node.text().unwrap_or_default();
            // Text in svg and math is counted, never drawn.
            let drawn = !node
                .ancestors()
                .any(|a| matches!(a.tag_name().name(), "svg" | "math"));
            self.has_text |= drawn && text.chars().any(|c| !c.is_whitespace());
            self.pos += text.nfc().count();
        } else if node.is_element() && !uncounted(node.tag_name().name()) {
            self.svg_image |= node.tag_name().name() == "image"
                && node.ancestors().any(|a| a.tag_name().name() == "svg");
            self.note_id(node);
            self.count(node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Align, Block, BlockKind, Style, Text};

    fn wrap(body: &str) -> Vec<u8> {
        format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head>\
             <body>{body}</body></html>"
        )
        .into_bytes()
    }

    fn read(body: &str) -> Xhtml {
        parse(&wrap(body)).expect("parses")
    }

    fn text(block: &Block) -> &Text {
        match &block.kind {
            BlockKind::Heading { text, .. }
            | BlockKind::Paragraph(text)
            | BlockKind::Code { text, .. } => text,
            other => panic!("no text in {other:?}"),
        }
    }

    #[test]
    fn paragraphs_and_headings_become_blocks() {
        let x = read("<h2>Title</h2><p>One.</p><p>Two.</p>");
        let b = &x.tree.blocks;
        assert!(matches!(b[0].kind, BlockKind::Heading { level: 2, .. }));
        assert!(matches!(b[1].kind, BlockKind::Paragraph(_)));
        assert!(matches!(b[2].kind, BlockKind::Paragraph(_)));
        let strings: Vec<_> = b.iter().map(|b| text(b).string.as_str()).collect();
        assert_eq!(strings, ["Title", "One.", "Two."]);
        assert_eq!(b[0].span, 0..5);
        assert_eq!(b[1].span, 5..9);
        assert_eq!(x.chars, 13);
    }

    #[test]
    fn whitespace_collapses_and_trims_at_block_edges() {
        let x = read("<p>\n  a   b\n\tc  </p>");
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "a b c");
        // Body text: "\n  a   b\n\tc  ".
        let sources: Vec<_> = t
            .string
            .char_indices()
            .map(|(i, _)| t.source_of(i))
            .collect();
        assert_eq!(sources, [3, 4, 7, 8, 10]);
        assert_eq!(x.chars, 13);
    }

    #[test]
    fn whitespace_between_blocks_makes_no_paragraph_but_counts() {
        let x = read("<p>a</p>\n   \n<p>b</p>");
        assert_eq!(x.tree.blocks.len(), 2);
        assert_eq!(x.chars, 7);
        assert_eq!(text(&x.tree.blocks[1]).source_of(0), 6);
    }

    #[test]
    fn containers_flatten_and_loose_text_is_its_own_paragraph() {
        let x = read("<div>loose <i>x</i><p>inner</p>tail</div>");
        let strings: Vec<_> = x
            .tree
            .blocks
            .iter()
            .map(|b| {
                assert!(matches!(b.kind, BlockKind::Paragraph(_)));
                text(b).string.as_str()
            })
            .collect();
        assert_eq!(strings, ["loose x", "inner", "tail"]);
        assert_eq!(x.tree.blocks[0].span, 0..7);
        assert_eq!(x.tree.blocks[2].span, 12..16);
    }

    #[test]
    fn blocks_by_element() {
        let x = read(
            "<blockquote><p>q</p></blockquote>\
             <ul><li>a</li><li>b</li></ul>\
             <ol start=\"3\"><li><p>c</p></li></ol>\
             <pre>  two  spaces\nnext</pre>\
             <table><thead><tr><th>H1</th><th>H2</th></tr></thead>\
             <tbody><tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></tbody></table>\
             <hr/>\
             <dl><dt>term</dt><dd>def</dd></dl>",
        );
        let b = &x.tree.blocks;
        let BlockKind::Quote(inner) = &b[0].kind else {
            panic!("{:?}", b[0])
        };
        assert_eq!(text(&inner[0]).string, "q");
        let BlockKind::List { start, items } = &b[1].kind else {
            panic!("{:?}", b[1])
        };
        assert_eq!(*start, None);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].task, None);
        assert_eq!(items[0].span, 1..2);
        assert_eq!(text(&items[1].blocks[0]).string, "b");
        let BlockKind::List { start, items } = &b[2].kind else {
            panic!("{:?}", b[2])
        };
        assert_eq!(*start, Some(3));
        assert_eq!(text(&items[0].blocks[0]).string, "c");
        let BlockKind::Code { info, text: t } = &b[3].kind else {
            panic!("{:?}", b[3])
        };
        assert_eq!(*info, None);
        assert_eq!(t.string, "  two  spaces\nnext");
        let BlockKind::Table { align, head, rows } = &b[4].kind else {
            panic!("{:?}", b[4])
        };
        assert!(align.iter().all(|a| *a == Align::None));
        let heads: Vec<_> = head.iter().map(|c| c.text.string.as_str()).collect();
        assert_eq!(heads, ["H1", "H2"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][0].text.string, "3");
        assert!(matches!(b[5].kind, BlockKind::Rule));
        assert!(b[5].span.is_empty());
        assert_eq!(text(&b[6]).string, "term");
        assert_eq!(text(&b[7]).string, "def");
        assert_eq!(b.len(), 8);
    }

    #[test]
    fn head_script_style_svg_math_are_not_read() {
        let x = read(
            "<p>a</p><style>p { color: red }</style><script>var x;</script>\
             <svg><text>s</text></svg><math><mi>m</mi></math><p>b</p>",
        );
        // svg and math text counts (a, s, m, b) but is not drawn.
        assert_eq!(x.tree.blocks.len(), 2);
        assert_eq!(x.chars, 4);
        assert_eq!(text(&x.tree.blocks[1]).source_of(0), 3);
    }

    #[test]
    fn entities_and_nfc() {
        let x = read("<p>a&nbsp;b&mdash;c e&#x301;</p>");
        assert_eq!(text(&x.tree.blocks[0]).string, "a\u{a0}b\u{2014}c \u{e9}");
        assert_eq!(x.chars, 7);
    }

    #[test]
    fn a_doctype_is_allowed() {
        let doc = "<?xml version=\"1.0\"?><!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \
                   \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">\
                   <html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>a&nbsp;b</p></body></html>";
        let x = parse(doc.as_bytes()).expect("parses");
        assert_eq!(text(&x.tree.blocks[0]).string, "a\u{a0}b");
    }

    #[test]
    fn a_decomposed_character_counts_once_before_what_follows() {
        let x = read("<p>e\u{301}x</p>");
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "\u{e9}x");
        assert_eq!(t.source_of(t.string.find('x').unwrap()), 1);
        assert_eq!(x.chars, 2);
    }

    #[test]
    fn a_bom_is_skipped_and_other_encodings_fail() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(wrap("<p>x</p>"));
        assert_eq!(parse(&bytes).expect("parses").chars, 1);
        let mut latin1 = wrap("<p>Caf?</p>");
        let at = latin1.iter().position(|&b| b == b'?').unwrap();
        latin1[at] = 0xE9;
        assert!(parse(&latin1).is_err());
        assert!(parse(b"<html><body><p>unclosed</body></html>").is_err());
    }

    fn styles_of(t: &Text) -> Vec<(Style, &str, Range<usize>)> {
        t.styles
            .iter()
            .map(|s| (s.style.clone(), &t.string[s.drawn.clone()], s.span.clone()))
            .collect()
    }

    #[test]
    fn inline_elements_become_styles() {
        let x = read(
            "<p><i>a</i><em>b</em><cite>c</cite><b>d</b><strong>e</strong><s>f</s><del>g</del>\
             <code>h</code><kbd>i</kbd><sup>j</sup><sub>k</sub><a href=\"x.html#y\">l</a>\
             <span>m</span><small>n</small><u>o</u><a>p</a></p>",
        );
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "abcdefghijklmnop");
        let want = [
            (Style::Emphasis, "a"),
            (Style::Emphasis, "b"),
            (Style::Emphasis, "c"),
            (Style::Strong, "d"),
            (Style::Strong, "e"),
            (Style::Strikethrough, "f"),
            (Style::Strikethrough, "g"),
            (Style::Code, "h"),
            (Style::Code, "i"),
            (Style::Superscript, "j"),
            (Style::Subscript, "k"),
            (
                Style::Link {
                    dest: "x.html#y".into(),
                },
                "l",
            ),
        ];
        let got = styles_of(t);
        assert_eq!(got.len(), want.len());
        for (i, ((style, drawn, span), (w, wd))) in got.iter().zip(want).enumerate() {
            assert_eq!((style, *drawn), (&w, wd));
            assert_eq!(*span, i..i + 1);
        }
    }

    #[test]
    fn a_style_does_not_take_the_space_before_it() {
        let x = read("<p>a <i>b</i> c</p>");
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "a b c");
        assert_eq!(styles_of(t), [(Style::Emphasis, "b", 2..3)]);
    }

    #[test]
    fn a_br_is_a_hard_break() {
        let x = read("<p>a<br/>b</p>");
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "a\nb");
        assert_eq!(t.source_of(1), 1);
        assert_eq!(t.source_of(2), 1);
        assert_eq!(x.chars, 2);
    }

    #[test]
    fn an_image_is_an_object_character() {
        let x = read("<p>x<img src=\"i/f1.png\" alt=\"f1\"/></p>");
        let t = text(&x.tree.blocks[0]);
        assert_eq!(t.string, "x\u{fffc}");
        assert_eq!(
            styles_of(t),
            [(
                Style::Image {
                    src: "i/f1.png".into(),
                    alt: "f1".into()
                },
                "\u{fffc}",
                1..1
            )]
        );
        assert_eq!(t.source_of(1), 1);
    }

    #[test]
    fn ids_record_where_their_text_begins() {
        let x = read("<h1 id=\"h\">Hi</h1>\n<p>Note<a id=\"r1\"/><sup>1</sup></p><a id=\"end\"/>");
        let want: Vec<(String, usize)> = vec![("h".into(), 0), ("r1".into(), 7), ("end".into(), 8)];
        assert_eq!(x.ids, want);
        assert_eq!(x.chars, 8);
    }

    #[test]
    fn ids_on_lists_and_tables_are_recorded() {
        let x = read(
            "<ul id=\"u\"><li id=\"l\">a</li></ul><table><tr id=\"r\"><td id=\"c\">b</td></tr></table>",
        );
        let ids: Vec<_> = x.ids.iter().map(|(i, p)| (i.as_str(), *p)).collect();
        assert_eq!(ids, [("u", 0), ("l", 0), ("r", 1), ("c", 1)]);
    }

    #[test]
    fn a_repeated_id_keeps_its_first_place() {
        let x = read("<p id=\"a\">x</p><p id=\"a\">y</p>");
        let want: Vec<(String, usize)> = vec![("a".into(), 0)];
        assert_eq!(x.ids, want);
    }

    #[test]
    fn a_legacy_a_name_is_an_id() {
        let x = read("<p>ab<a name=\"n\"/>c</p>");
        let want: Vec<(String, usize)> = vec![("n".into(), 2)];
        assert_eq!(x.ids, want);
    }

    #[test]
    fn whitespace_is_counted_but_is_not_text() {
        let x = read("\n  <div>\n  </div>\n");
        assert_eq!((x.chars, x.has_text), (7, false));
        assert!(read("\n<p>x</p>\n").has_text);
        // Text nobody draws counts for offsets but is not text.
        assert!(!read("<svg><text>t</text></svg>").has_text);
    }

    #[test]
    fn many_distinct_ids_take_a_linear_number_of_lookups() {
        const N: usize = 2000;
        let body: String = (0..N).map(|i| format!("<p id=\"i{i}\">a</p>")).collect();
        ID_LOOKUPS.with(|c| c.set(0));
        let x = read(&body);
        assert_eq!(x.ids.len(), N);
        assert_eq!(x.ids.last().map(|(i, _)| i.as_str()), Some("i1999"));
        assert!(ID_LOOKUPS.with(|c| c.get()) <= 2 * N);
    }

    #[test]
    fn an_svg_image_is_noted() {
        let svg = |inner: &str| {
            read(&format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\">{inner}</svg>"
            ))
        };
        assert!(svg("<image/>").svg_image);
        assert!(!svg("<rect/>").svg_image);
        assert!(!read("<p>x</p>").svg_image);
    }

    #[test]
    fn the_cover_mark() {
        let doc = |body: &str| {
            format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\" \
                 xmlns:epub=\"http://www.idpf.org/2007/ops\"><body{body}</body></html>"
            )
        };
        let cover = |body: &str| parse(doc(body).as_bytes()).expect("parses").cover;
        assert!(cover(" epub:type=\"cover\"><p>x</p>"));
        assert!(cover(
            "><section epub:type=\"frontmatter cover\"><p>x</p></section>"
        ));
        assert!(!cover("><p>x</p>"));
        assert!(!cover(" epub:type=\"bodymatter\"><p>x</p>"));
    }
}
