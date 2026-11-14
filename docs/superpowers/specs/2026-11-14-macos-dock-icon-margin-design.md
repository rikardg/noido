# The Dock icon at the size of other apps' icons

Date: 2026-11-14. Agreed with the user. A small change: on macOS, the
icon noido gives the Dock and the app switcher has the margin and
shadow of Apple's icon template.

## Intent and why

In the Dock and the ⌘-Tab switcher, noido's icon looks about a quarter
bigger than other apps' icons. The root viewport's icon is the window
icon, which fills its whole square, and eframe hands it to AppKit as
the application icon (`setApplicationIconImage`). That replaces the
bundle's `.icns` once the app runs. The `.icns` already has the
template's margin. A launch from the command line (`~/.cargo/bin/noido`,
`target/release/noido`) runs outside any bundle, so the runtime icon is
the only one macOS has.

## Goals

- On macOS, the root viewport's icon is rendered from the same SVG as
  the `.icns`: the icon shrunk to an 824 tile in the middle of a 1024
  canvas, with the drop shadow in the margin.
- The same icon whether noido runs from `noido.app` or from a bare
  binary.

## Non-goals

- Starting `noido.app` from the command line instead of the binary.
- Linux and Windows: the window icon stays full-bleed.

## Design

`build.rs` also renders the macOS SVG (`macos_icon_svg`) to
`$OUT_DIR/icon-macos.rgba`, at the window icon's side (256).
`icon::app_icon_macos()` returns it, and `root_viewport()` uses it in
place of `window_icon()`.

## Testing

- `icon`: the macOS icon has a transparent margin at the sides, the
  tile's green inside it, and the shadow below the tile.
- `main`: the hidden root carries the macOS icon.
