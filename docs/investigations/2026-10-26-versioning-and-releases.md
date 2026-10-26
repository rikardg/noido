# Versioning and releases: investigation

Date: 2026-10-26

## Question

How should noido be versioned, and how do releases get built and published
to GitHub (and Homebrew), with a step where the user approves before
anything goes public?

## Answer

**SemVer on the workspace version, dist for building and publishing, a
hand-written `CHANGELOG.md`, and releases started by hand from GitHub's
Actions page after a dry run.** macOS and Linux only at first; Windows,
the macOS `.app` and a Homebrew cask come later (see
`2026-10-26-macos-app.md`).

## Where noido stands

- One version for everything: `0.1.0` in `[workspace.package]`, every
  crate has `version.workspace = true`.
- `noido --version` works (clap's `#[command(version)]` in `main.rs`).
- No tags, no `.github/`, no changelog. Remote: `github.com/rikardg/noido`.
- No crate has `publish = false`, though none is meant for crates.io.

## SemVer for an application

SemVer is written for libraries. For an app, read it as:

- **Major:** something users must deal with: state or settings files that
  no longer load, a CLI flag removed, a relied-on key changed.
- **Minor:** features. **Patch:** fixes only.
- **Before 1.0:** `0.MINOR` for features and anything that might break,
  `0.x.PATCH` for fixes. Go to 1.0 when the state and settings formats are
  promised stable.
- Tags are `v0.2.0`; every tool expects that.
- Only the binary's version is visible to users. The internal crates follow
  the workspace version and should get `publish = false`.

## Tools in the Rust world

| Tool | What it does | Fit |
| --- | --- | --- |
| cargo-release | Local: bump, commit, tag, push | Assumes git branches; jj sits on a detached HEAD. Bumping one line by hand (or `cargo set-version --bump minor` from cargo-edit) is simpler. |
| release-plz | Bot opens a release PR with bump and changelog from Conventional Commits; publishes to crates.io | Poor: commit messages here are `Area: plain words`, and nothing goes to crates.io. |
| git-cliff | Changelog from commits | Possible with custom parsers; a hand-written changelog reads better for an app. |
| dist (formerly cargo-dist, axodotdev; 0.31 in Feb 2026) | Generates a GitHub Actions workflow from config: builds per target, archives, checksums, installers (shell, PowerShell, MSI, Homebrew formula), GitHub release | Chosen. The common "release an application" setup in Rust. |
| Hand-written workflow | Tag → build matrix → release, ~80 lines | What bygg does. Fine, but dist gives Homebrew for free. |

bygg (the user's Python project) takes its version from the tag
(setuptools-scm) and its release notes from GitHub's generated notes,
which are built from merged PRs. Neither carries over: Cargo keeps the
version in `Cargo.toml`, and noido has no PRs, so generated notes would be
empty.

## The setup

**Version.** In `[workspace.package]`, bumped by hand or with
`cargo set-version --bump minor`. `publish = false` on the internal crates.

**Changelog.** `CHANGELOG.md` with a `## 0.2.0 - 2026-11-xx` section per
release, written by hand. dist uses the section for the version being
released as the GitHub release notes. A starting list can come from
`git log <last tag>..` grouped by the `Area:` prefix (dropping `Spec:`,
`Plan:`, `Backlog:`), edited down; that's a habit, not tooling.

**dist.** `dist init` writes `dist-workspace.toml` and
`.github/workflows/release.yml` (generated; not edited by hand):

```toml
[dist]
targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
installers = ["shell", "homebrew"]
tap = "rikardg/homebrew-tap"
publish-jobs = ["homebrew"]
dispatch-releases = true

[dist.dependencies.apt]
# what eframe needs on the Ubuntu runner (libxkbcommon, wayland, …), found by trial
```

Pull requests get a "plan" check that catches config mistakes early.

What a release offers:

- Linux: a `.tar.xz` with the binary; the shell installer (`curl … | sh`)
  puts it in `~/.cargo/bin`, then `noido --install` adds it to the
  desktop's apps.
- macOS (Apple Silicon): `brew install rikardg/tap/noido` or the shell
  installer. Command-line binary only until the `.app` exists.
- The Homebrew formula covers Linux too.
- The shell installer's default, `~/.cargo/bin`, is where today's
  `cargo install` copy lives; a release would replace it. `install-path`
  changes that if wanted.

## The approval step

dist has no approval gate of its own. Options considered:

1. The tag push is the approval (plain dist).
2. **`dispatch-releases = true` (chosen):** no tag push; the release is
   started from GitHub's Actions page with a tag name. A `dry-run` run
   builds everything without publishing.
3. A GitHub environment gate like bygg's: needs edits to the generated
   workflow and `allow-dirty = ["ci"]`, which dist advises against;
   `dist init` would overwrite it.
4. `create-release = false`, uploading to a draft made beforehand: unclear
   how it works with the Homebrew job; untested.

## A release, step by step

1. Bump the version, write the changelog section,
   `jj commit -m "Release: 0.2.0"`, push `main`.
2. Actions → Release → Run workflow on `main`, tag `dry-run`. Download the
   artifacts and try them. Nothing is published.
3. Run it again with tag `v0.2.0`. It checks the tag against the version
   in `Cargo.toml`, builds, creates the tag and GitHub release on the
   commit it ran from, and updates the Homebrew formula. Nothing should be
   pushed to `main` between steps 2 and 3. (That the tag is created on the
   run's commit is how it's understood to work; confirm when setting up.)

The real run rebuilds rather than reusing the dry run's artifacts.

## Things to watch

- **glibc:** Linux binaries built on a new Ubuntu don't run on older
  distributions; check which runner dist uses (22.04 is the safe choice).
- **Toolchain:** `rust-toolchain.toml` is pinned; rustup on the runners
  follows it.
- **Unsigned macOS binaries:** fine through the Homebrew formula and the
  shell installer (no quarantine, as far as known); a downloaded `.app`
  is a different matter (see the macOS note).

## Set up by the user on GitHub

- An empty `rikardg/homebrew-tap` repository.
- A fine-grained personal access token with contents access to that repo,
  stored as the `HOMEBREW_TAP_TOKEN` secret in the noido repo.

## Later

- Windows: add `x86_64-pc-windows-msvc` and `"powershell"`, run
  `dist init` again.
- macOS `.app`, `.dmg` and a cask: not something dist makes; a custom
  dist job (it has hooks per phase), probably with cargo-packager.
- A PR workflow running test, clippy and fmt.
- Dev builds showing `0.2.0-14-gabc123` via a `build.rs` with
  `git describe`.
- A "new version available" check against the GitHub releases API.

## Sources

- dist config reference: https://axodotdev.github.io/cargo-dist/book/reference/config.html
- release-plz: https://github.com/orhun/release-plz
- bygg's release workflow: https://github.com/rikardg/bygg/blob/HEAD/.github/workflows/pypi_publish.yml
