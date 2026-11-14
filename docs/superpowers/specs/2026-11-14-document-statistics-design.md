# Statistics in Document info

Date: 2026-11-14. Agreed with the user. A small change: Document info gains a
**Statistics** section with the page count and, once the document's text has
been read, its word and character counts.

## Intent and why

The reader wants a quick sense of how much text a document holds — how long
the book is in words, not only in pages. Nothing in noido counts words today.

## Goals

- A **Statistics** section between Format and Fonts, holding:
  - **Pages** — the page count, moved here from Format (two rows with the
    same number would be odd), always available, costs nothing.
  - **Words**, **Characters**, **Characters (no spaces)** — LibreOffice's set,
    with Word's parenthetical naming for the last one.
- The counts appear as soon as every page has been read; until then those
  three rows say "Counting…", as the Document section says "Reading…" for
  the properties.
- Nothing is read until the reader asks: opening Document info starts the
  walk, as focusing the search field already does. Once a page's text has
  been read it is kept, counts included, so a second look costs nothing.

## Non-goals

- Paragraph and line counts (the two rows of Word's dialog nobody reads).
- Counting a selection, or a live count in the status bar.
- Keeping the counts across sessions: they belong to the text the session
  read, and the text is not kept across sessions either.
- Bettering the text: scanned pages with no text layer count 0, and that is
  the honest answer.

## Constraints

PDF text is what the engine's glyphs assemble into, not a word processor's
text (`noido-text`'s `PageText`: lines joined with newlines, spaces inferred
from gaps). So:

- A **word** is a run of non-space characters after the line-end hyphens
  noido already rejoins are rejoined: `statis-\ntics` is one word, as it is
  for copy and search.
- A **page boundary breaks a word**: counts are kept per page, so a hyphen at
  the foot of a page ends its word rather than joining the next page's first.
  Rare, and the alternative is counts that cannot be cached per page.
- **Characters** are the characters the document draws: the line breaks we
  insert are not characters and are not counted, and a line-end hyphen that
  words rejoin is still counted here — the glyph is on the page.
- **Characters (no spaces)** drops the whitespace.

## Approaches

1. **Count on the text thread's existing walk** — chosen by the user. The
   text thread (`noido-render/src/text.rs`) already reads every page's glyphs
   in the background, keeps each page's `PageText`, and drops a page's text
   when it is asked to read it again. Counting a page as its text is read
   costs a pass over that page's characters, once, and the totals come out
   of the pages already read.
2. A short-lived thread of its own, like the properties reader: open a copy,
   walk every page, send one reply. Less plumbing, but it reads glyphs the
   text thread may already hold — a second full walk over a long book — and
   caches nothing.

## Design

`noido-text`, a new module `count.rs`:

- `pub struct Counts { pub words: usize, pub chars: usize, pub chars_no_spaces: usize }`,
  `Counts::of(text: &str)`, and `AddAssign` so pages add up. The hyphen
  rejoin uses `search::line_end_hyphen`, which is already `pub(crate)`.

`noido-render`, `text.rs`:

- The `Worker` gains `counts: Vec<Option<Counts>>` beside `texts`, `counting`
  and `want_count`, and answers a new `Command::Count`.
- `Command::Count` asks for extraction too (both flags on), so the frontend
  asks for one thing.
- While `counting`, each page counted sends
  `SearchUpdate::Counts { counted, pages, total: Counts }`, totals over the
  pages that have counts. The last one, with `counted == pages`, is the
  answer.
- A page whose text is already there is counted when the `Count` command
  arrives, in the idle step, so a document already read answers at once.
- `Forget(page)` drops the page's counts with its text; the next read counts
  it again, and the totals stay right.
- Public: `TextSearcher::count(&self)`.

`gui-egui`:

- `search::Search` keeps the latest `SearchUpdate::Counts` and hands it out;
  a new searcher (a reload) clears it, as extraction is asked for again.
- `info::show` takes the counts (or their absence) and draws the section:
  Pages always, the three text rows as "Counting…" until `counted == pages`.
- `NoidoApp::info_page` passes them, and opening the page calls
  `search_state.count()`.

Numbers are grouped with a plain space, as the page already writes
"216 × 279 mm" and "2.4 MB": `94 781`. Sizes below a thousand are bare.

## Tests

- `noido-text`: `Counts::of` on lines, on a document whose lines join with
  hyphens, on a hanging hyphen that is the word's own, on punctuation and
  digits, on empty text; characters with and without spaces.
- `noido-render`, `tests/searcher.rs`: `count()` reads every page and the
  last `Counts` update is the document's total; nothing is read until
  `count()` is called; a document already extracted answers at once; a
  forgotten page re-read and recounted leaves the totals unchanged.
- Harness: Document info on a document with text shows "Statistics" and
  "Pages", "Counting…" before the counts land, and the word and character
  numbers after; the counts are what `Counts::of` says over the pages' text.
  Format no longer shows a "Pages" row.

## By hand

Open a book, Ctrl+I: Pages at once, then Words, Characters and Characters
(no spaces) once "Counting…" goes. A scan with no text layer: zeros. Search
first, then Ctrl+I: the numbers are there on the first frame.

## Left to the builder

Whether the totals live in `Search` or are passed straight from its updates;
the exact wording of "Counting…" when the dialog finishes before a slow
document does; and whether the section draws its text rows while the page is
still reading (my call: it does, showing "Counting…", so the section does not
appear and reflow late).
