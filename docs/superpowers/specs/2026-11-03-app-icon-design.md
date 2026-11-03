# App icon: a Computer Modern `n`, rendered from one SVG

Date: 2026-11-03. Agreed with the user.

## Intent and why

noido has no icon: the window shows the platform's default and the
`.desktop` file borrows `x-office-document`. The icon is a lowercase `n`
in Computer Modern math italic (`cmmi10`, the `n` of `$n$`), white on a
rounded square of `#0f3302`, a deep leaf green.

## What changes

1. **One source.** `crates/frontends/gui-egui/assets/noido.svg`, 1024×1024,
   corner radius a fifth of the side; the letter is an outline path, so
   the file needs no font. It is the only icon file in the repo; every
   other form is made from it. `assets/make_icon.py` records how it was
   made (the glyph taken from `cmmi10.ttf` with fonttools, fitted to 56 %
   of the side and centred on its bounding box); the build does not run
   it.
2. **Window icon, rendered at build time.** `build.rs` renders the SVG to
   256×256 RGBA in `OUT_DIR` with `resvg` (pure Rust, a build-dependency
   only, text features off). `main.rs` passes the pixels to
   `ViewportBuilder::with_icon` as `IconData`; nothing is decoded at
   start-up. This is the icon macOS, Windows and X11 show.
3. **Linux desktop.** GNOME on Wayland takes the icon from the `.desktop`
   file, by name. `--install` also writes the SVG to
   `$XDG_DATA_HOME/icons/hicolor/scalable/apps/noido.svg` and the entry
   says `Icon=noido`; `--uninstall` removes it. Each file is written only
   when it is missing or differs, so an install from before the icon
   gets the icon. Writing or removing the icon touches the `hicolor`
   directory: GTK and GNOME Shell rescan a theme, and distrust its cache,
   only when that directory is newer. The app ID is already `noido`,
   matching `noido.desktop`.

## Not in this change

A macOS `.icns` with an app bundle and a Windows `.ico` in the `.exe`;
`build.rs` can make them from the same SVG when they are needed.

## Cost

resvg and its SVG parser, about a dozen crates, built once for the
build script. Nothing at run time beyond 256 KB of pixels in the binary.

## Tests

The embedded icon: 256×256, a transparent corner, `#0f3302` near the
edge, white inside the letter. The install plan: a fresh install writes
the entry and the icon; an install with only the icon missing or
different writes only the icon; an install with both current does
nothing; each write or removal of the icon is followed by the touch.
Uninstall removes whichever of the two is there. The entry says
`Icon=noido`.
