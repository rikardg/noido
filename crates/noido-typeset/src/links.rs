//! What a typeset book offers besides glyphs: links, named destinations,
//! the outline and the properties.

use std::collections::{HashMap, HashSet};

use noido_engine::{
    Destination, Format, Link, LinkTarget, Location, LocationMap, OutlineEntry, Properties, Rect,
};
use noido_epub::{Epub, PartKind, TocEntry};

use crate::book::{Book, SetPart};
use crate::house::LEFT;
use crate::lines::SetLine;
use crate::pages::Placed;
use crate::paragraphs::{Front, Para, Role};

/// Heading slugs by GitHub's rules: lowercase, punctuation dropped
/// (but '-' and '_' kept), spaces to '-', repeats numbered from 1.
pub(crate) struct Slugs {
    seen: HashSet<String>,
    repeats: HashMap<String, usize>,
}

impl Slugs {
    pub fn new() -> Self {
        Slugs {
            seen: HashSet::new(),
            repeats: HashMap::new(),
        }
    }

    pub fn slug(&mut self, heading: &str) -> String {
        let base: String = heading
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                '-' | '_' => Some(c),
                c if c.is_alphanumeric() => Some(c),
                _ => None,
            })
            .collect();
        let mut slug = base.clone();
        while !self.seen.insert(slug.clone()) {
            let n = self.repeats.entry(base.clone()).or_insert(0);
            *n += 1;
            slug = format!("{base}-{n}");
        }
        slug
    }
}

/// A link as the lines have it: the destination as written, its areas,
/// and the paragraph and line it last reached.
struct RawLink {
    dest: String,
    areas: Vec<Rect>,
    flow: usize,
    last: usize,
}

/// What the book keeps for links and the outline.
pub(crate) struct Marks {
    /// Per page, its links.
    links: Vec<Vec<Link>>,
    /// Heading slugs.
    names: HashMap<String, Destination>,
    /// Footnotes by their (lowercase) label.
    notes: HashMap<String, Destination>,
    /// Level, text and place of each heading with text.
    headings: Vec<(u8, String, Destination)>,
    title: Option<String>,
    author: Option<String>,
    format: Format,
    subject: Option<String>,
    /// An EPUB's outline, from its table of contents.
    outline: Option<Vec<OutlineEntry>>,
}

/// Adds the links of the lines in `set` (those of `paras`, placed as
/// `placed` says) to `raw`, by page; `first_line` sees each paragraph's
/// first line.
fn gather(
    paras: &[Para],
    set: &[Vec<SetLine>],
    placed: &[Placed],
    raw: &mut [Vec<RawLink>],
    mut first_line: impl FnMut(&Placed, &Para),
) {
    for at in placed {
        let (para, line) = (&paras[at.flow], &set[at.flow][at.line]);
        if at.line == 0 {
            first_line(at, para);
        }
        for (dest, x0, x1) in &line.links {
            let area = Rect {
                x: LEFT + x0,
                y: at.y,
                width: x1 - x0,
                height: line.height,
            };
            let page = &mut raw[at.page];
            // The same link carried on from the line before.
            match page.last_mut() {
                Some(l) if l.dest == *dest && l.flow == at.flow && l.last + 1 == at.line => {
                    l.areas.push(area);
                    l.last = at.line;
                }
                _ => page.push(RawLink {
                    dest: dest.clone(),
                    areas: vec![area],
                    flow: at.flow,
                    last: at.line,
                }),
            }
        }
    }
}

/// Gathers the marks of a typeset book: `placed` says where each line of
/// `set` (the lines of `paras`) went.
pub(crate) fn marks(
    paras: &[Para],
    set: &[Vec<SetLine>],
    placed: &[Placed],
    pages: usize,
    front: &Front,
) -> Marks {
    let mut slugs = Slugs::new();
    let mut names = HashMap::new();
    let mut notes = HashMap::new();
    let mut headings = Vec::new();
    let mut raw: Vec<Vec<RawLink>> = (0..pages).map(|_| Vec::new()).collect();
    gather(paras, set, placed, &mut raw, |at, para| {
        let here = Destination {
            page: at.page,
            x: None,
            y: Some(at.y),
        };
        if let Role::Heading(level) = para.role {
            let title = para.text.string.trim().to_string();
            names.entry(slugs.slug(&title)).or_insert(here);
            if !title.is_empty() {
                headings.push((level, title, here));
            }
        }
        if let Some(label) = &para.note {
            notes.entry(label.clone()).or_insert(here);
        }
    });
    let links = raw
        .into_iter()
        .map(|page| {
            page.into_iter()
                .filter_map(|l| {
                    let target = match l.dest.strip_prefix('#') {
                        // A footnote reference goes to its note.
                        Some(name) if name.starts_with('^') => {
                            LinkTarget::Internal(*notes.get(&name[1..])?)
                        }
                        Some(name) => LinkTarget::Internal(find(&names, &notes, name)?),
                        None => LinkTarget::Web(l.dest),
                    };
                    Some(Link {
                        areas: l.areas,
                        target,
                    })
                })
                .collect()
        })
        .collect();
    let first_heading = headings.first().map(|h| h.1.clone());
    Marks {
        links,
        names,
        notes,
        headings,
        title: front.title.clone().or(first_heading),
        author: front.author.clone(),
        format: Format::Markdown,
        subject: None,
        outline: None,
    }
}

/// The marks of an EPUB set as `parts`, over `pages` pages: its links
/// and outline go where `map` puts the ids they name.
pub(crate) fn epub_marks(epub: &Epub, parts: &[SetPart], map: &LocationMap, pages: usize) -> Marks {
    // Each part's links, by page, with the part they were written in.
    let mut links: Vec<Vec<Link>> = (0..pages).map(|_| Vec::new()).collect();
    for (number, part) in parts.iter().enumerate() {
        let mut raw: Vec<Vec<RawLink>> = (0..pages).map(|_| Vec::new()).collect();
        gather(&part.paras, &part.set, &part.placed, &mut raw, |_, _| {});
        for (page, found) in raw.into_iter().enumerate() {
            for l in found {
                let target = if has_scheme(&l.dest) {
                    LinkTarget::Web(l.dest)
                } else {
                    let from = &epub.parts[number].path;
                    let href = noido_epub::resolve(from, &l.dest);
                    match locate(epub, map, &href) {
                        Some(at) => LinkTarget::Internal(at),
                        None => continue,
                    }
                };
                links[page].push(Link {
                    areas: l.areas,
                    target,
                });
            }
        }
    }
    Marks {
        links,
        names: HashMap::new(),
        notes: HashMap::new(),
        headings: Vec::new(),
        title: epub.meta.title.clone(),
        author: Some(epub.meta.creators.join(", ")).filter(|a| !a.is_empty()),
        format: Format::Epub,
        subject: epub.meta.subject.clone(),
        outline: Some(outline_of(epub, map, &epub.toc)),
    }
}

/// Whether a dest starts with a URI scheme (`https:`, `mailto:`).
fn has_scheme(dest: &str) -> bool {
    let Some((scheme, _)) = dest.split_once(':') else {
        return false;
    };
    scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The page and line top of `path#id` (a container path, as
/// `noido_epub::resolve` gives): the start of the part, or where the id's
/// text begins. An id the part lacks is the part's start; one past its
/// last drawn line (trailing whitespace, undrawn text) is its last line.
/// None when no part has the path or the part draws nothing.
fn locate(epub: &Epub, map: &LocationMap, href: &str) -> Option<Destination> {
    let (path, id) = href.split_once('#').unwrap_or((href, ""));
    let part = epub
        .parts
        .iter()
        .position(|p| !p.path.is_empty() && p.path == path)?;
    let char = match (&epub.parts[part].kind, id) {
        (_, "") | (PartKind::Cover | PartKind::Unreadable, _) => 0,
        (PartKind::Text(x), id) => x.ids.iter().find(|(i, _)| i == id).map_or(0, |(_, c)| *c),
    };
    let (page, run) = map
        .line(Location { part, char })
        .or_else(|| map.last_line_of(part))?;
    Some(Destination {
        page,
        x: None,
        y: Some(run.top),
    })
}

/// The table of contents as outline entries; one that doesn't resolve
/// stays without a target. An entry without a title is left out, its
/// children taking its place.
fn outline_of(epub: &Epub, map: &LocationMap, entries: &[TocEntry]) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let children = outline_of(epub, map, &entry.children);
        if entry.title.trim().is_empty() {
            out.extend(children);
            continue;
        }
        let target = (!entry.href.is_empty())
            .then(|| locate(epub, map, &entry.href))
            .flatten()
            .map(LinkTarget::Internal);
        out.push(OutlineEntry {
            title: entry.title.clone(),
            target,
            open: true,
            children,
        });
    }
    out
}

impl Book {
    pub fn links(&self, page: usize) -> Vec<Link> {
        self.marks.links[page].clone()
    }

    /// A heading by its slug, else a footnote by its label (a heading
    /// wins a name both have).
    pub fn named_destination(&self, name: &str) -> Option<Destination> {
        find(&self.marks.names, &self.marks.notes, name)
    }

    /// The headings, nested by level; all open.
    pub fn outline(&self) -> Vec<OutlineEntry> {
        if let Some(outline) = &self.marks.outline {
            return outline.clone();
        }
        // The open entries, shallowest first, with their levels.
        let mut path: Vec<(u8, OutlineEntry)> = Vec::new();
        let mut top = Vec::new();
        for (level, title, at) in &self.marks.headings {
            while path.last().is_some_and(|(l, _)| l >= level) {
                close(&mut path, &mut top);
            }
            path.push((
                *level,
                OutlineEntry {
                    title: title.clone(),
                    target: Some(LinkTarget::Internal(*at)),
                    open: true,
                    children: Vec::new(),
                },
            ));
        }
        while !path.is_empty() {
            close(&mut path, &mut top);
        }
        top
    }

    pub fn properties(&self) -> Properties {
        Properties {
            format: self.marks.format,
            title: self.marks.title.clone(),
            author: self.marks.author.clone(),
            subject: self.marks.subject.clone(),
            has_outline: match &self.marks.outline {
                Some(outline) => !outline.is_empty(),
                None => !self.marks.headings.is_empty(),
            },
            ..Properties::default()
        }
    }
}

fn find(
    names: &HashMap<String, Destination>,
    notes: &HashMap<String, Destination>,
    name: &str,
) -> Option<Destination> {
    names
        .get(name)
        .or_else(|| notes.get(&name.to_lowercase()))
        .copied()
}

/// Moves the deepest open entry into its parent, or into `top`.
fn close(path: &mut Vec<(u8, OutlineEntry)>, top: &mut Vec<OutlineEntry>) {
    let Some((_, entry)) = path.pop() else { return };
    match path.last_mut() {
        Some((_, parent)) => parent.children.push(entry),
        None => top.push(entry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::typeset;
    use crate::house::{LEFT, TOP};
    use noido_engine::{Format, LinkTarget};

    #[test]
    fn slugs_follow_github() {
        let mut slugs = Slugs::new();
        assert_eq!(slugs.slug("Hello, World!"), "hello-world");
        assert_eq!(slugs.slug("Hello, World!"), "hello-world-1");
        assert_eq!(slugs.slug("Hello, World!"), "hello-world-2");
        assert_eq!(slugs.slug("Snake_case and-dash"), "snake_case-and-dash");
        assert_eq!(slugs.slug("Ünïcode Ärende"), "ünïcode-ärende");
    }

    fn internal(link: &Link) -> Destination {
        match &link.target {
            LinkTarget::Internal(d) => *d,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_internal_link_goes_to_its_heading() {
        let book = typeset("# A\n\n[go](#b)\n\n# B\n");
        let links = book.links(0);
        assert_eq!(links.len(), 1, "{links:?}");
        let d = internal(&links[0]);
        assert_eq!((d.page, d.x, d.y), (1, None, Some(TOP)));
        assert_eq!(book.named_destination("b"), Some(d));
        assert!(book.links(1).is_empty());
        let area = links[0].areas[0];
        assert!(area.x >= LEFT && area.width > 0.0 && area.height > 0.0);
    }

    #[test]
    fn a_link_to_nowhere_is_dropped() {
        let book = typeset("[go](#nope)\n");
        assert!(book.links(0).is_empty());
        assert_eq!(book.named_destination("nope"), None);
    }

    #[test]
    fn a_web_link_is_kept_as_written() {
        let book = typeset("See [it](http://x.org/a?b=1).\n");
        let links = book.links(0);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target,
            LinkTarget::Web("http://x.org/a?b=1".to_string())
        );
    }

    #[test]
    fn a_footnote_reference_links_to_its_note() {
        let book = typeset("Text[^N]\n\n[^n]: The note.\n");
        let links = book.links(0);
        assert_eq!(links.len(), 1, "{links:?}");
        let d = internal(&links[0]);
        assert_eq!(d.page, 0);
        assert_eq!(d.x, None);
        assert!(d.y.unwrap() > TOP, "{d:?}");
        assert_eq!(book.named_destination("n"), Some(d));
        assert_eq!(book.named_destination("N"), Some(d));
    }

    #[test]
    fn a_link_by_footnote_label_goes_to_the_note() {
        let book = typeset("Text [see](#n).\n\n[^n]: The note.\n");
        let links = book.links(0);
        assert_eq!(links.len(), 1, "{links:?}");
        let d = internal(&links[0]);
        assert!(d.y.unwrap() > TOP, "{d:?}");
        assert_eq!(book.named_destination("n"), Some(d));
    }

    #[test]
    fn a_heading_wins_over_a_footnote_of_the_same_name() {
        let book = typeset("# N\n\nText[^n] [see](#n)\n\n[^n]: The note.\n");
        let heading = book.named_destination("n").unwrap();
        assert_eq!((heading.page, heading.y), (0, Some(TOP)));
        let links = book.links(0);
        assert_eq!(links.len(), 2, "{links:?}");
        // The reference goes to the note, the written link to the heading.
        let note = internal(&links[0]);
        assert!(note.y.unwrap() > TOP, "{note:?}");
        assert_eq!(internal(&links[1]), heading);
    }

    #[test]
    fn a_link_area_covers_its_glyphs() {
        let book = typeset("Some [linked words](http://x.org) here.\n");
        let area = book.links(0)[0].areas[0];
        assert_eq!(area.y, TOP);
        let glyphs = book.glyphs(0);
        let at = glyphs.iter().position(|g| g.text == "l").unwrap();
        let last = glyphs.iter().position(|g| g.text == "s").unwrap();
        let (x0, x1) = (glyphs[at].quad.bounds().0.x, glyphs[last].quad.bounds().1.x);
        assert!((area.x - x0).abs() < 0.5, "{area:?} {x0}");
        assert!((area.x + area.width - x1).abs() < 0.5, "{area:?} {x1}");
    }

    #[test]
    fn a_skipped_level_nests_and_a_shallower_heading_is_a_sibling() {
        let outline = typeset("# A\n### B\n").outline();
        assert_eq!(outline.len(), 1, "{outline:?}");
        assert_eq!(outline[0].children[0].title, "B");
        let outline = typeset("## X\n# Y\n").outline();
        let titles: Vec<_> = outline.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["X", "Y"]);
    }

    #[test]
    fn a_link_over_two_lines_has_two_areas() {
        let words = "word ".repeat(80);
        let book = typeset(&format!("[{words}](http://x.org)\n"));
        let links = book.links(0);
        assert_eq!(links.len(), 1, "{links:?}");
        assert!(links[0].areas.len() >= 2, "{:?}", links[0].areas);
        let (a, b) = (links[0].areas[0], links[0].areas[1]);
        assert!(b.y > a.y);
    }

    #[test]
    fn the_outline_nests_headings_by_level() {
        let book = typeset("# A\n## B\n# C\n");
        let outline = book.outline();
        assert_eq!(outline.len(), 2, "{outline:?}");
        assert_eq!(outline[0].title, "A");
        assert!(outline[0].open && outline[1].open);
        assert_eq!(outline[0].children.len(), 1);
        assert_eq!(outline[0].children[0].title, "B");
        assert_eq!(outline[1].title, "C");
        assert!(outline[1].children.is_empty());
        let Some(LinkTarget::Internal(d)) = &outline[1].target else {
            panic!("{outline:?}")
        };
        assert_eq!((d.page, d.y), (1, Some(TOP)));
    }

    #[test]
    fn the_title_comes_from_front_matter_or_the_first_heading() {
        let p = typeset("---\ntitle: The Book\nauthor: Me\n---\n\n# Chapter\n").properties();
        assert_eq!(p.format, Format::Markdown);
        assert_eq!(p.title.as_deref(), Some("The Book"));
        assert_eq!(p.author.as_deref(), Some("Me"));
        let p = typeset("Intro.\n\n# Chapter\n").properties();
        assert_eq!(p.title.as_deref(), Some("Chapter"));
        assert_eq!(p.author, None);
        assert_eq!(typeset("").properties().title, None);
    }

    #[test]
    fn the_properties_say_whether_there_is_an_outline() {
        assert!(typeset("# Chapter\n\nText.\n").properties().has_outline);
        assert!(!typeset("Just text.\n").properties().has_outline);
        assert!(!typeset("").properties().has_outline);
    }
}
