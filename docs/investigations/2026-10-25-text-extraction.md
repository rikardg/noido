# Text extraction with hayro: investigation

Date: 2026-10-25

## Question

Is hayro's text extraction good enough for search, selection and anchoring
notes, or do we need another engine (PDFium/poppler) for text?

## Answer

**Yes, with one gap to close.** hayro gives every glyph with its Unicode
text and its exact position, fast, including invisible OCR text. Quality
matches `pdftotext` on four of five test files. The gap is **ActualText**,
which hayro reads but doesn't pass on; it matters for books typeset with
ligatures (the Vaesen book loses letters without it). That needs a small
change in hayro (upstream or a patch), not a different engine.

Everything else that differs from `pdftotext` is assembly work that is ours
anyway: joining glyphs into words and lines, de-hyphenation, ligature
normalisation, accents, reading order.

## How hayro exposes text

- `hayro_interpret::interpret_page` drives a custom `Device`;
  `draw_glyph_run` receives positioned glyphs. No image decoding happens for
  a device that ignores images, so extraction is cheap.
- Per glyph: `as_unicode()` (ToUnicode → glyph name via the Adobe Glyph List
  → `uniXXXX` names), `transform()` (combined with `DrawProps::transform`
  gives page position and size), `advance_width()` for outline glyphs.
- Invisible text (render mode 3, i.e. OCR layers) arrives with
  `DrawMode::Invisible`.
- Marked content: `begin_marked_content(tag, mcid)` only; **the properties
  dict, and so ActualText, is not passed**.

Prototype: `crates/engine-hayro/examples/extract_text.rs` (glyphs in
content-stream order; line break on a vertical jump or moving left; space on
a gap > 0.1 em). `DUMP=1` prints every glyph.

## Results

| File | Kind | hayro | pdftotext | Notes |
| --- | --- | --- | --- | --- |
| tracemonkey.pdf (14 p) | born-digital, two columns | 35 ms, 13 418 words | 127 ms, 13 316 | Same text and column order. |
| Vaesen (240 p) | art-heavy, ligature fonts | 334 ms, 97 197 | 1 164 ms, 94 903 | Order better than pdftotext on a slanted handwritten letter; letters lost to ligatures (see below). |
| Vehicles (163 p) | scan, cover-only OCR | 4 ms, 24 | 18 ms, 27 | Nothing to extract; see OCR in backlog. |
| Dennett, Elbow Room (220 p) | scan + good OCR (Internet Archive) | 157 ms, 91 476 | 261 ms, 91 375 | All invisible text; same quality. |
| frankfurt1969.pdf (12 p) | scan + OCR (JSTOR) | 14 ms, 5 729 | –, 5 634 | Same text, including OCR errors. |

Unicode coverage: of about 1.2 million glyphs, 13 had no Unicode.

## Differences found, and who owns them

1. **ActualText (hayro gap).** InDesign maps ligature glyphs to only their
   first letter in ToUnicode and puts the real text in
   `/Span <</ActualText (…)>> BDC … EMC` (8 568 times in Vaesen). The
   handwriting font has ligatures like `o_n`, `T_h`, `w_e`, so "pond" comes
   out as "po d". `pdftotext` uses ActualText and gets it right. Likely also
   behind full stops coming out as U+FFFD in Vaesen's body font (not
   verified). Fix: extend hayro's `begin_marked_content` to pass the
   properties (or the ActualText string) — a small upstream PR; patch
   locally until it lands.
2. **Word spacing (ours).** A fixed gap threshold misses tight spaces on
   justified lines (0.2 em lost words, 0.1 em works here). A robust rule
   should use the font's space width and the line's typical gap, and respect
   explicit space glyphs.
3. **De-hyphenation (ours).** Join "inter-" + "preter" across line ends for
   search, and OCR's `¬` line-end marker (771 times in Dennett), while
   keeping real hyphens ("self-induced"). Most remaining word-count
   differences in tracemonkey are this.
4. **Ligature characters (ours).** `ﬁ` (U+FB01) must be normalised (NFKC)
   for search; selection/copy should give "fi".
5. **Accents placed as separate glyphs (ours).** "naı̈ve" is drawn as "ı" plus
   a diaeresis; attach combining marks to the base glyph they overlap.
6. **Reading order (ours).** Content-stream order was right for all files
   tried (including two columns); `pdftotext`'s geometric analysis did
   worse on Vaesen's slanted text. Start with stream order plus line
   grouping; add geometric ordering only for files that need it.
7. **OCR position vs. scan (inherent).** In scans the invisible text only
   approximately overlays the image; selection and highlight rectangles
   will be a bit off there.

## Recommendation

Keep hayro for text. Next steps when text work starts:

1. ActualText in hayro (upstream PR, local patch meanwhile).
2. A `noido-engine` text API: per page, glyphs with Unicode, quad and
   flags (invisible), assembled into words/lines by our own code, with
   per-word quads for selection and note anchoring.
3. Search on top: normalised (NFKC, case-folded), de-hyphenated text with a
   map back to glyph quads.
