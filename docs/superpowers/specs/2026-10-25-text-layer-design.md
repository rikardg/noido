# Text layer and search engine

Date: 2026-10-25 · Status: draft for review

Part 1 of 2 of search. Part 2, the search UI, is
`2026-10-25-search-ui-design.md`; it builds on what this part produces.

## Intent

noido can't read the text of a page yet. Search needs it now; text
selection, copy and anchoring notes to text need it later. This part pulls
each page's text out with character positions, makes it searchable the way
agreed for search, and runs searches on a thread of their own so the window
never waits. It has no UI; its tests run headless.

The design document puts this in core: "Search, selection and note anchoring
are built in `core` on `PageText`, not on engine-specific APIs", and names a
`noido-text` crate ("text extraction, reading order, search; usable
standalone"). The investigation `docs/investigations/2026-10-25-text-extraction.md`
found hayro's text good enough and lists the assembly work that is ours.

## Behaviour of search (agreed)

- **Case is ignored.** No smart case: "Kant" also finds "kant".
- **Normalisation:** ligatures and compatibility characters match their plain
  letters (`ﬁ` matches "fi"), as NFKC does.
- **Accents:** a search without accents ignores them ("godel" finds
  "Gödel", "cafe" finds "café"); a search with accents matches them exactly
  ("gödel" doesn't find "godel"). Decided per search, by whether the query
  has any accent at all.
- **Across line breaks:** a match may span lines. A word broken by a hyphen
  at a line end is found whole ("phenom-" / "enology" matches
  "phenomenology"), and so is one broken by OCR's `¬` marker. Hyphens that
  belong to the word ("self-induced") stay.
- **No whole-word option.** A match may start or end inside a word.
- **Whitespace:** any run of spaces and line breaks in the text matches any
  run of whitespace in the query.

## Approaches considered

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. A text thread that extracts and also searches | UI never holds or scans text; results stream per page; a new query cancels the old one between pages. One more parsed copy (1–3 MB) plus the text (10–20 MB for 600 pages) | **Chosen** |
| B. A text thread that only extracts; matching in the UI | Matching is fast (ms per book), selection could read text directly; but the UI holds all the text and the "pages extracted so far" logic lives in UI code | Rejected |
| C. Extract on the thumbnailer or the sidebar renderer | No new thread; but a search would hold up thumbnails while the reader scrolls the sidebar | Rejected |

## What this part builds

### 1. Glyphs from the engine (`noido-engine`, `engine-hayro`)

A new `Document` method returns a page's glyphs in content-stream order. Each
glyph has its Unicode text (possibly several characters, possibly none),
its outline as a quad of four corners in page points (the same space and
orientation as `page_size` and rendering: origin top-left, y down, page
rotation applied), and whether it is invisible (OCR layers). Engines that
can't give text return no glyphs.

The quad spans the glyph's advance horizontally and the font's em box
vertically around the baseline (ascent and descent from the font where hayro
gives them, else 0.8 em above and 0.2 em below: my call). A glyph without an
advance (Type 3 fonts) gets 0.5 em, as in the prototype. Quads rather than
rectangles keep rotated and slanted text (the handwritten letter in Vaesen)
correct for later selection; rectangles are derived from them.

hayro's `begin_marked_content` doesn't pass ActualText, so ligature-heavy
books lose letters (backlog item). Not fixed here; the glyph type should let
a run carry replacement text later without changing callers.

### 2. Page text (`noido-text`, new crate, pure)

`noido-text` depends only on `noido-engine`'s types. From a page's glyphs it
builds a `PageText`:

- **Lines and words** in content-stream order. A new line starts on a
  vertical jump or a move left (as in the prototype). A space is inserted on
  a gap wider than 0.1 em unless the text already has one; explicit space
  glyphs count. Content-stream order was right for every test file,
  including two columns; no geometric reordering in this part.
- **Combining marks drawn as separate glyphs** ("ı" + "¨" in "naı̈ve") join
  the base glyph they overlap, so the text reads "naïve".
- **Every character maps back to its glyph**, so a range of text gives the
  glyph quads to highlight. A ligature glyph's characters all map to the
  ligature glyph.
- **The page's plain text** (lines joined with newlines) is available, for
  context lines now and copy and export later.

### 3. Matching (`noido-text`)

Each `PageText` keeps two search forms of its text, built once:

- **folded:** NFKC, lowercase, whitespace runs collapsed to one space,
  line-end hyphens and `¬` removed where they break a word;
- **folded, unaccented:** the same, with combining marks removed after
  decomposition (NFD), so "ö" becomes "o".

Each search form keeps a map from its characters back to the page text's
characters. The query is folded the same way; if its decomposition has any
combining mark it is matched against the folded form, else against the
unaccented one. Matching is plain substring search; hits don't overlap.

A line-end hyphen is removed for search when the line ends in "-" (or "¬")
right after a letter and the next line starts with a lowercase letter. This
joins "inter-" + "preter" and keeps "Anglo-" + "Saxon"; it wrongly joins a
real hyphenated compound broken at its hyphen ("self-" + "induced" becomes
"selfinduced"). Accepted for now: searching "self-induced" across that break
then fails, which is rare. (My call; revisit with a dictionary if it bites.)

A hit gives:

- its page and its range in the page text;
- its highlight areas: one rectangle per line it touches, the union of its
  glyphs' quads' bounds on that line, in page points;
- a context line: the page text around the hit, about 40 characters before
  and 80 after, cut at word boundaries, single line (newlines become
  spaces), with the hit's range in it so the UI can bold it.

Lowercasing is `char::to_lowercase` (not full Unicode case folding, so "ß"
doesn't match "ss"): my call; good enough for the test files.

### 4. The search thread (`noido-render` or a new module next to it)

A `TextSearcher` started like the thumbnailer (own thread, own parsed copy,
`start_background`-style so it never blocks, with a waker), but idle until
asked:

- `extract()`: start extracting pages, if not started. Part 2 calls this when
  the search field first gets focus. Pages are extracted in order from the
  first page and kept for the life of the document.
- `search(query, from_page)`: replaces any running search. The thread
  searches every page, starting at `from_page` and wrapping around, so the
  hits nearest the reader come first. Pages not yet extracted are extracted
  on the way. It checks for a newer query between pages and drops the old
  one.
- `try_recv()`: results, each tagged with its query's generation so stale
  ones can be dropped: per page searched, its hits (possibly none); and a
  final "done" once every page has been searched.
- An empty query cancels and sends nothing.
- **At most 5 000 hits per search** (my call). With no minimum query
  length, one letter on a 600-page book could give a million hits and
  well over 100 MB of context lines. The search stops at the cap and its
  "done" says it was cut short, so the UI can show "5000+". Since the search
  starts at `from_page`, the kept hits are the ones nearest the reader.
- A page whose text fails to extract counts as searched with no hits; the
  error goes to stderr, like render errors.
- A reload brings a new `TextSearcher` along with the other renderers.

Starting the thread when the document opens but extracting only on
`extract()` keeps opening as cheap as today.

## Non-goals

- The search UI, highlights on screen, keys: part 2.
- ActualText (backlog).
- Text selection, copy, export, note anchoring: later features on this layer.
- Geometric reading order, multi-column detection.
- Full Unicode case folding, whole-word search, regular expressions.
- Saving extracted text between runs.
- OCR.

## Testing

- `noido-text` unit tests from hand-made glyph lists: line and word
  assembly; combining marks; the character-to-glyph map; each matching rule
  above (case, NFKC ligature, accents both ways, across lines, de-hyphenation
  both kept and joined, `¬`, whitespace runs); hit rectangles on one and two
  lines; context lines with the hit range, at page start and end.
- `engine-hayro` tests on a new fixture page with known text in a standard
  font (added to `fixtures/make_fixtures.py`): the glyphs' text, quads within
  the page and in reading order; and on the rotated fixture page.
- `TextSearcher` tests with a fake engine: nothing extracted before
  `extract()` or `search()`; results start at `from_page` and wrap; a newer
  query drops the older one's results; "done" after the last page; a failing
  page counts as no hits.
- An ignored test or example (not in CI) that runs extraction and a few
  searches over `../testfiles` and prints timings, to compare with the
  investigation.

## Left to the builder

- Names of types and methods, and whether the searcher lives in
  `noido-render` or `noido-text` (the latter would then need a thread and
  the engine trait).
- The exact data layout of `PageText` and the maps.
- The Unicode normalisation crate (`unicode-normalization` is the obvious
  one).
