# CI on every push and pull request: design

Date: 2026-10-31. Agreed with the user.

## Intent and why

Every push and every pull request runs, on the two platforms noido is
developed and released for, the same checks a developer runs by hand:
formatting, clippy, tests, and a release-profile build. Linux x86_64 and
Apple Silicon are the platforms in
`2026-10-26-versioning-and-releases.md`; running the check on both is
what keeps the macOS-only code honest before a release.

Nothing is published here. Releases, tags, Homebrew and installers stay
with the planned dist workflow; this design only covers the check.

## Goals

- `ubuntu-latest` (`x86_64-unknown-linux-gnu`) and `macos-14`
  (`aarch64-apple-darwin`) each run the checks and a release build.
- macOS-only code — `crates/frontends/gui-egui/src/open.rs`'s
  `#[cfg(target_os = "macos")]` branches — is compiled and linted, not
  only skipped.
- `just check` no longer changes files, anywhere.

## Non-goals

- Publishing artifacts, tags, releases, Homebrew, the macOS `.app`,
  Windows. dist owns those.
- A display server: the GUI harness is headless (fake engine, `RawInput`).
- Windows runners, pinned action SHAs, Dependabot (possible later).

## Anti-goals

- **A check that rewrites code and passes.** That is the bug being
  fixed: `just check` runs `cargo fmt`, which reformats instead of
  failing. A gate that fixes what it is checking can never fail on it.
- **A workflow that can't be reproduced locally.** CI calls the justfile
  recipes; it does not carry its own copy of the commands.

## Constraints

- Toolchain comes from `rust-toolchain.toml` (1.99.1, `rustfmt` and
  `clippy`); rustup installs it on the runner.
- `just` is not preinstalled on the runners; an action installs it.
  `nextest` is deliberately not installed: `just test` falls back to
  `cargo test`, and not installing saves CI the install step. The
  nextest preference stays a local convenience. CI therefore also runs
  doctests, which nextest would skip; there are none today, and if one
  appears CI still covers it.
- Linux build dependencies for eframe's X11/Wayland stack are believed to
  be none (winit, `x11-dl` and `xkbcommon-dl` load at runtime), but the
  release investigation flags this as "found by trial". If the Linux leg
  fails, an apt step is added.
- No secrets, public repository, free arm64 macOS runners.

## Approaches

**A. One workflow, one full leg per platform (picked).** `ci.yml` with a
matrix over `ubuntu-latest` and `macos-14`; each leg installs the
toolchain, caches, then runs the checks and the build. Costs a second
clippy run, which is exactly the point of the macOS leg.

**B. Split: one Linux lint job, tests and build as a two-OS matrix.**
Faster, but macOS-only code goes unlinted.

**C. Let the planned cargo-dist PR run own the check.** That workflow
does not exist yet and does not run tests; it couples two decisions.

For the formatting bug, **change `just check` in place (picked)** rather
than have CI call `cargo fmt --all -- --check` directly: the footgun
stays for anyone who runs `just check` by hand, and the commands would
live in two places.

## The change

**`justfile`**

- `fmt` becomes `cargo fmt --all`. The fix for a failing check must use
  the same scope the check uses.
- New `fmt-check`: `cargo fmt --all -- --check`. Formatting is not
  applied; the developer runs `just fmt`.
- `lint`, `test` and `build` all pass `--locked`
  (`cargo clippy --locked …`, `cargo build --release --locked`, and the
  same in both branches of `test`). Without it the first cargo command in
  CI silently refreshes a stale `Cargo.lock`, and a build step that
  checks for a stale lock afterwards would be looking at a lock the same
  run had just rewritten. A hand-edited manifest now fails with cargo's
  "lock file needs to be updated", whose fix is a `cargo check` or
  `cargo update` away.
- `check: fmt-check lint test`. Nothing else in the file moves.

**`.github/workflows/ci.yml`**

- Triggers `push` (all branches) and `pull_request`; `workflow_dispatch`
  for re-running by hand. `permissions: contents: read`.
- Concurrency group
  `${{ github.workflow }}-${{ github.event_name }}-${{ github.head_ref || github.ref_name }}`,
  `cancel-in-progress: true`, so a second push to a branch cancels the
  run the first one started. On a pull request `head_ref` is the branch
  name and on a push `ref_name` is the same name; the event name keeps
  the two apart, for the reason under the job condition below.
- One job, matrix `os: [ubuntu-latest, macos-26]`, `fail-fast: false`.
  macOS 14 was retired on 2026-11-02, and jobs on it fail outright; of
  the current labels `macos-26` is the Arm64 one that will last longest
  (`macos-15` is next to go).
- A job condition, so the duplicate that a branch push and its pull
  request make never reaches a runner:

  ```yaml
  if: >-
    github.event_name != 'pull_request'
    || github.event.pull_request.head.repo.full_name != github.repository
  ```

  That runs the check on every push, on a manual dispatch, and on a pull
  request only when its head branch lives in another repository. What
  that saves, and what it does not:

  - a job skipped by its condition allocates nothing, so the duplicate
    queues neither a Linux nor a macOS machine, and both matrix legs
    would otherwise be queued together;
  - the duplicate *run* still exists, though, and still enters the
    concurrency group. With `cancel-in-progress`, that is enough to
    cancel the other run before its jobs get runners: opening a pull
    request killed the push run, the pull request's own job then
    skipped, and nothing was checked at all. Hence the event name in the
    concurrency group, so the two never share one.

  A same-repo pull request is still checked, by the push run on its head
  commit: that check sits on the pull request's head SHA and satisfies a
  required status check, while the skipped entry is inert. Fork pull
  requests have no push run in this repository, so they get one of their
  own.
- Steps: checkout; `actions-rust-lang/setup-rust-toolchain` (reads the
  toolchain file); `Swatinem/rust-cache`; `taiki-e/install-action` for
  `just`; then `just fmt-check`, `just lint`, `just test` — which takes
  the `cargo test` branch, since no nextest is installed — and
  `just build` as separate steps, so one failure still shows the others'
  results and each step names what broke.

## What this leaves to the builder

- Whether the Linux leg needs apt packages, and which.
- Action versions, which are expected to move.
- Everything in the release investigation: dist, tags, artifacts.

## How it is checked

- By hand that `just check` now fails on badly formatted code and leaves
  the tree untouched, and that `just fmt` is the fix.
- `just fmt-check`, `just lint`, `just test`, `just build` run locally,
  so the names CI calls exist and pass.
- The workflow file is validated before it is pushed.
