//! The whole pipeline on the sample: the page text and the location map
//! must agree character for character.

use std::path::PathBuf;

use noido_blocks::markdown::parse;
use noido_blocks::{Block, BlockKind, Style, Text};
use noido_engine::{LocationMap, Run};
use noido_text::{PageText, on_page};
use noido_typeset::typeset;
use unicode_normalization::UnicodeNormalization;

/// B-format, as the window sees it.
const PAGE: (f32, f32) = (365.67, 561.26);

/// A page's text as the window builds it: only the glyphs on the paper.
fn window_text(glyphs: Vec<noido_engine::Glyph>) -> PageText {
    PageText::new(on_page(glyphs, PAGE.0, PAGE.1))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn sample() -> String {
    std::fs::read_to_string(fixture("sample.md")).unwrap()
}

/// Every text of the tree, in source order.
fn texts<'a>(blocks: &'a [Block], out: &mut Vec<&'a Text>) {
    for block in blocks {
        match &block.kind {
            BlockKind::Heading { text, .. }
            | BlockKind::Paragraph(text)
            | BlockKind::Code { text, .. } => out.push(text),
            BlockKind::Quote(inner) | BlockKind::Footnote { blocks: inner, .. } => {
                texts(inner, out);
            }
            BlockKind::List { items, .. } => {
                for item in items {
                    texts(&item.blocks, out);
                }
            }
            BlockKind::Table { head, rows, .. } => {
                out.extend(head.iter().chain(rows.iter().flatten()).map(|c| &c.text));
            }
            _ => {}
        }
    }
}

/// The runs of `page`, each with its offset among the page's characters.
fn placed_runs(map: &LocationMap, page: usize, chars: usize) -> Vec<(usize, Run)> {
    let mut out: Vec<(usize, Run)> = Vec::new();
    for c in 0..chars {
        let Some(at) = map.location(page, c) else {
            break;
        };
        if map.page_char(at) != Some((page, c)) {
            continue; // a character without a source
        }
        let Some((p, run)) = map.line(at) else {
            continue;
        };
        let inside = at.char - run.start.char;
        if p != page || inside >= run.chars {
            continue;
        }
        let offset = c - inside;
        if out.last().is_none_or(|(o, _)| *o != offset) {
            out.push((offset, run));
        }
    }
    out
}

/// What the page should show for `run`: the drawn characters of its
/// source, or the stand-in drawn in their place. None: not checked
/// (a footnote number, which the notes numbering decides).
fn drawn_for(texts: &[&Text], run: &Run) -> Option<String> {
    let text = texts
        .iter()
        .find(|t| {
            t.segments
                .iter()
                .any(|s| s.source.start <= run.start.char && run.start.char < s.source.end)
        })
        .expect("a run starts in some text");
    let drawn = text
        .drawn_of(run.start.char)
        .expect("a run starts on a drawn character");
    // The text as shown from `drawn` on: stand-ins in place of what they
    // stand for. A footnote's number is not checked.
    let mut shown = String::new();
    let mut at = drawn;
    while at < text.string.len() && shown.chars().count() < run.chars + 2 {
        match text.styles.iter().find(|s| s.drawn.contains(&at)) {
            Some(s) => {
                match &s.style {
                    Style::FootnoteRef { .. } if shown.chars().count() < run.chars => return None,
                    Style::FootnoteRef { .. } => break,
                    Style::Image { alt, .. } if alt.is_empty() => shown.push_str("[image]"),
                    Style::Image { alt, .. } => shown.push_str(alt),
                    Style::TaskBox { checked } => {
                        shown.push(if *checked { '\u{2611}' } else { '\u{2610}' });
                    }
                    _ => shown.push_str(&text.string[at..s.drawn.end]),
                }
                at = s.drawn.end;
            }
            None => {
                let c = text.string[at..].chars().next().unwrap();
                shown.push(c);
                at += c.len_utf8();
            }
        }
    }
    // Page text is NFC; a composed accent is one character there.
    Some(shown.nfc().take(run.chars).collect())
}

fn page_text_and_runs_agree_on(source: &str, at_least: usize) -> usize {
    let book = typeset(source);
    let map = book.locations();
    let tree = parse(source);
    let mut all = Vec::new();
    texts(&tree.blocks, &mut all);
    let mut checked = 0;
    for page in 0..book.page_count() {
        let text = window_text(book.glyphs(page));
        let chars = text.char_count();
        let runs = placed_runs(&map, page, chars);
        assert!(
            !runs.is_empty() || text.text().chars().all(char::is_whitespace),
            "page {page} has text but no runs"
        );
        for (offset, run) in runs {
            let on_page: String = text.text().chars().skip(offset).take(run.chars).collect();
            let Some(from_source) = drawn_for(&all, &run) else {
                continue;
            };
            assert_eq!(on_page, from_source, "page {page}, run {run:?}");
            checked += 1;
        }
    }
    assert!(checked >= at_least, "only {checked} runs checked");
    book.page_count()
}

#[test]
fn page_text_and_runs_agree() {
    page_text_and_runs_agree_on(&sample(), 20);
}

#[test]
fn page_text_and_runs_agree_with_crlf_and_bom() {
    let source = format!("\u{feff}{}", sample().replace('\n', "\r\n"));
    page_text_and_runs_agree_on(&source, 20);
}

#[test]
fn an_empty_file_is_one_blank_page() {
    let book = typeset("");
    assert_eq!(book.page_count(), 1);
    assert!(book.glyphs(0).is_empty());
}

#[test]
fn front_matter_alone_is_one_blank_page() {
    assert_eq!(typeset("---\ntitle: x\n---\n").page_count(), 1);
}

#[test]
fn locations_come_in_source_order() {
    let book = typeset(&sample());
    let map = book.locations();
    let mut last = None;
    for page in 0..book.page_count() {
        let chars = window_text(book.glyphs(page)).char_count();
        for (_, run) in placed_runs(&map, page, chars) {
            assert!(last.is_none_or(|l| l < run.start), "{run:?} after {last:?}");
            last = Some(run.start);
        }
    }
    assert!(last.is_some());
}

#[test]
fn awkward_text_keeps_page_text_and_runs_together() {
    let long = "word ".repeat(1500);
    let source = format!(
        "# Title\n\ncaf\u{65}\u{301} naive \u{1F600} \u{4E2D}\u{6587} \u{fb01}ne\n\n\
         http://example.org/{}\n\n{long}\n\n## Two\n\nA &amp; B -- \"c\" `code` end  \nnext\n",
        "x".repeat(400)
    );
    let pages = page_text_and_runs_agree_on(&source, 50);
    assert!(pages > 2, "{pages} pages");
}

#[test]
fn spacing_diacritics_and_stray_marks_keep_page_text_and_runs_together() {
    let source = "a^b `c` \u{b4}d\u{a8}e \u{2dc}f and a \u{301}b\n\n\u{301}start ^ end\n\n\
                  ^caret\n";
    let book = typeset(source);
    let text = PageText::new(book.glyphs(0));
    assert!(text.char_count() > 20);
    page_text_and_runs_agree_on(source, 5);
}

/// Every glyph lies on the paper, and no non-blank character is lost.
fn sets_inside_the_paper(source: &str, expected: &str) {
    let book = typeset(source);
    let mut shown = String::new();
    for page in 0..book.page_count() {
        let glyphs = book.glyphs(page);
        for g in &glyphs {
            for c in g.quad.corners {
                assert!(
                    (0.0..=PAGE.0).contains(&c.x),
                    "page {page}: {:?} at x {}",
                    g.text,
                    c.x
                );
            }
        }
        let text = PageText::new(glyphs);
        // Break marks (soft hyphens) show nothing.
        shown.extend(
            text.text()
                .chars()
                .filter(|&c| !c.is_whitespace() && c != '\u{ad}'),
        );
    }
    assert_eq!(shown, expected);
    page_text_and_runs_agree_on(source, 1);
}

#[test]
fn long_inline_code_stays_on_the_paper() {
    let code = "a_b".repeat(40);
    sets_inside_the_paper(
        &format!("Before `{code}` after.\n"),
        &format!("Before{code}after."),
    );
}

#[test]
fn a_long_url_stays_on_the_paper() {
    let url = format!("http://example.org/{}", "x".repeat(400));
    sets_inside_the_paper(&format!("See {url} now.\n"), &format!("See{url}now."));
}

#[test]
fn a_cjk_paragraph_stays_on_the_paper() {
    let cjk: String = "\u{4E2D}\u{6587}\u{5B57}".repeat(70);
    sets_inside_the_paper(&format!("{cjk}\n"), &cjk);
}

/// What copying every page gives, the pages read as the window reads a
/// reflowing document.
fn copied(source: &str) -> String {
    let book = typeset(source);
    let pages: Vec<PageText> = (0..book.page_count())
        .map(|page| PageText::with_marked_hyphens(on_page(book.glyphs(page), PAGE.0, PAGE.1)))
        .collect();
    let pieces: Vec<(&PageText, std::ops::Range<usize>)> =
        pages.iter().map(|p| (p, 0..p.char_count())).collect();
    noido_text::copy_text(&pieces)
}

#[test]
fn a_word_broken_for_want_of_room_copies_whole() {
    let url = format!("http://example.org/{}", "x/".repeat(200));
    assert_eq!(
        copied(&format!("See {url} now.\n")),
        format!("See {url} now.")
    );
    let code = "a_b".repeat(40);
    assert_eq!(
        copied(&format!("Before `{code}` after.\n")),
        format!("Before {code} after.")
    );
    let cjk: String = "\u{4E2D}\u{6587}\u{5B57}".repeat(70);
    assert_eq!(copied(&format!("{cjk}\n")), cjk);
}

#[test]
fn a_wrapped_code_line_copies_whole() {
    let line = "let x = ".to_string() + &"a_b + ".repeat(30) + "c;";
    assert_eq!(copied(&format!("```\n{line}\n```\n")), line);
}
