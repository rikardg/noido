# macOS icon: Apple's margin, shadow and squircle

Date: 2026-11-05. Agreed with the user.

## Intent and why

In Launchpad, the Dock and Finder, noido's icon is about a quarter
larger than its neighbours, and its corners are a different shape.
`noido.svg` fills its whole 1024 canvas with a circular-cornered rounded
rect. Apple's macOS template (Big Sur on) puts an 824×824 tile with
continuous ("squircle") corners in the middle of the 1024 canvas, and
uses the margin for a drop shadow. From macOS 26, an icon that is not a
squircle is put inside a grey one.

## What changes

1. **Squircle tile, everywhere.** In `noido.svg`, the tile becomes
   Apple's shape as Figma's corner smoothing draws it: straight sides,
   and at each corner a Bézier easing into a circular arc and another
   out of it (radius 22.5 % of the side, 185 of Apple's 824 tile;
   smoothing 60 %, so each corner takes 1.6 × the radius of each side).
   A superellipse (n = 5) was tried first; its sides bulge visibly.
   `make_icon.py` generates the path, so the script still records how
   the file was made. The letter doesn't change. The window icon and the
   Linux SVG get the new shape too, and stay full-bleed, as GNOME and
   Windows expect.
2. **Margin and shadow, macOS only.** `build.rs` wraps the SVG's
   contents in a new 1024 SVG for the `.icns`: the original goes in as a
   nested `<svg x="100" y="100" width="824" height="824"
   viewBox="0 0 1024 1024">`, inside a group with an `feDropShadow`:
   black at 30 % opacity, 10 px down, `stdDeviation` 5 (Apple's template
   numbers as usually given; easy to tune). Only `write_icns` uses it.
   The SVG stays the one source.
3. **Reinstall.** `--install` already replaces a bundle whose `.icns`
   differs from the build's, so a reinstall picks up the new icon.

## Tests

- Window icon (256): the corner is transparent and the green is near the
  edge, as before. New: the left edge is opaque from row 93 to 162 (the
  straight part), and pixel (1, 50) in the corner's curve is mostly
  covered, where a superellipse would leave it empty.
- `.icns`: every size is present, as before. New: the 1024 PNG, decoded
  with tiny-skia (a dev-dependency, already built for `build.rs`): the
  margin beside the tile is transparent, just inside the tile's edge is
  opaque `#0f3302`, just above the tile is transparent, just below it is
  partly opaque shadow, and the canvas's bottom edge is transparent.

## Cost

None at run time.
