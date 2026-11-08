//! The document outline (bookmarks): the catalog's `/Outlines` tree, read
//! through `/First` and `/Next`.

use std::collections::HashSet;

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{Dict, Number, ObjectIdentifier, String as PdfString};
use noido_engine::OutlineEntry;

use crate::links::{PageIndex, target};
use crate::pdf_string::decode_text;

/// Levels below this are left out.
const MAX_DEPTH: usize = 64;
/// Entries past this many are left out.
const MAX_ENTRIES: usize = 100_000;

pub(crate) fn outline(pdf: &Pdf, index: &PageIndex) -> Vec<OutlineEntry> {
    read(pdf, index, MAX_ENTRIES)
}

/// The outline, with at most `limit` entries in all.
fn read(pdf: &Pdf, index: &PageIndex, limit: usize) -> Vec<OutlineEntry> {
    let Some(root) = pdf
        .xref()
        .get::<Dict>(pdf.xref().root_id())
        .and_then(|catalog| catalog.get::<Dict>(b"Outlines"))
    else {
        return Vec::new();
    };
    let mut walk = Walk {
        pdf,
        index,
        limit,
        count: 0,
        visited: root.obj_id().into_iter().collect(),
    };
    walk.list(root.get::<Dict>(b"First"), 0)
}

struct Walk<'a> {
    pdf: &'a Pdf,
    index: &'a PageIndex,
    limit: usize,
    count: usize,
    /// Every item read so far, over the whole walk: an item met again ends
    /// its list, so cycles and shared items can't make the walk run long.
    visited: HashSet<ObjectIdentifier>,
}

impl Walk<'_> {
    /// The siblings from `first` on, through `/Next`.
    fn list(&mut self, first: Option<Dict<'_>>, depth: usize) -> Vec<OutlineEntry> {
        let mut entries = Vec::new();
        if depth >= MAX_DEPTH {
            return entries;
        }
        let mut item = first;
        while let Some(current) = item {
            if let Some(id) = current.obj_id()
                && !self.visited.insert(id)
            {
                break;
            }
            if self.count >= self.limit {
                break;
            }
            self.count += 1;
            let children = self.list(current.get::<Dict>(b"First"), depth + 1);
            entries.push(OutlineEntry {
                title: title(&current),
                target: target(self.pdf, self.index, &current),
                open: current
                    .get::<Number>(b"Count")
                    .is_some_and(|count| count.as_i64() > 0),
                children,
            });
            item = current.get::<Dict>(b"Next");
        }
        entries
    }
}

fn title(item: &Dict<'_>) -> String {
    let text = item
        .get::<PdfString>(b"Title")
        .map(|title| decode_text(title.as_bytes()))
        .unwrap_or_default();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        "Untitled".to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::page_index;

    fn count(entries: &[OutlineEntry]) -> usize {
        entries.iter().map(|e| 1 + count(&e.children)).sum()
    }

    #[test]
    fn entries_are_capped() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/outline.pdf"
        ))
        .unwrap();
        let pdf = Pdf::new(std::sync::Arc::new(bytes)).unwrap();
        let index = page_index(&pdf);
        assert_eq!(count(&read(&pdf, &index, 3)), 3);
    }
}
