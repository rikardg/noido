# Search forms belong to the search

A page's folded forms are search state, not page state. Today the first query
that touches a page keeps its forms for as long as the page's text is held:
a searched page costs about 100 kB instead of 20 kB, and the 16 MB text budget
holds 160 pages instead of 835. After this change a held page costs the same
searched or not; the forms live for one `find`, on the text thread, and go.

## Today

`PageText` holds `forms: OnceLock<Forms>` (`crates/noido-text/src/page.rs`).
`PageText::find` fills it on first use with `Forms::new(text, marked_hyphens)`:
the page's characters as `char`s, both folded forms, and a `usize` source index
per folded character. `PageText::memory_bytes` counts it, so the budget counts
it.

The text thread (`crates/noido-render/src/text.rs`) calls `find` once per page
per query and releases every page it read for a walk unless the app holds it
(`wanted[page]`). So the cache survives only on the pages the app keeps — the
pages `Texts::evict` counts — and it survives past the query, past replacing
the query, and past closing the search. Only evicting the page or closing the
document drops it.

Measured on the three books with the counting allocator (page text only):

| book | pages | unsearched | searched | forms a page |
| --- | ---: | ---: | ---: | ---: |
| management history | 356 | 7.15 MB | 37.17 MB | 84 kB |
| Git book | 737 | 16.05 MB | 74.83 MB | 80 kB |
| RPG rulebook | 240 | 5.44 MB | 25.85 MB | 85 kB |

## The change

- `find` builds `Forms::new(self.text(), self.marked_hyphens())`, uses it for
  the matches and their contexts, and drops it. `Forms` stays private to
  `noido-text::search`.
- Remove `PageText.forms`, the `OnceLock` import, `Forms::memory_bytes`, and
  the forms term in `PageText::memory_bytes`.
- No public API change: `find`, `Query`, `TextHit` and the results are as they
  were.

## What it costs

The fold is about 0.07 ms a page (issue #96), paid once for each page a
query visits. The same harness measured the searches faster, not slower: from
107 to 72 ms on the 356-page book, 344 to 90 ms on the 737-page one, and 74
to 33 ms on the 240-page one. It holds every page, so the old path kept a
form for every page (30, 58 and 21 MB); dropping each form after its page is
searched leaves the allocator one page's worth. In the app the walk already
released the pages it was not asked for, so the search time changes less; the
memory is the gain. A held page now measures 7.10 MB for the whole 356-page
book whether it has been searched or not (it was 37.17 MB searched).

The search still runs on the text thread 0.2 s after typing stops (`DELAY` in
`crates/frontends/gui-egui/src/search.rs`), so the fold is not on the frame;
the first page's hits still arrive in about a millisecond.

Rejected alternatives:

- **Cache in the text thread**: faster repeat queries, but the cache is
  memory outside the page budget, needs a bound, and must be invalidated when
  a page's transform or reflow changes. More state for CPU we do not need.
- **One form per held page for the query's lifetime**: still ~35 kB a held
  page during a query, and nothing reads the form after the page's hits are
  computed. It buys nothing over building and dropping.

## Tests

- A page's `memory_bytes` is the same before and after `find`, and `find`
  still returns its hit; a prose page stays under 40 kB searched.
- Two `find`s return equal hits and leave the cost unchanged: no hidden state.
- The existing fold tests (accents, ligatures, dehyphenation, context),
  `Joined`, the text thread's tests and the GUI's search tests stay green.

## Budget

With the forms gone, a page costs ~20 kB searched or not. `TEXT_BUDGET` goes
to 9 MB as decided with the text geometry change: about 470 pages, which is
the whole 356-page book (7.10 MB) and more than the 160 searched pages 16 MB
held before. It gives up the unsearched reach of 16 MB (~835 pages); the
737-page book no longer fits whole (15.95 MB) and its pages are re-read from
the engine as they are shown. That is the trade the geometry spec decided.
The constant changes in its own `egui:` commit after the measurement.

## Not in scope

- `u32` source indices: with the form transient they would save a few kB of
  one page's temporary and nothing held. The folding code stays as it is.
- Folding, dehyphenation, the two forms, `Joined`, the hit rects and contexts.
- #130, which follows this.
