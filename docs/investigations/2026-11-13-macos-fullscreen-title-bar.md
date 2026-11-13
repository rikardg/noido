# The macOS title bar in fullscreen: investigation

Date: 2026-11-13

Status: tried and set aside. The bar stays shown in fullscreen.

## Question

In fullscreen, Preview, Finder and Xcode hide their toolbar. It comes
down with the menu bar when the pointer reaches the top of the screen.
Can noido's own title bar, drawn by egui under an empty unified
`NSToolbar` (see the spec `2026-11-13-macos-title-bar-height-design.md`),
do the same?

## Answer

Not well, while the bar's contents are egui's. AppKit's fullscreen title
bar lives in a window of its own. noido can follow it as it slides, but
AppKit pulls it in once the pointer is about 40 pt down. noido's bar
hangs below it and reaches about 80 pt down, so the reveal ends while
the pointer is still on noido's bar. Nothing public changes that limit.
A reveal that holds needs the bar's contents in AppKit's own toolbar,
as native items.

noido keeps the bar shown in fullscreen and hides the empty toolbar
there. While the pointer is at the top, AppKit's title bar, with the
window buttons, slides over the top of noido's bar.

## What AppKit does in fullscreen

Measured on a MacBook with a notch (screen 1800 × 1169 pt), using a bare
`NSWindow` set up as winit sets up noido's: full-size content view,
transparent title bar, hidden title, empty unified toolbar. The test
programs logged on every frame (60 Hz).

- **The window starts below the menu bar.** With the notch, the
  fullscreen window is 1126 pt tall and starts 43 pt below the top of
  the screen. The menu bar (`NSMenu.menuBarHeight` 43, the notch's
  `safeAreaInsets.top` 38) sits in that band above the window, so it
  never covers it. A screen without a notch was not measured; there
  the menu bar may cover the window's top.
- **The menu bar.** It comes down once the pointer enters the band above
  the window, so it is out of noido's window. noido gets `PointerGone`
  then, and no event when the menu bar has finished coming down. The
  menu bar goes once the pointer is about 40–60 pt down into the window.
  `NSMenu.menuBarVisible()` follows it.
- **The toolbar, if left shown.** It stays as a grey strip over the
  window's top: no auto-hide, since winit's delegate does not set
  `NSApplicationPresentationAutoHideToolbar`. noido therefore hides the
  toolbar while the window is fullscreen (`styleMask` has `FullScreen`),
  from the per-frame `unify_title_bars()`.
- **The title bar in fullscreen.** Even with the toolbar hidden, AppKit
  slides out a title bar with the window buttons, below the menu bar.
  - **Where it lives.** It is in a window of noido's app,
    `NSToolbarFullScreenWindow`, 44 pt tall at a fixed place over the
    top of noido's window. A `_NSFullScreenMouseDetectionWindow` (alpha
    0) covers the band above.
  - **Shown and hidden.** The strip window's alpha is 1 while the title
    bar is out or moving, and 0 while it is in. Its frame does not
    change.
  - **The slide.** The title bar slides 28 pt within the strip window,
    in about 0.12 s, with the close button going from 50 to 22 pt above
    the strip window's bottom.
  - **Reading it.** The slide can be read on every frame from
    `standardWindowButton(.closeButton)` and its superview (the title
    bar view), in screen coordinates against the document window's top.
- **The 40 pt hold.** AppKit pulls its title bar and the menu bar in
  once the pointer is about 40 pt below the window's top.
  - **Not the window.** Stretching the `NSToolbarFullScreenWindow` to
    80 pt changes nothing: AppKit keeps the larger frame, but still pulls
    in at about 40 pt.
  - **Clicks.** They go through the stretched window's empty part to the
    document window below.
- **The background is solid.** AppKit's fullscreen title bar is opaque,
  so noido's bar cannot sit behind it as it does in a window.

## What was tried

1. **noido's own reveal.**
   - **The design.** The bar is hidden in fullscreen and slides out
     (egui's `animate_bool`, 0.2 s) when the pointer reaches the top. It
     is drawn in a foreground `Area` over the pages, so they don't move.
   - **Why it failed.** The pointer never reaches the top: it leaves the
     window for the menu bar first. Taking `PointerGone` near the top as
     the signal fixed that. But AppKit's title bar then slid down over
     noido's.
2. **Hanging under AppKit's title bar.**
   - **The design.** noido reads AppKit's slide on every frame (above)
     and places its bar under it, with no trigger or animation of its
     own. It watches with `request_repaint` for 1.5 s after the pointer
     leaves over the top, since AppKit tells noido nothing.
   - **What worked.** The bar came and went together with AppKit's.
   - **Why it failed.** AppKit pulls in at about 40 pt. So noido's bar
     either stayed with a gap above it, or would have to jump to the top
     under the pointer.
3. **Stretching AppKit's strip window** to cover noido's bar, to move
   the hold down: no effect, see above.

The code for 1 and 2 was not kept. It had harness tests that put
AppKit's title bar position into the app, as the harness has no
AppKit, and a `noido_macos::fullscreen_title_bar()` that read it.

## If it is taken up again

- **Native toolbar items.** With the bar's contents in the toolbar
  (`NSToolbarItem`s, an `NSSearchToolbarItem`), AppKit shows and hides
  the bar as in Preview, and the reveal comes for free. That is the
  larger step put off in the title bar spec.
- **Taking AppKit's place.** noido's bar could take AppKit's place when
  AppKit pulls in: it slides to the top and stays while the pointer is
  above the bar's old bottom edge. It works, but the bar moves about
  28 pt under the pointer just as it is reached for.
- **A screen without a notch** needs measuring first. There the menu bar
  may cover the window's top.
