# Zoom, fit modes, and keeping position

Date: 2026-10-25 · Status: draft for review

## Intent

Let the reader change the zoom, and keep their place in the document while
zooming or resizing the window. This also moves the viewer's scroll position
into `noido-core`, which view modes (single, dual, flowing) build on next.

## Behaviour (agreed)

- Three zoom modes:
  - **Fit width** (start mode, `w`): pages fill the window width.
  - **Fit page** (`f`): the current page (the one at the window centre) fits
    the window in both directions. It only sets the zoom; it does not scroll
    the page into view. That distinction matters once view modes exist.
  - **Custom**: a fixed scale, set by zooming.
- In fit modes a window resize refits; in custom mode the scale stays.
- Keys (native layer, `Mod` = Ctrl, Cmd on macOS):
  - `Mod` + `+` or `Mod` + `=`: zoom in. `Mod` + `-`: zoom out.
  - `Mod` + scroll wheel, and trackpad pinch: zoom continuously.
  - `w`: fit width. `f`: fit page. Plain keys, ignored while a text field has
    focus.
  - No `Mod` + `0` (dropped).
  - PageUp/PageDown keep their behaviour.
- egui's own `Mod` +/-/0 interface scaling is turned off.

## Proposals accepted in conversation (my calls)

- Keyboard zoom steps by ×1.25 from the current scale; in a fit mode it
  starts from the fitted scale and switches to custom.
- Zoom is limited to 10%–400% (scale 0.1–4.0 UI points per PDF point) until
  tiled rendering makes deeper zoom sharp. Fit modes are clamped to the same range.
- Anchors — the document point held still on screen:
  - `Mod` + wheel / pinch: the point under the mouse pointer (window centre if
    the pointer is outside the page view).
  - Keyboard zoom, `w`, `f`, window resize: the point at the window centre.
- No zoom indicator yet (no status bar to show it in).

## Approaches considered (who owns the scroll position)

| Approach | Trade-off | Outcome |
| --- | --- | --- |
| A. Core holds the position as a document point; egui keeps doing the scrolling; synced every frame | Keeps egui's scrollbars, wheel, drag, momentum; zoom/resize logic testable in core and reusable by a TUI. Cost: two-way sync with egui each frame. | **Chosen** |
| B. Core owns all scrolling on a custom canvas | Single source of truth, but rebuilds scrollbars and scroll feel now | Possible later if sync proves fragile |
| C. egui owns the position; core only converts coordinates | Least code, but zoom logic spreads into the frontend | Rejected |

## Design

### Coordinates

- **Content space**: UI points in the scroll content, origin top-left (what
  `Layout` produces). The **scroll offset** is the content point at the
  viewport's top-left.
- **Viewport space**: UI points relative to the visible area's top-left.
- **Document point**: `DocPoint { page, x, y }` in PDF points from the page's
  top-left (after rotation). Points in margins or gaps are expressed relative
  to the nearest page above (or page 0), so `x`/`y` may lie outside the page;
  this keeps anchoring smooth instead of snapping.

### Layout (changed)

`Layout` no longer computes the fit scale. It arranges pages for a given scale
and available width:

- `arrange(scale, available_width)`: content width is
  `max(available_width, widest * scale + 2 * MARGIN)`; each page is centred in
  it horizontally; vertical stacking unchanged (`MARGIN`, `PAGE_GAP`).
- Exposes `page_rect`, `content_width`, `content_height`, `visible_pages`, and
  conversion between content points and `DocPoint`.

### View (new, `noido-core`)

Holds `Layout`, `ZoomMode`, current scale, viewport size and scroll offset.
Pure logic, no egui.

- `View::new(page_sizes)`: fit width, empty viewport, offset (0, 0).
- `set_viewport_size(width, height)`: refit in fit modes; keep the
  window-centre anchor. When the previous viewport was empty (first frame),
  keep the offset at the top instead.
- `scrolled_to(x, y)`: record a scroll the frontend did itself (wheel, drag,
  scrollbar).
- `apply(action)`: zoom in/out, fit width, fit page, scroll a screen up/down.
- `zoom_by(factor, anchor)`: continuous zoom around a viewport point.
- Read-only: `offset()`, `scale()`, `layout()`, `content_size()`.
- Every change re-clamps the offset to the content
  (`0..=max(0, content - viewport)` on each axis).

Fit scales:
- Fit width: `(viewport_width - 2*MARGIN) / widest_page_width`.
- Fit page: `min((viewport_width - 2*MARGIN) / page_width, (viewport_height - 2*MARGIN) / page_height)` for the page at the window centre.

### Keymap (extended)

- Key presses carry the `Mod` state: `KeyPress { key, command: bool }`.
- New keys `Plus`, `Equals`, `Minus`, `W`, `F`; new actions `ZoomIn`,
  `ZoomOut`, `FitWidth`, `FitPage`.
- Native bindings: `Mod`+`Plus`/`Equals` → ZoomIn, `Mod`+`Minus` → ZoomOut,
  `W` → FitWidth, `F` → FitPage, PageUp/PageDown as before (no `Mod`).

### Frontend (egui)

Each frame:
1. Receive finished renders (unchanged).
2. From last frame's scroll area output: `set_viewport_size(inner size)`, then
   `scrolled_to(state offset)`.
3. Unless a text field has focus: key presses → `apply`; `zoom_delta() != 1`
   → `zoom_by(delta, pointer position in the viewport)`.
4. Show `ScrollArea::both()`; pass `view.offset()` to it only when it differs
   from the offset egui reported (so egui's own smooth scrolling isn't cut off).
5. Draw pages from `view.layout()`; render scale is `view.scale() × pixels_per_point`,
   capped as before.

At startup: turn off egui's `zoom_with_keyboard`.

While zooming, the old page image is drawn stretched until the sharper one
arrives (the existing tracker behaviour).

## Non-goals

Tiled rendering and zoom beyond 400%, zoom indicator, `Mod`+`0`, view modes,
animated zoom, remembering zoom between runs, the vim layer.

## Testing

`View`, `Layout` and `Keymap` are unit-tested in `noido-core`: fit scales,
step zoom and clamping, anchor preservation for wheel zoom, keyboard zoom,
fit modes and resize, offset clamping, the first-frame rule, zero pages, and
key bindings with and without `Mod`. The egui wiring is checked by hand.

## Left to the builder

Exact helper names for content ↔ document conversion; how the frontend
detects "offset differs" (a small epsilon is fine).

## Resolves

Backlog item "Position lost on window resize": hold the window-centre point.
