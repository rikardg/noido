# Snapshot builds: one button, one entry in the release list

Date: 2026-11-01. Agreed with the user.

## Intent and why

There is no release yet, and the release pipeline
(`docs/investigations/2026-10-26-versioning-and-releases.md`) is deferred
work with its own setup: dist, a changelog, a Homebrew tap, a token. In the
meantime the user wants to hand someone a working `noido`, occasionally,
without building it and without a release process.

So: press the button in Actions, get a pre-release in the release list with
a Linux and a macOS binary. No tag to write, no version to bump, no notes to
compose before pressing.

## What a snapshot is

- A GitHub **pre-release**, so `releases/latest` and anything that follows
  it keep ignoring these.
- The tag carries the time and the commit: `snapshot-20261101-2045-1bae2fac`,
  named in the publish job once the builds are done. Every press that does
  not ask for a draft adds an entry to the release list. Two presses share a
  tag only if the same commit finishes within the same minute; the
  concurrency group makes the second wait, and a collision then fails in
  `gh release create` rather than overwriting the entry.
- The body is written by the workflow — the commit, a link to that commit as
  the source, and the Gatekeeper note — and edited on GitHub afterwards if
  the user has something to add. The dispatch form's optional `note` input
  becomes its first line.
- `noido --version` prints `0.1.0+1bae2fac`: the package version plus the
  commit as SemVer build metadata, which never changes how versions compare.

## The stamp

`crates/frontends/gui-egui/build.rs` reads `NOIDO_GIT_SHA`, falls back to
`git rev-parse --short=8 HEAD`, and then to `unknown`; it sets
`NOIDO_VERSION`, which `main.rs` hands to clap. The workflow sets the
variable from `github.sha`, so the release path never asks git and never
depends on checkout depth.

Two details that make it correct rather than almost correct:

- **`cargo:rerun-if-env-changed=NOIDO_GIT_SHA`.** Without it, the Rust cache
  in CI (on by default through `setup-rust-toolchain`) keeps the fresh build
  and the job uploads the previous binary with the previous commit in it.
- **The justfile hands the commit in** when the workflow does not, because jj
  moves the commit without touching a file and Cargo would otherwise keep the
  stamp from whenever the build script last ran. A bare `cargo build` still
  can; builds through `just` do not.
- **No `-dirty` marker.** The working copy is jj's, where uncommitted work is
  always present; the marker would be on every local build.

## What the artifacts are

`dist/noido-<target>-<version>.tar.gz` and its `.sha256`, per target:

- `x86_64-unknown-linux-gnu` on `ubuntu-22.04`,
- `aarch64-apple-darwin` on `macos-26`.

The archive holds a directory with `noido` and `LICENSE`: GPL-3.0-or-later
binaries are distributed with the licence, and the release body links to the
source commit they came from.

Built by `just snapshot <target>`, so a snapshot can be reproduced locally
and the workflow holds no copy of the steps.

Choices worth naming:

- **gzip, not xz.** Universal to pack and unpack; a stripped 45 MB binary is
  13.5 MB either way.
- **strip Linux only.** Stripping a macOS binary drops the ad-hoc signature
  the linker gave it, and an unsigned arm64 binary does not run at all.
- **Not cross-compiled.** A macOS binary built on Linux would need Mach-O
  linking (zig or an Apple SDK) plus re-signing, and would ship code nobody
  can run — including exactly the macOS-only branches CI compiles today.

## The image CI uses

`ci.yml`'s Linux leg moves from `ubuntu-latest` to `ubuntu-22.04`, the image
snapshots are built on, so what is checked is what ships and the binary's
glibc floor is a property the checks actually exercise. This amends
`2026-10-31-ci-workflow-design.md`. The snapshot job therefore builds only;
it runs no checks of its own.

## Trying it without publishing

There are two ways to try the flow, and both make a **draft**: the same `gh
release create` with the same assets and notes, minus the entry in the
release list, visible to maintainers only.

- Push a branch whose name starts with `snapshot-test`: it runs the whole
  flow and leaves a draft, because a push has no form to tick. It never
  publishes on its own — as long as the branch carries this version of the
  file, since a push runs the workflow from the pushed branch, not from the
  default one. The trigger stays after landing, until further notice, at
  the price of a full two-platform build per matching push, on top of CI's.
- Dispatch from any branch once the workflow is on the default branch — the
  Actions page chooses which branch — with `draft` ticked.

A draft can be inspected, published, or deleted from its page. There is no
"dry run" that leaves nothing behind: a draft already proves the asset
names, the notes and the tag, which is the part that could be wrong.

## Non-goals

- Tagged releases, versions, a changelog, Homebrew, installers, Windows.
- The macOS `.app` and a cask: both need a Mac and the open-file work.
- Reproducible builds, signed artifacts, notarization.

## To check by hand

- The `+` in the asset name survives GitHub's asset renaming; if a tool
  trips over it in a URL, the fallback is `0.1.0.1bae2fac`.
- The Linux tarball runs on a machine older than the runner's image.
- The macOS tarball unpacks and runs from a `curl` download, and what a
  browser download does on first launch.
