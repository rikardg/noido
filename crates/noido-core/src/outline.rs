//! The outline as a tree with folding, and the entry the reader is in.

use std::cmp::Ordering;

use noido_engine::{LinkTarget, OutlineEntry};

use crate::DocPoint;

/// One visible row of the outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineRow {
    /// Index in tree order, as `title` and `target` take it.
    pub entry: usize,
    pub depth: usize,
    pub has_children: bool,
    pub open: bool,
}

struct Node {
    title: String,
    target: Option<LinkTarget>,
    /// Whether the document has it unfolded.
    doc_open: bool,
    open: bool,
    depth: usize,
    has_children: bool,
    /// One past the last entry of its subtree.
    end: usize,
}

/// The outline flattened in tree order, with folding.
pub struct OutlineTree {
    nodes: Vec<Node>,
    rows: Vec<OutlineRow>,
    /// Per entry, the row that stands for it: its own, or that of its
    /// nearest visible ancestor.
    marked: Vec<usize>,
    /// (page, y, entry) of the entries with an internal target, by page
    /// then y, in tree order among ties.
    positions: Vec<(usize, f32, usize)>,
}

fn flatten(entries: Vec<OutlineEntry>) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    // Each level's remaining entries and the node that owns them.
    let mut stack = vec![(entries.into_iter(), None::<usize>)];
    while let Some((level, owner)) = stack.last_mut() {
        let owner = *owner;
        match level.next() {
            Some(e) => {
                let index = nodes.len();
                nodes.push(Node {
                    title: e.title,
                    target: e.target,
                    doc_open: e.open,
                    open: e.open,
                    depth: stack.len() - 1,
                    has_children: !e.children.is_empty(),
                    end: index + 1,
                });
                if !e.children.is_empty() {
                    stack.push((e.children.into_iter(), Some(index)));
                }
            }
            None => {
                stack.pop();
                if let Some(owner) = owner {
                    nodes[owner].end = nodes.len();
                }
            }
        }
    }
    nodes
}

impl OutlineTree {
    pub fn new(entries: Vec<OutlineEntry>) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            rows: Vec::new(),
            marked: Vec::new(),
            positions: Vec::new(),
        };
        tree.set(flatten(entries));
        tree
    }

    fn set(&mut self, nodes: Vec<Node>) {
        self.nodes = nodes;
        self.positions = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| match &n.target {
                Some(LinkTarget::Internal(d)) => Some((d.page, d.y.unwrap_or(0.0), i)),
                _ => None,
            })
            .collect();
        // Stable: tree order is kept among equal places.
        self.positions
            .sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        self.marked.clear();
        self.marked.resize(self.nodes.len(), 0);
        let mut i = 0;
        while i < self.nodes.len() {
            let n = &self.nodes[i];
            let row = self.rows.len();
            self.rows.push(OutlineRow {
                entry: i,
                depth: n.depth,
                has_children: n.has_children,
                open: n.open,
            });
            let next = if n.has_children && !n.open {
                n.end
            } else {
                i + 1
            };
            self.marked[i..next].fill(row);
            i = next;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The visible rows.
    pub fn rows(&self) -> &[OutlineRow] {
        &self.rows
    }

    pub fn title(&self, entry: usize) -> &str {
        &self.nodes[entry].title
    }

    pub fn target(&self, entry: usize) -> Option<&LinkTarget> {
        self.nodes[entry].target.as_ref()
    }

    /// Folds or unfolds an entry with children; others and entries out of
    /// range are left alone.
    pub fn toggle(&mut self, entry: usize) {
        if let Some(n) = self.nodes.get_mut(entry).filter(|n| n.has_children) {
            n.open = !n.open;
            self.rebuild();
        }
    }

    /// The entry the reader is in: of those with an internal target, the
    /// last at or before `at`, the later in tree order on a tie. A target
    /// without y counts as the top of its page.
    pub fn current(&self, at: DocPoint) -> Option<usize> {
        let after = self.positions.partition_point(|&(page, y, _)| {
            page.cmp(&at.page).then(y.total_cmp(&at.y)) != Ordering::Greater
        });
        after.checked_sub(1).map(|i| self.positions[i].2)
    }

    /// The row of `current`, or of its nearest visible ancestor when it is
    /// inside a folded branch. An index into `rows()`.
    pub fn marked_row(&self, current: usize) -> Option<usize> {
        self.marked.get(current).copied()
    }

    /// Takes in a new version: keeps the folding if it is the same tree,
    /// and returns whether it was.
    pub fn replace(&mut self, entries: Vec<OutlineEntry>) -> bool {
        let nodes = flatten(entries);
        let same = nodes.len() == self.nodes.len()
            && nodes.iter().zip(&self.nodes).all(|(a, b)| {
                a.title == b.title
                    && a.target == b.target
                    && a.doc_open == b.doc_open
                    && a.depth == b.depth
            });
        if !same {
            self.set(nodes);
        }
        same
    }
}

/// The offset that brings a row into view, moving as little as possible;
/// `None` when it already is.
pub fn reveal_row(row: usize, row_height: f32, offset: f32, view_height: f32) -> Option<f32> {
    let top = row as f32 * row_height;
    let bottom = top + row_height;
    if top < offset {
        Some(top)
    } else if bottom > offset + view_height {
        Some((bottom - view_height).min(top))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noido_engine::Destination;

    fn dest(page: usize, y: Option<f32>) -> Option<LinkTarget> {
        Some(LinkTarget::Internal(Destination { page, x: None, y }))
    }

    fn entry(
        title: &str,
        target: Option<LinkTarget>,
        open: bool,
        children: Vec<OutlineEntry>,
    ) -> OutlineEntry {
        OutlineEntry {
            title: title.into(),
            target,
            open,
            children,
        }
    }

    /// Tree order: A0 B1 C2 D3 E4 W5 N6.
    /// A (page 1, y 100) { B (1, 100), C (2, 50, folded) { D (3, no y) } },
    /// E (5), W (web), N (no target).
    fn tree() -> Vec<OutlineEntry> {
        vec![
            entry(
                "A",
                dest(1, Some(100.0)),
                true,
                vec![
                    entry("B", dest(1, Some(100.0)), false, vec![]),
                    entry(
                        "C",
                        dest(2, Some(50.0)),
                        false,
                        vec![entry("D", dest(3, None), false, vec![])],
                    ),
                ],
            ),
            entry("E", dest(5, Some(0.0)), false, vec![]),
            entry(
                "W",
                Some(LinkTarget::Web("https://x.org".into())),
                false,
                vec![],
            ),
            entry("N", None, false, vec![]),
        ]
    }

    fn at(page: usize, y: f32) -> DocPoint {
        DocPoint { page, x: 0.0, y }
    }

    fn shown(tree: &OutlineTree) -> Vec<(&str, usize)> {
        tree.rows()
            .iter()
            .map(|r| (tree.title(r.entry), r.depth))
            .collect()
    }

    #[test]
    fn rows_follow_folding() {
        let mut t = OutlineTree::new(tree());
        assert!(!t.is_empty());
        assert_eq!(
            shown(&t),
            [("A", 0), ("B", 1), ("C", 1), ("E", 0), ("W", 0), ("N", 0)]
        );
        assert_eq!(
            t.rows()[2],
            OutlineRow {
                entry: 2,
                depth: 1,
                has_children: true,
                open: false
            }
        );
        assert!(!t.rows()[1].has_children);
        t.toggle(2);
        assert_eq!(t.rows().len(), 7);
        assert_eq!(t.title(t.rows()[3].entry), "D");
        assert!(t.rows()[2].open);
        // Folding the parent hides everything; C's own folding survives.
        t.toggle(0);
        assert_eq!(shown(&t), [("A", 0), ("E", 0), ("W", 0), ("N", 0)]);
        t.toggle(0);
        assert_eq!(t.rows().len(), 7);
        assert!(t.rows()[2].open);
        // Toggling a leaf changes nothing.
        t.toggle(1);
        assert_eq!(t.rows().len(), 7);
    }

    #[test]
    fn current_before_first_is_none() {
        let t = OutlineTree::new(tree());
        assert_eq!(t.current(at(0, 0.0)), None);
        assert_eq!(t.current(at(0, 9999.0)), None);
    }

    #[test]
    fn current_between_headings() {
        let t = OutlineTree::new(tree());
        assert_eq!(t.current(at(2, 400.0)), Some(2));
    }

    #[test]
    fn current_on_a_page_with_two() {
        let t = OutlineTree::new(tree());
        assert_eq!(t.current(at(1, 50.0)), None);
        // A and B share a place: the later in tree order.
        assert_eq!(t.current(at(1, 100.0)), Some(1));
    }

    #[test]
    fn current_without_y_is_page_top() {
        let t = OutlineTree::new(tree());
        assert_eq!(t.current(at(3, 0.0)), Some(3));
    }

    #[test]
    fn current_out_of_order() {
        let t = OutlineTree::new(vec![
            entry("late", dest(5, Some(0.0)), false, vec![]),
            entry("early", dest(1, Some(0.0)), false, vec![]),
        ]);
        assert_eq!(t.current(at(2, 0.0)), Some(1));
        assert_eq!(t.current(at(5, 0.0)), Some(0));
    }

    #[test]
    fn web_and_no_target_are_skipped() {
        let t = OutlineTree::new(tree());
        // Past E: W and N come later in tree order but have no place.
        assert_eq!(t.current(at(9, 0.0)), Some(4));
        assert_eq!(t.target(5), Some(&LinkTarget::Web("https://x.org".into())));
        assert_eq!(t.target(6), None);
    }

    #[test]
    fn marked_row_in_folded_branch() {
        let mut t = OutlineTree::new(tree());
        // D is current; C is folded, so C's row.
        assert_eq!(t.marked_row(3), Some(2));
        assert_eq!(t.marked_row(1), Some(1));
        t.toggle(2);
        assert_eq!(t.marked_row(3), Some(3));
        t.toggle(0);
        assert_eq!(t.marked_row(3), Some(0));
        assert_eq!(t.marked_row(1), Some(0));
        assert_eq!(t.marked_row(4), Some(1));
        assert_eq!(t.marked_row(99), None);
    }

    #[test]
    fn replace_same_keeps_folding() {
        let mut t = OutlineTree::new(tree());
        t.toggle(2);
        t.toggle(0);
        assert!(t.replace(tree()));
        assert_eq!(shown(&t), [("A", 0), ("E", 0), ("W", 0), ("N", 0)]);
        t.toggle(0);
        assert!(t.rows()[2].open);
    }

    #[test]
    fn replace_different_starts_over() {
        let mut t = OutlineTree::new(tree());
        t.toggle(0);
        let shorter = vec![entry(
            "X",
            dest(0, None),
            true,
            vec![entry("Y", dest(1, None), false, vec![])],
        )];
        assert!(!t.replace(shorter));
        assert_eq!(shown(&t), [("X", 0), ("Y", 1)]);
        assert_eq!(t.marked_row(1), Some(1));
        assert_eq!(t.marked_row(3), None);
        assert_eq!(t.current(at(1, 0.0)), Some(1));
        t.toggle(6);
        assert!(!t.replace(vec![]));
        assert!(t.is_empty());
        assert!(t.rows().is_empty());
        assert_eq!(t.current(at(1, 0.0)), None);
    }

    #[test]
    fn reveal_row_minimal() {
        // Rows of 20 in a view of 100: rows 0..5 fit at offset 0.
        assert_eq!(reveal_row(4, 20.0, 0.0, 100.0), None);
        assert_eq!(reveal_row(5, 20.0, 0.0, 100.0), Some(20.0));
        assert_eq!(reveal_row(3, 20.0, 200.0, 100.0), Some(60.0));
        assert_eq!(reveal_row(10, 20.0, 120.0, 100.0), None);
    }
}
