# Letter-spaced text: no spaces inside words

Date: 2026-11-17 · Status: small change, draft for agreement

## Problem

A search for "eagle eye" finds nothing in a role-playing rulebook,
though the talent heading is there. Its headings are capitals set with
letter-spacing (`Tc`) plus kerning in a TJ array:

    0.036 Tc … 10.5 0 0 10.5 … Tm
    [(E)-69.6 (AG)-17.6 (L)-48.1 (E)-14 ( E)-51 (Y)-73.4 (E)]TJ

noido-text calls any gap over 0.1 em a space. E→A is 0.036 em of
tracking plus 0.070 of kerning, 0.106 em, so the heading reads
"E AGLE EY E". Others on the page do the same: "ELEMENTA RY" (tracking
0.068, A→R 0.112 em in all), "W ELL-TR AV ELED". Preview and Foxit read
them right; pdftotext gets them wrong as noido does.

## What PDFium does

`core/fpdftext/cpdf_textpage.cpp`, within one text object:
`CalculateBaseSpace` and `CalculateBaseSpaceAdjustment` take the
tracking out, so a gap is measured from the tightest the run sets its
letters; only what is left can be a space, against a threshold of half
the font's space width. Without the tracking, every gap in the headings
above is under 0.07 em: the threshold is not what goes wrong.

## Decision

Take the tracking out; keep the 0.1 em threshold.

- **Engine:** `Glyph` gains `run: u32`. Glyphs drawn by one text-showing
  operator (one Tj or TJ: one hayro glyph run) share it; the hayro text
  device counts runs up from 0 per page. hayro gives neither `Tc` nor the
  font's space width, so the run is the most the engine can pass on.
- **noido-text:** before assembling, each run gets its tracking: the
  smallest gap, in ems, between neighbouring glyphs of the run on one
  baseline, leaving out gaps next to a whitespace glyph (word spacing
  `Tw` lands there). A run's tracking is 0 when it has fewer than two
  such gaps, when the smallest is below 0 (kerning that pulls in: plain
  text), or when it is above 0.15 em (letters set that far apart are
  meant as apart, as in a table drawn with one TJ).
- A gap becomes a space when the gap less the tracking is over 0.1 em.
  Between glyphs of two runs, the smaller of the two runs' tracking is
  taken off.

Plain body text (tracking 0, or kerning that tightens) reads as it does
now. Left for later: half the font's space width as the threshold, which
needs hayro to give the width, and gaps between text objects, which
PDFium judges more loosely.

## Testing

- noido-text, failing first: the heading's TJ above as glyphs in one run
  reads "EAGLE EYE"; "ELEMENTARY" with its kerning reads as one word.
- A run with tracking 0 and a 0.2 em gap still gets a space; a run of
  letters 0.3 em apart (over the cap) gets spaces between them; a
  two-glyph run is left as it is; glyphs in different runs take off the
  smaller tracking.
- engine-hayro: glyphs of one TJ share a run; the next Tj gets another.
- Measured by hand: `extract_text` on the files in `../testfiles/`
  before and after, the diffs read through for words wrongly joined;
  the heading found by search in the book.
