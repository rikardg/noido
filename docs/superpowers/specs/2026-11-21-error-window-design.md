# A window for a document noido couldn't open

Date: 2026-11-21. Agreed with the user. A small change: a launch that is
asked to open a document and can't shows the reason in a small window of
its own, instead of only on stderr.

## Intent and why

Issue #5. Launched from the desktop — a file manager, or a `noido:` or
`file:` link — nobody sees stderr, so a document that has moved, is
unreadable or is damaged produces no visible response at all. The messages
themselves are already good (`/books/x.pdf: no such file`, `…: not a
readable document`, `/x: only local files can be opened`); only the place
they are shown is wrong.

## Goals

- A launch that cannot open the document it was asked to open shows why in
  a window: what could not be opened, and the reason, as stderr gets it.
- Closing the window ends the process, with the exit status it has today
  (1) and the same line on stderr, so terminals, scripts and whatever
  started noido are unchanged.
- It is shown always, from a terminal as well: no `isatty` sniffing, one
  code path. A terminal launch sees the window and the line at the prompt's
  return.
- If the window itself cannot be made (no display), stderr is all there is
  and the process still exits as it does today.
- Escape closes it, and so does Close.

## Non-goals

- A failure inside a running macOS app, where the app receives the link as
  an Apple event and there is no launching process to show anything: issue
  #137.
- A macOS launch that must start the app itself and whose document then
  fails: the failure happens inside the shell's `create`, by which time the
  app's winit event loop exists, and a second one cannot be made for the
  window (`EventLoop can't be recreated`). It stays on stderr, as it is
  today, and #137 covers it.
- Documents restored at the start that fail to open: still skipped quietly
  (`open_all_quietly`), because a window per missing remembered file would
  be a nuisance.
- `--install`, `--uninstall` and `--app-bundle` failures: stderr as today.
  They are terminal commands, and the message is about the desktop's files,
  not a document.
- An "Open…", "Show folder" or copy button, or a way to try again.
- Sorting a document's failure from a failure to start noido itself (no
  display, no socket): the window is attempted for any failure of the
  launch path, and where it can't be made either, stderr is what is left.

## Where the failure goes today

- **A Linux or Windows launch:** `main::run` reads the document before eframe
  exists, so `document::open`'s `Err` is returned to `main`, printed by
  `exit_code`, and the process exits. No window is ever created.
- **A bad argument** (`https://…`, a `?` in a link): the same, one step
  earlier, from `main::opening` and `noido_core::parse_argument`.
- **A macOS launch:** the file it names must open inside the shell's
  `create`, so a failure fails the app start; a launch handed to the running
  app gets `Answer::Failed` and prints it. Both reach `main` as an `Err`.
- **A macOS URL event** into the running app: printed from `shell::loaded`.
  Out of scope (#137).

So every path in scope holds the message in `main`, in the process that is
about to exit: one place to show it.

## Approaches

- **A window of noido's own (chosen).** A new `failure.rs`: an app of one
  message and one button, drawn from a plain function so the headless tests
  can drive it like the rest of the interface. It follows the system theme,
  needs nothing from a document, and works on all three platforms.
- **The desktop's own message dialog (`rfd`).** Fewer lines, but `rfd` is
  not a dependency on macOS (its AppKit open panel comes through
  `noido-macos`), a message dialog on Linux goes through GTK or the XDG
  portal, it can't be driven by the repo's headless tests, and it would not
  follow noido's theme.
- **A normal document window, saying so where the page would be.** Needs a
  renderer and a document that do not exist, and gives the reader a title
  bar full of search, sidebar and page controls that do nothing.

## Behaviour

- The window: 480 × 200, not resizable, titled `noido`, noido's icon where
  the window manager takes one from the app, app id `noido` for the
  desktop's, with the system's title bar — noido's own bar holds a
  document's controls, which this window has none of.
- The window is painted in the theme's background colour. eframe would
  clear it in its own dark grey, which a light theme's text sits
  unreadable on.
- Inside: a heading, "Cannot open", and the message as stderr gets it, one
  string, wrapped, in a scrolling block so that a reason taller than the
  window scrolls instead of painting over the button. A right-aligned
  Close button. Escape, Ctrl-C and the title bar's close button do the
  same as Close.
- The message is one string rather than a path and a reason on separate
  lines: it is what stderr says, it is already built at one place
  (`load.rs`, `document.rs`, `parse_argument`), and splitting it again would
  mean guessing at a `": "` a file name may hold.
- Closing the window ends the run: the process prints its line and exits
  with the failure, exactly as now.
- The window writes nothing: no state, no recent file, no window place. The
  instance claim a launch made before reading the file is dropped when the
  launch fails, before the window runs, so a later launch of the same file
  is not told it is already open.

## Code shape

- `crates/frontends/gui-egui/src/failure.rs` (new):
  - `fn show(ui: &mut egui::Ui, message: &str) -> bool`: draws the window's
    contents; `true` to close.
  - `struct Failure`, with `eframe::App` calling it: a `drawn` method,
    which the tests drive too, sends the window's close command.
  - `pub fn report(message: &str)`: makes the window and runs it; a window
    that can't be made is noted on stderr (`noido: cannot show the error:
    …`) and nothing else, since the message itself is on its way out too.
- `crates/frontends/gui-egui/src/main.rs`:
  - the two `Some(_)` arms of `main`'s match become one call to a
    `fn launch(&args) -> Result<(), String>`, which is
    `opening(&args).and_then(run)` on Linux and Windows, and the same with
    `hint_if_stale()` before `run(Some(opening), args.foreground)` on macOS;
    on an `Err`, `launch` calls `failure::report` with the message and
    returns it unchanged.
  - `exit_code` is untouched: it prints the line after the window closes,
    and the exit status is the one it already gives.
  - `configure` becomes `pub(crate)`, so the window's context is set up as
    every other noido window's is.
  - `run`'s `interrupt::close_on_interrupt` moves to after
    `document::open`, just before the window is made: a process gets one
    Ctrl-C handler, and a launch that failed leaves it to the window it
    shows instead.

## Testing

- `failure.rs`, driven with `egui::Context::run_ui` as the harness is: the
  message is drawn; a message longer than the window is wrapped rather than
  cut (its galley takes more than one row); the whole window is filled
  with the theme's background, in both a light and a dark theme; a message
  taller than the window leaves the Close button inside the window and
  unpainted over; Close clicked and Escape both ask the window to close,
  by the command the app sends it.
- `main.rs`: `launch` shows a failure and returns it unchanged; a launch
  that worked shows nothing. The seam is the message → shower pair, so no
  window is made in tests.
- By hand on Linux, from a scratch `XDG_STATE_HOME`: a PDF deleted from the
  file manager, a `noido:///…` link to a moved file, and a damaged PDF
  copied to a scratch directory — each shows the window with the message;
  `noido gone.pdf` in a terminal shows it too and prints the line when it
  closes, exiting 1.
