# Search results as cards

Date: 2026-10-25 · Status: small change, agreed in conversation

## Intent

One line per match shows too little to tell matches apart. Make each entry
a small card: a header line with the page, then three lines of text with the
match in the middle one. The header also separates the cards.

## Approaches (agreed: 1)

1. **One card per match: header plus three lines.** Fixed card height, so
   the list keeps drawing only the visible cards and following the current
   one by arithmetic. Chosen.
2. Group by page, Preview-style (one header, several excerpts): variable
   heights; later, maybe.
3. A thumbnail beside each card: low-res thumbnails exist only near the
   reader; later, maybe.

## Behaviour

- Header: `Page 389` at the left, `3/10 hits` at the right (the match's
  number among its page's matches, and how many the page has). In the small
  text style and the weak text colour, so lighter and smaller than the
  excerpt. *My call:* a page with one match shows `1 hit`.
- Excerpt: up to three lines, wrapped to the card's width at word
  boundaries, with the line holding the start of the match in the middle.
  When the match is in the first line of the context (start of the page),
  or the last, the three lines are the first or last three; a short
  context gives fewer lines. The match keeps today's highlight.
- The current card has today's selection fill, over header and excerpt.
- Card height is fixed: header, three lines, and a gap; the list's
  scrolling, follow-the-current and click handling stay as they are.

## Code shape

- `noido-text`: a wider context, 150 characters before and 150 after
  (today 40 and 80), so three lines around the match are filled at any
  sidebar width. Still cut at word boundaries, on one line.
- `sidebar.rs`: `result_row` becomes `result_card`. The context is laid out
  wrapped without a row limit; a pure helper picks the character range of
  the three rows around the match from the rows' lengths, and that range is
  laid out again, three rows at most. Row height from the fonts:
  header + 3 × body row + margins.

## Testing

- `noido-text`: the context reaches about 150 characters each side.
- The helper: match in the middle row → rows before and after; in the
  first or last row → the first or last three; fewer than three rows → all.
- `noido-core` or the sidebar: the header text, `3/10 hits` and `1 hit`.
- Harness: rows are as tall as a card, and clicking a card still selects it
  (existing tests, adjusted).
