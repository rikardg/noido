//! The sample markdown fixture, checked as a whole: every drawn
//! character maps into the source, and exactly the text is reached,
//! never a marker.

use std::collections::BTreeSet;
use std::ops::Range;

use noido_blocks::{Block, BlockKind, Text, Tree, markdown};

fn source() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/sample.md");
    std::fs::read_to_string(path).expect("run fixtures/make_fixtures.py")
}

/// Every text in the tree, with the span of the block holding it.
fn texts(tree: &Tree) -> Vec<(&Text, Range<usize>)> {
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<(&'a Text, Range<usize>)>) {
        for block in blocks {
            let span = block.span.clone();
            match &block.kind {
                BlockKind::Heading { text, .. }
                | BlockKind::Paragraph(text)
                | BlockKind::Code { text, .. } => out.push((text, span)),
                BlockKind::Quote(blocks) | BlockKind::Footnote { blocks, .. } => walk(blocks, out),
                BlockKind::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out)
                    }
                }
                BlockKind::Table { head, rows, .. } => {
                    for cell in head.iter().chain(rows.iter().flatten()) {
                        out.push((&cell.text, cell.span.clone()))
                    }
                }
                BlockKind::Rule | BlockKind::Html | BlockKind::FrontMatter => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree.blocks, &mut out);
    out
}

/// Where segment `i` of `text` ends in its string.
fn drawn_end(text: &Text, i: usize) -> usize {
    text.segments
        .get(i + 1)
        .map_or(text.string.len(), |next| next.drawn)
}

#[test]
fn every_drawn_character_maps_into_the_source() {
    let source = source();
    let count = source.chars().count();
    let tree = markdown::parse(&source);
    for (text, _) in texts(&tree) {
        assert_eq!(text.segments.first().map(|s| s.drawn), Some(0), "{text:?}");
        for pair in text.segments.windows(2) {
            assert!(pair[0].drawn < pair[1].drawn, "{text:?}");
            assert!(pair[0].source.end <= pair[1].source.start, "{text:?}");
        }
        for (byte, _) in text.string.char_indices() {
            assert!(text.source_of(byte) < count, "{text:?}");
        }
        assert!(text.source_of(text.string.len()) <= count);
    }
}

#[test]
fn one_to_one_segments_draw_their_source() {
    let source: Vec<char> = source().chars().collect();
    let tree = markdown::parse(&source.iter().collect::<String>());
    for (text, _) in texts(&tree) {
        for (i, segment) in text.segments.iter().enumerate() {
            let drawn: Vec<char> = text.string[segment.drawn..drawn_end(text, i)]
                .chars()
                .collect();
            if drawn.len() != segment.source.len() {
                continue;
            }
            for (d, s) in drawn.iter().zip(&source[segment.source.clone()]) {
                let same = d == s
                    || (*d == ' ' && *s == '\n')
                    || (matches!(d, '“' | '”') && *s == '"')
                    || (matches!(d, '‘' | '’') && *s == '\'');
                assert!(same, "{d:?} drawn for {s:?} in {text:?}");
            }
        }
    }
}

/// The source's text, as opposed to its markers, in order: what drawn
/// text may reach. Written from the spec's list of markers, not from the
/// parser. A construct drawn as one character (an entity, `--`, a hard
/// break, an image, a task box) reaches all of its source.
const TEXT: &[&str] = &[
    "Café ",
    "naïve",
    " heading",
    "Setext heading",
    "Plain ",
    "strong",
    " and ",
    "struck",
    " text, a ",
    "code span",
    ", a ",
    "link",
    ",\nan image ",
    "![alt text](pic.png)",
    ", an escaped ",
    "*star",
    "*, an entity &amp; \"quotes\" -- a dash.\nA hard break  \nand another\\\nline.",
    " A footnote",
    "note",
    ".",
    "[x] done",
    "[ ] todo",
    "three",
    "four",
    "Quoted",
    "nested",
    "fn main() {}\n",
    "indented code\n",
    "Left",
    "Centre",
    "a",
    "b",
    "The note.",
];

#[test]
fn exactly_the_text_is_reached() {
    let source = source();
    let mut expected = BTreeSet::new();
    let mut cursor = 0;
    for snippet in TEXT {
        let at = cursor
            + source[cursor..]
                .find(snippet)
                .unwrap_or_else(|| panic!("{snippet:?} not after byte {cursor}"));
        let start = source[..at].chars().count();
        expected.extend(start..start + snippet.chars().count());
        cursor = at + snippet.len();
    }
    let tree = markdown::parse(&source);
    let reached: BTreeSet<usize> = texts(&tree)
        .iter()
        .flat_map(|(text, _)| text.segments.iter().flat_map(|s| s.source.clone()))
        .collect();
    let chars: Vec<char> = source.chars().collect();
    let show = |set: &BTreeSet<usize>| -> String { set.iter().map(|&i| chars[i]).collect() };
    let missing: BTreeSet<_> = expected.difference(&reached).copied().collect();
    let extra: BTreeSet<_> = reached.difference(&expected).copied().collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "not reached: {:?}\nreached but markers: {:?}",
        show(&missing),
        show(&extra)
    );
}

#[test]
fn spans_cover_their_markers() {
    let source = source();
    let chars: Vec<char> = source.chars().collect();
    let tree = markdown::parse(&source);
    let atx = tree
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .unwrap();
    assert_eq!(chars[atx.span.start], '#');
    for (text, block) in texts(&tree) {
        for segment in &text.segments {
            assert!(
                block.start <= segment.source.start && segment.source.end <= block.end,
                "{segment:?} outside {block:?}"
            );
        }
        for styled in &text.styles {
            let first = chars[styled.span.start];
            let last = chars[styled.span.end - 1];
            assert!(
                matches!(first, '*' | '~' | '`' | '[' | '!'),
                "{styled:?} starts at {first:?}"
            );
            assert!(
                matches!(last, '*' | '~' | '`' | ')' | ']'),
                "{styled:?} ends at {last:?}"
            );
        }
    }
}
