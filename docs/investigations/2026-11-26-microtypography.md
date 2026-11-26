# Microtypography: what noido has, what's missing, and the way to typst's and LaTeX's level

Date: 2026-11-26

Status: survey. Nothing has been built. One measurement was made:
where layout time goes today (see "What it would cost in time").
noido's side was read from main (`klwzznpk`). Typst's was read from the
source of typst-layout and typst-library 0.15.1, the versions the
typesetting prototype used (`2026-11-15-typesetting.md`). The OpenType
features of the bundled fonts were listed with fontTools. LaTeX's side
comes from the microtype manual and TeX's documented defaults, not from
a run.

## Question

How hard would it be to add microtypography to the typesetter? Which
features are there, which are already on the backlog, which are quick
wins and which aren't? What would it take to match typst, and to match
LaTeX with microtype?

## Answer

- **Much of it is already built.** Knuth–Plass line breaking,
  hyphenation in the document's language, widow and orphan control,
  kerning and ligatures, and smart punctuation in markdown all came
  with the typesetter (Y3).
- **Matching typst's defaults is cheap.** It takes a cost for a short
  last line, typst's hanging punctuation (which is applied after the
  lines are broken, the cheap way) and quotes in the document's
  language. Each is a small change in `noido-typeset`. Old-style
  figures are a cheap house-style addition on top; typst leaves figures
  to the font. Letter-spacing in justification is configurable in typst
  but off by default, so it isn't needed to match typst's defaults.
- **Matching LaTeX with microtype costs more.** On top of the typst
  steps it needs hanging punctuation counted while lines are broken,
  font expansion, TeX's stronger costs for hyphens, a cost for a
  hyphen at the foot of a page, and pages that all end on the same
  line. TeX's costs are quick, font expansion and even page bottoms are
  the expensive two, and the rest is medium.
- **Nothing on the backlog covers this.** The typesetter spec lists
  "hanging punctuation, font expansion" as non-goals for later. The
  review minors in #119 are line-breaking edge cases, worth doing
  alongside this but not microtypography themselves.
- **It costs little time.** The parts it touches (shaping, line
  breaking, placing lines) are about 30% of layout on a long novel, and
  Knuth–Plass itself is 3.5%. Everything here together should cost well
  under 10% of layout, and drawing nothing measurable. Bookkeeping
  after layout costs more than all of it would (#120).
- **No collision with other work.** repo-45's EPUB image work has
  merged, and so have repo-ca's hanging indents for list items, which
  changed `set_width` in `lines.rs` (the line width that `set_text`
  breaks and places lines in). Build on main after those.

## What noido has today

| Feature | Where | Notes |
|---|---|---|
| Knuth–Plass line breaking | `noido-typeset/src/kp.rs` | Badness and demerits in TeX's units. Line penalty 10 (TeX's), extra cost for two hyphens in a row 3000, for a sudden change between tight and loose lines 100. The last two are the paper's example values; TeX's defaults are 10000 for both. Ratio tolerance 2 (badness 800; LaTeX's default `\tolerance` is 200), then relaxed in passes: 4, 20, anything. |
| Word spacing | `lines.rs`, `set_text` | A space stretches by half its width and shrinks by a third: close to TeX's fonts, and the same as typst's default word-spacing limits (2/3 to 150%). A paragraph's last line and ragged lines aren't stretched. |
| Hyphenation | `lines.rs`, `soft_points` | hypher's patterns in the document's language. Hyphen penalty 50 (TeX's; typst uses 135). None in code, headings, or words under 5 letters. A last-resort break inside a word too long for the line costs 5000. |
| Widows and orphans | `pages.rs` | Hard rules: no single line at a page's foot or head; a heading keeps at least two lines of what follows on its page. |
| Kerning, ligatures | parley's shaping | On by default. A ligature is one glyph that keeps all its characters for selection. |
| Smart punctuation | `noido-blocks/src/markdown.rs` | pulldown-cmark's smart punctuation: curly quotes, en and em dashes, ellipses. English quotes only. EPUBs usually arrive with these already in the text. |
| Superscripts, subscripts, note numbers | `lines.rs` | The ordinary glyphs at 70% size, raised or lowered. |

### Line-breaking costs side by side

| Cost | noido | typst 0.15 | TeX / LaTeX |
|---|---|---|---|
| A line's own cost (fewer lines win) | 10 (`line_penalty`) | 1 | 10 (`\linepenalty`) |
| A hyphen | 50 | 135, plus 15% for each letter it falls short of 5 from either end of the word | 50 (`\hyphenpenalty`) |
| Two hyphens in a row | 3000, added to the demerits | one more hyphen cost (135), inside the square, and for any two dashes | 10000 (`\doublehyphendemerits`) |
| A hyphen on a paragraph's last full line | 3000: the paragraph's final break is flagged, so this shares the two-hyphens cost | none | 5000 (`\finalhyphendemerits`) |
| A break after a written dash (`-`, –, —) | 0, and not counted as a hyphen | counted among dashes for the two-in-a-row cost | 50 (`\exhyphenpenalty`), and counted as a hyphen for the two costs above |
| A tight line next to a loose one | 100, between classes two apart | none (no classes) | 10000 (`\adjdemerits`) |
| A short last line | none | 100: the last line is a single word | none (only tricks and report-only packages) |
| Tolerance | ratio 2 (badness 800), relaxed to 4, 20, any | no fixed tolerance; overfull lines cost 1 000 000 | `\pretolerance` 100 without hyphens, then `\tolerance` 200; `\emergencystretch` 0 |
| A hyphen at the foot of a page | none | none | 100 (`\brokenpenalty`) |
| Widow, orphan | hard rules: never | prevented outright unless the cost is 0% | 150 each (`\widowpenalty`, `\clubpenalty`) |

How the numbers combine differs, so they don't compare one to one.
noido follows TeX: a line's demerits are (line + badness)², plus the
penalty squared, plus the extra costs added after squaring. typst puts
everything inside the square, (1 + badness + costs)², and a ratio over
1 is measured against half an em per gap rather than the glue's
stretch. typst's own comment says it raised the hyphen cost from the
paper's 50 because it hyphenated too eagerly otherwise. noido's 3000
and 100 are the Knuth–Plass paper's example values, not TeX's
defaults.

The bundled fonts are capable: Libron (the body face) has `onum`,
`smcp`, `c2sc`, `sups`, `subs`, `frac` and `kern`. Libertinus Serif and
Sans also have `cpsp` (extra spacing for capitals) and `dlig`. NewCM has
most of Libertinus's set, but not `sups` or `subs`; Libron also has
`case`, `lnum`, `pnum` and `tnum`.

## What's missing

| Feature | Effort | Changes line breaks? |
|---|---|---|
| A cost for a short last line | Quick | Yes |
| A separate cost for a hyphen on a paragraph's last full line (today it shares the two-hyphens cost) | Quick | Yes |
| A cost for breaks after written dashes | Quick | Yes |
| A first pass without hyphenation, as TeX's `\pretolerance` | Quick | Yes |
| TeX's stronger costs for hyphens and tight/loose changes | Quick (tuning) | Yes |
| Old-style figures | Quick | Yes (widths change) |
| True superscripts and note numbers (`sups`) | Quick to medium | Barely |
| Quotes in the document's language | Quick to medium | Rarely |
| Hanging punctuation, applied after breaking | Quick to medium | No |
| Hanging punctuation, counted while breaking | Medium | Yes |
| A cost for a hyphen at the foot of a page | Medium | No (page breaks only) |
| Letter-spacing in justification, within limits | Medium | Yes |
| Non-breaking and thin spaces by language rule | Medium | Yes |
| True small caps | Medium (waits on E2) | Yes |
| Widows and orphans as costs, not hard rules | Medium | No (page breaks only) |
| Font expansion | Medium to hard | Yes |
| Pages that all end on the same line | Medium to hard | No (page layout only) |

### Quick

- **Short last line.** typst adds a cost (100 by default) to a line
  that ends at a forced break (a paragraph's end or a hard line break)
  and has no break point inside it: a single word. In `kp.rs` this is a
  check on the line's content when a forced break is scored. LaTeX has no direct switch. The usual tricks are a
  non-zero `\parfillskip` or packages that only report short last lines
  (lua-typo).
- **Final hyphen, as a cost of its own.** noido already charges a
  hyphen on a paragraph's last full line: the final break is flagged
  (`lines.rs`, the last penalty in `set_text`), so `flagged_demerits`
  (3000) applies, as in the Knuth–Plass paper. TeX prices it on its own
  (`\finalhyphendemerits`, 5000). Splitting it out of `flagged_demerits`
  is one more parameter in `kp::Params`.
- **Breaks after written dashes.** noido breaks after `-`, – and — for
  free and doesn't count them as hyphens. TeX charges 50
  (`\exhyphenpenalty`) and counts them for the two-hyphens and final
  hyphen costs; typst counts them for its two-in-a-row cost. A flag and
  a penalty on the existing item in `set_text`.
- **Tuning.** The two-hyphens cost and the tight/loose cost are the
  paper's example values (3000, 100). TeX uses 10000 for both, and
  typst prices a hyphen at 135 against our 50. Compare by eye on a
  novel and a paper from the test files before choosing.
- **Old-style figures.** `StyleProperty::FontFeatures` with `onum` for
  body text in parley. Keep lining figures in headings, tables and code.
  Libron, Libertinus and NewCM all have the feature.

### Quick to medium

- **True superscripts.** Shape note numbers and `<sup>` with `sups`
  where the font covers the characters, and fall back to today's scaled
  glyphs elsewhere. Libron and Libertinus have `sups` and `subs`; NewCM,
  the last fallback, doesn't. Checking coverage
  needs the font's substitution table, since parley doesn't report
  whether a feature applied.
- **Quotes in the document's language.** Swedish ”…” and ’…’, German
  „…“, French « … » and so on. Markdown's curly quotes are put in by
  pulldown-cmark, which emits ’ both for apostrophes and for closing
  single quotes. A blind character map would therefore turn "geht’s"
  into "geht‘s" in German, whose closing single quote is ‘. The pass has
  to tell opening, closing and apostrophe apart, either from
  pulldown-cmark's events or by doing the quoting itself. Swapping one
  character for one keeps each glyph's source range. French « … » takes
  a narrow non-breaking space inside (typst inserts U+202F), which adds
  characters; that part belongs to the language-rules item below.
- **Hanging punctuation, applied after breaking.** This is typst's
  approach (`typst-layout/src/inline/line.rs`, `overhang`). Lines are
  broken without it. Then, when a line is placed (justified or not), it
  is widened by the share of its last glyph's width that may hang into
  the margin, and a justified line's spaces are stretched again to fill
  it. typst's table: `.` and `,` 0.8, hyphen and soft hyphen 0.55, `:`
  and `;` 0.3, en and em dash 0.2, Arabic comma and full stop 0.4. It
  hangs only at a line's end, in either writing direction (on the left
  in right-to-left text); opening quotes and parentheses never hang. In
  noido the place for it is the loop in `set_text` that places glyphs (after the `kp::breaks` call,
  around `lines.rs:860`). Two details there: an added hyphen is pushed
  after that loop, so it isn't the line's last box; and trailing spaces
  are emitted at zero width after the last glyph, so "the last glyph"
  has to skip them. It must skip inline images and figure lines. Line
  breaks don't change, so `LAYOUT_KEY` stays the same.

### Medium

- **Hanging punctuation, counted while breaking.** This is how pdfTeX
  and microtype do it. A line ending in a period may then be a little
  longer, which can change where it breaks. In Knuth–Plass that means
  letting a box (and a hyphen's penalty) carry how far it may hang, and
  widening the line by that much when the line ends there. Add opening
  quotes and parentheses hanging at the line start, with microtype's
  table as the reference. Interacts with repo-ca's list indents, which
  move where a line starts.
- **A hyphen at the foot of a page.** TeX's `\brokenpenalty` (100).
  `pages.rs` is a greedy first-fit page breaker with hard rules and no
  costs, and knows only line heights. A hard-rule version is quick to
  medium: mark the lines that end in a hyphen, and move such a line to
  the next page when the widow and orphan rules still hold. A weighted
  version, as TeX's, first needs a cost model in the page breaker,
  shared with the next item.
- **Letter-spacing in justification.** typst's
  `par.justification-limits`: letter spacing may change within a range
  (typst's default range is zero; its documentation's example uses
  −0.01 em to +0.02 em). Word and letter spacing stretch together, each
  in proportion to its limits; only what's left once both are used up
  goes to the spaces. The letters' stretch also counts while lines are
  broken. In noido,
  words would carry stretch and shrink of their own (today only spaces
  do), and placing a line would spread its letters. A ligature must
  stay one glyph, and inline images take no spacing. microtype's
  tracking is a separate feature, mostly used for small caps and
  letter-spaced text.
- **Spaces by language rule.** A narrow non-breaking space before
  French `;:!?` and inside guillemets. Non-breaking spaces after single
  letters in Czech and Polish. Thin spaces around em dashes where the
  house style wants them. repo-45 noted that such added or changed
  characters must keep each glyph's source range on the original
  characters, or highlights and reading positions drift
  (`locations_do_not_move` and the EPUB location tests guard this).
  Inserted characters get no source (`drawn: None`), as added hyphens
  and list markers already do.
- **True small caps.** `smcp` and `c2sc` are in the fonts, but nothing
  marks text as small caps: markdown has no syntax for it, and the
  EPUB reader doesn't read the book's CSS yet (E2). Once E2 lands it's
  quick.
- **Widows and orphans as costs.** Today they are hard rules. TeX
  weighs `\widowpenalty` and `\clubpenalty` (150 each in LaTeX) against
  other page-break costs; typst prevents both outright unless their
  cost is set to 0%. The hard rules match typst, so this is only
  needed to match LaTeX, and isn't worth it on its own. Like the
  weighted foot-of-page hyphen, it needs the page breaker to weigh
  costs instead of applying rules.

### Medium to hard

- **Font expansion.** pdfTeX and LuaTeX may make each line's glyphs up
  to about 2% wider or narrower (microtype's default range), which
  evens out word spacing. The cost is in the line breaker: boxes that
  stretch and shrink, as with letter-spacing, and lines that choose a
  scale. Drawing is the easy part, one more factor in the glyph
  transform in `draw.rs`, and selection and search follow on their own
  if each `SetGlyph`'s advance holds the scaled width. The gain over hanging punctuation and good hyphenation is small and
  visible mostly at narrow widths. typst doesn't have it.
- **Pages that all end on the same line.** LaTeX's `\flushbottom`:
  stretchable space between paragraphs and around headings, stretched
  so the last line sits at the page's foot. `Flow` would need its space
  before and after to stretch, and page breaking would need to weigh
  that stretch. typst doesn't have it either.

### Smaller items not rated above

- **Wider space after a sentence.** Plain TeX and LaTeX default to
  `\nonfrenchspacing`: more space and more stretch after a full stop,
  a little more after a comma. Part of matching LaTeX's look (microtype's
  `spacing` only extends it); many book styles set the same space
  throughout, as noido and typst do. Quick: the space's width and
  stretch in `set_text` by the character before it.
- **Letters before and after a hyphen.** TeX's `\lefthyphenmin` and
  `\righthyphenmin` (2 and 3 for English); typst charges extra near
  a word's edges. noido relies on hypher's own bounds and its 5-letter
  word minimum. Quick to check, quick to tune.
- **Tabular figures in tables** (`tnum`), so columns of numbers line
  up. Quick, with the same feature setting as old-style figures.
- **All-caps text:** case-sensitive forms (`case`, hyphens and brackets
  raised to capital height) and capital spacing (`cpsp`). Quick once
  something marks text as all caps, which, like small caps, waits on
  E2.
- **Engines differ in LaTeX.** XeTeX supports only hanging punctuation
  of microtype's features; expansion needs pdfTeX or LuaTeX, and extra
  kerning and character-dependent spacing need pdfTeX. "LaTeX" in this
  document means pdfLaTeX or LuaLaTeX.

## Matching typst

typst 0.15's defaults have: optimized line breaking with costs for
hyphenation (135) and short last lines (100); hanging punctuation at
line ends (`text.overhang`, on by default, applied after breaking);
widows and orphans prevented; smart quotes in the document's language;
kerning and ligatures. As options: letter-spacing in justification,
number styles, small caps and any OpenType feature.

To match its defaults, in order:

1. A cost for a short last line, and a hyphen cost compared by eye
   against typst's 135.
2. Hanging punctuation after breaking, with typst's table.
3. Quotes in the document's language.
4. Old-style figures, as noido's house style (typst leaves figures to
   the font).

All four are quick. Steps 1, 3 and 4 can move line breaks (new quote
glyphs have other widths), so they bump `LAYOUT_KEY`. Letter-spacing in justification would match typst's
options, not its defaults; it's medium and can wait.

## Matching LaTeX with microtype

pdfLaTeX and LuaLaTeX with microtype turn on hanging punctuation and
font expansion by default. Letter-spacing ("tracking") is opt-in,
mostly for small caps. pdfTeX also offers extra kerning (for example
the French space before `;:!?`) and character-dependent word spacing.
TeX itself adds stronger costs for hyphens and for sudden changes
between tight and loose lines, a cost for a hyphen at the foot of a
page, weighted widows and orphans, and `\flushbottom` in two-sided
classes.

To match that, after the typst steps:

5. TeX's costs: two-hyphens cost 10000, tight/loose cost 10000, a
   separate cost for a hyphen on a paragraph's last full line (5000),
   breaks after written dashes priced (50) and counted as hyphens, a
   first pass without hyphen breaks at about ratio 1 (badness 100, TeX's
   `\pretolerance`; TeX hyphenates less largely because of it), and a
   tolerance nearer LaTeX's 200 (a ratio of about 1.26, against our 2).
   Quick, but compare by counts and by eye: TeX's values were tuned for
   TeX's spacing, not ours.
6. Hanging punctuation counted while breaking, at both ends of a line.
   Medium.
7. A hyphen at the foot of a page: as a hard rule quick to medium, as
   a weighted cost medium, after a cost model in the page breaker.
8. Language rules for spaces: the counterpart of microtype's French
   kerning. Medium.
9. Font expansion. Medium to hard.
10. Pages that all end on the same line. Medium to hard; optional, as
    many book designs leave the bottom ragged.

## The costs deserve a step of their own

The cost table above is the part to settle first in a microtypography
task, before any new feature. The values came from the paper, not from
looking at noido's pages. Every later step (short last lines, final
hyphens, hanging punctuation, letter-spacing) adds a cost to the same
sum, and none can be judged until the costs it competes with are right.

A way to do it:

- **Count, don't only look.** A throwaway measurement run (not a test
  or example in the repo, since those never refer to `../testfiles/`)
  sets a few documents of each kind from the test files (a novel, a
  paper, narrow and wide measures) and reports per hundred lines: hyphens, two or more hyphens
  in a row, paragraphs ending on one word, the spread of adjustment
  ratios (how loose or tight lines are), and lines that needed the
  relaxed passes.
- **Compare three settings:** noido's current values, TeX's, and values
  scaled to match typst's behaviour (its numbers can't be copied, since
  they are combined differently).
- **Then look,** at the pages where the counts differ most, and choose.
- The result is a set of `kp::Params` defaults with the counts that
  justified them recorded next to them, and a `LAYOUT_KEY` bump.

## What it would cost in time

Measured 2026-11-27: one layout of a long novel in markdown (3.2 MB,
2267 pages), release build, macOS arm64, with temporary timers around
each stage (removed again; nothing committed). Code as of `klwzznpk`,
before the list indents. Single run; a second run agreed within 5%.

Layout took 2.5 s, 1.1 ms a page:

| Stage | Time | Share | Touched by microtypography? |
|---|---|---|---|
| Shaping (parley) | 430 ms | 17% | Slightly: OpenType features |
| Knuth–Plass (`kp::breaks`) | 89 ms | 3.5% | Yes: costs, extra passes |
| Hyphenation points (`soft_points`) | 77 ms | 3% | Yes, if a first pass skips them |
| Items and placing lines (rest of `set_text`) | 181 ms | 7% | Yes: hanging, letter-spacing, expansion |
| The rest of `set_width` (`Shape::new`, atoms, metrics) | ≈360 ms | 14% | No |
| Assembling pages (glyphs, ink, sources) | 383 ms | 15% | No |
| `page_runs` (the source map per page) | 678 ms | 27% | No |
| Parsing markdown | 60 ms | 2% | No |
| Page breaking (`paginate`) | 0.3 ms | 0% | Page costs; free today |

Drawing an 800 × 1200 tile at scale 2 took 6.7 ms. Microtypography
leaves drawing alone, except font expansion's one extra scale factor
per glyph.

What follows from it:

- The parts microtypography touches are about 30% of layout. If they
  all doubled, layout would be 30% slower. Realistically, everything
  in this document adds well under 10%: more passes or a pass without
  hyphenation multiply only the 3.5% Knuth–Plass share, hanging
  punctuation is a lookup per line, and letter-spacing or expansion
  is arithmetic per glyph in the 7% placing share.
- **A quarter of paragraphs need a relaxed pass.** Of 11 590 calls to
  `kp::breaks`, 8826 succeeded at tolerance 2, 2541 needed 4, and 223
  needed 20. Stricter TeX-like costs or tolerance would push more into
  the later passes. It stays cheap, but the cost-tuning step should
  count it.
- **Bookkeeping costs more than typesetting.** `page_runs` (27%) clones
  each glyph into its own vector to ask whether it is on the page, then
  builds a `PageText` per page. With page assembly that's about 40% of
  opening a book. Recorded on #120; fixing it saves more time than all
  of microtypography would cost.

## Constraints for whoever builds this

- An inline image is an atom with a width and no glyphs, and a figure
  line is a single image glyph. Hanging punctuation, expansion and
  letter-spacing must leave both alone.
- Every glyph keeps its source range. Changed characters map one to
  one; added characters get no source (`drawn: None`), as added hyphens
  and list markers do.
- Anything that moves line breaks bumps `LAYOUT_KEY` in
  `noido-typeset/src/house.rs`, so saved places re-anchor.
- The list indents (merged after this survey began) changed the line
  width and where a line starts, the same code these steps touch.
  Line references above are from before them; check them on main.

## Sources

- noido main (`klwzznpk`): `noido-typeset/src/{kp,lines,pages,house}.rs`,
  `noido-blocks/src/markdown.rs`.
- typst 0.15.1 source: `typst-layout/src/inline/{linebreak,line,shaping}.rs`
  (costs, `overhang`, justification limits),
  `typst-library/src/{text/mod.rs,model/par.rs}` (`overhang`, `costs`,
  `justification_limits`).
- The microtype manual (CTAN), and TeX's defaults for
  `\hyphenpenalty`, `\doublehyphendemerits`, `\finalhyphendemerits`,
  `\adjdemerits`, `\brokenpenalty`, `\widowpenalty`, `\clubpenalty`.
- Bundled fonts' feature lists: fontTools over the faces
  `noido-fonts` ships.
