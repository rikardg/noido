# Keeping a page's text: design

Date: 2026-11-15

## Problem

Reading a book and searching it costs 126 bytes a character, and none of it
is ever given back: 89 MB for a 356-page book, 158 MB for a 737-page one.
Measured in [the investigation](../investigations/2026-11-15-text-and-search-memory.md):

| | 356 pp | 737 pp |
| --- | ---: | ---: |
| the page text the app holds | 37.8 MB | 80.3 MB |
| what the engine keeps per page read | 17.1 MB | 21.6 MB |
| both folded forms, once searched | 28.6 MB | 56.1 MB |
| hits | 5.3 MB | 9.8 MB |

All of it comes back for 0.69 ms a page (text) and 0.07 ms a page (folding),
so the fix is to keep less rather than to store it smaller. Three things are
kept that do not need to be:

- The app keeps every page's text it has ever shown, for the life of the
  document (`Texts` in `crates/frontends/gui-egui/src/texts.rs`).
- The text thread keeps every page it has ever read — and a search reads
  every page, so one query leaves the whole book there.
- Focusing the search field reads the whole document before anything is
  asked for (`search_state.extract()` at `app.rs:1544`).

## Change

Two changes, in this order, plus the one-line deletion that makes the second
one count.

### 1. The app's page text is an LRU with a byte budget

`Texts` keeps at most `TEXT_BUDGET` bytes of page text. A page's size is its
own, from `PageText::memory_bytes()`: the text, the per-character map, the
glyph rectangles and, once the page is searched, its folded forms. Entries
are counted by what they hold *now* — a page's forms appear when it is first
searched and nearly double it — so the budget is enforced once a frame, not
only when text arrives.

When the budget is over, the pages asked for longest ago go, except the
pages on screen, which are never dropped, and the pages something needs
whole: a copy being made, a highlight under edit, a selection's ends. Those
can be more than the budget holds, and then the budget gives way — it is a
bound on what is kept for later, not on what is being used now. A dropped
page is forgotten in the text thread too (`Command::Forget`), so the `Arc`
goes and the thread does not read it again until it is asked for.

`TEXT_BUDGET` is 32 MB: about 200 pages of prose, or 40 of them with their
forms and a selection across all of it, which is more reach than a reader
has. It is a bound on the whole document, not a working set.

### 2. The text thread keeps only what the app asked for

A page read for a walk — a search, the counting pass — is dropped once the
walk has used it. Pages the app asked for (`Command::Text`) are kept until it
forgets them, as now. The worker needs to know which is which: a `wanted`
flag per page, set by `Text(page)`, cleared by `Forget(page)`.

A page whose text cannot be read is remembered as having none (`failed`),
so a walk that reads the book again does not read — and fail on — that page
again.

The cost is that a second query re-reads the document: 0.24–0.78 s of
thread time on the measured books, with the first page's hits arriving after
the first page's read, as the first query always has. The counts are kept
per page as now, so the info dialog does not re-read anything.

### 3. Focusing the field no longer reads the document

`search_state.extract()` is what reads the whole book when the search field
gains focus. Nothing needs it: the walk reads what it searches, and
`count()` reads what it counts. The command goes with it; the idle pass is
the counting one, and a walk counts each page as it reads it, so Document
info afterwards costs no reads.

## What the reader sees

Nothing different except memory. Page turns, selection, copy, highlights and
hit contexts read the same text — from memory when it was kept, from the
engine for 0.7 ms when it was not. A selection dragged over more pages than
the budget holds re-reads the pages it re-enters; a copy or a highlight over
that many pages holds them all while it needs them.

## Tests

- `noido-text`: `memory_bytes` covers its parts, and a searched page is
  bigger by both folded forms at least.
- `texts.rs`: a page out of reach goes when the budget is full; the pages on
  screen stay whatever the budget; the least recently asked for goes first;
  a page that comes back is asked for again.
- `noido-render` searcher: a page read for a search is not kept unless it
  was asked for; a page the app asked for is not read twice; a second query
  reads the pages again; the counts are kept after the walk that read them.
- `gui-egui`: focusing the search field reads only the page on screen; a
  book read through keeps no more text than the budget; a search leaves the
  app holding only the page shown; a copy of a selection larger than the
  budget finishes and stops reading.

## Not changed

- `MAX_HITS`, the hit cap and the context a hit carries (2.7 MB at the cap;
  its own option in the investigation, deferred).
- `CharInfo` and the glyph rectangles (options 5 and 6 in the investigation):
  they shrink what is kept, and what is kept is now bounded.
- What the engine keeps per page read: 17.1 MB on the 356-page book, the
  floor under all of this, and its own item.
