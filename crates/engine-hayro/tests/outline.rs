use std::sync::Arc;

use engine_hayro::HayroEngine;
use noido_engine::{Destination, Document, LinkTarget, OutlineEntry, PdfBytes, PdfEngine};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn open(name: &str) -> Box<dyn Document> {
    let bytes: PdfBytes = Arc::new(std::fs::read(format!("{FIXTURES}/{name}")).unwrap());
    HayroEngine.open(bytes).unwrap()
}

fn fixture() -> Vec<OutlineEntry> {
    open("outline.pdf").outline()
}

fn titles(entries: &[OutlineEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.title.as_str()).collect()
}

fn internal(entry: &OutlineEntry) -> Destination {
    match &entry.target {
        Some(LinkTarget::Internal(destination)) => *destination,
        other => panic!("{}: not internal: {other:?}", entry.title),
    }
}

#[test]
fn whole_tree_titles() {
    let tree = fixture();
    assert_eq!(
        titles(&tree),
        [
            "Chapter 1",
            "Chapter 2",
            "Part",
            "Åsa",
            "Untitled",
            "Two lines",
            "Cycle A",
            "Cycle B",
        ]
    );
    assert_eq!(titles(&tree[0].children), ["Section 1.1", "Section 1.2"]);
    assert_eq!(titles(&tree[0].children[0].children), ["Deep"]);
    assert_eq!(titles(&tree[1].children), ["Hidden"]);
    assert_eq!(titles(&tree[2].children), ["Unresolved"]);
}

#[test]
fn explicit_and_named_targets() {
    let tree = fixture();
    let chapter = internal(&tree[0]);
    assert_eq!(chapter.page, 0);
    assert_eq!(chapter.x, Some(72.0));
    assert_eq!(chapter.y, Some(92.0));
    let section = internal(&tree[0].children[0]);
    assert_eq!(section.page, 1);
    assert_eq!(section.y, Some(192.0));
    assert_eq!(internal(&tree[0].children[0].children[0]).page, 2);
    assert_eq!(internal(&tree[3]).page, 4);
}

#[test]
fn web_target() {
    assert_eq!(
        fixture()[1].target,
        Some(LinkTarget::Web("https://example.org/".to_owned()))
    );
}

#[test]
fn entries_without_a_usable_target_are_kept() {
    let tree = fixture();
    assert_eq!(tree[2].target, None);
    assert_eq!(tree[2].children[0].target, None);
    assert_eq!(tree[4].target, None);
    assert_eq!(tree[5].target, None);
}

#[test]
fn open_flags() {
    let tree = fixture();
    assert!(tree[0].open);
    assert!(!tree[1].open);
    assert!(tree[2].open);
    assert!(!tree[6].open);
}

#[test]
fn cycles_end() {
    let tree = fixture();
    assert_eq!(tree.last().unwrap().title, "Cycle B");
    assert!(tree[6].children.is_empty());
    assert!(tree[7].children.is_empty());
}

#[test]
fn no_outline_is_empty() {
    assert!(open("two-pages.pdf").outline().is_empty());
}

#[test]
fn empty_outline_is_empty() {
    assert!(open("outline-empty.pdf").outline().is_empty());
}

#[test]
fn depth_is_capped() {
    let mut level = open("outline-deep.pdf").outline();
    let mut depth = 0;
    while let Some(entry) = level.pop() {
        depth += 1;
        level = entry.children;
    }
    assert_eq!(depth, 64);
}
