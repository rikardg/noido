# Unified locations: where notes and places point, in any format

Date: 2026-11-15 · Status: agreed 2026-11-15 · TODO L1 (the session
list; no issue yet)

## Intent

noido will open reflowable formats (EPUB, markdown) alongside the
fixed-page ones (PDF, and CBZ in a spike). A reflowable document's pages
come from layout, so they change when the layout does. Every position
noido keeps is a page number today, so a highlight or a reading position
in an EPUB would point somewhere else after a reflow.

This change gives every format one kind of location that doesn't depend
on layout, and moves PDF onto it with no change in behaviour. It also
adds a quote to each highlight, so a highlight can find its text again
when the text has moved. PDFs need that too. A PDF is often the end of
a chain (markdown → pandoc → LaTeX → PDF), rebuilt while it's open.
noido then reloads it under a new hash and keeps its state by path, and
today its highlights land on whatever text now sits at the old offsets.

Out of scope: reflow itself (L3), EPUB and markdown, and fuzzy
re-anchoring after edits (M4).

## Today

| What | Type | Where it lives |
|---|---|---|
| Highlights | `Caret { page, char }` × 2 + colour | core `Highlight`; state `start_page`, `start_char`, `end_page`, `end_char` |
| Selection | `Caret` × 2 | core, not saved |
| View centre | `DocPoint { page, x, y }` | core `ViewState.centre`; state `page`, `x`, `y` |
| Back / Forward | `DocPoint` | core `History`; state `back`, `forward` |
| Pins | page numbers | core `Pins`; state `pins` |
| Pagelet | page number | state `pagelet.page` |
| Page rotations | page number | core `PageTransforms`; state `pages` |
| Links, outline | `Destination { page, x?, y? }` | from the engine each time; not saved |

`Caret.char` counts characters of the page's `PageText` (NFC, with the
spaces and newlines that `noido-text` inserts).

## Layout: virtual paper, and when text reflows

Reflowable text is set on **virtual paper**: a page size, a text size
and font, and margins. Reflowable EPUBs suggest no page size, so noido
picks one (a trade paperback, 5.5 × 8.5 in, to start with). The pages
then behave like a PDF's. Resizing the window and zooming magnify them,
and page modes, spreads, fit-width and zooming in on an image work
unchanged.

A layout is either **locked** to its virtual paper or **unlocked**, with
pages as wide as the window:

| | Locked | Unlocked |
|---|---|---|
| Typeset theme | EPUB, typeset markdown (default) | possible later |
| Source theme | raw markdown on paper, a code-like printed feel | raw markdown (default), like `glow` |

Text reflows when:

- the virtual paper changes (page size, text size or font, margins),
  which is rare and deliberate;
- the window's width changes, while the layout is unlocked.

Defaults come first; the settings to change them (E3) and the
lock toggle (M2) come later. Fixed-layout EPUBs declare a size per
page. Those are fixed pages, like PDF, and never reflow.

## Design

### Parts

A **part** is the piece of a document that layout doesn't change. Parts
are numbered from 0:

| Format | Part | Characters counted |
|---|---|---|
| PDF | a page | its `PageText`, as today |
| CBZ | an image | none |
| EPUB | a spine item (a chapter file) | its text content in document order, NFC |
| Markdown | the whole file (one part) | the source |

Markdown counts characters of the source, so raw and typeset mode share
locations, and a location is in the same terms as the file being
edited. In typeset mode the `##` and `_` markers are never drawn, so no
location falls on them. A quote, though, is taken from the drawn text
(see Highlights). The search for a quote therefore runs over the
rendered text, and matches map back to source offsets through the
typesetter's source spans.

### Location

```rust
/// A place in a document's text that layout doesn't move: before
/// character `char` of part `part`.
pub struct Location { pub part: usize, pub char: usize }
```

For PDF, `Location { part, char }` is exactly today's
`Caret { page, char }`.

### Place: where the reader is, or where a jump goes

```rust
pub enum Place {
    /// On a fixed page: a point in its page space (PDF, CBZ).
    Point { part: usize, x: f32, y: f32 },
    /// In reflowable text: the first character in view, in reading order.
    Text(Location),
}
```

Fixed-page documents keep exact points, as today. Reflowable ones keep a
character, because a point means nothing after a reflow.

- **Keeping the place across a reflow:** before it, take the first
  character in view, in reading order. After it, put the line holding
  that character at the same height on screen if the new layout allows,
  otherwise at the top of the view. In a fit-page mode that is simply
  the page holding what was being read. The same rule serves a rare
  settings change and a resize in an unlocked layout.
- **The saved view** of a reflowable document keeps today's centre
  point, plus the anchor `Location` and a key for the layout settings
  (paper, text, margins, theme, lock). If the key still matches on
  opening, the point restores the view exactly. Otherwise the anchor
  places it.
- **Back/Forward, pins and the pagelet** become `Place`s. A pinned PDF
  page is `Point { part, 0, 0 }`. A pin in an EPUB is the first
  character of the page that was showing, and the pin list shows its
  current page number.

Page rotations stay per page. They only exist for fixed pages, and
reflowable documents don't offer rotation. Links and outline targets
stay `Destination`s, made fresh from the current layout every time.

### Mapping: a value the window holds

`Document` isn't `Send`: it lives on the render and text threads, so
the window can't ask it where a location is. The engine hands the
mapping over as a value instead, as it hands over page sizes:

```rust
/// Whether pages come from layout (EPUB, markdown) rather than the file.
fn reflows(&self) -> bool { false }
/// Where each page's characters sit in the parts: per page, runs of
/// characters, each starting at a `Location`. The default makes each
/// page its own part.
fn locations(&self) -> LocationMap { LocationMap::identity() }
```

`LocationMap` answers `location(page, char) -> Location` and
`page_char(Location) -> Option<(usize, usize)>`, and the window keeps
the current one beside the page sizes, replaced on every reflow. It
arrives with L3, where text first reflows. In L2 the conversion is the
identity, written as `From` between `Caret` and `Location`.

The view, history, pins and the pagelet move to `Place` in L3 as well.
For PDF their saved form is the same either way.

Core keeps working in pages and carets inside a session: layout,
selection, drawing and search are untouched. It converts at three
points:

- when a highlight is made (carets to locations),
- when it draws highlights or goes to a place (locations to pages),
- when it saves.

For PDF every conversion is the identity.

### Highlights

```rust
pub struct Highlight {
    pub start: Location,
    pub end: Location,
    pub colour: HighlightColour,
    pub quote: Option<Quote>,
    /// Its text couldn't be found again; kept, not drawn.
    pub orphaned: bool,
}

pub struct Quote {
    pub exact: String,
    /// Up to 32 characters before and after, for telling repeats apart.
    pub prefix: String,
    pub suffix: String,
}
```

The quote is taken when the highlight is made, with `copy_text` across
its pages, so it reads the way copying would. Highlights saved before
this change have no quote. They get one the first time the document
opens unchanged (same hash), so they're covered from then on.

### Finding a highlight's text again

This runs when the document's text may have changed: opened by path
under a new hash (edited in place or rebuilt), or reloaded while open.
For a PDF built from a source that is being edited, that is every
rebuild, so it's a main case, not a rare one. It doesn't run when the
hash matches. For each highlight with a quote:

1. If the text at `start..end` is `exact` and its neighbours agree with
   `prefix` and `suffix`, nothing changes.
2. Otherwise, look for `exact` in the whole document, across parts. Take
   the match whose neighbours agree most with `prefix` and `suffix`,
   then the one nearest the old offset, counted in characters through
   the document (which usually puts the same part first). The highlight
   moves there only if the match is sure:
   - one side agrees in full: all of `prefix`, or all of `suffix`. An
     empty side (the document's start or end) doesn't count, unless
     both are empty;
   - no other highlight holds that same span. When several highlights
     would move onto one span, the one whose neighbours agree most
     takes it (then the nearest). Highlights on one span with one
     quote (two colours, two notes) move together.
3. Otherwise, mark the highlight orphaned. It stays in the state and
   isn't drawn. It comes back if a later version of the file has the
   text again.

When unsure, orphan rather than guess. An orphan keeps its quote and
comes back when its text does, notes and all. A wrong move takes a new
quote, so its old place is lost for good: even putting the text back
doesn't return it. The cost is that text edited on both sides within
the quote's neighbours is orphaned; M4's fuzzy matching is for that.

Text often moves to another part unchanged: a rebuilt PDF pushes a
paragraph onto the next page, or a passage now straddles a page break.
The neighbours decide before the part does, so a repeat of the same
words on the old page doesn't capture the highlight. For the same
reason the prefix and suffix are read across part boundaries.

It runs on the text thread and never holds the whole text. Each
highlight is checked on its own pages first. Only quotes that no longer
match there send a walk through the document, which holds a few pages
at a time, as search does (see the text retention design).

The match is exact, after the same folding search uses for whitespace
and hyphenation, so a word re-wrapped across a line break is still
found. Fuzzy matching, for text that was itself edited, is M4; it
applies to rebuilt PDFs as much as to markdown.

Orphaned highlights are not shown anywhere for now. M4 will make them
common, so it revisits that (a list of notes, or a count).

### Saved form

Highlights move to part-based field names. The old names stay readable
as serde aliases, and the new names are always written, so a state file
switches the first time the new binary saves it:

```toml
[[highlights]]
start_part = 3        # a page for PDF, a spine item for EPUB; reads start_page too
start_char = 120
end_part = 3          # reads end_page too
end_char = 164
colour = "yellow"
quote = "the nature and limits of the power"
prefix = "Civil, or Social Liberty: "
suffix = " which can be legitimately"
# orphaned = true     # only when it is
```

An older binary (such as a separate `~/.cargo/bin/noido`) requires
`start_page`, so it can't read a converted file's entry at all: it
opens the document with no saved state (view, history, pins, pagelet,
rotations, sidebar and highlights) and overwrites the file when it
saves. Update every installed copy (`just install`) before using the
new build on documents with highlights.
Reading the old names goes some time after the branch has landed (TODO
M5).

A `Place` is saved as today's `page`, `x`, `y` for `Point`, and as
`part`, `char` for `Text`. Pins and the pagelet save a `Place` the same
way: a pin as a table when it is `Text`, and as a bare number when it is
a page. The view of a reflowable document adds `anchor_part`,
`anchor_char` and `layout` (the settings key) next to `page`, `x`, `y`.

## Behaviour (what to test)

- A PDF opened with state from before this change: same view, history,
  pins, pagelet and highlights as before. Its highlights gain quotes.
  The file is written with the new field names and reads back the same.
- A PDF edited in place or rebuilt, where the highlighted text moved:
  - on the same page, the highlight follows it;
  - to another page, it follows there;
  - the text was deleted: the highlight is orphaned, still in the state
    file, and comes back when the text does.
- A highlight on text that occurs twice on the page re-anchors to the
  occurrence whose neighbours match.
- A highlight across a page break keeps its quote and re-anchors as a
  whole.
- Text moved unchanged to the next page, while the same words occur
  elsewhere on the old page: the highlight follows its text, not the
  repeat.
- Text moved so it now straddles a page break: the highlight spans it.
- The same words highlighted in two passages with different
  neighbours, and the first passage deleted: its highlight is orphaned,
  not moved onto the second; putting the passage back brings it back.
- Two identical passages, each highlighted, one deleted: one highlight
  stays on the remaining passage and the other is orphaned. Text alone
  can't tell which copy went, so when the remaining one slides into the
  deleted one's place, the deleted one's highlight stays and the other
  is orphaned. Nothing is lost: putting the passage back brings both
  back.
- Text whose neighbours changed on both sides: orphaned.
- CBZ (with the spike): view, history and pins work. Highlights don't
  exist (no text).
- A fake reflowable engine in the core tests: locations survive a
  change of page size, which L3 then uses for the view.

## Relation to W3C Web Annotation

This is the W3C Web Annotation Data Model's approach, and Hypothesis's
anchoring. `Quote` is a `TextQuoteSelector` (the same `exact`, `prefix`,
`suffix`), and `start`/`end` are a `TextPositionSelector`, except that
our offsets count within a part. In that model the part is a
`FragmentSelector` (a page, a spine item) refined by the text
selectors. Hypothesis matches approximately; exact matching here, and
approximate matching in M4. Exporting highlights as Web Annotation JSON
would be a straight mapping; not planned.

## Decisions (2026-11-15)

1. Markdown counts source characters; quotes are searched in the
   rendered text and mapped back.
2. Orphaned highlights are kept and hidden; M4 revisits showing them.
3. Highlights move to `start_part`/`end_part`, reading the old names as
   aliases until M5 removes them.
4. Reflowable text sits on virtual paper, locked by default (raw
   markdown unlocked). Across a reflow the first character in view keeps
   its height on screen.
5. Re-anchoring moves a highlight only to a sure match (one side's
   neighbours agree in full, and no other highlight holds the span);
   otherwise it is orphaned, so a note is never moved to the wrong text.
