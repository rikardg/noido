# Release notes from a template file

The snapshot release body is a shell heredoc inside `.github/workflows/snapshot.yml`.
Editing it means editing YAML, and the backticks around the commit have to be
escaped because the heredoc is unquoted. There is no way to see the body except
by dispatching the workflow. After this change the body is a markdown file with
`{{ name }}` placeholders, rendered by `minijinja-cli`; `just notes` renders it
to stdout with mock values.

## Today

`publish` builds the body with `cat <<EOF … EOF > notes.md`
(`.github/workflows/snapshot.yml:138–172`). The values are shell variables:
`$short`, `$release` (the `releases/download/$tag` prefix), and the five asset
names read out of `dist/`. `$NOTE` is prepended above the body when the
dispatch form supplied one. `gh release create` then takes the body from the
file (`:176–181`); the tag and title are built in the same step.

The body is a contract with the tap: `homebrew-tap/scripts/render.sh:39` and
`homebrew-tap/.github/workflows/update.yml:43` grep it for
`releases/download/$tag/noido-<target>-….app.zip`. A body without those URLs
fails the tap's run, which is how a half-published release is caught.

`justfile` is where the repo's commands live; it has no tool installation and
no way to render a release body.

## The change

- **`release/snapshot-notes.md.in`** holds the body verbatim, with
  `{{ short }}`, `{{ release_url }}`, `{{ source_url }}`, `{{ linux }}`,
  `{{ mac_arm }}`, `{{ mac_intel }}`, `{{ app_arm }}`, `{{ app_intel }}` in
  place of the shell expansions. No backticks to escape.
- **`just notes`** renders it to stdout with
  `minijinja-cli --strict --no-include`. Each value is read as `${NAME:-mock}`,
  so a bare `just notes` shows a realistic body: tag
  `snapshot-20260101-120000-deadbeef`, assets `…-0.1.0+deadbeef…`. The
  workflow sets the same variables to the real values and redirects to
  `notes.md`; `NOTE`, the tag and the title stay in the step as they are.
- **The publish job** downloads `minijinja-cli` 3.1.0 for
  `x86_64-unknown-linux-gnu` and its published `.sha256` from the minijinja
  releases, verifies the digest, unpacks it under `$RUNNER_TEMP`, and puts it
  on `PATH`. No binstall: its fallback can quietly compile from source.
- **`scripts/test-release-notes.sh`**, run by `just notes-check` (and by
  `just check`), renders through `just notes` with the real-value environment
  and asserts both `.app.zip` URLs, the Linux tarball and the source link are
  present, and that no `{{` is left. A renamed placeholder fails the render
  itself, so the test does not have to look for one.
- **`just tools`** installs the pinned developer tools with
  `cargo binstall --no-confirm --locked`, versions held in justfile
  variables, `--list` afterwards. It starts with `cargo-nextest` only and
  gains `minijinja-cli@3.1.0` in the notes commit. `just` itself is the
  bootstrap: `cargo binstall just`, in the README.

## Why minijinja

Rejected alternatives:

- **A `sed` renderer**, the tap's idiom (`Casks/noido-snapshot.rb.in` plus
  `scripts/render.sh`). It works and needs nothing from the runner, but
  `sed` cannot tell a renamed placeholder from a filled one, and the
  replacement is subject to `&` and `\`.
- **ERB**, preinstalled on the Ubuntu runner. A template with no branching
  becomes a program, and the release job gains a language for one file.
- **An asset list as data** plus a renderer: a schema for five fixed sections.
- **Only moving the heredoc into a script**: testable, but the escaping and
  the YAML stay.

`minijinja-cli` publishes a tarball and a `.sha256` for every target,
`--strict` turns an unknown variable into an error, and it installs with
`cargo binstall minijinja-cli@3.1.0` or `brew install minijinja-cli` for the
local preview.

## What it costs

One pinned third-party binary (1.2 MB, checksum-verified) in the publish job.
`just check` now needs `minijinja-cli`, because `notes-check` guards the tap's
contract; `just tools` is the one-line fix, and the README says so.

Not a tool manager: mise installs Rust through rustup anyway and would add a
config file and a shell hook for a tool rustup already provides, and proto and
aqua earn their config files only once a non-cargo tool needs managing. Every
tool here is a crate.

## Left out

- Loops, conditionals, or sections that appear and disappear.
- The stable-release path in `docs/investigations/2026-10-26-versioning-and-releases.md`;
  `dist` generates its own notes.
- Any change to the wording, the tag, or the title.

## Order

1. This spec.
2. `Repo:` install pinned developer tools with `just tools`, `cargo-nextest` only.
3. The test first, then the template, `just notes`, the workflow step, and
   `minijinja-cli` added to `just tools`.
