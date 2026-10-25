# Odd pages left

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

Dual mode always pairs pages 1|2, 3|4: right for slides, papers and scanned
spreads, wrong for books, whose page 1 is a recto and whose spreads are
2|3, 4|5. A per-document toggle chooses.

## Approaches (agreed: 1)

1. **A flag in `LayoutMode`, the spread arithmetic in `Layout` reading it.**
   Everything else (paged flipping, prefetch, search reveal, the sidebar's
   highlight) already goes through `spread_of` / `spread_pages`. Chosen.
2. A blank placeholder page before page 1: no layout change, but a fake page
   leaks into page counts, rendering, thumbnails and search.

## Behaviour

- "Odd pages left" on (the default, today's layout): 1|2, 3|4, …
- Off: page 1 alone in the right half of its row, then 2|3, 4|5, …; a lone
  last page (an odd page count) sits in the left half, as today.
- Key O toggles it; the menu has a checkable "Odd pages left" after "Dual
  page". In single-page mode it toggles the setting with no visible change.
- Toggling keeps the page at the centre in view, as switching dual does:
  in paged mode its new spread from the top, in continuous mode the centre
  point in place.
- Saved per document with the other view state; files written before it
  existed read as on.

## Code shape

- `noido-core/layout.rs`: `LayoutMode.odd_left` (Default: true).
  `spread_count`, `spread_of`, `spread_pages` shift by one page when dual
  and not odd-left; `arrange` puts a lone right-hand page in the right half.
- `noido-core/view.rs`: `ViewState.odd_left`; `Action::ToggleOddLeft` via
  `switch_mode`.
- `keymap.rs`: `Key::O`, binding, label. `menu.rs`: `Check::OddLeft`.
- `noido-state`: `odd_left` in the entry, `#[serde(default)]` true.
- `gui-egui`: `core_key`, the title bar's checkmark, `same_place`.

## Testing

- Layout: spreads, `spread_of`, `spread_pages` and positions both ways,
  with even and odd page counts and a one-page document.
- View: toggling keeps the centre page, in paged and continuous mode;
  state round trip.
- Keymap/menu: O, the entry and its check.
- State: an old file without the field restores odd pages left.
- Harness: O switches the arrangement.
