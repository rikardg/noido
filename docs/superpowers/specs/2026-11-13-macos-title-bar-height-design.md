# The macOS title bar at AppKit's height, with its window buttons

Date: 2026-11-13. Agreed with the user. A small change: on macOS the
title bar is as tall as Preview's and Safari's, and the window buttons
(the "traffic lights") sit where they sit in those apps. What the bar
holds is still drawn by egui.

## Intent and why

Beside Preview, noido's bar looks off: it is lower (40 pt against 52),
and the window buttons hug the top left corner, where AppKit puts them
for a plain title bar. Preview and Safari get their bar from a toolbar in
the unified style; the height and the buttons' place come with it.

## Goals

- Every document window on macOS gets an empty `NSToolbar` in the
  unified style, with no separator line under the bar. AppKit then
  makes the bar 52 pt tall and puts the window buttons in it, centred
  from top to bottom, 19 pt from the left edge, and keeps them there
  through resizes.
- `TITLE_BAR_HEIGHT` is 52 on macOS, the bar AppKit makes; 40
  elsewhere, as now.
- The room kept for the buttons grows with them: the zoom button now
  ends 73 pt in (61 before), and the bar's first control keeps the gap
  it has now, 15 pt, so starts at 88.
- The controls keep their sizes and stay centred in the taller bar.
- The outermost glyphs mirror the window buttons: the menu's is 19 pt
  from the right edge, and in fullscreen, where there are no window
  buttons, the sidebar toggle's is 19 pt from the left.
- In fullscreen the toolbar is hidden: AppKit would keep it there as a
  grey strip over noido's bar.
- A double-click on the bar's empty part is AppKit's (the toolbar takes
  it), and follows the system's setting for it.

## Non-goals

- Native contents: the window's title and subtitle, toolbar items, a
  native search field. Maybe later.
- The menu: the ☰ menu and the menu bar are to be rethought for all
  platforms together.
- Larger controls for the taller bar: judged from screenshots
  afterwards.
- Linux and Windows: unchanged.

## Approaches

1. **An empty toolbar in the unified style** — chosen. AppKit does the
   placing, for every window size, in fullscreen and on later macOS
   versions.
2. Moving the buttons by hand (`standardWindowButton` frames). AppKit
   puts them back on resize and on fullscreen, so they would have to be
   moved again each time; Tauri-style apps that do this flicker.
3. The compact unified style (38 pt, buttons 12 pt in): close to today's
   height, but not what Preview and Safari look like.

Measured with a bare `NSWindow` set up as winit sets up noido's
(transparent title bar, hidden title, full-size content view):

| toolbar        | bar  | close button: left, top |
|----------------|------|-------------------------|
| none (today)   | 28   | 7, 6                    |
| unified        | 52   | 19, 18                  |
| unified compact| 38   | 12, 11                  |

## Design

- `noido-macos` gets `unify_title_bars()`: for each of the app's windows
  that is not a panel, has a full-size content view and has no toolbar
  yet, it sets an empty `NSToolbar`, the unified style, and no title
  bar separator. AppKit refuses a toolbar to a window with no title
  bar. The toolbar is shown or hidden as the window leaves or enters
  fullscreen.
- egui does not hand out the `NSWindow` of a deferred viewport, so the
  shell calls `unify_title_bars()` in each document window's pass; the
  first pass after AppKit makes the window gives it its toolbar.
- `titlebar.rs`: `TITLE_BAR_HEIGHT` and `TRAFFIC_LIGHTS_INSET` take
  their macOS values by `cfg!`. In fullscreen the bar keeps no room for
  the buttons, as now.

## Testing

- Harness (macOS): the title bar panel is 52 pt tall, its first
  control starts at 88 pt and is centred in the bar, and the outermost
  glyphs are 19 pt from the edges, in fullscreen too.
- By hand and by window capture beside Preview: the height, the
  buttons' place, the bar after a resize, entering and leaving
  fullscreen (no grey strip, a smooth transition), and a second
  window.
