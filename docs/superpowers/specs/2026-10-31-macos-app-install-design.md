# The macOS app, click-to-open, and Homebrew: design

Date: 2026-10-31 · Status: draft, for approval

## Intent and why

Clicking a PDF in Finder should open noido. Today it cannot: Finder only
opens *apps* (bundles, never bare binaries), and it hands the file over as
an `application:openURLs:` Apple event rather than on the command line,
which winit 0.30 drops. `docs/investigations/2026-10-26-macos-app.md`
worked out what that needs; this design decides how noido gets it, and how
the result reaches a user through Homebrew.

Homebrew is the distribution the release investigation already picked
(`2026-10-26-versioning-and-releases.md`: dist, a formula, a tap). What
that plan does not cover is the app: dist builds a formula and a tarball,
never a `.app`. So macOS gets a bundle from noido itself, and Homebrew
installs the command that builds and registers it.

The shape the user asked for, as VS Code and Zed ship it: one app that
gets installed, plus a `noido` command that leads into it for terminal use.

Homebrew is not how most people install an app on macOS, though: a
`.dmg` with the app and an *Applications* shortcut is what a download
looks like, and a zip of the app is what a cask needs. Both cost a few
lines in the release job, so both ship; what they cannot buy is a quiet
first launch (see Constraints).

## Goals

- Finder shows noido in *Open With* for PDFs, and a double-click opens
  the document once noido has been chosen as the default.
- Once noido has it, the read works as on Linux: one window per file; a
  second click on a file that is already open brings that window forward.
- `noido <file>` from a terminal keeps working, with no separate CLI
  installation path.
- `brew install rikardg/tap/noido` on macOS gets a user from nothing to a
  registered app, and `brew upgrade` does not leave a stale copy behind
  without saying so.
- A user who does not use Homebrew gets the app from the GitHub release
  as a `.dmg` (drag to *Applications*) or a `.zip`, built from the same
  code as `--install`.
- Everything except the AppKit calls is plain, tested Rust: the plist, the
  bundle layout, the plan, the routing of a path that arrives from Finder.

## Non-goals

- Making noido the default PDF viewer for the user. Finder's *Get Info →
  Open With → Change All…* stays the user's step, as on Linux.
- Notarization and a Developer ID; signing is ad hoc.
- A noido icon (the bundle gets Finder's generic app icon, like the
  `.desktop` file gets the theme's generic document icon).
- Windows.
- A styled `.dmg` (background image, icon positions, licence screen):
  the image is a plain volume holding the app and an *Applications*
  link.
- A welcome window for a noido started with no document.
- Universal binaries: the release stays Apple Silicon.

## Anti-goals

- **A second, divergent copy of the bundle.** The plist and the layout
  are written once, in one function, used by both `--install` and the
  release job.
- **`noido` as a broken symlink into a bundle.** Measured below: a launch
  through a symlink loses the bundle's identity. If Homebrew links the
  command, it must be to something that survives that.
- **Silently owning the default-PDF choice**, or writing anything into
  `/Applications` (Homebrew's cask territory) without the user's y.
- **Depending on winit's launch internals where a documented seam will
  do.** Those internals change between winit versions; the release notes
  of each winit upgrade get checked against this design.

## Constraints

Measured on this machine (macOS 15.8, macOS 26 SDK):

- **The launch document arrives before the app finishes launching.** With
  a hand-rolled `NSApplication` and an `application:openURLs:` delegate,
  the order was `applicationWillFinishLaunching` → `openURLs` (1.2 ms
  later) → `applicationDidFinishLaunching` (0.2 ms later). So the path is
  available before any window exists — but only to a delegate that
  already has the method.
- **winit's delegate class exists only inside `run_native.`** winit 0.30.13
  creates `WinitApplicationDelegate` and sets it as the app delegate in
  `EventLoop::new`, which eframe calls *within* `run_native`; there is no
  point where noido's code runs between that and the launch events being
  dispatched. The class declares only `applicationDidFinishLaunching:`
  and `applicationWillTerminate:`, so `class_addMethod` works (no
  swizzle) — but it cannot be in place for the launch document.
- **`NSAppleEventManager` handlers do not receive the launch document.**
  Registered for `kCoreEventClass`/`kAEOpenDocuments` before the run
  loop, they saw nothing while the delegate route got the path. That
  door is closed.
- **A symlinked launch loses the bundle.** A probe binary inside a fake
  `.app`, started directly, reported `bundlePath=…/b.app id=test.probe`;
  started through a symlink in another directory it reported
  `bundlePath=/tmp id=nil`. Homebrew's `binary` stanza is a symlink.
- **Quarantine decides who gets a warning, not which format is used.**
  An app built on the user's machine is not quarantined. One that arrived
  in a downloaded `.dmg`, `.zip` or cask is, and Gatekeeper then wants
  *System Settings → Privacy & Security → Open Anyway* once per download
  unless it is notarized; a Developer ID is the only way out of that.
  Ad-hoc signing is still worth it: an unsigned bundle on current macOS
  can be refused outright instead of offering the button. Format is
  therefore not a Gatekeeper question at all — the `.dmg` and the cask
  are equally affected, and a formula that installs no bundle is the
  only route without a warning.
- `hdiutil` and `ditto`, which build the `.dmg` and the `.zip`, ship with
  macOS, and the release job already runs on an Apple Silicon runner for
  the macOS target.
- `unsafe_code` is forbidden workspace-wide. The AppKit work goes in its
  own crate that does not take the workspace lints, as the investigation
  says.
- noido opens one document per process, chosen at startup; `open.rs`
  already opens a path by launching a new detached `noido <path>`, and
  the single-instance IPC hands over when that file is already open.
- No icons, no `.icns`, nowhere in the repo.

## Approaches

**Where the bundle is built: noido builds it (picked)**, not
cargo-packager.

1. **noido writes the bundle itself.** A function from (exe, version,
   identifier, destination) to a plist and a layout, tested in a temp
   dir, with the same plan/confirm/steps shape as `desktop.rs`. The
   release job runs the same code through `noido --app-bundle <dir>`.
   Chosen: one source of truth, no new tool in CI, and the plist is the
   part we care about (see below) rather than something to work around.
2. cargo-packager 0.11.8. Builds `.app` and `.dmg` and signs/notarizes,
   but its `file-associations` writes the deprecated
   `CFBundleTypeExtensions` and no `LSItemContentTypes`/`LSHandlerRank`,
   so we would supply a plist through `info-plist-path` anyway — and
   `--install` would need a second copy of that plist for its own bundle.
3. cargo-bundle 0.12. Same split as 2, less signing.

**How the launch document reaches noido: a launcher mode inside noido
(picked).**

1. **No arguments and inside a `.app`: start in launcher mode.** The
   `noido-macos` crate runs a bare `NSApplication` loop with an
   `application:openURLs:` delegate (no winit, no egui), takes the first
   path the launch event carries, launches `noido <path>` detached and
   exits. Chosen: the event is caught before anything else exists, the
   GUI's own startup is untouched, and the failure mode is a loud "no
   document" rather than a mysteriously empty window. It is also what the
   throwaway AppleScript droplet does today, which works.
2. Swizzle `-[NSApplication setDelegate:]` at startup so winit's delegate
   gets the method as winit installs it, then load the document inside
   eframe's app creator. One process and no launcher mode, but it changes
   AppKit behaviour for the whole process and depends on winit setting a
   delegate at all.
3. An AppleScript droplet as the bundle's executable. Works today, but
   ships an AppleScript runtime as the app.

**Later opens: the GUI process keeps a handler (picked).** A file opened
while noido runs may be routed by Launch Services to the running instance
rather than starting a new one, so the GUI installs the same
`application:openURLs:` method on winit's delegate class once, in the
first frame — no timing pressure there — and drains the queue each frame
into the existing `open` hook.

**Distribution: all three routes, one bundle (picked).** They differ in
who they serve, not in what they build:

| Route | Who | First launch |
| --- | --- | --- |
| `brew install rikardg/tap/noido` (formula) + `noido --install` | Homebrew users | No warning: the app is built on their machine |
| `brew install --cask rikardg/tap/noido` | Homebrew users who want a normal app install | *Open Anyway* once, again per upgrade |
| `.dmg` / `.zip` from the GitHub release | everyone else | *Open Anyway* once |

Downloading a `.dmg` "instead of" a cask buys nothing: both are
quarantined. What the `.dmg` buys is not needing Homebrew. The formula is
the only quiet route, which is why it stays the one the README leads
with on macOS.

The cask waits for its own small step rather than its own design: it is
a cask file in the tap plus the version/`sha256` bump in the release job,
and it needs the `.dmg`/`.zip` to have a URL to point at.

## Behaviour

**The bundle** — `noido.app/Contents/`:

- `MacOS/noido`: the binary, COPIED, not linked (see anti-goals).
- `Info.plist`: `CFBundleExecutable=noido`, `CFBundleIdentifier=
  io.github.rikardg.noido`, `CFBundleName`/`CFBundleDisplayName=noido`,
  `CFBundleShortVersionString` = the version, `CFBundlePackageType=APPL`,
  `LSMinimumSystemVersion` from the toolchain's macOS baseline, and
  `CFBundleDocumentTypes` = one entry with `LSItemContentTypes
  = [com.adobe.pdf]`, `CFBundleTypeName = "PDF document"`,
  `CFBundleTypeRole = Viewer`, `LSHandlerRank = Alternate`, plus
  `CFBundleURLTypes` for the `noido:` scheme that the `.desktop` file
  already claims.
- No icon file, no `PkgInfo`, no `LSUIElement`.

**`noido --install`** (macOS):

- Builds `~/Applications/noido.app` from the running binary (version
  taken from this build) and prints what it will do first, in the same
  one-line-per-step form as on Linux, asking `Proceed? [y/N]`.
- A bundle of the same version already there: "already installed",
  nothing to do. A different version: the step says it replaces it, so a
  `brew upgrade` is followed by `noido --install` with a reason to say
  yes.
- `/Applications/noido.app` present (a Homebrew cask): says so and
  stops; no second bundle with the same identifier.
- Then ad-hoc signs the bundle (`codesign -s - --force`) and registers it
  (`lsregister -f`), each a step whose failure is a note, not an error,
  except the signing, which is a failure on Apple Silicon.
- The user is left to pick noido in Finder: the summary says that once,
  with the *Get Info → Open With → Change All…* path, and that noido does
  not touch the default itself.
- `--uninstall` removes `~/Applications/noido.app` when its
  `Info.plist` carries noido's identifier, and nothing otherwise.

**`noido --app-bundle <dir>`** (macOS, release tooling): writes the same
bundle to `<dir>`, signs it ad hoc, and prints the path. `--install` is
this plus the destination and the registration. On other platforms it is
not compiled in.

**The release artifacts** (macOS leg of the release job, from that one
bundle):

- `noido.app.zip`: `ditto -c -k --keepParent noido.app`. What the cask
  points at.
- `noido-<version>-macos-arm64.dmg`: a staging directory holding
  `noido.app` and a symlink to `/Applications`, packed with
  `hdiutil create -volname noido -srcfolder … -format UDZO -ov`. What a
  user without Homebrew downloads and drags.
- The same `.dmg` and `.zip` attached to the GitHub release beside the
  Linux tarballs; the signing happens before either is made, so what is
  inside them is the ad-hoc signed app.

**Launch modes:**

- A file argument: unchanged.
- No argument and the path noido was started by, unresolved, is inside
  `.app/Contents/MacOS`: launcher mode. It waits for the launch event's
  paths; the first becomes `noido <path>`, launched detached, and this
  process exits 0. No document within two seconds (a double-click on the
  app itself, which sends no open event): exit 0, quietly.
- No argument, otherwise: the clap error, as now. The unresolved path is
  the test: a Homebrew `binary` symlink started from a terminal keeps the
  symlink's path, so plain `noido` there is the clap error rather than a
  process waiting for a document no one will send (see Constraints).
- A path queue that is non-empty during a normal run: each path goes to
  the `open` hook, exactly as a picked file does today, so an already
  open file hands over and a new one gets a window.

## Order of work

1. **`noido --app-bundle`, and ad-hoc signing plus registration.** The
   pure part of everything else; testable without a Mac's Finder, and it
   gives the harness for step 2 by hand.
2. **Launcher mode and the later-opens handler** (`noido-macos` crate,
   objc2). This is the part that only a Mac can check; it lands with the
   hand-testing list below.
3. **`noido --install` / `--uninstall` on macOS**, around step 1's code,
   sharing the plist and layout.
4. **The release artifacts:** the macOS leg of the release job runs
   `--app-bundle`, then builds `noido.app.zip` and the `.dmg` from it and
   attaches both to the release; dist's formula gains a `caveats` line for
   `noido --install`; the README says which route to take and what each
   one's first launch looks like.
5. **The tap's cask:** `noido` with `app` and `binary` stanzas, plus the
   version/`sha256` bump in the release job, once step 4 has published
   something for it to point at.

Click-to-open is complete after step 2 for a bundle built by hand, and
after step 3 for everyone else. Step 4 is what makes it reachable without
a checkout; nothing before it depends on Homebrew, and step 5 depends on
step 4 only for its URL.

## Code shape

- `crates/noido-macos/`: new, macOS-only dependencies (`objc2`,
  `objc2-app-kit`, `objc2-foundation`), `unsafe_code = "deny"` with
  narrow `allow`s, and no workspace lints. Two entry points:
  `run_launcher(&exe) -> !`-ish for launch mode, and
  `install_open_urls(handler)` for the GUI, plus the path queue behind a
  `Mutex`.
- `crates/frontends/gui-egui/src/app_bundle.rs`: the plist text, the
  layout, the plan/steps/`carry_out` shape borrowed from `desktop.rs`,
  the `--app-bundle`/`--install`/`--uninstall` entry points, and the
  decision "is this a Finder launch?" as a pure function. Compiled on
  every platform; only the entry points are macOS-only, so CI's Linux leg
  still tests the plist and the plans.
- `gui-egui/src/main.rs`: the launch-mode branch before clap's
  required-file rule (an argument-less, bundled start is not an error);
  clap gains `--app-bundle <dir>`.
- `gui-egui/src/app.rs`: one `receive_open_files()` step in `show()`,
  beside `receive_renders` and `receive_reload`, feeding the existing
  `open` hook.
- `gui-egui/src/desktop.rs`: unchanged; `--install` dispatches on
  `cfg!(target_os = "macos")`.
- `justfile`: `app` (build the release binary with `--app-bundle` into a
  scratch dir), and `dmg`/`zip` recipes wrapping `hdiutil` and `ditto`
  so they can be tried by hand before the release job runs them; `run`
  unchanged.

## Testing

- `app_bundle`: the plist parses and carries the UTI declarations, the
  identifier, the version and the URL scheme; the layout of a bundle
  written into a temp dir (binary present and executable, plist where
  Finder expects it); plans for fresh install, same version, other
  version, `/Applications` present, uninstall with and without ours
  there.
- The Finder-launch decision: no argument inside a bundle is a launcher
  run; no argument outside one, or through a symlink, is the clap error;
  an argument is never a launcher run.
- Harness (GUI): a path in the queue during a frame reaches the `open`
  hook; two paths do too, in order; an empty queue does nothing.
- By hand on a Mac, and only that: the launch event is caught by the
  launcher mode; Finder's *Open With* lists the bundle; a double-click
  after *Change All* opens the PDF; a second click on the same PDF brings
  the window forward; a click on a different one while noido runs opens a
  second window; `noido --version` and `noido <file>` from a terminal
  behave, and the Dock shows the app's name; `codesign -dv` says ad hoc;
  `brew uninstall`/`--uninstall` leave nothing behind.
- By hand, the artifacts: the `.dmg` opens with the app and the
  *Applications* link in it, and dragging the app across lands a working
  copy that *Open With* sees; the `.dmg` and the `.zip` both come out of a
  fresh `just dmg`/`just zip`; `spctl -a -vv` and a first launch from a
  quarantined copy show the warning the README promises, not a worse one.
- State during hand testing: on macOS it is
  `~/Library/Application Support/noido` whatever `XDG_STATE_HOME` says,
  so a hand run touches the real one; use a scratch `HOME` for anything
  that should not.
- Registering a bundle with Launch Services is global to the user's
  session, not to a scratch `HOME`, and a bundle that declares PDFs
  competes with the real ones the moment it is registered: while this
  design was being written, throwaway probe bundles became candidates for
  PDFs and macOS complained about them. Any bundle used for testing gets
  unregistered (`lsregister -u <path>`) as soon as it has been tried, and
  never shares noido's own bundle identifier.

## To check on a Mac, before the plan is written

- Whether Launch Services, with the app already running, starts the
  bundle again or sends the open event to the running process. The design
  handles both, but which one happens decides whether the launcher mode
  runs on every click.
- Whether `LSItemContentTypes` alone is enough for *Open With*, or
  `CFBundleTypeExtensions = [pdf]` is still needed beside it.
- Whether ad-hoc signing the bundle is needed at all for it to launch
  from `~/Applications`, or only for Gatekeeper to offer *Open Anyway*
  instead of refusing a quarantined copy.
- Which warning a quarantined, ad-hoc signed `.dmg` app actually gets on
  macOS 26, so the README can promise the right one.

## Sources

- `docs/investigations/2026-10-26-macos-app.md`,
  `2026-10-26-versioning-and-releases.md`,
  `2026-10-26-desktop-install-design.md`,
  `2026-10-31-open-file-dialog-design.md`
- winit 0.30.13: `src/platform_impl/macos/app_state.rs` (`declare_class!`
  for `WinitApplicationDelegate`), `event_loop.rs` (`EventLoop::new`)
- eframe 0.37.0: `src/native/run.rs` (`with_event_loop`, `create_event_loop`)
- cargo-packager `crates/packager/src/package/app/mod.rs` (`create_info_plist`,
  `file_associations`), `config/mod.rs`
- dist config reference: https://axodotdev.github.io/cargo-dist/book/reference/config.html
  (Homebrew installer; no cask support)
- Homebrew cask `binary` and `app` stanzas, as used by
  `zed` (`Contents/MacOS/cli`) and `visual-studio-code`
  (`Contents/Resources/app/bin/code`)
