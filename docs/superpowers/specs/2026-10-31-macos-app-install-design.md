# The macOS app and click-to-open: design

Date: 2026-10-31, revised 2026-11-01 after a spike · Status: draft, for
approval

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
- An installed app that is older than the `noido` command (after a `brew
  upgrade`, say) is named as such, with the command that updates it.
- Everything except the AppKit calls is plain, tested Rust: the plist, the
  bundle layout, the plan, the routing of a path that arrives from Finder.

## Non-goals

- Making noido the default PDF viewer for the user. Finder's *Get Info →
  Open With → Change All…* stays the user's step, as on Linux.
- Updating the installed app by itself. The user runs `noido --install`
  again; noido only says when it is due.
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
- **Symlinks between the command and the bundle, either way.** Measured
  below: a launch through a symlink, into the bundle or out of it, loses
  the bundle's identity inside the process.
- **Silently owning the default-PDF choice**, or writing anything into
  `/Applications` without the user's y.
- **Depending on winit's launch internals, or on undocumented Launch
  Services behaviour, where a documented seam will do.** winit's
  internals change between versions; the release notes of each winit
  upgrade get checked against this design.

## Constraints

Measured on this machine (macOS 15.8, macOS 26 SDK), first while the
investigation was written and then in a spike on 2026-11-01 with probe
apps (their own bundle identifiers, opened with `open -a` and `open -b`,
the route Finder's clicks take; all unregistered and deleted afterwards).

**How documents arrive:**

- **On a cold start the document arrives before the app finishes
  launching.** The order was `applicationWillFinishLaunching` →
  `application:openURLs:` → `applicationDidFinishLaunching`, the open
  event 0.5 ms ahead of the last in each of six cold launches. So a
  process that has had no open event by `applicationDidFinishLaunching`
  will not get one.
- **Several files opened at once arrive as one event** with every URL in
  it.
- **While the app runs, Launch Services sends later opens to the running
  process**, not to a new one. With several running, every open went to
  the *first* one, which Launch Services also brought to the front to
  deliver it, even when another was in front.
- **winit's delegate class exists only inside `run_native`.** winit 0.30.13
  creates `WinitApplicationDelegate` and sets it as the app delegate in
  `EventLoop::new`, which eframe calls *within* `run_native`; there is no
  point where noido's code runs between that and the launch events being
  dispatched. The class declares only `applicationDidFinishLaunching:`
  and `applicationWillTerminate:`, so `class_addMethod` works (no
  swizzle) — but it cannot be in place for the launch document.
- **`NSAppleEventManager` handlers do not receive the launch document.**
  Registered for `kCoreEventClass`/`kAEOpenDocuments` before the run
  loop, they saw nothing while the delegate route got the path.
- **AppKit does not turn command-line arguments into open events.** A
  bundled probe with `application:openURLs:` started with a file argument,
  from a terminal or through Launch Services, got no event for it.

**Bundle identity:**

- **A process takes its bundle from the path it was started by.** A probe
  inside a `.app`, started through a symlink from elsewhere, reported no
  bundle. The other way round is no better: with `Contents/MacOS/<exe>` a
  symlink to a binary outside, Launch Services starts the *resolved*
  binary, so `argv[0]` and `Bundle.main` point outside the bundle and the
  identifier is nil. Launch Services itself still counts the process as
  the app, and `NSRunningApplication.current.bundleURL` gives the bundle
  once AppKit is up; but that is not documented behaviour, and `codesign`
  refuses a bundle whose main executable is a symlink.
- **Launch Services disables apps under a temp directory** (`launch-disabled
  in-temp-dir` in `lsregister -dump`): a bundle built under `/tmp` or
  `$TMPDIR` can be registered but not launched or offered in *Open With*.
- **`LSItemContentTypes = [com.adobe.pdf]` alone is enough** for the
  probe to be among the apps macOS offers for a PDF
  (`NSWorkspace urlsForApplicationsToOpenURL:`, the list Finder's *Open
  With* uses); no `CFBundleTypeExtensions` needed.

**Coming to the front** (activation is cooperative since macOS 14):

- A window started with a plain spawn, as `open.rs` does today, stays
  behind the app that was in front.
- A window started with `NSWorkspace openApplicationAtURL:configuration:`
  with `activates` (and `createsNewApplicationInstance`, the path as its
  argument) comes to the front.
- For the hand-over to a window that already has the file,
  `yieldActivationToApplication:` from the sender followed by
  `activate()` in the receiver did *not* bring the receiver forward. The
  sender calling `activateWithOptions:` (or `activateFromApplication:`)
  on the receiver's `NSRunningApplication` did, and did so also from a
  plain process started in a terminal. The receiver's own `Focus`
  (`activateIgnoringOtherApps:` in winit) is not what does it.

**Other:**

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
   dir, with the same plan/confirm/steps shape as `desktop.rs`. `noido
   --app-bundle <dir>` runs the same code for the release job and for
   trying by hand. Chosen: one source of truth, no new tool in CI, and
   the plist is the part we care about (see below) rather than something
   to work around.
2. cargo-packager 0.11.8. Builds `.app` and `.dmg` and signs/notarizes,
   but its `file-associations` writes the deprecated
   `CFBundleTypeExtensions` and no `LSItemContentTypes`/`LSHandlerRank`,
   so we would supply a plist through `info-plist-path` anyway — and
   `--install` would need a second copy of that plist for its own bundle.
3. cargo-bundle 0.12. Same split as 2, less signing.

**How documents reach noido: a handler in the running GUI, and a
launcher mode for a cold start (picked).**

Most clicks reach a noido that is already running, so the GUI's handler
is the main route; the launcher only covers the first document, which
arrives before any of noido's code could take it.

1. **The GUI installs `application:openURLs:`** on winit's delegate class
   once, in the first frame (no timing pressure there), and drains the
   queue each frame into the existing `open` hook. Launch Services brings
   the first noido window forward to deliver the event; the new window,
   or the one that already has the file, then takes the front from it.
2. **No arguments and inside a `.app`: launcher mode.** The `noido-macos`
   crate runs a bare `NSApplication` loop with an `application:openURLs:`
   delegate (no winit, no egui), starts one GUI per URL the launch event
   carries (see "Coming to the front") and exits. Chosen over the
   alternatives below: the event is caught before anything else exists,
   the GUI's own startup is untouched, and the failure mode is a loud "no
   document" rather than a mysteriously empty window. Its cost: on a cold
   start the Dock shows the launcher bounce and go before the GUI's icon
   appears.
3. Rejected: swizzling `-[NSApplication setDelegate:]` at startup so
   winit's delegate has the method from the start, then loading the
   document inside eframe's app creator. One process and no launcher
   mode, but it changes AppKit behaviour for the whole process and
   depends on winit setting a delegate at all.
4. Rejected: an AppleScript droplet as the bundle's executable. Works,
   but ships an AppleScript runtime as the app.

**How the app gets its binary: a copy (picked).**

1. **`Contents/MacOS/noido` is a copy** of the binary that ran
   `--install` or `--app-bundle`, ad-hoc signed with the bundle. Chosen:
   the bundle keeps its identity by the documented route and can be
   signed. Its cost: after `brew upgrade` the app is the old version
   until `noido --install` runs again, which noido says (see Behaviour).
2. Rejected: a symlink to Homebrew's stable `opt` path, so `brew upgrade`
   would update the app too. Measured: the process loses its bundle (see
   Constraints), so noido would have to start AppKit before reading its
   arguments to learn its bundle from Launch Services, rely on that
   undocumented behaviour, and ship a bundle `codesign` refuses.

**Coming to the front: the process that is in front passes it on
(picked).**

- The launcher, and the GUI's `open` hook when it runs inside the bundle,
  start each new window with `NSWorkspace
  openApplicationAtURL:configuration:` on the bundle, with the path as
  its argument and `createsNewApplicationInstance` and `activates` set,
  instead of `Command::spawn`. A noido outside a bundle (started from a
  terminal) keeps spawning as today.
- The single-instance hand-over: the running noido's answer to
  "activate" carries its pid, and the process that found the file open
  calls `activateWithOptions:` on that pid's `NSRunningApplication`
  before it exits. On macOS this replaces relying on the receiver's
  `Focus`, which stays for the other platforms.

## Behaviour

**The bundle** — `noido.app/Contents/`:

- `MacOS/noido`: the binary, copied. Nothing in the bundle is a link.
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

**`noido --app-bundle <dir>`** (macOS): writes the bundle to `<dir>`,
signs it ad hoc (`codesign -s - --force`), and prints the path. For the
release job and for trying by hand; never registers. A `<dir>` under a
temp directory is refused with the reason (Launch Services would not run
the app). Not compiled in on other platforms.

**`noido --install`** (macOS):

- Builds `~/Applications/noido.app` from the running binary and prints
  what it will do first, in the same one-line-per-step form as on Linux,
  asking `Proceed? [y/N]`.
- A bundle of the same version already there: "already installed",
  nothing to do. Another version: the step says it replaces version X
  with Y.
- `/Applications/noido.app` present (a cask or a dragged copy): says so
  and stops; no second bundle with the same identifier.
- Then ad-hoc signs the bundle and registers it (`lsregister -f`). A
  failed registration is a note; a failed signing is a failure.
- The user is left to pick noido in Finder: the summary says that once,
  with the *Get Info → Open With → Change All…* path, and that noido does
  not touch the default itself.
- `--uninstall` unregisters and removes `~/Applications/noido.app` when
  its `Info.plist` carries noido's identifier, and nothing otherwise.

**The stale-app hint** (macOS): a `noido` started outside a bundle (the
terminal command) that finds `~/Applications/noido.app` with noido's
identifier and another version prints one line to stderr, `noido.app in
~/Applications is version X, this is Y; noido --install updates it`, and
carries on. Nothing when the app is missing, is the same version, or is
someone else's; nothing from inside the bundle.

**Launch modes:**

- A file or link argument: unchanged.
- A `-psn_…` argument, which older Launch Services passed on a Finder
  launch, is dropped before clap sees the arguments.
- No argument and the path noido was started by, unresolved, is inside
  `.app/Contents/MacOS`: launcher mode. Each URL the launch event carries
  becomes one GUI, in order: a `file:` URL as its path, any other URL
  (`noido:`) as its text, started as under "Coming to the front". Then
  this process exits 0. No URL by `applicationDidFinishLaunching` (a
  double-click on the app itself, which sends no open event): exit 0 at
  once, quietly.
- No argument, otherwise: the clap error, as now. A symlink started from
  a terminal keeps the symlink's path, so plain `noido` there is the clap
  error rather than a process waiting for a document no one will send.
- URLs queued by the GUI's handler during a normal run: each goes to the
  `open` hook, exactly as a picked file does today, so an already open
  file hands over and a new one gets a window.

## Not settled by the spike

- An open event that reaches a GUI between its start and its first frame,
  before the handler is installed: AppKit's answer to a delegate without
  the method was not tried. The window is about 100 ms and needs a click
  on a second file while the first noido starts; it goes on the
  hand-testing list, and is fixed only if it shows.
- What Finder itself shows: the probe was in the list Finder's *Open
  With* is built from, not looked at in Finder.

## Order of work

1. **`noido --app-bundle`**: the plist, the layout, the ad-hoc signing.
   The pure part of everything else; testable without a Mac's Finder,
   and it gives the bundle that step 2 is tried with by hand.
2. **The open handler, launcher mode, and coming to the front**
   (`noido-macos` crate, objc2), including the macOS `open` hook and the
   hand-over's activation. This is the part that only a Mac can check;
   it lands with the hand-testing list below.
3. **`noido --install` / `--uninstall` on macOS** around step 1's code,
   and the stale-app hint.

Click-to-open is complete after step 2 for a bundle built by hand, and
after step 3 for anyone with a checkout or the formula.

## Left to the release design

What this design found that the release-job design should start from:

- `noido --app-bundle <dir>` is the seam: the release job builds the app
  with it, then `ditto -c -k --keepParent noido.app noido.app.zip` (what
  a cask points at) and a `.dmg` from a staging directory holding the app
  and a symlink to `/Applications` (`hdiutil create -volname noido
  -srcfolder … -format UDZO -ov`). Both tools ship with macOS. The
  staging directory must not be under a temp directory if the app is
  launched from it in the job.
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
- The formula's `caveats` should name `noido --install`, and say that it
  is run again after an upgrade (the stale-app hint says the same).
- Which warning a quarantined, ad-hoc signed app actually gets on
  macOS 26 is still to be seen, so the README promises the right one.

## Code shape

- `crates/noido-macos/`: new, macOS-only dependencies (`objc2`,
  `objc2-app-kit`, `objc2-foundation`), `unsafe_code = "deny"` with
  narrow `allow`s, and no workspace lints. Entry points:
  `run_launcher(&bundle) -> !`-ish for launch mode;
  `install_open_urls()` plus the URL queue behind a `Mutex` for the GUI;
  `launch_in_bundle(&bundle, arg)` for starting a window in front; and
  `activate_pid(pid)` for the hand-over.
- `crates/frontends/gui-egui/src/app_bundle.rs`: the plist text, the
  layout, the plan/steps/`carry_out` shape borrowed from `desktop.rs`,
  the `--app-bundle`/`--install`/`--uninstall` entry points, the
  stale-app check, the temp-directory refusal, and "is this a Finder
  launch?" as a pure function. Compiled on every platform; only the
  entry points are macOS-only, so CI's Linux leg still tests the plist
  and the plans.
- `gui-egui/src/main.rs`: dropping `-psn_…`, then the launch-mode branch
  before clap's required-file rule (an argument-less, bundled start is
  not an error); the stale-app hint for a start outside a bundle; clap
  gains `--app-bundle <dir>`.
- `gui-egui/src/app.rs`: one `receive_open_files()` step in `show()`,
  beside `receive_renders` and `receive_reload`, feeding the existing
  `open` hook.
- `gui-egui/src/open.rs`: on macOS inside a bundle, `open` starts the
  new window through `launch_in_bundle`.
- `gui-egui/src/instance.rs`: the answer to "activate" carries the
  running process's pid; on macOS the sender activates that pid before
  exiting.
- `gui-egui/src/desktop.rs`: unchanged; `--install` dispatches on
  `cfg!(target_os = "macos")`.
- `justfile`: `app`, building the release binary and running
  `--app-bundle` into `target/app/` (git-ignored, not a temp directory);
  `run` unchanged.

## Testing

- `app_bundle`: the plist parses and carries the UTI declarations, the
  identifier, the version and the URL scheme; the layout of a bundle
  written into a temp dir (binary a regular, executable file, plist where
  Finder expects it); a temp-directory destination refused; plans for
  fresh install, same version, other version, `/Applications` present,
  uninstall with and without ours there; the stale-app check for a
  missing app, same version, other version, someone else's identifier.
- The Finder-launch decision: no argument inside a bundle is a launcher
  run; no argument outside one, or through a symlink, is the clap error;
  an argument is never a launcher run; `-psn_…` is dropped.
- The launcher's URL routing, as a pure function: several `file:` URLs
  give several paths in order; a `noido:` URL gives its text.
- `instance`: the "activate" answer carries a pid.
- Harness (GUI): a path in the queue during a frame reaches the `open`
  hook; two paths do too, in order; an empty queue does nothing.
- By hand on a Mac, and only that, each window checked to be *in front*:
  Finder's *Open With* lists the app; a double-click after *Change All*
  opens the PDF; three PDFs opened at once give three windows; a second
  click on the same PDF brings its window forward, also from a terminal
  `noido <that file>`; a click on a different one while noido runs opens
  a second window; a click on a second file while the first noido is
  still starting; a `noido:` link opens; double-clicking the app itself
  exits quietly; `noido --version` and `noido <file>` from a terminal
  behave; the Dock shows the app's name; the launcher's bounce and the
  first window's flash forward are tolerable; `codesign -dv` says ad hoc;
  after a version bump the terminal `noido` prints the stale-app hint
  and `--install` clears it; `--uninstall` leaves nothing behind.
- State during hand testing: on macOS it is
  `~/Library/Application Support/noido` whatever `XDG_STATE_HOME` says,
  so a hand run touches the real one; use a scratch `HOME` for anything
  that should not.
- Registering a bundle with Launch Services is global to the user's
  session, not to a scratch `HOME`, and a bundle that declares PDFs
  competes with the real ones the moment it is registered. Any bundle
  used for testing gets unregistered (`lsregister -u <path>`) as soon as
  it has been tried, and never shares noido's own bundle identifier
  unless it is the one being tested. Test bundles live outside temp
  directories (see Constraints).

## Sources

- `docs/investigations/2026-10-26-macos-app.md`,
  `2026-10-26-versioning-and-releases.md`,
  `2026-10-26-desktop-install-design.md`,
  `2026-10-31-open-file-dialog-design.md`
- The spike of 2026-11-01: Swift probe apps, thrown away; results under
  Constraints.
- winit 0.30.13: `src/platform_impl/macos/app_state.rs` (`declare_class!`
  for `WinitApplicationDelegate`), `event_loop.rs` (`EventLoop::new`),
  `window_delegate.rs` (`focus_window`)
- eframe 0.37.0: `src/native/run.rs` (`with_event_loop`, `create_event_loop`)
- cargo-packager `crates/packager/src/package/app/mod.rs` (`create_info_plist`,
  `file_associations`), `config/mod.rs`
- Apple: `NSWorkspace.OpenConfiguration` (`activates`,
  `createsNewApplicationInstance`, `arguments`),
  `NSRunningApplication.activate(options:)`,
  `NSWorkspace.urlsForApplications(toOpen:)`
- Homebrew cask `binary` and `app` stanzas, as used by
  `zed` (`Contents/MacOS/cli`) and `visual-studio-code`
  (`Contents/Resources/app/bin/code`)
