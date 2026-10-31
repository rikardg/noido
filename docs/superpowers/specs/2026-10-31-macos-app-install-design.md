# The macOS app and click-to-open: design

Date: 2026-10-31, revised 2026-11-01 · Status: draft, for approval

## Intent and why

Clicking a PDF in Finder should open noido. Today it cannot: Finder only
opens *apps* (bundles, never bare binaries), and it hands the file over as
an `application:openURLs:` Apple event rather than on the command line,
which winit 0.30 drops. `docs/investigations/2026-10-26-macos-app.md`
worked out what that needs; this design decides how noido gets it.

The shape the user asked for, as VS Code and Zed ship it: one app that
gets installed, plus a `noido` command that leads into it for terminal use.

How the app reaches users who have no checkout (the release job's `.dmg`
and `.zip`, a Homebrew cask) is left to the release-job design, which does
not exist yet; see "Left to the release design" for what this design
hands over to it. This one ends with an app that `noido --install` builds
and registers, and a `noido --app-bundle <dir>` the release job can call.

## Goals

- Finder shows noido in *Open With* for PDFs, and a double-click opens
  the document once noido has been chosen as the default.
- Once noido has it, the read works as on Linux: one window per file; a
  second click on a file that is already open brings that window forward.
  Every window noido opens or hands over to comes to the front.
- Opening several PDFs at once from Finder opens each of them.
- `noido <file>` from a terminal keeps working, with no separate CLI
  installation path.
- After `noido --install` from a Homebrew formula, `brew upgrade` updates
  the app too; from anywhere else, a stale app is at least named as such.
- Everything except the AppKit calls is plain, tested Rust: the plist, the
  bundle layout, the plan, the routing of a path that arrives from Finder.

## Non-goals

- Making noido the default PDF viewer for the user. Finder's *Get Info →
  Open With → Change All…* stays the user's step, as on Linux.
- Release artifacts (`.dmg`, `.zip`), a Homebrew cask, and the formula's
  `caveats`: the release-job design.
- Notarization and a Developer ID; signing is ad hoc.
- A noido icon (the bundle gets Finder's generic app icon, like the
  `.desktop` file gets the theme's generic document icon).
- Windows.
- A welcome window for a noido started with no document.
- Universal binaries: Apple Silicon only.

## Anti-goals

- **A second, divergent copy of the bundle.** The plist and the layout
  are written once, in one function, used by both `--install` and
  `--app-bundle`.
- **`noido` as a symlink into a bundle.** Measured below: a launch
  through such a symlink loses the bundle's identity.
- **Silently owning the default-PDF choice**, or writing anything into
  `/Applications` without the user's y.
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
- **winit's delegate class exists only inside `run_native`.** winit 0.30.13
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
- **A launch through a symlink takes the symlink's path as its own.** A
  probe binary inside a fake `.app`, started directly, reported
  `bundlePath=…/b.app id=test.probe`; started through a symlink in
  another directory it reported `bundlePath=/tmp id=nil`. The bundle is
  found from the path the process was started by, unresolved. The same
  rule, run the other way, is what the Homebrew route below relies on:
  a symlink *inside* the bundle should keep the bundle (to be confirmed,
  see "To check").
- **Activation is cooperative since macOS 14.** An app can no longer take
  the front by itself: `activateIgnoringOtherApps:` (what winit's
  `focus_window`, and so egui's `ViewportCommand::Focus`, calls) is
  deprecated and mostly ignored. The active app has to yield
  (`yieldActivationToApplication:`), or the launch has to go through
  Launch Services with activation asked for (`NSWorkspace
  openApplicationAtURL:configuration:` with `activates`). A process
  started with a plain `spawn`, as `open.rs` does today, gets neither.
  This hits the launcher's GUI, a second file opened from a running
  noido, and the single-instance hand-over (`app.rs`, which sends
  `Focus` and `RequestUserAttention`).
- **AppKit can read command-line arguments as documents.** With the
  `NSTreatUnknownArgumentsAsOpen` default left alone, a GUI started as
  `…/MacOS/noido file.pdf` may get an open event for its own argument
  (to be confirmed, see "To check").
- `codesign` will not sign a bundle whose main executable is a symlink;
  the binary keeps its own ad-hoc signature from the linker.
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
   identifier, destination, how the binary gets there) to a plist and a
   layout, tested in a temp dir, with the same plan/confirm/steps shape
   as `desktop.rs`. `noido --app-bundle <dir>` runs the same code for
   the release job and for trying by hand. Chosen: one source of truth,
   no new tool in CI, and the plist is the part we care about (see
   below) rather than something to work around.
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
   `application:openURLs:` delegate (no winit, no egui), takes every URL
   the launch event carries, starts one GUI per URL through Launch
   Services (see Behaviour) and exits. Chosen: the event is caught before
   anything else exists, the GUI's own startup is untouched, and the
   failure mode is a loud "no document" rather than a mysteriously empty
   window. It is also what the throwaway AppleScript droplet does today,
   which works. Its cost: the Dock shows the launcher bounce and go
   before the GUI's icon appears.
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

**How the app finds its binary: a symlink for Homebrew, a copy otherwise
(picked, if the spike confirms the symlink).**

1. **`Contents/MacOS/noido` is a symlink to Homebrew's stable `opt`
   path** (`/opt/homebrew/opt/noido/bin/noido`) when the running binary
   resolves into a Homebrew Cellar, and a copy otherwise. Chosen: `brew
   upgrade` moves `opt` to the new version, so the app never goes stale
   and `--install` never has to be re-run. The bundle is left unsigned
   at bundle level (codesign refuses a symlinked main executable), which
   a bundle built on the user's machine and never quarantined does not
   need.
2. Always copy, and have `--install` replace a bundle of another
   version. Simpler, and the fallback if the symlink fails the spike;
   but every `brew upgrade` leaves the app on the old version until the
   user runs `noido --install` again.

**Coming to the front: whoever is in front yields (picked).** Every
place where one noido process starts or wakes another passes activation
along explicitly:

- The launcher, and the GUI's `open` hook on macOS, start the new
  process with `NSWorkspace openApplicationAtURL:configuration:` on the
  bundle, with the path as its argument, `createsNewApplicationInstance`
  and `activates` set — instead of `Command::spawn`. Outside a bundle
  (a terminal `noido`), `open` keeps spawning as today.
- The single-instance hand-over: the process that finds the file already
  open yields activation to the running one before it exits, and the
  running one calls `activate` (the cooperative one) on receipt besides
  `Focus`. The sender needs the running process's pid for that; the IPC
  answer is where it comes from.

## Behaviour

**The bundle** — `noido.app/Contents/`:

- `MacOS/noido`: the binary, copied, or the symlink described above.
  Nothing else in the bundle is ever a link.
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

**`noido --app-bundle <dir>`** (macOS): writes the bundle with a copied
binary to `<dir>`, signs it ad hoc (`codesign -s - --force`), and prints
the path. For the release job and for trying by hand; never registers.
Not compiled in on other platforms.

**`noido --install`** (macOS):

- Builds `~/Applications/noido.app` from the running binary: a symlink
  to the `opt` path when the binary resolves into a Homebrew Cellar, a
  copy otherwise. Prints what it will do first, in the same
  one-line-per-step form as on Linux, asking `Proceed? [y/N]`; the step
  says which of the two it is.
- A bundle already there: the same link target, or a copy of the same
  version, is "already installed", nothing to do. Anything else: the step
  says it replaces it and with what.
- `/Applications/noido.app` present (a cask or a dragged copy): says so
  and stops; no second bundle with the same identifier.
- Then, for a copy, ad-hoc signs the bundle; and registers it
  (`lsregister -f`). A failed registration is a note; a failed signing
  is a failure.
- The user is left to pick noido in Finder: the summary says that once,
  with the *Get Info → Open With → Change All…* path, and that noido does
  not touch the default itself.
- `--uninstall` unregisters and removes `~/Applications/noido.app` when
  its `Info.plist` carries noido's identifier, and nothing otherwise.

**Launch modes:**

- A file or link argument: unchanged. In a bundle, the GUI first sets
  `NSTreatUnknownArgumentsAsOpen` to `NO` for itself, so AppKit does not
  turn its own argument into an open event (if the spike shows it would).
- A `-psn_…` argument, which older Launch Services passed on a Finder
  launch, is dropped before clap sees the arguments.
- No argument and the path noido was started by, unresolved, is inside
  `.app/Contents/MacOS`: launcher mode. Each URL the launch event carries
  becomes one GUI, in order: a `file:` URL as its path, any other URL
  (`noido:`) as its text, started as under "Coming to the front". Then
  this process exits 0. No URL by `applicationDidFinishLaunching` plus a
  short grace (a double-click on the app itself, which sends no open
  event): exit 0, quietly. The grace is set from the spike's timings,
  well under a second.
- No argument, otherwise: the clap error, as now. The unresolved path is
  the test: a symlink started from a terminal keeps the symlink's path,
  so plain `noido` there is the clap error rather than a process waiting
  for a document no one will send.
- A path queue that is non-empty during a normal run: each URL goes to
  the `open` hook, exactly as a picked file does today, so an already
  open file hands over and a new one gets a window.

## To check on a Mac, before the plan is written

A throwaway spike, outside the repo: probe bundles with their own
identifier, opened with `open -a <probe>.app <file>` (Launch Services'
own route, the same event Finder sends), each unregistered with
`lsregister -u` as soon as it has been tried.

- Whether Launch Services, with the app already running, starts the
  bundle again or sends the open event to the running process; and with
  several instances of the bundle running, which one gets it.
- Whether a GUI started through `openApplicationAtURL:configuration:`
  with `activates` comes to the front over Finder, and whether one
  started with a plain spawn does not (the problem this design assumes).
- Whether `yieldActivationToApplication:` from a process that is about
  to exit lets the other one come forward with `activate`.
- Whether AppKit turns a bundled GUI's argument into an open event, and
  whether `NSTreatUnknownArgumentsAsOpen = NO` stops it.
- Whether a symlinked `Contents/MacOS/noido` keeps the bundle's identity
  and its document types when Launch Services starts it, and whether an
  unsigned bundle of that shape launches from `~/Applications`.
- Whether `LSItemContentTypes` alone is enough for *Open With*, or
  `CFBundleTypeExtensions = [pdf]` is still needed beside it.
- How long after `applicationDidFinishLaunching` an open event can still
  arrive, to set the launcher's grace.

The answers may move picks above: the symlink falls back to a copy, and
if a running instance always gets the open event, the launcher only ever
handles a cold start.

## Order of work

1. **`noido --app-bundle`**: the plist, the layout, the ad-hoc signing.
   The pure part of everything else; testable without a Mac's Finder,
   and it gives the bundle that step 2 is tried with by hand.
2. **Launcher mode, the later-opens handler, and coming to the front**
   (`noido-macos` crate, objc2), including the macOS `open` hook and the
   hand-over's yield. This is the part that only a Mac can check; it
   lands with the hand-testing list below.
3. **`noido --install` / `--uninstall` on macOS**, around step 1's code:
   the symlink-or-copy choice, the plans, registration.

Click-to-open is complete after step 2 for a bundle built by hand, and
after step 3 for anyone with a checkout or the formula.

## Left to the release design

What this design found that the release-job design should start from:

- `noido --app-bundle <dir>` is the seam: the release job builds the app
  with it, then `ditto -c -k --keepParent noido.app noido.app.zip` (what
  a cask points at) and a `.dmg` from a staging directory holding the app
  and a symlink to `/Applications` (`hdiutil create -volname noido
  -srcfolder … -format UDZO -ov`). Both tools ship with macOS.
- **Quarantine decides who gets a warning, not which format is used.** A
  downloaded `.dmg`, `.zip` or cask is quarantined, and Gatekeeper then
  wants *System Settings → Privacy & Security → Open Anyway* once per
  download unless it is notarized. Ad-hoc signing is still needed: an
  unsigned bundle can be refused outright instead of offering the
  button. The formula plus `noido --install` is the only quiet route,
  which is why the README should lead with it.
- A cask and a formula both called `noido` in one tap both install a
  `noido` command: the cask needs `conflicts_with formula: "noido"`, and
  `noido --install` already stops when `/Applications/noido.app` exists.
- The formula's `caveats` should name `noido --install`.
- Which warning a quarantined, ad-hoc signed app actually gets on
  macOS 26 is still to be seen, so the README promises the right one.

## Code shape

- `crates/noido-macos/`: new, macOS-only dependencies (`objc2`,
  `objc2-app-kit`, `objc2-foundation`), `unsafe_code = "deny"` with
  narrow `allow`s, and no workspace lints. Entry points:
  `run_launcher(&bundle) -> !`-ish for launch mode,
  `install_open_urls()` plus the URL queue behind a `Mutex` for the GUI,
  `launch_in_bundle(&bundle, arg)` for starting a GUI with activation,
  and `yield_to(pid)` / `activate()` for the hand-over.
- `crates/frontends/gui-egui/src/app_bundle.rs`: the plist text, the
  layout, the symlink-or-copy decision (from the resolved exe path to the
  `opt` path, or none), the plan/steps/`carry_out` shape borrowed from
  `desktop.rs`, the `--app-bundle`/`--install`/`--uninstall` entry
  points, and "is this a Finder launch?" as a pure function. Compiled on
  every platform; only the entry points are macOS-only, so CI's Linux leg
  still tests the plist and the plans.
- `gui-egui/src/main.rs`: dropping `-psn_…`, then the launch-mode branch
  before clap's required-file rule (an argument-less, bundled start is
  not an error); clap gains `--app-bundle <dir>`.
- `gui-egui/src/app.rs`: one `receive_open_files()` step in `show()`,
  beside `receive_renders` and `receive_reload`, feeding the existing
  `open` hook; the hand-over activates as well as focusing.
- `gui-egui/src/open.rs`: on macOS inside a bundle, `open` starts the
  new window through `launch_in_bundle`.
- `gui-egui/src/instance.rs`: the answer to "activate" carries the
  running process's pid.
- `gui-egui/src/desktop.rs`: unchanged; `--install` dispatches on
  `cfg!(target_os = "macos")`.
- `justfile`: `app`, building the release binary and running
  `--app-bundle` into a scratch dir; `run` unchanged.

## Testing

- `app_bundle`: the plist parses and carries the UTI declarations, the
  identifier, the version and the URL scheme; the layout of a bundle
  written into a temp dir (binary present and executable, plist where
  Finder expects it); the symlink-or-copy decision for a Cellar path, an
  `opt` path, a `target/release` path; plans for fresh install, same
  link, other link, same version, other version, `/Applications`
  present, uninstall with and without ours there.
- The Finder-launch decision: no argument inside a bundle is a launcher
  run; no argument outside one, or through a symlink, is the clap error;
  an argument is never a launcher run; `-psn_…` is dropped.
- The launcher's URL routing, as a pure function: several `file:` URLs
  give several paths in order; a `noido:` URL gives its text.
- `instance`: the "activate" answer carries a pid.
- Harness (GUI): a path in the queue during a frame reaches the `open`
  hook; two paths do too, in order; an empty queue does nothing.
- By hand on a Mac, and only that, each window checked to be *in front*:
  Finder's *Open With* lists the bundle; a double-click after *Change
  All* opens the PDF; three PDFs opened at once give three windows; a
  second click on the same PDF brings its window forward; a click on a
  different one while noido runs opens a second window; a `noido:` link
  opens; double-clicking the app itself exits quietly and quickly;
  `noido --version` and `noido <file>` from a terminal behave; the Dock
  shows the app's name, and the launcher's bounce is tolerable;
  `codesign -dv` says ad hoc for a copied bundle; after `brew upgrade`
  the symlinked app runs the new version; `--uninstall` leaves nothing
  behind.
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

## Sources

- `docs/investigations/2026-10-26-macos-app.md`,
  `2026-10-26-versioning-and-releases.md`,
  `2026-10-26-desktop-install-design.md`,
  `2026-10-31-open-file-dialog-design.md`
- winit 0.30.13: `src/platform_impl/macos/app_state.rs` (`declare_class!`
  for `WinitApplicationDelegate`), `event_loop.rs` (`EventLoop::new`),
  `window_delegate.rs` (`focus_window`)
- eframe 0.37.0: `src/native/run.rs` (`with_event_loop`, `create_event_loop`)
- cargo-packager `crates/packager/src/package/app/mod.rs` (`create_info_plist`,
  `file_associations`), `config/mod.rs`
- Apple: `NSApplication.yieldActivation(to:)`, `activate()`,
  `NSWorkspace.OpenConfiguration` (`activates`,
  `createsNewApplicationInstance`, `arguments`)
- Homebrew cask `binary` and `app` stanzas, as used by
  `zed` (`Contents/MacOS/cli`) and `visual-studio-code`
  (`Contents/Resources/app/bin/code`)
