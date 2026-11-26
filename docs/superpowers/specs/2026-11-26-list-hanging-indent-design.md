# List items with a hanging indent

## Why

A list in an EPUB or Markdown file is set with its marker in the text
("• We will have…") and every line at the margin. Every browser and
reader indents the item and hangs the marker to the left of it, which
is how the publisher expects it to look (a plain `<ul>` in a popular
science book, with no list CSS, showed the problem).

## Goals

- A list item's lines are set at a narrower measure, inset from the
  left by no more than its markers need: a book page has no room to
  spare, and a marker floating in from the margin reads as a paragraph
  indent.
- The list's widest marker sits flush with the margin (for a nested
  list, with its parent item's text); the others are right-aligned
  against it, so "9." and "10." line up. A small gap separates marker
  and text.
- A nested list is inset by its own markers' width from its parent
  item's text.
- Every paragraph of an item is inset; only the first carries the
  marker.
- A figure that is a list item is inset too, its marker hung as for
  text, raised to the figure's top.

## Non-goals

- Paragraphs that only look like lists (`<p class="indentbullet">• …`,
  `<span class="list_ornament">`): their indent comes from publisher
  CSS, which is E2's (house style) to decide on.
- List CSS: `list-style-type` (none, roman, alpha), `<li value>`. E2.
- Block quotes: unindented as now. The inset this change adds is the
  mechanism they can use later.
- Markdown task boxes stay in the text after the marker.
- A footnote's number ("1 ") stays in the text as now.

The page was first built with a fixed 2 em step per level and the
marker right-aligned in it, as browsers do; on the page that wasted
space and put a bullet about 1 em in. This is our own house style;
weighing it against author styles is E2's.

## Approaches

1. **Inset plus a marker set apart (picked).** The item is set at the
   measure less its inset and moved right by it; the marker is set on
   its own and placed in the inset. Numbers align, the marker takes no
   part in justification or hyphenation, and the inset is a general
   mechanism E2 can reuse for quotes and CSS margins.
2. Negative first-line indent, as CSS does: the marker stays in the
   text, padded to a fixed width, and the first line starts left of the
   others. Less code, but numbers can't be right-aligned and the padded
   marker is part of the first line, stretched by justification.

## Design

- `Para` gets an inset (points from the left of the measure) and the
  list marker moves out of `prefix` into its own field. `prefix` stays
  for a footnote's number.
- The paragraph builder measures a list's markers in the body face
  (through the shaper) and insets its items by the widest plus
  `MARKER_GAP`, 0.5 em, from where the list starts: the margin, or the
  parent item's text. Each marker's right edge sits `MARKER_GAP` before
  the text. The inset is capped at half the measure, so a deeply nested
  (or malformed) list cannot run off the page.
- An item that opens with a list of its own hangs both markers, each at
  its own level.
- Lines are set at `MEASURE − inset`; their glyphs, decorations and
  links are moved right by the inset. The marker is set as body text and
  placed on the first line's baseline. It keeps no source span.
- The layout key in `house.rs` changes, since line breaks move and saved
  places re-anchor.

## Left to the builder

Where the offset is applied (glyph x, or an x on `SetLine`), and how
the figure-item case shares the marker placement.
