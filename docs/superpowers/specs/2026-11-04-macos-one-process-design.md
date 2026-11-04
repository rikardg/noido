# macOS: one process with many windows, like a Mac app

Date: 2026-11-04. Agreed with the user. Issue #32.

## Intent and why

noido runs one process per document. On macOS that departs from what a
Mac app does: each document has its own Dock icon, ⌘Q and the Dock
menu's Quit close one window instead of the app, the Dock menu lists no
windows, and a file handed to "noido" goes to whichever instance Launch
Services picks (the oldest). The user runs noido on both Linux and
macOS daily and wants each to feel native on its own platform.

On macOS, noido becomes one process with a window per document. Linux
keeps one process per document: that is what native document viewers
there do (Evince deliberately went to a process per document, with a
hand-over to the window that has the file, which is what `instance.rs`
does now; Okular and zathura are per process too), and GNOME groups
windows into one app by app ID, not by process, so none of the four
problems above exist there.

## Goals (macOS)

1. **One process, one Dock icon.** Every document opened in any way
   (Finder, the Open panel, Open…, a `noido:` link, a terminal launch)
   opens as a new window of the running process. A document already
   open brings its window forward (and goes to the target, if one was
   given), as today.
2. **Quit quits the app.** ⌘Q and the Dock menu's Quit close every
   window, saving each document's view state as each window does today
   on exit. ⌘W (`CloseWindow`) closes one window.
3. **The app outlives its windows.** When the last window closes, noido
   keeps running with its menu bar. A click on the Dock icon, or
   Open…, then shows the Open panel (as opening noido.app itself does
   now, `b5dd360`).
4. **The system's window list.** The Dock menu and the Window menu list
   noido's windows, and the system's "Move focus to next window"
   shortcut cycles them. That shortcut depends on the layout: ⌘` on a
   US keyboard, ⌘< on Swedish and other ISO ones. noido binds no key
   for it; it must make its windows take part in AppKit's window list
   and must not swallow the shortcut in its keymap.
5. **Windows come back after a quit, following the system setting.**
   System Settings › Desktop & Dock › "Close windows when quitting an
   application". When it is off, quitting and starting noido again
   reopens the documents that were open at quit, each at its saved
   place and window frame. When it is on, noido starts with no windows
   (a launch with a file opens that file only). A start with documents
   to restore restores them in addition to any file the launch named.
6. **A window opens where it last was**, from the window file each
   document already has (`Store::window_file`).

## Non-goals

- Any change to how noido runs on Linux (or Windows): one process per
  document, `instance.rs` per file, as now. Shared code may be
  restructured, but behaviour there stays the same and its tests stay.
- Tabs, or several windows on the same document.
- Restoring anything beyond which documents were open (no restoring of
  window order or which one had focus, beyond what falls out naturally).
- The Open panel as a sheet (#15).
- An error shown inside the app when a file cannot be opened (#5).
- macOS's own window tabs: turned off (`tabbingMode` disallowed); they
  would clash with noido's own title bar.
- A native menu that mirrors noido's actions (see the menu below).

## Anti-goals

- A slow or busy document making the other windows stutter in normal
  use. All windows share one main thread (see Constraints), so heavy
  work stays on the worker threads.
- Losing a document's saved state because another window quit the app.
- Behaviour on macOS that a Mac user would notice as foreign: if a
  choice below turns out to conflict with how Mac apps behave, the Mac
  behaviour wins and the builder says so.

## Constraints

- **eframe 0.37 has one root viewport; closing it ends the process.**
  Documents are therefore secondary viewports, with a hidden root that
  holds the app (and keeps it alive with no documents open).
- **One main thread.** AppKit requires all windows on the main thread;
  winit runs one event loop there and eframe calls every viewport's
  callback from it, in turn. A slow frame in one window stalls all of
  them.
- **Deferred viewports need `Send + Sync` callbacks**
  (`Context::show_viewport_deferred`), even though eframe only calls
  them on the main thread.
- **eframe persists only the root window** (`persist_window`,
  `persistence_path`), so document windows' frames are saved and
  restored by noido itself.
- **winit drops `application:openURLs:`**, which is why
  `noido-macos::open_urls` adds it to winit's delegate, and why the
  launcher (`noido-macos::launch`) exists for opens that arrive before
  the app is up.
- The headless `Harness` in `app.rs` drives one window today; most of
  the GUI behaviour is tested there and must stay so.

## Approaches

**Process model on macOS.**

- *One process, documents as eframe deferred viewports, each
  document's state behind a lock.* **Chosen.** A deferred viewport
  repaints only when it needs to; the lock (`Arc<Mutex<…>>` around
  today's per-document state) is a one-time cost in the types and is
  never contended, since every callback runs on the main thread.
- *Immediate viewports.* No lock, but they draw inside the root's
  frame: a repaint of one repaints all, so with ten documents every
  page render redraws ten windows. Rejected.
- *Keep one process per document and fake the Mac behaviour* (forward
  ⌘Q to every process, a shared Dock menu). Cannot give one Dock icon
  or the system's window list. Rejected.

**Linux.** One process everywhere was considered (one code path) and
rejected: on Linux the per-document process already is the native
behaviour, and the user runs both platforms daily.

**How a second launch reaches the running app on macOS.**

- *One socket for the whole app.* **Chosen.** `instance.rs` with one
  address per app (per user) instead of per file. It already carries
  the target and answers with the pid to bring forward, and it works
  for the bundle and for a bare `target/release/noido` (`just run`)
  alike. Finder's opens keep coming as Apple events; the two meet in
  the same "open this" path in the app.
- *Launch Services only* (a terminal launch hands the file to
  noido.app). One way in, but a bare build would open the installed
  app instead of itself, and a page target would need the `noido:` URL
  for every launch. Rejected.

## The design, as decided

1. **Per-document state becomes a window.** Today's `NoidoApp` is one
   document. It splits into the app (one per process: settings, theme,
   incoming opens, the list of windows, the hand-over listener) and a
   document window (everything else that `NoidoApp` holds now: view,
   renderer, threads, watcher, persistence, activation, its viewport
   id). On macOS the app shows each document window as a deferred
   viewport; on Linux the app has exactly one document window and runs
   it in the root viewport as now.
2. **Threads stay per document.** Each document window keeps its own
   renderer (foreground and background), thumbnailer, searcher, linker
   and watcher. Each waker wakes its own window
   (`request_repaint_of(viewport id)`), not the root.
3. **Shared across windows: settings, theme, incoming opens.** One
   settings file and one theme watcher per app. A settings change made
   in one window shows in all of them.
4. **Opening.** On macOS, `open::open` (Open…, the Open panel's pick)
   and Finder's opens (`Incoming`) open a window in this process
   instead of starting one. A path already open brings its window
   forward instead.
5. **Hand-over.** On macOS, `instance.rs` claims one address per
   executable (derived from its canonical path), so a `just run` build
   and the installed noido.app are separate apps that can run side by
   side. A second launch with a file sends it and its target, and the
   running app opens it (or brings its window forward and goes to the
   target). The running app answers only after it has tried the file:
   the launch prints the error, or that the file was opened in (or
   brought forward by) the running noido, and exits non-zero on an
   error. On Linux, `instance.rs` keeps its per-file address and its
   answer as now.
6. **Terminal launches detach by default.** On macOS, `noido a.pdf`
   from a terminal hands the file to the running app; with none
   running, it starts the app in the background (detached from the
   terminal) and hands over to it the same way, waits for the answer,
   and returns. `--foreground` makes the launch the app itself, tied to
   the terminal: Ctrl-C quits it with every window saved, as
   `interrupt.rs` does now for one window. If that executable's app is
   already running, `--foreground` hands over like any launch. `just
   run` passes `--foreground`. On Linux the flag is accepted and changes
   nothing (each launch already is its own process in the terminal).
7. **A minimal native menu bar on macOS.** noido (About, Settings…,
   Quit), File (Open…, Close Window), and Window (filled in by AppKit).
   The in-window egui menu stays as it is. The keymap keeps handling
   every shortcut; the native items show their key equivalents (⌘Q,
   ⌘W, ⌘O, ⌘,) but a key press is not handled twice. With no windows
   open the native menu is the only one, and its items work.
8. **Quit and close.** `Action::Quit` quits the app on macOS (every
   window saves as it closes); `Action::CloseWindow` closes the focused
   document window. On Linux both keep closing the one window, as now.
   The app quitting through the Dock menu or the system takes the same
   path as ⌘Q.
9. **No windows left.** On macOS the app keeps running with the root
   hidden. A Dock click with no windows open (`applicationShouldHandle
   Reopen`, which winit may not forward; see below) shows the Open
   panel.
10. **Restore.** On quit, noido records which documents are open (in the
   state store); on start it reads the system setting and reopens them
   when the setting says windows are kept.

## Left to the builder

- Why winit's windows do not appear in AppKit's window list, and the
  fix (it may be a window property winit sets, or the hidden root
  getting in the way). This decides goals 4 and part of 2: check it
  first, before the restructure.
- The exact defaults key for "Close windows when quitting an
  application" (believed to be `NSQuitAlwaysKeepsWindows` in the global
  domain; verify) and where the open-documents list lives in
  `noido-state`.
- Whether the launcher in `noido-macos` is still needed. It exists for
  opens that arrive before winit's delegate is up; if the real app can
  catch those itself, the launcher can go. If not, it stays and starts
  the one process (or hands over to the running one through the
  socket).
- Handling `applicationShouldHandleReopen:` and the Dock menu's Quit if
  winit does not, in the style of `open_urls.rs`.
- How the native menu items reach the app's actions, and how a key
  equivalent is kept from being handled both by AppKit and the keymap.
- How a detached launch starts the app (re-exec of the executable with
  `--foreground` and its output dropped, or `launch_in_bundle` inside
  the bundle) and how long it waits for the new app's socket.
- Whether ⌥⌘Q ("Quit and Keep Windows") is offered; if it is, it
  restores on the next start whatever the system setting says.
- How the `Harness` grows to cover several windows (an app-level
  harness driving several document windows), keeping today's
  single-window tests working.
- How document window frames are saved: noido's own code, in the
  existing per-document window file.
- Measuring the shared main thread: several large documents open,
  rendering and scrolling one, and checking that the others do not
  stutter.
- The order of the work (the plan).
