# Fitting the window to the page

## Intent

Fit width and fit page make the page follow the window. The reader who
wants the opposite — the window as big as the page and no bigger — has to
drag its edges by hand. Two keys do it: Shift+W for the width, Shift+F for
width and height, so the window takes only the screen it uses.

## Goals

- **Shift+W** resizes the window's width so the page area is as wide as the
  spread the reader is on at the current zoom; the height stays as it is.
- **Shift+F** does both: the page area becomes the spread's size.
- The title bar, the sidebar, the central margins and the scroll bars come
  along: the page area is what ends up the page's size, not the window.
- The sidebar keeps its width it has; it is counted in the window.
- The zoom is held: both keys put the view on a fixed zoom at the scale it
  has, so the resize doesn't refit the pages.
- The window grows when the page at this zoom is bigger than it, up to the
  screen's width and height; a page bigger than the screen still scrolls.

## Non-goals

- Changing the zoom, the page arrangement or the sidebar's width.
- Fitting the whole document: in continuous mode the strip is many pages
  tall, and the target is the spread the reader is on, not the strip.
- Staying fitted as the reader moves: the window keeps the size it was
  given until the keys are pressed again.
- Moving the window: it keeps its top-left corner, so the title bar stays
  where the reader left it.

## Design

### `noido-core`

- `Action::FitWindowWidth` and `Action::FitWindowPage`, bound to Shift+W and
  Shift+F. Menu entries "Fit window to width" and "Fit window to page" after
  "Fit page". `View::apply` ignores both: they are about the window, which
  the view doesn't know.
- `View::hold_zoom()`: `ZoomMode::Custom` at the scale the view has, so a
  later viewport change keeps the scale instead of refitting.
- `View::spread_viewport_size() -> Option<(f32, f32)>`: the viewport that
  shows the spread the reader is on (`fit_spread`, as the fits use) at the
  current scale — the spread's pages, the gap between them in dual mode and
  a margin on all four sides. `None` with no pages. It changes nothing. In
  dual mode a lone last page counts as the full spread it is shown in, as
  `spread_size` counts it for fit page: the window is a page wide there and
  the page keeps its side.

### `gui-egui`

A new module, `shrink.rs`, holds what is asked for and works out the window
size; the app holds one request at a time and sends it.

- `Shrink` is the page area asked for and the sends left. Both keys build it
  with `Shrink::new(width, height)`: the height is the spread's for Shift+F
  and the page area's own height now for Shift+W, where only the width is
  asked for.
- `shrink::target(shrink, room)` is pure: the window's inner size now, the
  page area it gives now, the sidebar's width while it is open, the window's
  minimum size and the screen the window is on. Chrome is the difference
  between the window and the page area, both measured in the same frame. The
  target is the page area asked for plus the chrome, at least twice the
  sidebar's width while the sidebar is open (below that egui narrows it),
  at least the minimum window size, and no more than the screen.
- On the key, the app calls `hold_zoom`, then stores a `Shrink` with two
  sends left. It does neither when the page area has no size yet, as on the
  first frame, nor when there is no spread to fit to: a document with no
  pages leaves the view and the window as they are.
- At the point in the frame where the page area is laid out — where the app
  already tells the view its viewport size — `settle_shrink` compares the
  target with the window. Equal, and the request is forgotten. Different,
  and it sends `ViewportCommand::InnerSize` and keeps one fewer send. A
  maximized or full-screen window is restored first, and resized on the next
  frame, since a maximized window takes no size; still maximized on that
  frame, the request is dropped rather than asked again, which would keep
  the window from ever being maximized.
- Two sends, because a scroll bar coming or going changes the page area by
  its thickness: the first resize can land a bar's width off, and the next
  frame corrects it from the chrome it then measures.

## Tests

- Keymap: Shift+W is `FitWindowWidth`, Shift+F is `FitWindowPage`; plain W
  and F still fit; with text being edited in the search field both do
  nothing, as the plain keys do.
- Menu: the two entries follow "Fit page" and show Shift+W and Shift+F;
  every entry still has a key.
- `spread_viewport_size`: one page at a fixed scale; dual with the gap, and
  gapless without it; continuous mode uses the spread at the centre; no
  pages is `None`; scale, mode and offset are untouched.
- `hold_zoom`: the mode becomes `Custom` and the scale is what it was; the
  viewport then keeps that scale when it changes.
- `shrink::target`: the chrome is the window minus the page area; the
  sidebar is counted and the window stops at twice its width; the minimum
  window size and the screen cap each win over a smaller or larger target.
- Harness: Shift+F shrinks the window until the page area is the page's
  size; Shift+W leaves the height the page area had; the sidebar keeps its
  width; a maximized window is restored before it is resized, and asked once
  only if it stays maximized; a window with no page area yet asks for
  nothing; the zoom is the scale it was, and fit width still refits
  afterwards.
- Harness, the screen: the window reports a screen size, and with a page
  bigger than it the fit stops at the screen on both axes and then settles.
- Harness, full screen: the window is restored first and asked once only if
  it stays full screen, as a maximized one is.
- Harness, a second key press in the frame that resizes: the last press is
  the one that counts, one size is asked for, and the window settles.
- Harness, a document with no pages: both keys change nothing — no size is
  asked for, and the zoom mode and scale stay as they were.
