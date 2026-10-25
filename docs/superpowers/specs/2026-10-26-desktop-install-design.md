# Desktop integration: `noido --install` / `--uninstall`

Date: 2026-10-26 · Status: small change, agreed in conversation

## Intent

Open PDFs in noido from Nautilus and the rest of the Linux desktop ("Open
With", the app grid, the dock), the way Evince is opened. noido sets this up
itself, so it works from wherever the binary lives (a `cargo install`, or
`~/.cargo/bin/noido` linking to `target/release`).

Non-goals: making noido the default PDF app. Defaults are the user's to set
(Nautilus' "Open With…", GNOME Settings, `xdg-mime`); noido never touches
them. A real noido icon. macOS and Windows.

## Approaches (agreed: 1)

1. **noido writes the `.desktop` file itself and runs the standard tools.**
   Working out what to do is plain, tested code; only carrying it out
   touches the system. Chosen.
2. Edit `~/.config/mimeapps.list` by hand: copies xdg-utils' rules, and
   moot now that defaults are left alone.
3. A shell script inside noido: harder to test, and the plan-then-confirm
   part becomes shell code.

## Behaviour

- `noido --install` and `noido --uninstall` take no file; a file is still
  required otherwise. They conflict with each other and with a file.
- Each first prints what it will do, one line per step, then asks
  `Proceed? [y/N] `. Only an answer starting with y or Y goes ahead; anything
  else, including end of input, prints "Nothing changed." and exits 0. When
  there is nothing to do it says so and doesn't ask.
- **Install:**
  - Writes `$XDG_DATA_HOME/applications/noido.desktop` (default
    `~/.local/share/applications`), creating the directory if needed.
    `Exec=` is the binary's real location: `current_exe()` with links
    resolved, quoted per the desktop entry spec, followed by `%f` (one
    process per file; noido opens one file per window).
  - Contents: `Type=Application`, `Name=noido`, `GenericName=PDF Reader`,
    `Comment=Read PDFs and take notes`, `Icon=x-office-document` (the
    theme's generic document icon, until noido has its own),
    `Terminal=false`, `StartupNotify=true`, `StartupWMClass=noido`,
    `MimeType=application/pdf;`, `Categories=Office;Viewer;`.
  - An identical file already there: "already installed", nothing to do.
    A different one (e.g. an older path): the step says it replaces it.
  - Then runs `update-desktop-database <dir>`. Not installed: a note that
    it was skipped (GNOME picks up the file anyway); not an error.
- **Uninstall:**
  - Removes the `noido.desktop` above if present, then refreshes the
    database the same way. No file: "not installed", nothing to do.
  - A default that still names `noido.desktop` needs no warning: GLib and
    xdg-mime skip a default whose file is gone and fall back to the next
    app (checked: Evince). An earlier draft warned here.
- On other systems both print that they are for Linux desktops only, and
  exit 1.
- **Window app ID** `noido` (`ViewportBuilder::with_app_id`), so GNOME ties
  the window to the `.desktop` file and the dock shows "noido" with its icon.
  eframe doesn't set it from the app name.
- **Focus when opened from Nautilus:** egui-winit 0.37 already passes
  `XDG_ACTIVATION_TOKEN` to the first window. No code; tried by hand.

## Code shape

- `gui-egui/src/desktop.rs`: the paths and the binary location go in as
  arguments; `plan_install` / `plan_uninstall` return the steps;
  describing them and the y/N answer are pure functions; a thin `run`
  carries the steps out.
- `main.rs`: the two flags (clap), dispatch before any window work;
  `with_app_id("noido")`.
- `docs/backlog.md`: the Nautilus entry leaves "Next up"; trying it by hand
  goes under Known issues for the user.

## Testing

- The `.desktop` contents, including a path with spaces quoted.
- Install plans: fresh, identical file present, different file present.
- Uninstall plans: file present, absent.
- Executing a plan in a temp dir writes / removes the file; a missing
  `update-desktop-database` is a skipped note, not an error.
- The answer: y, Y, yes go ahead; n, empty, end of input don't.
- Argument parsing: `--install` without a file; a file still required
  otherwise; `--install` with a file rejected.
- By hand (user): Open With from Nautilus, the dock icon and name, focus.
