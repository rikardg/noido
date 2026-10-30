# noido

A cross-platform PDF reader and note taker in Rust. Linux (GNOME, Wayland)
is the daily driver; macOS and Windows build but are unchecked.

## Where things are

- GitHub issues (`gh`, repo `rikardg/noido`): everything not done yet.
  Labels: `bug` (known issues), `enhancement` (deferred features),
  `idea`, `next-up`, `try-by-hand` (things the user should check),
  `review-minor`. Review minors: one issue per review, titled
  `<Feature>: review minors (YYYY-MM-DD)`, a checklist in the body.
  Close an issue when it is fixed, with `Fixes #N` in the commit message.
- `docs/backlog.md`: the old backlog, frozen. Add nothing to it. When an
  entry is fixed or moved to an issue, remove it; it goes once it is empty.
- `docs/superpowers/specs/`: one design per change, `YYYY-MM-DD-<topic>-design.md`.
- `docs/superpowers/plans/`: implementation plans for the larger ones.
- `docs/investigations/`: write-ups of things looked into.
- Test PDFs: tiny generated ones in `fixtures/` (`make_fixtures.py`);
  tests and examples use only these, so the repo stands on its own.
  `../testfiles/` (outside the repo; books, papers, scans) is for trying
  things by hand and for measuring; no test or example refers to it, and
  docs name its files by kind ("a network analysis paper"), not by name.

## Crates

- `noido-engine`: the engine trait (pages, rendering, glyphs).
- `engine-hayro`: the engine on hayro.
- `noido-text`: lines and words from glyphs; search (folding, accents,
  de-hyphenation).
- `noido-core`: pure, no GUI: layout (`spread_of` / `spread_pages` /
  `spread_count`; every spread question goes through these), view state
  and actions, keymap, menu. Most behaviour is tested here.
- `noido-render`: render, thumbnail and search threads.
- `noido-state`: per-document view state, TOML via serde.
- `frontends/gui-egui`: the `noido` binary (eframe/egui 0.37). `app.rs`
  has a headless test harness (`Harness`: real app, fake engine, key
  presses and window sizes via `RawInput`); GUI behaviour is tested there.

## Build and check

- Toolchain pinned in `rust-toolchain.toml`. Edition 2024.
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings` (must be clean)
- `cargo fmt`
- `cargo build --release`, then run `target/release/noido <file.pdf>`.
  `~/.cargo/bin/noido` is a separate copy from `cargo install --path
  crates/frontends/gui-egui`; building doesn't update it, so say which
  one the user should run.
- `noido --install` / `--uninstall` adds noido to the desktop's apps,
  pointing at the binary that ran it.

## How work goes

- Larger work: brainstorm → spec → plan → execute inline with a ledger in
  `.superpowers/sdd/<plan>/progress.md` (git-ignored; delete when done) →
  final review → minors to an issue.
- Small changes: a short spec, agreed with the user, then the change.
- Always TDD: the failing test first, watched failing.
- After a change: a review by a fresh subagent on the most capable model.
  Critical and Important findings are fixed test-first; minors go to an
  issue, not into the code.
- Rebuild release after anything the user will try, and tell them what to
  check by hand.

## Commits

- jj, colocated with git. Commit with `jj commit -m "<message>"`; the
  `main` bookmark is not moved by the agent.
- One commit per step that stands on its own; specs and plans get their
  own commit (`Spec: …`, `Plan: …`).
- Messages: `Area: what changed, in plain words`, e.g. `Arrow keys: leave
  them to egui while a widget has focus or a menu is open`.

## Hard rules

- Never send synthetic keyboard or mouse input to the user's desktop
  (XTest, xdotool, ydotool …); on GNOME it triggers a remote-desktop
  prompt. Launching the app, screenshots and signals are fine; drive the
  app through the headless harness instead.
- When launching noido for a check, use a scratch `XDG_STATE_HOME` (and
  `XDG_DATA_HOME` for `--install`) so the user's state isn't touched.
- Stop it with `pgrep -x noido | xargs -r kill -INT`. Never `pkill -f`
  with a pattern that also matches the shell's own command line.
- Don't change the user's default apps or other desktop settings.

## egui notes learned the hard way

- Clicks don't give a widget focus; Tab does. `memory().focused()` and
  `Popup::is_any_open` tell when egui is using the arrow keys itself.
- `ScrollArea` has no keyboard scrolling.
- Nested `ctx` locks deadlock: resolve `global_style()` before
  `fonts_mut`.
- `painter().with_clip_rect` replaces the clip; intersect with the
  existing one.
