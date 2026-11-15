# Text and search memory: investigation

Date: 2026-11-15

## Question

Reading through a book, and searching one, grows the app's memory by tens of
megabytes and keeps it: searching a 356-page book added about 123 MB. Where
does that go, and what can be done about it?

## Answer

Reading and searching a book costs **126 bytes a character** — 89 MB for a
356-page book, 158 MB for a 737-page one (169 MB counting every hit the walk
finds rather than the 5,000 the app keeps) — in four parts, three of which
are the app's and one of which is the engine's:

| Part | 356 pp | 737 pp | 240 pp | Its price |
| --- | ---: | ---: | ---: | --- |
| the text itself (UTF-8) | 1.1 MB | 2.3 MB | 0.7 MB | — |
| `chars`: one `CharInfo` per character | 26.1 MB | 54.0 MB | 17.4 MB | 0.69 ms a page, read with the text |
| `bounds`: a `Rect` per glyph | 10.6 MB | 24.0 MB | 8.0 MB | 0.69 ms a page, read with the text |
| **the page text the app holds** | **37.8 MB** | **80.3 MB** | **26.1 MB** | 0.69 ms a page |
| what the engine keeps once a page is read | 17.1 MB | 21.6 MB | 8.0 MB | 0.69 ms a page, and not ours to drop |
| `forms`: both folded forms, per page, once searched | 28.6 MB | 56.1 MB | 19.5 MB | 0.07 ms a page on its own |
| hits, 540 bytes each (9,828 / 18,161 / 7,858; the app caps at 5,000) | 5.3 MB | 9.8 MB | 4.2 MB | 24 ms a query, redone per query |
| **the whole of it** | **88.8 MB** | **167.8 MB** | **57.8 MB** | |
| characters | 707,196 | 1,605,641 | 543,581 | |
| glyphs / lines | 694,831 / 12,721 | 1,574,296 / 31,305 | 525,349 / 13,607 | |

Measured with a counting allocator in a throwaway crate that depends on
`noido-text` and `engine-hayro` by path, on three text-heavy books outside
the repo (a management history, a build tool's manual, a role-playing game
rulebook), release build, the cost of opening the document excluded. Reading
every page's text took 245, 779 and 320 ms (0.7–1.1 ms a page); folding and
matching every page of the 356-page book took 24 ms (0.07 ms a page).

The engine's share was found by reading every page, dropping every
`PageText`, and measuring what was left: 17.1 MB for the 356-page book,
21.6 MB for the 737-page one, 8.0 MB for the third — 18 to 49 kB a page,
growing with every page read rather than in one step, so it is per page and
not per font (0.2 MB after 10 pages of the 356, 4.3 MB after 100, 17.0 MB
after all of them). The render cache is not the cause: `engine-hayro` starts
a fresh one every 64 renders. What is left is inside hayro's `Pdf`/`Pages`,
which is why it is a separate item below and not part of the app's bill.

The shapes of the waste are four, and they are independent:

1. **Nothing is ever dropped.** A page's text is kept for the life of the
   document, though it is needed only while the page is in reach or a
   selection crosses it.
2. **Focusing the search field reads the whole document.** `search_state.extract()`
   (`app.rs:1544`) tells the text thread to extract every page, and nothing
   drops them afterwards. The counts dialog does the same, only to count
   (`Counts` is 48 bytes a page — the text is not needed for it at all).
3. **The per-character structures are fat.** `CharInfo` is 24 bytes
   (`Option<usize>` is 16 and `usize` 8) against 24 bits of information; a
   glyph's `Rect` is 16 bytes of `f32`; and the folded forms keep the page's
   characters a second time (`Forms.chars: Vec<char>`, 4 bytes a character)
   as well as a `usize` source index each (8 bytes on a 64-bit build).
4. **The engine does not give its page back.** 49 kB of every page read is
   hayro's, held for the document's life, and dropping the page's text does
   not return it.

Two more costs are real but small: the hits of a query (about 540 bytes a
hit, because each carries 300 characters of context as its own `String`),
capped at 5000 hits; and `Counts`, 48 bytes a page, which is worth keeping.

The 123 MB seen in the app is this 89 MB plus allocator slack: `chars` is
built by pushing, so it sits 62% full; `bounds` is collected from a known
length and is exact.

## Where it is kept

- `Texts` in `app.rs` (`crates/frontends/gui-egui/src/texts.rs`): one
  `Arc<PageText>` per page ever shown, never dropped; `forget(page)` exists
  but is called only when a page's shape changes.
- `Worker.texts` in the text thread
  (`crates/noido-render/src/text.rs:177`): every page it has read, forever.
  Both hold the same `Arc`, so this is one allocation seen twice, not two.
- `PageText.forms`: a `OnceLock`, so once a page has been searched its forms
  cannot be given back. `Forms::new` always folds **both** forms plus a copy
  of the page's characters, whether the query has accents or not, and the
  forms are query-independent: only the choice between them depends on the
  query.

## Options

| | Change | Saves, 356 pp | Cost | Effort |
| --- | --- | ---: | --- | --- |
| 1 | Focus the field without reading the whole book | 37.8 MB | none; the walk reads what it needs | small |
| 2 | Stream the search walk: drop what was read for the walk, keep what the app asked for | 37.8 MB, and the forms with it | 0.7 ms a page again on the next query | small–medium |
| 3 | Forms belong to the search, not the page: build the form the query needs, without the character copy, and drop them when the query goes | 28.6 MB | 24 ms a query, already paid on the first | medium |
| 4 | Keep the app's page text in an LRU, counted by bytes (12–32 MB) | bounds all of it, the forms included | re-read on a miss, 0.7 ms a page | small |
| 5 | `CharInfo` → a `u32` glyph index plus line starts (`u32` a line) | 22 MB of whatever option 4 keeps | `line_of` binary-searches a start (one caller); `lines()` walks lines instead of characters | medium |
| 6 | `bounds` as four `i16` at a 64th of a point | 8 MB of the same | sub-pixel rounding, invisible at a point of tolerance | medium |
| 7 | Hits store the context's range into the page's text, or a shorter context | 2 MB | context needs the page's text alive, which option 4 fights | small |
| 8 | Bound or re-create what hayro keeps per page | 17.1 MB | inside hayro: the render cache is already bounded, the page objects are not | unknown |

Options 5 and 6 shrink what remains; they are not a substitute for 1–4,
because the bulk is *every page of the book* rather than each page being
fat, and once 4 has a byte budget their saving is whatever is left of that
budget, not the 30 MB they would save at today's retention. Option 8 is not
the app's to do, but it is the floor: no amount of dropping the app's text
takes a 356-page book below 17 MB.

Option 2 makes option 3 mostly redundant — a page the walk drops takes its
forms with it — so 3 is worth doing only for the pages that stay.

**Recommended: 4 and 2 (with 1, which is what lets 2 help), then 3.**
Options 4 and 2 together take the 356-page book from 89 MB to about 18 MB —
what the reader can reach plus the engine's 17 MB — and the 737-page one
from 158 MB to about 22 MB. The price is one re-read of the document per
query (0.24–0.78 s on the measured books, threaded, after the first page's
hits are in) and a re-read of a page that falls out of reach (0.7 ms).

One caveat on option 4: count the entry by what it holds *now*, not by what
it held when it arrived. A page's forms appear when it is first searched and
nearly double its footprint; a budget that counts pages on insert would
silently overrun after a search.

Doing nothing is defensible — a page turn is dominated by rendering, and the
app's peak belongs to textures and the mapped file — but 158 MB of text on a
737-page book is more than half a texture budget spent on words, and the
fixes above are small and testable.

### What not to do

- **Drop `bounds` and read them back from the engine when drawing.**
  Highlight rectangles are drawn every frame for every visible page, and
  reading glyphs again takes 0.7 ms a page through the text thread — off the
  frame's thread and out of its budget, for a saving an LRU would have made
  anyway whichever page was in view.
- **Spill the text to a file and map it**, as the document bytes are (the
  2026-11-14 mapping). Reading a page is 0.7 ms; a mapped cache would add a
  file per document, a lifetime and a failure path for less than a
  millisecond.
- **Compress the text.** It is 1.6 bytes a character already, 2% of the
  problem.
- **Index words and drop the text.** Copy, selection and highlight geometry
  all need the characters and their glyphs; the text is the cheap part.

## What a change would look like

- `Texts` (app) keeps an LRU keyed by bytes, not pages, so a page of
  pictures costs less than a page of prose; `forget` already exists and the
  text thread already accepts `Command::Forget(page)`, so the eviction path
  is there.
- The worker keeps two kinds of entry: asked for by the app (kept until
  forgotten) and read for its own purposes (dropped when the walk moves on).
  `extract` stops meaning "read all" and starts meaning "count all", or the
  counts are read through a walk of their own that keeps nothing.
- `PageText.forms` becomes `Mutex<Option<Forms>>` (or the forms move out of
  `PageText` into the worker's job, which suits their lifetime better), with
  one form per query and source indices as `u32`.
- `CharInfo` becomes `u32` (glyph index, `u32::MAX` for an inserted space or
  newline) with `line_starts: Vec<u32>` beside it. `line_of` binary-searches
  the starts — its only caller is the hit rectangles
  (`crates/noido-text/src/search.rs:247`) — and `lines()`, which unites each
  line's glyphs for link targets (`page.rs:166`), walks line ranges rather
  than every character, which is what it already does the hard way.

## Open questions

- Is 12–32 MB the right bound for the app's text? A selection drag across
  forty pages would thrash a small one; the cost is 0.7 ms a page, so a
  generous bound (32 MB, some 200 pages of prose) is cheap insurance.
- Should a query abort the moment it has a hit on the visible page, and let
  the rest fill in behind? It already reports pages as it goes; with a
  streaming walk the first hit is no slower, but the walk's total time
  doubles (0.69 + 0.07 ms a page).
- Do the counts need a walk of their own, or can the search walk report
  them? Today `Count` reads every page to count it; keeping `Counts` and
  dropping the text is enough, and `Counts` fits in a `u32` a kind.
