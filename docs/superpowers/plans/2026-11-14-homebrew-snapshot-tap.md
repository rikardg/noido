# Snapshot cask: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship every noido snapshot as a Homebrew cask in
`rikardg/homebrew-tap`, so `brew upgrade --cask noido-snapshot` installs a
new `noido.app` in `/Applications` with no Gatekeeper dialog and nothing
to re-run.

**Architecture:** `snapshot.yml` learns to pack the `.app` bundle per macOS
target and to advertise the archives in the release notes. A small,
credential-free workflow in the tap pulls `releases.atom`, renders
`Casks/noido-snapshot.rb` from that release, and commits it. The cask
strips `com.apple.quarantine` in a `preflight_steps` block, which is the
only reason an ad-hoc-signed app can launch without approval.

**Tech Stack:** GitHub Actions, `just`, `ditto`, Homebrew cask DSL
(7.0.13), POSIX shell, `gh`/`curl`.

**Spec:** `docs/superpowers/specs/2026-11-14-homebrew-snapshot-design.md`

## Global Constraints

- macOS only. Targets: `aarch64-apple-darwin` and `x86_64-apple-darwin`.
- Cask version: `0.1.0+<tag stamp>-<commit>`, stamp `YYYYMMDD-HHMMSS` in
  UTC, commit = the tag's trailing 8 hex characters. The leading `0.1.0`
  comes from the asset name, never from a constant.
- Assets, per target: `noido-<target>-<version>.app.zip` and the same name
  plus `.sha256`, where `<version>` is `0.1.0+<commit>` (the binary's own
  `--version`), *not* the cask version.
- The bundle's top-level directory inside the zip is `noido.app`. Cask
  artifacts: `app "noido.app"` and
  `binary "#{appdir}/noido.app/Contents/MacOS/noido"`.
- Quarantine is stripped in `preflight_steps`, never `postflight_steps`.
- No credential crosses repositories: the tap only reads public data.
- The tap is public and its workflow commits with its own `GITHUB_TOKEN`.
- Everything in the noido repo keeps `just check` green.

## Review Focus

1. **A release whose body was edited after publishing.** The tap takes
   asset names from the body; an edit can remove or rename them. Expected:
   the run fails red and writes nothing, rather than committing a cask
   whose URL 404s.
2. **The workspace version bumps** (`0.1.0` → `0.2.0`). Asset names change.
   Expected: nothing hard-codes the prefix; a bump moves the cask version
   with it.
3. **Two snapshots inside one second.** Tags differ only by commit hash,
   so their order is arbitrary. Expected: `brew upgrade` may skip one; the
   next press recovers. Not worth engineering around.
4. **One macOS leg fails.** A missing zip would ship a one-architecture
   cask. Expected: the release fails its completeness check; the tap never
   sees it.
5. **A tester who previously ran `noido --install`.** `~/Applications/
   noido.app` is still registered. Expected: README says `noido --uninstall`
   first; two apps in Open With is the failure to look for.

---

### Task 1: `just app-zip <target>`

**Files:**
- Modify: `justfile` (after the `snapshot` recipe)

**Interfaces:**
- Produces: recipe `app-zip target=<triple>` writing
  `dist/noido-<target>-<version>.app.zip` and `.sha256`, echoing the zip's
  path. `<version>` is `target/<triple>/release/noido --version`'s second
  field, with the same `+unknown` guard as `snapshot`.

- [ ] **Step 1: Confirm the recipe does not exist**

Run: `just app-zip aarch64-apple-darwin`
Expected: `just: error: Justfile does not contain recipe 'app-zip'` (or the
`just --list` output showing no such recipe).

- [ ] **Step 2: Add the recipe**

Shape, following `snapshot` in the same file: build with
`cargo build --release --locked --target {{target}}`, refuse a version
containing `+unknown`, run `"$release/noido" --app-bundle "dist/{{name}}"`,
then `ditto -c -k --keepParent "dist/{{name}}/noido.app"
"dist/{{name}}.app.zip"` and write the `.sha256` with the existing
`sha256sum`/`shasum` branch. `name="noido-{{target}}-$version"`. Remove the
staging directory with `rm -rf "dist/{{name}}"` after packing — the bundle
must not be left in `dist/`, where the matrix legs' uploads are merged.

- [ ] **Step 3: Verify layout and the checksum**

Run: `just app-zip aarch64-apple-darwin && unzip -l dist/*.app.zip | head`
Expected: the first entry is `noido.app/` (no leading directory), then
`noido.app/Contents/{Info.plist,MacOS/noido,Resources/noido.icns}`.
Then `cd dist && shasum -a 256 -c *.app.zip.sha256` → `OK`, and
`ls dist | grep -c '^noido.app$'` → `0` (no staging directory left).

- [ ] **Step 4: Verify the bundle still carries its ad-hoc signature**

Run: `unzip -q dist/*.app.zip -d /tmp/appzip-check` then
`codesign -dv --verbose=2 /tmp/appzip-check/noido.app`
Expected: `Signature=adhoc`, `Identifier=io.github.rikardg.noido`.

- [ ] **Step 5: `just check`, then commit**

Message: `Repo: add a recipe that packs the macOS app bundle`.

### Task 2: the tag stamp gains seconds

**Files:**
- Modify: `.github/workflows/snapshot.yml` (the `stamp=$(date -u ...)` line
  and nothing else)

**Interfaces:**
- Produces: tags of the form
  `snapshot-YYYYMMDD-HHMMSS-<8 hex>`, which Tasks 4 and 5 parse.

- [ ] **Step 1: Record the current behaviour**

Run: `grep -n 'stamp=' .github/workflows/snapshot.yml`
Expected: `stamp=$(date -u +%Y%m%d-%H%M)` — minutes, no seconds.

- [ ] **Step 2: Change the format to `%Y%m%d-%H%M%S`**

- [ ] **Step 3: Prove it in a draft**

Push a branch named `snapshot-test-seconds`; wait for the run. Expected: a
draft release whose tag matches `snapshot-[0-9]{8}-[0-9]{6}-[0-9a-f]{8}`,
and whose title reads `Snapshot 20261114-213045 (abc1234)`. Delete the
draft and the branch afterwards.

- [ ] **Step 4: Commit**

Message: `Repo: give the snapshot tag second resolution` with one line on
why: two presses inside a minute would otherwise be ordered by the hash.

### Task 3: the release carries the bundles

**Files:**
- Modify: `.github/workflows/snapshot.yml` (build matrix step, publish
  completeness check, notes heredoc)

**Interfaces:**
- Consumes: `just app-zip` (Task 1).
- Produces: each snapshot release has, per macOS target, an `.app.zip` and
  its `.sha256` as assets, and a body that contains their full
  `https://github.com/rikardg/noido/releases/download/<tag>/<asset>` URLs,
  one per line under a `**macOS, … (app)**` heading.

- [ ] **Step 1: Add the zip step for the two macOS legs**

In the build job, after `just snapshot`, run `just app-zip ${{ matrix.target }}`
only for the apple targets (a `if: contains(matrix.target, 'apple')` step),
so the Linux leg gains nothing.

- [ ] **Step 2: Extend the completeness check**

The publish job already fails when a tarball is missing. Add the two
`*.app.zip` and their `*.sha256` to the same check, so a leg that lost its
zip fails the release instead of publishing a dead source.

- [ ] **Step 3: Put the zip URLs in the body**

Add, in the heredoc, a second download block per architecture naming
`$release/$mac_arm_app` (and Intel's), each on its own line, so the tap can
find them by suffix. Derive the names in the shell the way the tarballs
are derived, and fail the job if any is empty.

- [ ] **Step 4: Verify on a draft**

Push `snapshot-test-bundles`, wait, then:
`gh release view <tag> --json assets --jq '.assets[].name'` lists three
tarballs, three `.sha256`, two `.app.zip`, two `.app.zip.sha256`; and
`gh release view <tag> --json body --jq .body | grep -c 'app.zip'` → `2`.
Delete the draft and branch.

- [ ] **Step 5: Commit**

Message: `Repo: publish the macOS app bundle with each snapshot`.

### Task 4: the tap, its template and its renderer

**Files:**
- Create (in a new repository `rikardg/homebrew-tap`, which the user
  creates empty on GitHub):
  `Casks/noido-snapshot.rb.in`, `scripts/render.sh`, `scripts/test.sh`,
  `test/fixtures/{feed.xml,sha-arm.txt,sha-intel.txt}`, `README.md`
- Modify: `CLAUDE.md` in the noido repo (Where things are)

**Interfaces:**
- Produces: `scripts/render.sh FEED SHADIR OUT` — reads an Atom feed file,
  finds the `snapshot-*` tag with the highest stamp, reads the two
  `.app.zip` URLs from that entry's body, takes the two digest files from
  `SHADIR` (named `<asset>.sha256`), and writes the cask to `OUT`.
  Exit 0 with no write when the version already in `OUT` sorts at or above
  the candidate; exit 1 when the body has no `.app.zip` URL or a digest is
  not 64 hex characters.
- The template's placeholders are `{{version}}`, `{{stamp}}`, `{{commit}}`,
  `{{url_arm}}`, `{{url_intel}}`, `{{sha_arm}}`, `{{sha_intel}}`.

- [ ] **Step 1: Write the failing test**

`scripts/test.sh`, no framework, `set -eu`, exits non-zero on failure.
Three cases against `test/fixtures/`:

```
case 1: renders  Casks/noido-snapshot.rb  containing
        version "0.1.0+20261114-213045-abc1234"
        url "…/noido-aarch64-apple-darwin-0.1.0+abc1234.app.zip"
        sha256 "<the arm digest>"
        preflight_steps do
case 2: given an OUT that already holds 0.1.0+20261114-213045-abc1234,
        a second run prints "no change" and leaves the file untouched
case 3: a feed whose body has no .app.zip URL exits 1 and writes nothing
```

The fixture feed is a two-entry excerpt: one older tag and the newer one,
with the body inline and HTML-escaped exactly as GitHub serves it, and the
older entry listed *first* to prove the script sorts by stamp.

- [ ] **Step 2: Run it and watch it fail**

Run: `sh scripts/test.sh`
Expected: failure naming the missing `render.sh`.

- [ ] **Step 3: Write the template**

`Casks/noido-snapshot.rb.in`, placeholders as listed, in stanza order:
`version`, `on_macos` with `on_arm`/`on_intel` (`sha256` then `url`), `name`,
`desc`, `homepage`, `app`, `binary`, then

```ruby
preflight_steps do
  run "/usr/bin/xattr", args: ["-r", "-d", "com.apple.quarantine", "{{staged_path}}"]
end
```

- [ ] **Step 4: Write `scripts/render.sh`**

Parse with `sed`/`awk` only, no network, no `jq`. Version comparison is a
lexicographic compare of `<stamp>-<commit>` (fixed width, so it is
monotonic) read back out of the existing `version` line.

- [ ] **Step 5: Watch the tests pass**

Run: `sh scripts/test.sh` → all three cases pass. Then
`ruby -c Casks/noido-snapshot.rb` on a rendered file → `Syntax OK`.

- [ ] **Step 6: README and the CLAUDE.md line**

README: what the cask is, the three commands (`brew tap`, `brew trust
--cask`, `brew install --cask`), why quarantine is stripped, that a
browser download of the same zip stays blocked, that `noido --uninstall`
retires an older `~/Applications` copy, and that a quiet repository's
scheduled workflow is disabled after 60 days.

`CLAUDE.md`: one line under "Where things are" naming the tap and its
purpose.

- [ ] **Step 7: Commit**

Tap: `noido-snapshot: render the cask from a release`. Noido repo:
`Repo: point at the snapshot tap`.

### Task 5: the tap's scheduled update

**Files:**
- Create: `.github/workflows/update.yml` in the tap

**Interfaces:**
- Consumes: `scripts/render.sh` (Task 4).
- Produces: commits titled `noido-snapshot: <version>` on a schedule.

- [ ] **Step 1: Write the workflow**

`on: schedule: [{cron: "17 * * * *"}]` and `workflow_dispatch`;
`permissions: contents: write`; steps: checkout, fetch
`https://github.com/rikardg/noido/releases.atom` into a temp file (plain
`curl`, no token), then a second `curl` for each `.sha256` the feed's body
names, then `sh scripts/render.sh`, then commit-and-push only if
`git status --porcelain` shows a change, with
`git -c user.name=github-actions[bot] -c user.email=…`.

- [ ] **Step 2: Validate it locally**

Run `actionlint .github/workflows/update.yml` if available (install via
`brew install actionlint` if not); otherwise at minimum
`ruby -ryaml -e 'YAML.load_file(ARGV[0])' .github/workflows/update.yml`.

- [ ] **Step 3: Run it against the real repository**

Actions → Run workflow. Expected: a commit
`noido-snapshot: 0.1.0+<stamp>-<commit>` touching only
`Casks/noido-snapshot.rb`, and the rendered file carrying both arch URLs
and both digests from the *latest* snapshot release.

- [ ] **Step 4: Run it again**

Expected: no commit, the log line saying no change.

- [ ] **Step 5: Commit**

Message: `noido-snapshot: update the cask hourly from the release feed`.

### Task 6: install it like a tester

**Files:** none.

- [ ] **Step 1: Clean the ground**

Run `noido --uninstall` if `~/Applications/noido.app` exists, and move the
experiment's bundle out of the way (`mv /Applications/noido.app ~/.Trash/`).

- [ ] **Step 2: Install**

`brew tap rikardg/tap && brew trust --cask rikardg/tap/noido-snapshot &&
brew install --cask rikardg/tap/noido-snapshot`. Expected: no Gatekeeper
dialog when the app is opened from Finder; `xattr -r -p
com.apple.quarantine /Applications/noido.app` prints nothing;
`which -a noido` shows `/opt/homebrew/bin/noido`; a PDF dropped on the app
opens.

- [ ] **Step 3: Upgrade twice**

Press the snapshot button, wait for the tap's workflow, then
`brew upgrade --cask noido-snapshot`. Expected: the new version installs,
again with no dialog (`opoo "… signer changed …"` is expected and known),
and a second `brew upgrade` says it is up to date.

- [ ] **Step 4: Record what happened**

Add the measured result (date, macOS version, arch, whether a dialog
appeared, `xattr` output) to the spec's *To check by hand* as a note, or
open an issue if anything differed.

---

## Execution handoff

Plan complete and saved to
`docs/superpowers/plans/2026-11-14-homebrew-snapshot-tap.md`.
