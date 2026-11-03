# Solid scroll bars: a thin thumb in a groove, and room for the bars

Date: 2026-11-03. Agreed with the user.

## Intent and why

With "Show scroll bars: Always" on macOS, native-theme reports non-overlay
scroll bars, and egui draws them solid: they take space instead of floating
over the content. noido was built against GNOME's overlay bars, so:

- the thumb is the whole groove wide (17 pt), where AppKit draws a 7 pt
  capsule in a groove;
- the sidebar lays its thumbnails out at the full panel width, and the bar
  covers their right edge;
- the page layout takes the full central area as its viewport, so its
  content is always wider than the scroll area's inner width, and a
  horizontal bar is always shown;
- the bars sit inside the central panel's 8 pt frame margin, inset from the
  window edge.

The user compared variants side by side with Preview and Finder and chose
the groove with the bars at the window edge.

## What changes

Only for solid bars (`overlay_mode` false). Overlay bars are unchanged,
except that the list of search matches now takes the theme's scroll bar
style, overlay or solid, as the thumbnails always have; it had egui's
default.

1. **Thumb.** A capsule `thumb_width` wide, centred in `groove_width`,
   lit on hover and while dragged, never wider. egui's bar is the whole
   groove, so a press anywhere across it scrolls; egui's own thumb and
   track are transparent, and noido paints the capsule where egui's thumb
   is. (First drawn as egui's thumb with the groove as its margins; the
   review found egui then takes presses only across the thumb's strip.)
2. **Groove.** Behind each bar shown, a strip the full allocated width,
   slightly lighter than the background, with a hairline in the border
   colour on its inner edge. On the right of the page area, of the
   sidebar's thumbnails and of its list of search matches; along the bottom
   of the page area when it has a horizontal bar.
3. **Room for the bars.** The page layout's viewport and the sidebar's
   thumbnail column are the space left after the bars, worked out in the
   same frame: no horizontal bar unless the pages are wider than the view,
   no thumbnails or matches under the sidebar's bar.
4. **Edge.** On macOS, with solid bars, the page area has no frame margin,
   so the bars meet the window edge; the pages keep the layout's own 12 pt
   margin. Linux and Windows keep the 8 pt margin: noido's resize edges
   there take the outer 5 pt of the window, which would take presses meant
   for a bar.

## Tests

In the headless harness, with a solid-bar theme: the thumbnails end where
the sidebar's bar begins; at fit-width there is no horizontal bar, and
zoomed wider there is; with an overlay theme the layout is as before. In
`theme`: the solid style's thumb, margins and allocated width.

## By hand

On a Mac with "Show scroll bars: Always": the look against Finder, no
horizontal bar at fit-width, thumbnails clear of the bar, and dragging the
thumb and clicking in the groove still scroll.
