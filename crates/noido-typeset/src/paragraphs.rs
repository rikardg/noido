//! The block tree as a flat run of paragraphs, in source order. Blocks
//! Y3b will set properly (lists, tables, quotes, notes) get simple
//! stand-ins here.

use std::collections::HashMap;

use noido_blocks::{Block, BlockKind, Style, Text, Tree};

/// How a paragraph is set.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Role {
    Body { indent: bool },
    Heading(u8),
    Code,
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
}

/// Footnote label to its number: the order of its first reference.
/// Notes never referenced follow, in definition order.
pub(crate) struct Notes(pub HashMap<String, usize>);

pub(crate) fn paragraphs(tree: &Tree) -> (Vec<Para>, Notes) {
    let notes = number_notes(&tree.blocks);
    let mut out = Out {
        paras: Vec::new(),
        notes: &notes,
        indent: false,
    };
    out.blocks(&tree.blocks);
    (out.paras, notes)
}

struct Out<'a> {
    paras: Vec<Para>,
    notes: &'a Notes,
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
        self.paras.push(Para {
            role: Role::Body {
                indent: self.indent,
            },
            text: text.clone(),
            prefix: None,
            new_page: false,
        });
        self.indent = true;
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
                self.paras.push(Para {
                    role: Role::Heading(*level),
                    text: text.clone(),
                    prefix: None,
                    new_page: *level == 1,
                });
                self.indent = false;
            }
            BlockKind::Paragraph(text) => self.body(text),
            BlockKind::Code { text, .. } => {
                self.paras.push(Para {
                    role: Role::Code,
                    text: text.clone(),
                    prefix: None,
                    new_page: false,
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
                match self.notes.0.get(label).map(|n| format!("{n} ")) {
                    Some(prefix) => self.with_prefix(blocks, prefix),
                    None => self.blocks(blocks),
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
        paragraphs(&parse(md)).0
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
        let (_, notes) = paragraphs(&parse(md));
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
        let (p, notes) = paragraphs(&parse("A[^x].\n\n[^x]: Note.\n\nB.\n"));
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].text.string, "Note.");
        assert_eq!(p[1].prefix.as_deref(), Some("1 "));
        assert_eq!(notes.0["x"], 1);
    }

    #[test]
    fn notes_are_numbered_by_first_reference_then_unreferenced() {
        let md = "A[^b] and [^a].\n\n[^a]: One.\n\n[^b]: Two.\n\n[^c]: Three.\n";
        let (_, notes) = paragraphs(&parse(md));
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
}
