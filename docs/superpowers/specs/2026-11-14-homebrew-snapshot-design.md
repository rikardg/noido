# Snapshot builds through Homebrew

Date: 2026-11-14. Agreed with the user. Amended from a formula to a cask
after a design review, and again after research into Gatekeeper and
Homebrew's quarantine handling.

## Intent and why

Snapshots already exist: `snapshot.yml` builds Linux and both macOS
targets and publishes a pre-release with a `.tar.gz` and a `.sha256` per
target (`2026-11-01-snapshot-builds-design.md`). Getting one today means
copying a `curl` line out of the release notes, unpacking it and putting
the binary somewhere — and on a Mac a PDF reader that is not an app in
`/Applications` is not really installed: Finder does not offer it for
PDFs.

The user and their testers run macOS. So the snapshot channel is a
**cask**, for both architectures: `brew upgrade` should be the whole
update, with no dialogs and nothing to re-run afterwards.

The regular channel is separate and already designed:
`docs/investigations/2026-10-26-versioning-and-releases.md` has a tap and
releases from a tag. That waits for the first real release; this work does
not, and nothing here depends on it.

## What it is

One new repo, `rikardg/homebrew-tap`, public, holding two things:

- `Casks/noido-snapshot.rb`, which installs `noido.app` and the `noido`
  command from the newest snapshot pre-release for the machine's
  architecture;
- a small workflow of its own that runs on a schedule, notices a new
  pre-release, rewrites that cask and commits it.

```
brew tap rikardg/tap
brew trust --cask rikardg/tap/noido-snapshot
brew install --cask rikardg/tap/noido-snapshot
```

Trust is not ours to skip: since Homebrew 6.0.0 a third-party tap's Ruby
is not loaded until it is trusted (`docs.brew.sh/Tap-Trust`), and trust
persists in `~/.homebrew/trust.json` until `brew untrust` — or until the
cask is uninstalled, which drops it (`cmd/uninstall.rb` calls
`Trust.untrust!` for each removed item). Installing by
full name records it on the spot — `brew install --cask` calls
`Trust.trust_fully_qualified_items!` — so the explicit `brew trust` line
only matters for the short name after `brew tap`. The README carries both.

macOS only, because the artifacts are macOS binaries. Homebrew 7 does
install casks on Linux (it has `appimage` artifacts), so the cask declares
`on_macos`, and Linux keeps the `curl` line it already has.

## The three changes in noido

**The tag's stamp gains seconds.** `snapshot-20261114-213045-abc1234`
instead of `...-2130-...`. The tag is the version the cask carries
(below), so two presses finishing inside one minute would otherwise be
ordered by the hash. One line in `snapshot.yml`.

**The snapshot job packs the bundle.** `noido --app-bundle` already
writes `noido.app/Contents/{Info.plist, MacOS/noido, Resources/noido.icns}`;
`just app` runs it for the host. A new recipe, shaped like `snapshot`,
builds for a named target, makes the bundle, and packs it:

```
dist/noido-<target>-<version>.app.zip          (ditto -c -k --keepParent)
dist/noido-<target>-<version>.app.zip.sha256
```

for `aarch64-apple-darwin` and `x86_64-apple-darwin`, on the macOS legs
only — `ditto` is what keeps the bundle's structure intact, and the
existing tarballs stay exactly as they are for the `curl` users. Two
details that bite: `--keepParent` must be given the *bundle* itself (given
a staging directory it embeds the directory and `app "noido.app"` fails),
and the bundle directory must not be left in `dist/`, since the matrix
legs' artifacts are merged there and both bundles are called
`noido.app`.

**The release notes list the bundles.** The publish job's body
(`snapshot.yml`) names the three tarballs today; it grows a line for each
`.app.zip`, with its `releases/download/<tag>/…` URL. The tap's script
reads the asset names from that body, so a release whose notes omit them
is a release the tap cannot use — and says so by failing, not by waiting.

## The cask

```ruby
cask "noido-snapshot" do
  version "0.1.0+20261114-213045-abc1234"

  on_macos do
    on_arm   { sha256 "…"; url "…/noido-aarch64-apple-darwin-0.1.0+abc1234.app.zip" }
    on_intel { sha256 "…"; url "…/noido-x86_64-apple-darwin-0.1.0+abc1234.app.zip" }
  end

  name "noido"
  desc "PDF reader and note taker"
  homepage "https://github.com/rikardg/noido"

  app "noido.app"
  binary "#{appdir}/noido.app/Contents/MacOS/noido"

  preflight_steps do
    run "/usr/bin/xattr", args: ["-r", "-d", "com.apple.quarantine", "{{staged_path}}"]
  end
end
```

- `on_macos` is not decoration: `on_arm` and `on_intel` are evaluated on
  every OS (`cask/dsl.rb`), so without it the cask would also be offered
  on Linux ARM.
- `version` is `0.1.0+<tag stamp>-<commit>`: the stamp comes from the
  tag, the `0.1.0` prefix from the asset name in the release body, so a
  workspace version bump flows through without touching the template.
  Homebrew does not treat `+` as SemVer build metadata — it drops the `+`
  and compares the rest as tokens — so the timestamp orders the snapshots
  numerically and the commit breaks ties inside one second. Everything
  real sorts above every snapshot from `0.1.1` up; a first stable release
  numbered `0.1.0` would sort *below* today's snapshots, which matters
  only if the two ever share a cask name.
- The stamp is the publish job's clock in UTC (`date -u`), carried in the
  tag the release was made with. Not a commit date: jj rewrites commits
  and a rebase can move one backwards, while `brew upgrade` has to follow
  the order things were published. UTC because a local stamp repeats an
  hour when daylight saving ends. Not the API's `createdAt` either: a
  draft published later keeps its creation time, so the tag is the single
  source for both name and version.
- `binary` puts the command on `PATH` from the same bundle, so one
  `brew upgrade` moves the app and the CLI together.
- The app's `Info.plist` says `0.1.0+abc1234` while the cask says
  `0.1.0+20261114-213045-abc1234`: a build knows its commit and not when
  it was published. `brew list --cask --versions` and `noido --version`
  therefore differ on purpose; noido's stale hint compares binary versions
  with each other, so it is unaffected.

### Why the quarantine comes off

The bundle is signed ad hoc by the app that builds it (`codesign -s -` in
`app_bundle.rs`, because macOS will not run an unsigned arm64 binary), and
it is not notarized. Casks quarantine every download unconditionally —
`cask/download.rb` calls `Quarantine` in `fetch`; `--no-quarantine` was
deprecated in Homebrew 5.0.0 and its last code deleted in 2026-07-30 — and
nothing replaces it.

For ad-hoc code that means a Gatekeeper block on every version, not once:
Apple's TN3127 states that an ad-hoc signature's designated requirement is
"tied to that specific version of the code", so "if you tweak the code and
run it again, macOS repeats that prompt". The approval is stored as flag
`0x0040` on that copy's own `com.apple.quarantine`, so each upgrade is a
fresh unapproved copy. Homebrew's own inheritance path
(`cask/upgrade.rb` → `inherit_user_approval!`) needs the new app to satisfy
the *old* app's designated requirement, which for ad-hoc code changes with
every build.

Rather than make the user walk System Settings → Privacy & Security → Open
Anyway after every snapshot, the cask strips the attribute from the staged
tree in `preflight_steps`, before any artifact is moved. It must be
*preflight*: `postflight_steps` run after the `app` artifact, and by then
`{{staged_path}}/noido.app` has been replaced by a symlink to
`/Applications/noido.app` (`Moved#post_move`), so the block would act on a
link — root only at best, an aborted install at worst. Preflight sees the
real extracted tree, which the sandbox already allows writing to.

The same stanza exists in the `open-tap` community tap for real unsigned
projects (TomatoBar, after `homebrew/cask` disabled its cask for failing
Gatekeeper), written as `postflight_steps`. That it works there is down to
`xattr` following the symlink; this design does not rely on that.

**Measured, 2026-11-14, macOS 15.8.2.** The bundle from `--app-bundle`,
ad-hoc signed, `spctl` `rejected`, with `com.apple.quarantine` set
recursively: launching it from Finder gives "Move to Trash" / "Done", and
the approval path then asks for a password or fingerprint. The same bundle
with the attribute removed — `com.apple.provenance` still on it — starts
from Finder with no dialog at all, and stays running. So the strip is the
thing that makes the difference, and provenance alone does not block.
Still to confirm on macOS 26 and through a real `brew install --cask`.

**What we accept by doing this:**

- We are turning off a security control on the user's behalf. The
  download is still HTTPS and its `sha256` is pinned in the cask, so what
  arrives is what the tap says; but Gatekeeper no longer gets a say.
- Homebrew does not endorse it. Maintainers have said they will not make
  bypassing Gatekeeper easy and asked for an audit that warns about
  `xattr` in flight steps; today's signing audit skips third-party taps
  entirely (`cask/audit.rb` returns unless the tap is official, and
  `--signing` is disabled), so nothing flags ours today.
- This cask can never move into `homebrew/cask`, which has required
  signing and notarization since 2026-09-01. It lives in our tap for good.

### Audit

`brew audit --cask --strict --online` reports two things, and only one of
them is fixable:

- **"… is a GitHub pre-release"** — true by construction. The tap carries
  `audit_exceptions/github_prerelease_allowlist.json` with
  `{"noido-snapshot": "all"}`, which is the sanctioned way to say so.
- **"Use `sha256 :no_check` when URL is unversioned"** — accepted, not
  fixed. `Cask::URL#unversioned?` asks whether the cask's `url` line
  interpolates `#{…}`, not whether the URL happens to contain the version,
  and ours cannot: the version's stamp lives in the release tag, while the
  asset name carries only `0.1.0+<commit>`. Making the two agree would put
  the stamp into the asset name, which needs a stamp job feeding both the
  build matrix and the publish job. Not worth it for a lint whose checks
  (`brew install`, `brew upgrade`) run anyway, on the user's machine.

The consequence: audit is not a gate here, so the tap's own tests
(`scripts/test.sh`) and the install checks below carry that weight.

## The scheduled update

In the tap repo, hourly at a quiet minute, plus `workflow_dispatch`:

1. Fetch `https://github.com/rikardg/noido/releases.atom`. It lists
   published releases newest first, pre-releases included and drafts
   absent, with no authentication and no API budget.
2. Take the snapshot tag with the **highest** stamp, not the first entry:
   a draft published later carries an older stamp, and the feed's order is
   publication, not version.
3. Read the two `.app.zip` asset names from that entry's release body —
   the notes embed the full `releases/download/` URLs — rather than
   constructing them. `snapshot.yml` has to list them, which is the third
   change in noido.
4. Fetch the two `.sha256` files unauthenticated. Check each holds a
   64-character digest and names the archive it belongs to.
5. Compare the candidate version with the one already in the cask. Write
   when it sorts above it, and also when it is the same version but the
   rendered bytes differ (a template change has to reach an unchanged
   release); never when it is older: publishing an older draft later
   cannot walk the cask backwards, and `brew` never downgrades, so a
   backwards step would silently stall `brew upgrade` until the next
   press.
6. Fill `noido-snapshot.rb.in`, `ruby -c` the result, commit
   `noido-snapshot: 0.1.0+20261114-213045-abc1234`, push with the tap's
   own `GITHUB_TOKEN` (`contents: write`, commit as
   `github-actions[bot]`).

Two things that must not be silent:

- **An unreadable feed fails the run.** It is the only thing that can
  leave the tap quietly stale, and a red mark is how the user notices.
- **A body without a `.app.zip` URL fails the run.** That is a permanent
  mismatch — the wrong kind of release — and skipping it green is how the
  tap would stall for weeks without anyone noticing.
- **A release with assets still uploading is skipped, green.** `gh
  release create` creates the release before it uploads the files, so a
  run can catch one half-published; a missing `.sha256` means "next hour".
  So that this cannot hide a permanent mismatch, `snapshot.yml`'s existing
  completeness check grows to cover the two `.app.zip` and their
  `.sha256`, and a leg that lost its zip fails the release instead of
  publishing a dead cask source.

The template is a file in the tap, `noido-snapshot.rb.in`, filled by a
short script beside it, so the cask can be produced and read locally with
the same script the workflow runs.

Pushes made with `GITHUB_TOKEN` do not start further workflow runs. If
the tap ever grows validation of its own, it has to run inside this job
(or the push has to use another identity).

## One cask, no manifest

The script names `noido-snapshot.rb` and its template, and writes nothing
else. One cask needs no manifest to loop over; a second one is where the
manifest, and with it the rule that a pull's commit rebases and retries
once before pushing, would be argued. Both wait for the second cask.

## Cadence: still the button

Snapshots stay started by hand, from the Actions page or a
`snapshot-test*` branch. Publishing on every push to `main` is a one-line
trigger change, but it gives up a property the snapshot design chose
deliberately — a push never publishes on its own, because a broken commit
would then go public and reach everyone's `brew upgrade`. The tap does
not need it either way: it picks up whatever pre-releases appear.

If the button gets tiresome, the follow-up is that trigger plus a green
CI requirement, argued in its own design.

## Non-goals

- The stable channel, a `noido` cask or formula, dist, tags, changelog.
  This amends `2026-10-26-versioning-and-releases.md`, which had dist
  publish a formula into this tap: the tap is a cask tap now.
- Any formula, for Linuxbrew or otherwise.
- Notarization and a Developer ID. If it ever happens, the cask loses the
  `preflight_steps` block and this section says why.
- Windows.
- Desktop integration from the cask beyond `app` and `binary`.
- Publishing on every push (see above).

## Left to the builder

- A self-test for the script: a saved feed excerpt, one release body and
  the two `.sha256` files as fixtures, so the parse and the "write only
  when newer" rule run without the network. Nothing else can catch a bad
  parse, which by construction looks like "no new snapshot".
- How the script reads the feed and the body (`grep` for the tag prefix
  and the `.app.zip` URLs, or a proper XML read).
- The exact recipe shape for the bundle zip: whether it reuses `snapshot`'s
  version check, and whether the bundle is built with `--app-bundle
  dist/noido.app` per target.
- Whether the version should also lead with the workflow run number
  (`0.1.0+1234.20261114213045`), the only strictly monotonic ordering
  available; not done unless a wrong-way upgrade is seen.
- The bot identity for the commit, and the tap's README: what the cask
  is, the three command install, why quarantine is stripped, and that a
  public repository's scheduled workflows are disabled after 60 days
  without activity, so a quiet tap may need the schedule re-enabled.
- A line in `CLAUDE.md` pointing at the tap, so a later session finds it,
  and what to do with the Homebrew line in `docs/backlog.md`.

## To check by hand

- `brew trust --cask` then `brew install --cask` on Apple Silicon and on
  an Intel Mac, if one is at hand: the app launches from Finder with **no
  Gatekeeper dialog**, opens a PDF dropped on it, can be chosen as the
  default PDF app, and `noido` is on `PATH` (`which -a noido` — a
  `~/.cargo/bin` copy shadows brew's).
- `xattr -r -p com.apple.quarantine /Applications/noido.app` says nothing
  anywhere — `Contents/MacOS/noido` and `Contents/Info.plist` included.
  Checking the bundle root alone passes even when the strip went wrong.
  On the same tree, `xattr` without arguments also records whether
  `com.apple.provenance` is present, which is what macOS 14+ can key a
  decision on instead.
- `spctl -a -vv /Applications/noido.app`: `rejected` with no `source=`, as
  expected for an unnotarized app that nonetheless launches.
- Do the launch checks in a fresh user account or a VM, not on a machine
  that has approved noido before; a previous approval would hide the
  failure this whole design exists to avoid.
- `brew upgrade --cask noido-snapshot` twice: the second says it is up to
  date; the first leaves a working app with no dialog.
- Press the snapshot button, wait for the workflow, then upgrade: the new
  commit arrives.
- `brew audit --cask --strict --online`: record what it says. The signing
  audit should stay quiet for a third-party tap; `livecheck` will not know
  a pre-release version, and that is expected.

## Known, accepted

- Unsigned, and the quarantine is stripped on purpose. A browser download
  of the same zip is still blocked by Gatekeeper; only the cask path and
  the `curl` path work.
- `brew upgrade` prints `<cask>'s signer changed so macOS may prompt at
  next launch` on every upgrade. Homebrew is comparing the old bundle's
  designated requirement, which for ad-hoc code is its cdhash; the warning
  is wrong about the consequence here, because the new copy is stripped
  before it is installed and nothing prompts. It cannot be silenced from
  the cask.
- `noido-snapshot` and any future stable cask both install
  `/Applications/noido.app`, so they cannot be installed side by side;
  switching is uninstall then install.
- A copy left by `noido --install` in `~/Applications` should be removed
  (`noido --uninstall`) before switching to the cask, or Open With offers
  two apps and noido's stale hint points at the old one.
- The tap must stay public: `brew` fetches taps without credentials, and
  `brew trust` is per machine.

## Sources

- <https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements>
  — ad-hoc designated requirements are per-build, so the prompt repeats.
- <https://brew.sh/2025/11/12/homebrew-5.0.0/> and
  <https://github.com/Homebrew/brew/pull/23363> — `--no-quarantine`
  deprecated, then removed.
- <https://github.com/Homebrew/brew/issues/20755> — maintainers on why
  they will not help bypass Gatekeeper.
- <https://docs.brew.sh/Tap-Trust> — the trust step for third-party taps.
- <https://github.com/open-tap/homebrew-casks> and
  <https://github.com/ivoronin/TomatoBar/issues/102> — the unsigned-cask
  precedent and what happens without it.
- <https://flaviocopes.com/macos-open-anyway/> and
  <https://eclecticlight.co/2026/08/11/how-can-you-run-code-that-hasnt-been-notarised/>
  — measured `spctl` output and the macOS 26 behaviour.

The longer research write-up these came from is not in the repo; the
claims above are the ones the design rests on, and each is cited here.
