# noido

Early stage PDF and comic reader with upcoming annotation functionality. Built on
[hayro](https://github.com/LaurenzV/hayro) and [egui](https://github.com/emilk/egui).

Features:

- Reads PDFs and CBZ comic archives.
- Fast viewer.
- Files are never modified. All edits are saved separately.

## Developing

- Prerequisites: a recent Rust toolchain, and the command runner
  [just](https://github.com/casey/just): `cargo binstall just`.
- `just tools` installs the pinned developer tools (currently `cargo-nextest`)
  with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall).
- Execute build commands from the command runner [just](https://github.com/casey/just), or look up
  the relevant commands in the [`justfile`](./justfile).
