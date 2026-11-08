# Outline in the sidebar

Date: 2026-11-08 · Status: draft for review

## Intent

Long documents are hard to move around in by thumbnails alone, and a PDF
often carries its own table of contents: the outline (`/Outlines`, also
called bookmarks). This change shows it in the left sidebar, makes its
entries jump to their places, and marks where the reader is in it, so it
answers both "take me to chapter 7" and "which chapter am I in?".

The sidebar today shows thumbnails, swapped for the list of matches while
a search is active. With the outline it holds three views, so it gets
tabs.

In the test files, outlines run from about 20 entries (a short paper) to
300–400 (two big books). Others have none, including two books.

## Goals

- Three tabs at the top of the sidebar: thumbnails, outline, search
  results. All three are always there, in the same places.
- The outline shows as a tree that can be folded. A click on an entry jumps
  to its place, with the flash links have, and adds a history entry, so
  Back returns to where the reader was.
- The entry for the reader's position is marked, and the tree scrolls to
  keep it in view as the reader moves. It never unfolds by itself.
- A document with no outline shows a short muted text in the tab.
- A search can be left running while the reader looks at another tab.
- The numbers printed on the pages, where the PDF declares them
  (`/PageLabels`), wherever noido shows a page number, so the outline's "61"
  agrees with what the reader lands on. Five of eight test files have them.

## Non-goals

- Building a contents list from the pages (headings, a printed contents
  page) for documents with no outline. A separate project, if ever.
- Editing the outline or saving changes to the PDF.
- Remembering which branches the reader folded, between sessions.
- Keys for switching tabs or moving through the tree (an issue for later).
- Searching or filtering the outline.
- Working out printed numbers from the page text when the PDF declares none.
- Going to a page by its printed number. `--page`, links into noido, saved
  state and jumps all keep file page numbers.
- The outline's colours and styles (`/C`, `/F`).
- Changing the zoom to the one a destination names, as for links.

## Anti-goals

- A tree that changes shape as the reader moves: no unfolding, no folding,
  and no rows that shift when the mark moves. This is why the mark is a
  fill and not bold (egui's default fonts have no bold, and bold would
  rewrap titles).
- A malformed outline that hangs or crashes noido.
- Hovers and clicks on links that visibly wait for the outline.
- The window stalling while the outline is read.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| 1. A new request on the links thread (`Linker`) | Reuses its parsed copy, destination code, reload restart and panic handling. A big outline can briefly delay a link request, once, at load | **Chosen** |
| 2. A thread of its own | Can never delay links, but another parsed copy (3–5 MB by the links spec's measurements) and more plumbing, to read something once | Rejected |
| 3. On the UI thread when the document opens | No messages to pass, but resolving hundreds of destinations in a large file could stall the window | Rejected |

A second PDF crate (such as `lopdf`) for reading the tree was also rejected:
it would parse the file a second time, could disagree with hayro on damaged
files and couldn't use our destination code, which works on hayro's types.
`hayro_syntax` already gives what's needed.

## What this builds

### 0. Printed page numbers (first, as a commit of their own)

**Reading them** (`noido-engine`, `engine-hayro`, a new `labels.rs`): a new
`Document` method, `page_labels()`, gives one `Option<String>` per page,
defaulting to none for engines that can't read them.

- The catalog's `/PageLabels` is a number tree: `/Nums` pairs of a first page
  index and a range's dictionary, or `/Kids` (with a visited set against
  cycles). A range runs until the next one starts; pairs are sorted by page.
- A range's label is its prefix `/P` (through `decode_text`) followed by a
  number in style `/S`: `D` decimal, `R` and `r` roman, `A` and `a` letters
  (A…Z, AA, BB…, by the PDF spec). The number starts at `/St`, default 1, and
  counts up through the range. No `/S` means the prefix alone.
- A range with neither `/S` nor `/P`, pages before the first range, and a
  label that is only whitespace all give `None`. Malformed input never panics.

The GUI asks the links thread once per `Linker`, like the outline
(`request_labels()`, `LinkReply::Labels`). File numbers show until the reply;
on reload the old labels stay until the new reply.

**Showing them** (`noido-core`, a new `page_numbers.rs`, pure): parentheses
always mean a file number, and a printed number never has them.

| | Pages tab, search cards | Outline tab | Title bar |
| --- | --- | --- | --- |
| The label differs from the file number | `61 (79)` | `61` | `61 (79 / 300)` |
| The label equals the file number | `5` | `5` | `5 / 300` |
| This page has no label, but others do | `(12)` | `(12)` | `(12 / 300)` |
| The document has no labels at all | `12` | `12` | `12 / 300` |

- Search cards read `Page 61 (79)`. The title bar keeps its zoom after this.
- A long label is elided and the file number in parentheses stays. Text such
  as `Cover Page` and `p. 829` occurs in the test files.
- Progress text ("Searching… page 4 of 300") and error messages keep file
  numbers: they count through the file.

### 1. The outline from the engine (`noido-engine`, `engine-hayro`)

A new `Document` method, `outline()`, returns the tree, defaulting to empty
(like `links`, for engines that can't read one):

```rust
pub struct OutlineEntry {
    pub title: String,
    /// Where it goes; `None` when it has no usable target.
    pub target: Option<LinkTarget>,
    /// Whether its children start unfolded.
    pub open: bool,
    pub children: Vec<OutlineEntry>,
}
```

Reading it, in a new `engine-hayro/src/outline.rs`, from the raw objects as
`links.rs` does:

- Start at the catalog's `/Outlines`. Each level is a linked list: an item's
  `/First` is its first child, and each child's `/Next` the following one.
  `/Last`, `/Prev` and `/Parent` are ignored.
- **Title:** `/Title`, through `decode_text` (moved out of `properties.rs`
  to be shared). Whitespace runs, including newlines, become one space, and
  the ends are trimmed. An empty or missing title reads "Untitled".
- **Target:** `/Dest` or `/A`, through the same code that reads a link's
  target. An outline item uses the same keys as a link annotation, so this
  is that function with its dictionary parameter reused, not a copy. The same
  rule applies as for links: only GoTo and URI count.
- An entry with no target, or one that won't resolve, is **kept** with
  `target: None`, unlike a link, which is dropped: dropping it would lose its
  children.
- **Open:** `/Count` greater than zero. Negative means closed, by the spec.
  A missing or zero `/Count` on an item with children is treated as closed.
- **Limits:** a visited set of object identifiers shared over the whole walk;
  an item already seen ends that list, keeping what was read. Depth is capped
  at 64 and the total at 100,000 entries; past a cap, reading stops and what
  was read so far is returned. Malformed input never panics.

`Properties::has_outline` stays as it is: it says the document has an
`/Outlines` dictionary, even an empty one.

### 2. The outline thread request (`noido-render`)

`Linker` gets `request_outline()`, answered by `LinkReply::Outline(Vec<…>)`,
under `caught_none`-style panic handling; a panic gives an empty outline.

The GUI asks once after starting each `Linker`: when a document opens and on
each reload (`watch::Reload` already carries a new linker). Until the first
reply, the outline tab shows nothing at all, not the empty-state text, so the
text doesn't flash on a document that has an outline.

### 3. The tree's logic (`noido-core`, a new `outline.rs`)

Pure and tested here, working on the engine's tree:

- **Folding:** which entries are unfolded, starting from each entry's `open`.
- **Visible rows:** the flattened list of rows given the folding, with each
  row's depth and whether it has children.
- **The current entry**, given a `DocPoint`: of all entries with an internal
  target, the one with the greatest position (page, then y) that is at or
  before the point. On a tie, the later in tree order, so a chapter and its
  first section at the same place mark the section. A target with no y counts
  as the top of its page; x is ignored. Comparing every entry, not stopping at
  the first past the point, copes with outlines out of document order. Before
  the first heading there is no current entry.
- **The marked row:** the current entry's row or, when that is inside a
  folded branch, its nearest visible ancestor.

### 4. The reader's position (`noido-core`, `gui-egui`)

A new method on `View` gives the point the current entry is measured from,
in continuous and paged mode alike:

- **One page per row:** `View::place()`, the viewport's centre.
- **Two-page mode:** the whole spread is the position: the viewport centre's
  height, on the last page of the spread at the centre, found through
  `spread_of` / `spread_pages`. So a heading anywhere on the left page, or
  in the top half of the right page, counts as reached. A heading low on the
  right page waits until it is scrolled past or the page is turned. `place()`
  alone won't do: the centre falls in the gutter, which `doc_point` gives to
  the left page (in gapless mode, to either page through rounding), so a
  chapter starting on a right-hand page would go unmarked for its whole
  spread.

When `current_page()` (which prefers `chosen`) is on a different spread, as
after a jump clamped at the end of the document, the point is the top of that
page instead. A `chosen` page on the same spread changes nothing, so clicking
the right page's thumbnail doesn't move the mark and move it back later.

After a click on an outline entry, that entry itself is the current one until
`chosen` is cleared (the reader moves on) or set again by another jump. A
target clamped near the end of the document would otherwise mark an earlier
entry than the one just clicked. The same goes for a target that lands
slightly off through rounding.

### 5. Tabs (`noido-core`, `gui-egui`, `noido-state`)

`SidebarState` gets the tab, in `noido-core/src/sidebar.rs`:

- `SidebarTab`: `Thumbnails`, `Outline`, `Results`.
- `active`, and `last`: the last tab chosen that isn't `Results`.
- Pure transitions, tested in core:
  - **The reader picks a tab:** it becomes active.
  - **The reader starts a new query:** `Results` becomes active. This
    includes typing another query from another tab. Stepping through hits
    with next/previous doesn't switch, and neither does a reload re-running
    the search.
  - **The search is cleared:** if `Results` is active, `last` becomes active.
    Otherwise nothing changes.

The rest of the search's behaviour stands: the first letter typed opens the
sidebar, and the Esc logic (`sidebar_opened_by_search`) is unchanged. A
search carries on and keeps its highlights and hit stepping while another tab
shows.

**Saved per document:** `last`, as a new `sidebar_tab` field on the state
file's `Entry` (`#[serde(default)]`, with an absent or unknown value meaning
thumbnails). `Results` is never saved, since searches aren't. A new document
opens on thumbnails. `Defaults` is unchanged.

`Results` with no search shows a muted "No search" text. It is reachable by
clicking its tab.

Off the thumbnails tab no new sharp renders are asked for; those already
made are kept, so going back shows them sharp at once; closing the sidebar
frees them.

The Search tab's cards keep clear of the scrollbar as the outline's rows
do: a solid bar is reserved, a floating bar only when the list overflows.

### 6. The outline tab (`gui-egui`, `sidebar.rs`)

- **Rows:** one line each, of fixed height, drawn only when visible
  (`show_rows` or equivalent), so 400 or 100,000 rows cost the same. Indented
  by depth. A title too long for the width is elided, with the full title as
  a tooltip.
- **Page number:** the target page's number in the outline's form (§0),
  right-aligned in the row in the weak text colour. The number takes at most
  about a third of the row, with a longer label elided; within that the title
  gives way. Rows with no internal target show none.
- **Fold chevron:** on entries with children, `>` when folded and `v` when
  unfolded, painted as lines rather than taken from a font. It sits in a hit
  area that toggles the branch and doesn't jump. Entries without children
  leave its space empty, so titles at one depth line up.
- **Click on a row:** jumps. On an entry with no target: toggles it if it has
  children, otherwise nothing. Those rows don't show the pointing hand.
- **Double click on a row:** jumps and unfolds. The first click does what a
  single click does; the second only unfolds the branch (it doesn't jump
  again or add a history entry), so a row with no target ends unfolded
  whether it started folded or not. An unfolded branch stays unfolded, a
  leaf just jumps once, and a web target opens once. Unfolding doesn't
  scroll. A double click on the chevron is two toggles.
- **Drag:** scrolls, as the other lists do (`DragScroll::Always`), and a drag
  never counts as a click.
- **The mark:** the current result card's fill (`visuals.selection.bg_fill`
  and its text colour), so the sidebar's views all mark "you are here" the
  same way.
- **Following:** the tree scrolls the marked row into view, only as far as
  needed, when the *current entry* changes, and when the tab shows or the
  width changes. Otherwise the reader's own scrolling stands, as with the
  thumbnails. Folding or unfolding a branch doesn't scroll.
- **Empty state:** "This document has no outline", muted, once the reply has
  come.
- **Reload:** the old tree and folding stay until the reply. If the new tree
  equals the old one the folding is kept; otherwise it starts over from the
  document's `open` flags.

`sidebar::Output` gets the entry clicked. `app.rs` carries it out as `follow`
does a link's target, so it gets the same flash, `chosen`, `reader_moved` and
`leave_here`, and only `http`, `https` and `mailto` web targets open. This
reuses `follow`'s target handling, split out if needed, rather than a copy.

## Calls made

These weren't discussed; they're the author's and easy to overturn.

- **Tab labels:** "Pages", "Outline", "Search". At the sidebar's narrowest
  (160 points) "Thumbnails" doesn't fit in a third of the width.
- **All three tabs always shown:** so the sidebar keeps its shape between
  documents and before a search.
- **Roman numerals past 3999** show as decimal, since standard numerals stop
  there and repeated `M`s would run long. A `/St` below 1 counts as 1.
- **Single-line elided rows,** not wrapped: rows stay one height, which is
  what makes huge outlines cheap and scrolling to a row simple.
- **Folding resets on reopening:** to the document's own flags.
- **Viewport centre's height as the position:** so a heading becomes current
  when it passes the middle of the window. In paged mode a chapter starting low
  on a page (or low on the right-hand page of a spread) isn't marked until the
  page is turned. Check this by hand.

## Testing

TDD throughout.

- **Fixture:** `fixtures/outline.pdf` from `make_fixtures.py`: three levels
  deep; a closed and an open branch; an explicit `/Dest`, a named one, a
  `/A` GoTo and a `/A` URI; an entry with no target and children; an
  unresolvable name; a UTF-16 title and an empty one; two entries at the same
  place; a `/Next` cycle and a `/First` cycle back to an ancestor. A second
  small file or case with `/Outlines` but no `/First`.
- **Labels fixture:** `fixtures/page-labels.pdf`: each style, a prefix alone, a
  prefix with a number, `/St`, an empty range, a late first range, and `/Kids`
  with a cycle.
- **Page labels:** the engine's labels for that fixture; every row of the table
  in §0 in core, and formatting past 3999, letters past Z; the harness shows
  them in the title bar, under a thumbnail, on a search card and in an outline
  row, and file numbers before the reply; a panicking `page_labels()` gives
  none.
- **engine-hayro:** the whole tree of the fixture; the cycles end; a document
  with no outline and one with an empty outline both give none; the depth cap.
- **noido-render:** the reply; a panicking `outline()` gives an empty reply.
- **noido-core:** visible rows under folding; the current entry before the
  first heading, between headings, on a page with two, with no y, on a tie and
  out of order; the marked row inside folded branches; tab transitions; the
  reader's position in single-page, dual and gapless dual modes, continuous and
  paged, odd-left spreads and a lone page at the end, a `chosen` page on the
  same spread and on another.
- **noido-state:** `last` round-trips; old files and unknown values give
  thumbnails; `Results` isn't saved.
- **Harness** (fake engine with an outline):
  - Clicking a row jumps, flashes and adds a history entry, and Back returns.
  - Clicking a chevron folds without jumping.
  - A row shows its target's page number, counted from 1.
  - A drag scrolls without jumping.
  - The mark moves as the pages scroll, and the tree scrolls to it without
    unfolding.
  - In dual mode, a chapter on the right-hand page is marked when its spread
    shows.
  - After a click near the end, the clicked entry stays marked.
  - The empty state shows after the reply, and nothing shows before it.
  - Starting a search goes to the results tab, and clearing it returns.
  - Switching away keeps the search and its stepping.
  - Off the thumbnails tab, no new sharp renders are asked for; those
    already made are kept, and closing the sidebar frees them.

By hand, on the test files: reading the largest outline at load (measure it
and report the time; if a link hover visibly waits, say so before going on),
and the "Viewport centre" call above.

## Left to the builder

The names of types and methods not given above; the tab strip's exact look,
within the theme; the row height, indent and the chevron's exact size; how the fold state
is keyed; how the harness fakes the outline; the wording of doc comments.

Once done: remove "Outline in the sidebar" from `docs/backlog.md`, and update
the line in `sidebar.rs`'s header that says the outline will appear there.
