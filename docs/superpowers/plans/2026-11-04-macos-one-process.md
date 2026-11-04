# macOS: one process with many windows — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On macOS, noido runs as one process with a window per document; Linux keeps one process per document.

**Architecture:** Today's `NoidoApp` stays the per-document window. A new macOS-only `Shell` is the eframe app: a hidden root viewport that owns the shared parts (settings, theme, incoming opens, the app-wide hand-over socket, the window list) and shows every document as a deferred viewport around an `Arc<Mutex<NoidoApp>>`. Windows talk to the shell through a channel of `Request`s. Linux runs exactly as now, one `NoidoApp` in the root viewport.

**Tech Stack:** Rust 2024, eframe/egui 0.37 (deferred viewports), winit, objc2 / objc2-app-kit (`noido-macos`), interprocess (local sockets).

**Spec:** `docs/superpowers/specs/2026-11-04-macos-one-process-design.md` (issue #32)

## Global Constraints

- Linux (and Windows) behaviour does not change: one process per document, `instance.rs` per file, its tests kept as they are.
- Each document keeps its own worker threads; each waker wakes its own viewport (`request_repaint_of`), never the root.
- Deferred viewports only, never immediate ones.
- One hand-over address per executable on macOS (from its canonical path), so a `just run` build and the installed noido.app are separate apps.
- A hand-over is answered only after the app has tried the file; the launch prints the error or what happened and exits non-zero on an error.
- Terminal launches on macOS detach by default; `--foreground` makes the launch the app, and Ctrl-C quits it with every window saved. `just run` passes `--foreground`. On Linux the flag is accepted and does nothing.
- Native menu on macOS: noido (About, Settings…, Quit ⌘Q), File (Open… ⌘O, Close Window ⌘W), Window (AppKit's own). The in-window egui menu stays. A key press is handled once.
- macOS window tabbing off. No key bound for window cycling; the system's shortcut (⌘` / ⌘<) must work.
- Restore on start follows "Close windows when quitting an application".
- In-app error display is out (#5); Finder and Open-panel failures go to stderr only.
- Commits: jj, `prefix: what changed`, prefixes `egui`, `macos`, `state`, `Plan`; TDD, failing test first, watched failing. `just check` clean after every task.

## Review Focus

1. The same file reached by two paths (a symlink, a relative path from a terminal): one window, brought forward. Test in Task 4.
2. Quitting while a window's state changed less than `SAVE_DELAY` ago: the change is saved. Test in Task 3.
3. A hand-over of a file that won't load (missing, not a PDF): no window, an error answer, and the app keeps running. Test in Task 6.
4. Closing the last window: the app keeps running, and a later quit records no documents to restore. Test in Task 6.
5. A stale app socket left by a crashed noido: the next launch takes it over instead of failing to reach it. Test in Task 5.

---

### Task 1: Spike: hidden root and deferred windows on macOS (throwaway)

Settles the spec's first builder item before the restructure. No tests, nothing kept but the findings.

**Files:**
- Create (not committed): `crates/frontends/gui-egui/examples/viewports_spike.rs`
- Record findings in: `.superpowers/sdd/2026-11-04-macos-one-process/progress.md`

- [ ] **Step 1:** Write an eframe example: root viewport `with_visible(false)`, two deferred viewports each drawing a label, a key that opens a third, a key that closes one. A root `logic` counter printed to stderr.
- [ ] **Step 2:** Run it (`cargo run --example viewports_spike`) and record, for each of these questions, yes/no and what was needed:
  - Does the hidden root's `logic`/`ui` keep running when only a deferred window repaints, and when a worker thread calls `request_repaint_of(root)`? (If not: shell logic runs from inside each deferred callback through the shared lock, and the plan's Task 6 does that.)
  - Do the windows appear in the Dock menu and in a Window menu after `NSApp.setWindowsMenu`? If not, why (`excludedFromWindowsMenu`, no title, the hidden root)? What fixes it?
  - Does the system's "Move focus to next window" shortcut cycle them, with egui focused?
  - Does closing a deferred window by its red button reach the callback as `close_requested()`?
  - Does `applicationShouldHandleReopen:` reach anything, and does the Dock menu's Quit end the process without `on_exit`?
- [ ] **Step 3:** Delete the example. Nothing to commit; Tasks 6 and 9 build on the findings, and their by-hand checks are adjusted to them.

### Task 2: A document window from a path, with wakers for its viewport

Moves the per-document construction out of `main.rs::run` so the shell can open documents, and points every waker at a viewport.

**Files:**
- Create: `crates/frontends/gui-egui/src/document.rs`
- Modify: `crates/frontends/gui-egui/src/main.rs` (`run` uses it), `crates/frontends/gui-egui/src/watch.rs` (its `Waker`)

**Interfaces:**
- Produces:
  - `pub fn waker(ctx: &egui::Context, viewport: egui::ViewportId) -> Box<dyn Fn() + Send>`
  - `pub struct Shared { pub settings_file: Option<SettingsFile>, pub settings: Settings, pub store: Option<Store> }` with `pub fn load() -> Shared` (today's settings and store loading, `prune` included)
  - `pub struct Opened { pub app: NoidoApp, pub name: String, pub canonical: PathBuf, pub slot: Option<Slot> }`
  - `pub fn open(path: &Path, target: Option<Target>, ctx: &egui::Context, viewport: egui::ViewportId, shared: &Shared) -> Result<Opened, String>`: loads, starts the threads and watcher with `waker(ctx, viewport)`, and builds the app as `run` does now, minus the theme, incoming opens, activation and eframe options.

- [ ] **Step 1: Failing test** `document::tests::a_waker_repaints_its_own_viewport`: a context, `let id = ViewportId::from_hash_of("doc")`, run one empty pass, call `waker(&ctx, id)()`, assert `ctx.has_requested_repaint_for(&id)`.
- [ ] **Step 2:** `just test document` fails (no module).
- [ ] **Step 3:** Implement `waker`, `Shared`, `Opened`, `open`. `run` calls `document::open(…, ViewportId::ROOT, &shared)`, then adds the theme, activation, incoming opens and options as now. `watch::Waker` is built by `waker` too.
- [ ] **Step 4:** `just check` passes; `just run fixtures/<any>.pdf` opens as before on Linux and macOS.
- [ ] **Step 5:** `jj commit -m "egui: build a document window from a path, its wakers aimed at its viewport"`

### Task 3: The document window can live behind a lock and talk to a shell

**Files:**
- Create: `crates/frontends/gui-egui/src/shell.rs` (only `Request` in this task)
- Modify: `crates/frontends/gui-egui/src/app.rs`, `crates/frontends/gui-egui/src/open.rs`

**Interfaces:**
- Produces:
  - `pub enum Request { Open { path: PathBuf, target: Option<Target> }, OpenPanel, Quit, SettingsSaved(Settings) }` in `shell.rs`. Task 5 adds `reply: Option<instance::Reply>` to `Open`.
  - `NoidoApp: Send` (`open::Pick` becomes `Box<dyn Fn(…) + Send>`; `Incoming` moves to the shell in Task 6 and stays on `NoidoApp` for Linux only if it is `Send`, else behind `#[cfg(not(target_os = "macos"))]`).
  - `pub fn with_requests(self, requests: mpsc::Sender<shell::Request>) -> Self`: with it, `Action::Quit` sends `Request::Quit`; Open…'s pick sends `Request::Open`; a saved settings page sends `Request::SettingsSaved`. Without it, everything as now.
  - `pub fn save_now(&mut self)`: what `on_exit` does now; `on_exit` calls it.
  - `pub fn set_settings(&mut self, settings: Settings)`: takes settings saved in another window.
  - `pub fn activation(&self) -> Option<Activation>` (a clone).

- [ ] **Step 1: Failing tests** in `app.rs`:
  - `the_app_is_send`: `fn send<T: Send>() {}; send::<NoidoApp>();`
  - `with_requests_quit_asks_the_shell`: Harness built with `with_requests(tx)`; press Mod+Q; `rx.try_recv() == Ok(Request::Quit)`; `h.commands` holds no `ViewportCommand::Close`.
  - `without_requests_quit_closes_the_window`: the existing behaviour, kept (the test at app.rs:3931 already covers it; leave it).
  - `close_window_with_requests_still_closes_its_window`: Mod+W sends `ViewportCommand::Close`, nothing on `rx`.
  - `save_now_saves_a_fresh_change` (Review Focus 2): Harness with persistence in a temp `Store`, scroll a page, call `save_now` at once (no `SAVE_DELAY` wait), and `Store::load` returns the new place.
  - `set_settings_replaces_the_window_settings`: `set_settings(s)` then `Action::OpenSettings` shows `s` (assert on `app.settings`).
- [ ] **Step 2:** Run them; they fail to compile.
- [ ] **Step 3:** Implement. `Request` derives `Debug` and `PartialEq`.
- [ ] **Step 4:** `just check` passes.
- [ ] **Step 5:** `jj commit -m "egui: a document window can be sent between threads and ask a shell to open and quit"`

### Task 4: The window list

**Files:**
- Create: `crates/frontends/gui-egui/src/windows.rs`

**Interfaces:**
- Produces: `pub struct Windows<W>`, in open order, with
  - `pub fn id_for(canonical: &Path) -> egui::ViewportId` (`ViewportId::from_hash_of(canonical)`)
  - `pub fn find(&self, canonical: &Path) -> Option<egui::ViewportId>`
  - `pub fn insert(&mut self, canonical: PathBuf, window: W) -> egui::ViewportId`
  - `pub fn remove(&mut self, id: egui::ViewportId) -> Option<W>`
  - `pub fn iter(&self) -> impl Iterator<Item = (egui::ViewportId, &W)>`
  - `pub fn paths(&self) -> Vec<PathBuf>` (open order), `pub fn is_empty(&self) -> bool`
  - `pub fn canonical(path: &Path) -> PathBuf` (`fs::canonicalize`, falling back to `path::absolute`)

- [ ] **Step 1: Failing tests:** `insert_then_find`, `a_second_path_is_another_window`, `remove_forgets_the_path`, `paths_keep_the_open_order`, and `a_symlink_finds_the_same_window` (Review Focus 1): in a temp dir, a file and a symlink to it, and a relative path to it; `find(&canonical(link))` and `find(&canonical(relative))` return the id inserted for `canonical(file)`.
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `just test windows` passes.
- [ ] **Step 5:** `jj commit -m "egui: a list of open document windows, one per file"`

### Task 5: One hand-over socket per executable

**Files:**
- Modify: `crates/frontends/gui-egui/src/instance.rs` (new items next to the per-file ones, which stay)

**Interfaces:**
- Produces:
  - `pub struct OpenRequest { pub path: PathBuf, pub target: Option<Target> }`
  - `pub enum Answer { Opened { pid: Option<u32> }, Forward { pid: Option<u32> }, Failed(String) }`
  - `pub enum AppClaim { Running(Option<Answer>), First(AppListener), Unavailable }` (`Running(None)`: it answered nothing in time)
  - `pub fn claim_app(exe: &Path, request: Option<&OpenRequest>) -> AppClaim` (no request: only checks, and answers `Running(None)` if one runs)
  - `pub fn send(exe: &Path, request: &OpenRequest) -> Option<Answer>`: never binds; `None` when no app listens
  - `pub struct AppListener`, `pub fn serve(self, waker: Waker) -> AppRequests`; `AppRequests::take(&self) -> Vec<(OpenRequest, Reply)>`; `Reply::send(self, outcome: Answer)` (the pid filled in by `send`'s side from the listener's process)
  - Wire format, one line each way: request `open <fragment or -> <path>` (path is the rest of the line), answer `opened <pid>`, `forward <pid>`, `failed <message>`. `MAX_APP_REQUEST = 4096` bytes. Connect timeout `TIMEOUT` as now; `ANSWER_TIMEOUT = 30 s` for the answer (a large file loads first).
  - Address: `path_hash(canonical exe)` with a `noido-app-` prefix, in the same directory as the per-file sockets.
- `Request::Open` (Task 3) gains `reply: Option<instance::Reply>`; the Task 3 tests compare it by `is_some`.

- [ ] **Step 1: Failing tests** (in the style of the per-file ones): `app_second_claim_sends_the_file` (served on a thread that replies `Opened`; the claim gets `Running(Some(Opened { pid: Some(_) }))` and the served side received the path and target), `app_answer_carries_a_failure`, `a_path_with_spaces_arrives_whole`, `another_executable_is_another_app`, `send_without_an_app_is_none`, `stale_app_socket_is_taken_over` (Review Focus 5), `an_overlong_app_request_is_ignored`.
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement, reusing `make_room`, `to_name`, `answer`'s bounded read.
- [ ] **Step 4:** `just test instance` passes, the per-file tests included.
- [ ] **Step 5:** `jj commit -m "egui: one hand-over socket per executable, answered once the file was tried"`

### Task 6: The macOS shell

**Files:**
- Modify: `crates/frontends/gui-egui/src/shell.rs`, `crates/frontends/gui-egui/src/main.rs`, `crates/frontends/gui-egui/src/interrupt.rs`

**Interfaces:**
- Consumes: `document::{open, Shared, Opened}`, `Windows`, `instance::{AppListener, AppRequests, Reply, Answer}`, `NoidoApp::{with_requests, save_now, set_settings, activation}`, `app::Incoming`.
- Produces:
  - `pub type OpenDoc = Box<dyn FnMut(&Path, Option<Target>, egui::ViewportId) -> Result<NoidoApp, String>>` (production: `document::open` with the shell's `Shared`; tests: a fake engine app)
  - `pub struct Shell` with `pub fn new(ctx: &egui::Context, open_doc: OpenDoc, requests: (Sender<Request>, Receiver<Request>)) -> Shell`, `pub fn handle(&mut self, ctx: &egui::Context, request: Request)`, `pub fn open_paths(&self) -> Vec<PathBuf>`, and `impl eframe::App for Shell`.
  - Behaviour of `handle`:
    - `Open` of a path already open: its `Activation::request(target)`, `Answer::Forward`.
    - `Open` of a new path: `open_doc`; on `Ok` insert and `Answer::Opened`; on `Err(e)` `Answer::Failed(e)`, nothing inserted, and with no reply `eprintln!`.
    - `Quit`: `save_now` on every window, record `open_paths()` (Task 10 stores it), then `ViewportCommand::Close` to the root.
    - `SettingsSaved(s)`: `set_settings(s.clone())` on every window.
    - `OpenPanel`: `open::pick_pdf(None, None, …)` whose pick sends `Request::Open`.
  - Each frame: drain `Incoming` (Finder) and `AppRequests` into `handle`; show each window with `show_viewport_deferred(id, builder, …)`, the callback locking its `NoidoApp` and running `logic_pass` and `show(ui, None)`. A window whose viewport reports `close_requested()` gets `save_now` and is removed; the shell keeps running with none left. The theme's `Live` is polled by the shell, not by windows. Where Task 1 found the hidden root doesn't run its pass, the drain runs at the top of every window's callback instead (through `Arc<Mutex<Shell state>>`).
  - Viewport builder per window: today's `own_title_bar(…)` builder, with the title `"{name} — noido"`.
  - `interrupt::close_on_interrupt` gets a variant `quit_on_interrupt(tx: Sender<Request>, ctx)` sending `Request::Quit` (second Ctrl-C exits as now).
- [ ] **Step 1: Failing tests** in `shell.rs`, driving `handle` with a fake `OpenDoc` (Harness-style `NoidoApp::new` on `BlankEngine`; files from `fixtures/`):
  - `opening_a_new_file_adds_a_window_and_answers_opened`
  - `opening_an_open_file_brings_it_forward` (its activation holds the target; answer `Forward`; still one window)
  - `a_file_that_fails_answers_failed_and_opens_nothing` (Review Focus 3: `OpenDoc` returning `Err`; no window; the shell takes the next `Open` fine)
  - `closing_the_last_window_keeps_the_shell` and `quit_after_the_last_window_records_nothing` (Review Focus 4)
  - `quit_saves_every_window_and_closes_the_root`
  - `saved_settings_reach_every_window`
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement; `main.rs` on macOS runs `Shell` (root `with_visible(false)`) with the file from the command line passed in as the first `Open`. Detaching and `--foreground` come in Task 7; in this task every macOS launch is a foreground app.
- [ ] **Step 4:** `just check`; by hand on macOS: `target/release/noido a.pdf`, Open… a second file: one Dock icon, two windows; ⌘W closes one; ⌘Q quits and both reopen at their page next time they are opened.
- [ ] **Step 5:** `jj commit -m "egui: on macOS, one process shows every document in a window of its own"`

### Task 7: Terminal launches: detach by default, `--foreground`

**Files:**
- Modify: `crates/frontends/gui-egui/src/main.rs`, `justfile`

**Interfaces:**
- Consumes: `instance::{claim_app, send, AppClaim, Answer}`, `noido_macos::launch_in_bundle`, `app_bundle::bundle_of`.
- Produces: `--foreground` (`Args::foreground: bool`); on macOS the file is optional with it (an app with no windows). `fn answered(path: &Path, target: Option<&Target>, answer: &Answer) -> String` replacing `handed_over` on macOS: `noido: opened {path}{at} in the running noido`, `noido: {path} is already open; brought its window forward{at}`, and for `Failed(m)` the message `m` (exit failure). `const START_WAIT: Duration = 5 s`, polled every 50 ms.

- [ ] **Step 1: Failing tests** in `main.rs`: `foreground_takes_a_file_or_none_on_macos` (`#[cfg(target_os = "macos")]`), `foreground_is_accepted_on_linux` (`#[cfg(not(target_os = "macos"))]`, with a file), `answer_messages_name_the_file_and_target` (the three answers, a page target).
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement the macOS launch: `claim_app`; `Running(Some(a))` prints `answered` and exits with its status; `Running(None)` reports no answer; `First(listener)` with `--foreground` runs the shell; `First` without it drops the listener, starts the app detached (inside the bundle `launch_in_bundle(&app, ["--foreground"])`, else `Command::new(exe).arg("--foreground")` with null stdio and `process_group(0)`), polls `send` until it answers or `START_WAIT` passes, and prints the answer. `just run` passes `--foreground`.
- [ ] **Step 4:** `just check`; by hand: `noido a.pdf` returns to the prompt with the opened message; a second `noido b.pdf` opens in the same app; `noido missing.pdf` prints the error and exits 1; `just run a.pdf` stays in the terminal and Ctrl-C quits with state saved.
- [ ] **Step 5:** `jj commit -m "egui: a terminal launch on macOS hands over to the app and returns"`

### Task 8: The launcher starts one app

**Files:**
- Modify: `crates/noido-macos/src/launch.rs`, `crates/noido-macos/src/lib.rs`, `crates/frontends/gui-egui/src/main.rs`

**Interfaces:**
- Produces: `run_launcher(app: &Path, hand_over: impl Fn(&OsString) -> bool + 'static) -> !` where `hand_over` sends one URL to the running app and reports whether it was taken (gui-egui passes a closure over `instance::send`). `Launches` changes from "one launch per URL" to "start the app at most once, then hand every URL over".

- [ ] **Step 1: Failing tests** on `Launches`: `urls_wait_for_the_app_to_start_once`, `a_second_url_does_not_start_another_app`, `the_launcher_exits_once_every_url_was_handed_over`, `a_url_never_taken_is_given_up_after_grace` (existing timing tests adjusted to the new meaning; the old per-URL ones that no longer hold are removed).
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement. Whether the launcher is needed at all is Task 1's finding about cold-start `openURLs`; if the shell receives them itself, this task instead removes the launcher and `is_finder_launch` leads to the shell.
- [ ] **Step 4:** `just check`; `just app`, then by hand: with noido not running, double-click two PDFs in Finder: one Dock icon, two windows; opening noido.app itself shows the Open panel.
- [ ] **Step 5:** `jj commit -m "macos: the launcher starts one noido and hands it every document"`

### Task 9: Native menu, Dock Quit, reopen, no tabs

**Files:**
- Create: `crates/noido-macos/src/menu.rs`
- Modify: `crates/noido-macos/src/lib.rs`, `crates/frontends/gui-egui/src/shell.rs`

**Interfaces:**
- Produces:
  - `pub enum MenuCommand { About, Settings, Open, CloseWindow, Quit, Reopen }`
  - `pub fn menu_items() -> Vec<(&'static str, &'static str, Option<MenuCommand>)>` (menu, title, command; `None` for separators): noido: About noido, Settings… ⌘,, Quit noido ⌘Q; File: Open… ⌘O, Close Window ⌘W; Window: AppKit's.
  - `pub fn install(wake: impl Fn() + Send + Sync + 'static)`: builds the menu bar, sets `NSApp.windowsMenu`, `NSWindow.setAllowsAutomaticWindowTabbing(false)`, adds `applicationShouldHandleReopen:hasVisibleWindows:` (→ `Reopen` when no windows) and `applicationShouldTerminate:` (→ `Quit`, answering `NSTerminateCancel`) to winit's delegate in the style of `open_urls.rs`, plus whatever Task 1 found makes the windows join the window list.
  - `pub fn take_commands() -> Vec<MenuCommand>`
  - The shell maps: Open/Reopen → `Request::OpenPanel`, Quit → `Request::Quit`, CloseWindow → `ViewportCommand::Close` to the focused window, Settings → `Action::OpenSettings` on the focused window (or the Open panel's absence: nothing with no windows), About → the focused window's info page, nothing with none.
  - The menu's key equivalents take the key press before winit sees it, so the keymap gets ⌘Q/⌘W/⌘O/⌘, only on Linux: no press handled twice.
- [ ] **Step 1: Failing test** `menu_items_are_the_minimal_menu` asserting the table above exactly (titles, key equivalents, commands).
- [ ] **Step 2:** It fails.
- [ ] **Step 3:** Implement and wire into `Shell` (`take_commands` drained each frame like `Incoming`).
- [ ] **Step 4:** `just check`; `just app`; by hand: the menu bar has the three menus; Window lists every document; the Dock menu lists them; ⌘` / ⌘< cycles them; Quit from the Dock saves and quits; with no windows, a Dock click and ⌘O show the Open panel; "Window › Merge All Windows" is absent.
- [ ] **Step 5:** `jj commit -m "macos: a native menu bar, the Dock's Quit and reopen, no window tabs"`

### Task 10: Restore documents, following the system setting

**Files:**
- Modify: `crates/noido-state/src/lib.rs`, `crates/noido-state/tests/store.rs`, `crates/noido-macos/src/lib.rs`, `crates/frontends/gui-egui/src/shell.rs`

**Interfaces:**
- Produces:
  - `Store::save_open(&self, paths: &[PathBuf]) -> io::Result<()>` and `Store::load_open(&self) -> Vec<PathBuf>` (`open.toml` next to the docs directory; missing or unreadable → empty)
  - `noido_macos::keeps_windows() -> bool`: `NSUserDefaults.standardUserDefaults.boolForKey("NSQuitAlwaysKeepsWindows")` (verify the key by toggling the setting and reading `defaults read -g NSQuitAlwaysKeepsWindows`; record in the ledger)
  - `shell::to_open(restored: Vec<PathBuf>, keeps: bool, named: Option<&Path>) -> Vec<PathBuf>`: restored first when `keeps`, then `named` unless already among them (by `windows::canonical`).
- [ ] **Step 1: Failing tests:** in `store.rs` `open_documents_round_trip` and `no_open_list_is_empty`; in `shell.rs` `restore_follows_the_setting`, `the_named_file_is_added_once`, `a_restored_file_that_is_gone_is_skipped` (its `Open` fails quietly).
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement; `Quit` saves `open_paths()`, the shell's start opens `to_open(…)`.
- [ ] **Step 4:** `just check`; by hand with the setting off: two documents, ⌘Q, start noido.app: both back; with it on: none.
- [ ] **Step 5:** `jj commit -m "egui: on macOS, quitting and starting again reopens the documents"` (the store part first, as `state: remember which documents were open`)

### Task 11: Window frames for document windows

**Files:**
- Modify: `crates/noido-state/src/lib.rs`, `crates/noido-state/tests/store.rs`, `crates/frontends/gui-egui/src/shell.rs`

**Interfaces:**
- Produces: `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)] pub struct Frame { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }`; `Store::save_frame(&self, slot: &Slot, frame: Frame) -> io::Result<()>`; `Store::load_frame(&self, slot: &Slot) -> Option<Frame>` (its own `<hex>.frame.toml`; with none, the newest frame's size, no position, as `window_file` falls back); `prune` removes frame files with their documents.
- The shell saves a window's `outer_rect` on close and quit, and builds each window with `with_position`/`with_inner_size` from `load_frame`, else today's default size.
- [ ] **Step 1: Failing tests:** `frame_round_trips`, `a_new_document_takes_the_newest_size_only`, `prune_removes_frames`.
- [ ] **Step 2:** They fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `just check`; by hand: move and resize a window, quit, reopen that file: same place and size.
- [ ] **Step 5:** `jj commit -m "state: keep a document window's frame"` and `jj commit -m "egui: on macOS, a document window opens where it last was"`

### Task 12: Measure the shared main thread

- [ ] **Step 1:** `just app`; open five large files from `../testfiles/` (a scanned book, long papers). Scroll one fast and zoom it; watch the others repaint (an animation in one: the sidebar glide) for stutter.
- [ ] **Step 2:** Record in the ledger what stalls, if anything, with `RUST_LOG`/egui's frame time. A stall gets a fix test-first (work moved to a worker thread) or an issue if it is outside this change.

### Final

- [ ] Whole-branch review by a fresh subagent on the most capable model; Critical/Important fixed test-first as fixups, squashed; minors presented in chat.
- [ ] `just app` / `just build`; tell the user what to check by hand (Tasks 6–11's lists); close #32 with `Fixes #32` in the last commit.
