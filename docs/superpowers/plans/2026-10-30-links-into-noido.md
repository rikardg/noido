# Links into noido Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open noido on a page from a `file:`/`noido:` URI with an RFC 8118 fragment or `--page`, hand the page over to an already open window, and copy links to the current page.

**Architecture:** A pure module in `noido-core` parses arguments and builds links (on the `url` crate). The engine gains a named-destination lookup that runs on the existing links thread. The single-instance socket carries the destination. The app applies an "arrival" (from the command line or a hand-over) at the start of a frame, like a sidebar jump.

**Tech Stack:** Rust 2024, eframe/egui 0.37, clap 4, `url` 2.5 and `percent-encoding` 2.3 (both already in `Cargo.lock`), interprocess, jj.

**Spec:** `docs/superpowers/specs/2026-10-30-links-into-noido-design.md`. Background: `docs/investigations/2026-10-30-links-into-noido.md`.

## Global Constraints

- TDD: the failing test first, watched failing.
- `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean after every task. Run `cargo fmt` before each commit.
- Commit with `jj commit -m "Area: what changed, in plain words"`. Never move the `main` bookmark.
- Pages are 0-based inside noido and 1-based in links, `--page` and the UI.
- Only `page=` and `nameddest=` are understood. Everything else in a fragment is ignored, never an error.
- Only an argument starting `file://` or `noido://` (scheme ASCII case-insensitive) is a URI. Anything else is a plain path, taken as is.
- The copied link is `noido:` + the `file:` URI of the canonical path + `#page=N`. The Markdown form is `[<name without .pdf>, p. N](<link>)`.
- No visible notice on copy (#4), no error window (#5), no named-place copying (#3).
- Never launch the user's desktop apps from tests. Never send synthetic input.

## Review Focus

1. A file name with `#`, `%`, `?`, spaces, `(`, `)` or non-ASCII must survive copy → parse unchanged (Task 2 round-trip test).
2. A link page past the end goes to the last page; `page=0`/`page=x` are ignored, not errors (Tasks 1 and 5).
3. Several hand-overs before a frame: the last destination wins; a hand-over with no page only raises the window (Tasks 4 and 5).
4. An unknown `nameddest` leaves the view where it was and adds no history entry; a named answer that arrives after a newer arrival or a reload is dropped (Tasks 3 and 5).
5. A plain path containing `#` or starting `noido:` without `//` is opened as that file (Task 1).

---

### Task 1: Parsing what noido is asked to open

**Files:**
- Create: `crates/noido-core/src/target.rs`
- Modify: `crates/noido-core/src/lib.rs` (add `mod target; pub use target::*;`)
- Modify: `Cargo.toml` (`[workspace.dependencies]`: `url = "2.5"`, `percent-encoding = "2.3"`), `crates/noido-core/Cargo.toml` (both, `.workspace = true`)

**Interfaces:**
- Produces:
  - `pub enum Target { Page(usize) /* 0-based */, Named(String) }` (`Clone, Debug, PartialEq, Eq`)
  - `pub struct Opening { pub path: PathBuf, pub target: Option<Target> }` (`Debug, PartialEq`)
  - `pub fn parse_argument(arg: &OsStr) -> Result<Opening, String>`
  - `pub fn parse_fragment(fragment: &str) -> Option<Target>`
  - `pub fn to_fragment(target: &Target) -> String` (`"page=52"` for `Page(51)`; `"nameddest=" +` percent-encoded name)

- [ ] **Step 1: Write the failing tests** in `target.rs`'s `mod tests`:
  - `plain_path_is_taken_as_is`: `parse_argument("dir/a#page=3.pdf")` → `Opening { path: "dir/a#page=3.pdf", target: None }`; same for `"noido:x.pdf"` and `"file:/abs.pdf"` (one slash).
  - `file_uri_with_page`: `"file:///home/u/my%20doc.pdf#page=3"` → path `/home/u/my doc.pdf`, `Some(Page(2))`.
  - `noido_uri_is_the_same`: `"noido:///home/u/x.pdf#page=3"` and `"NOIDO:///home/u/x.pdf#page=3"` equal the `file:` result.
  - `localhost_is_local`: `"file://localhost/home/u/x.pdf"` → `/home/u/x.pdf`.
  - `other_host_is_an_error`: `"file://server/x.pdf"` → `Err` containing `"not a local file"`.
  - `fragment_rules`: `parse_fragment("page=3")` → `Page(2)`; `"nameddest=sec%202"` → `Named("sec 2")`; `"page=3&nameddest=a"` → `Named("a")`; `"nameddest=a#page=4"` → `Page(3)`; `"zoom=200&page=2"` → `Page(1)`; `"page=0"`, `"page=x"`, `""`, `"zoom=50"` → `None`; `"page=3&page=0"` → `Page(2)` (malformed is skipped, not a reset).
  - `fragment_round_trip`: `parse_fragment(&to_fragment(&t)) == Some(t)` for `Page(0)`, `Page(51)`, `Named("a b&c#d%")`.

- [ ] **Step 2: Run** `cargo test -p noido-core target` → FAIL (module missing).

- [ ] **Step 3: Implement.** For a URI: replace a leading `noido:` with `file:`, then `url::Url::parse` and `Url::to_file_path` (it handles `localhost`, percent-decoding, Windows drive letters). Its `Err(())` becomes `"<arg>: not a local file"`. Split the fragment on `&` and `#`, then on the first `=`. Values are percent-decoded with `percent_encoding::percent_decode_str(..).decode_utf8_lossy()`. A parameter that parses replaces the slot; others are skipped. Encode `nameddest` values with `utf8_percent_encode` and a set that covers controls, space, `#`, `&`, `%`, `=`.

- [ ] **Step 4: Run** `cargo test -p noido-core target` → PASS. Run clippy.

- [ ] **Step 5: Commit** `jj commit -m "Links: read file: and noido: URIs with #page= and #nameddest="`

### Task 2: Building the links to copy

**Files:**
- Modify: `crates/noido-core/src/target.rs`

**Interfaces:**
- Consumes: Task 1's `parse_argument`, `Target`.
- Produces:
  - `pub fn page_link(canonical: &Path, page: usize) -> Option<String>` (page 0-based; `None` only when `Url::from_file_path` refuses, i.e. a relative path)
  - `pub fn markdown_page_link(canonical: &Path, name: &str, page: usize) -> Option<String>`

- [ ] **Step 1: Write the failing tests:**
  - `page_link_form`: `page_link("/home/u/books/Some Book.pdf", 50)` == `Some("noido:///home/u/books/Some%20Book.pdf#page=51")`.
  - `parentheses_are_encoded`: the link for `/b/x (2).pdf` contains `x%20%282%29.pdf` and no `(` or `)`.
  - `copied_links_open_the_same_file`: for each of `/b/a#b.pdf`, `/b/100%.pdf`, `/b/q?.pdf`, `/b/(x).pdf`, `/b/Smörgås ö.pdf`, `/b/a&b=c.pdf`, `parse_argument(page_link(p, 4))` == `Opening { path: p, target: Some(Page(4)) }`.
  - `relative_path_has_no_link`: `page_link("x.pdf", 0)` == `None`.
  - `markdown_form`: `markdown_page_link("/b/Some Book.pdf", "Some Book.pdf", 50)` == `Some("[Some Book, p. 51](noido:///b/Some%20Book.pdf#page=51)")`. `"Notes.PDF"` drops the `.PDF`. `"a.pdf.txt"` keeps everything. The name `"x [draft] \\ y.pdf"` gives the label `x \[draft\] \\ y, p. 1`.

- [ ] **Step 2: Run** `cargo test -p noido-core target` → the new tests FAIL.

- [ ] **Step 3: Implement.** Use `Url::from_file_path`, then replace `(` with `%28` and `)` with `%29` in the serialised string. The path part has no `(` or `)` that needs keeping. Replace the `file:` prefix with `noido:` and append `#page={page + 1}`. In the label, strip a trailing `.pdf` (ASCII case-insensitive) and backslash-escape `\`, `[` and `]`.

- [ ] **Step 4: Run** → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Links: build the noido: link and Markdown link to a page"`

### Task 3: Looking up a named place on the links thread

**Files:**
- Modify: `crates/noido-engine/src/engine.rs` (trait `Document`)
- Modify: `crates/engine-hayro/src/lib.rs`, `crates/engine-hayro/src/links.rs`
- Modify: `crates/noido-render/src/links.rs`
- Modify: `crates/frontends/gui-egui/src/links.rs`
- Test: `crates/engine-hayro/tests/links.rs`, `crates/noido-render/src/links.rs` tests (or alongside existing Linker tests), `crates/frontends/gui-egui/src/links.rs`

**Interfaces:**
- Produces:
  - `Document::named_destination(&self, name: &str) -> Option<Destination>`, default `None`.
  - `pub(crate) fn named_destination(pdf: &Pdf, index: &PageIndex, name: &[u8]) -> Option<Destination>` in hayro `links.rs`, which is `explicit(pdf, index, &named(pdf, name)?)`. It is the same code path link resolution already uses.
  - `Linker::request_named(&self, name: String)`; reply `LinkReply::Named { name: String, target: Option<Destination> }`.
  - gui `Links::resolve(&mut self, name: String)`: asks, and remembers `name` as the one pending (replacing any earlier).
  - `Links::cancel_named(&mut self)`.
  - `Links::receive(&mut self, now: f64) -> Option<Named>`, where `pub enum Named { Found(Destination), Unknown(String) }`. It is returned only for the pending name. `set_linker` (reload) already resets the struct, which drops it.

- [ ] **Step 1: Write the failing tests:**
  - hayro `named_destination_lookup`: on `fixtures/links.pdf`, `named_destination("tree.name")` → page 1, y ≈ 292; `"OldName"` → page 1; `"nope"` → `None`.
  - render: a fake document whose `named_destination("a")` is `Some(dest(3))`. After `request_named("a")`, a reply `Named { name: "a", target: Some(..) }` arrives (poll `try_recv` with a short timeout loop, as existing render tests do).
  - gui links: `resolve("a")` then `resolve("b")` → a reply for `"a"` returns `None` from `receive`, and the one for `"b"` returns `Some(Found(..))`. `cancel_named` → the reply for `"b"` returns `None`. An unknown name → `Some(Unknown("b"))`.

- [ ] **Step 2: Run** `cargo test -p engine-hayro -p noido-render -p gui-egui` → FAIL.

- [ ] **Step 3: Implement** as in Interfaces. `HayroDocument::named_destination` uses the cached `pages` index as `links()` does. The `Linker::run` loop gets a `Command::Named(String)` arm.

- [ ] **Step 4: Run** → PASS; clippy. Existing `receive` callers in `app.rs` ignore the new return value until Task 5. Use `let _ =` there, so Task 5 owns the behaviour.

- [ ] **Step 5: Commit** `jj commit -m "Links: look up a named place on the links thread"`

### Task 4: Carrying the destination in the hand-over

**Files:**
- Modify: `crates/frontends/gui-egui/src/instance.rs` (+ its tests)

**Interfaces:**
- Consumes: `noido_core::{Target, parse_fragment, to_fragment}`.
- Produces:
  - `pub fn claim(path: &Path, target: Option<&Target>) -> Claim`. The internal `claim_at` and `hand_over` gain the same parameter.
  - `Activation` holds `Arc<Mutex<Option<Request>>>`, with `pub struct Request { pub target: Option<Target> }`.
  - `Activation::take(&self) -> Option<Request>`
  - `pub(crate) fn request(&self, target: Option<Target>)`. A later request replaces an earlier one, except that a request without a target doesn't erase a waiting target.
- Wire format: the launch sends `activate`, or `activate <fragment>` using `to_fragment`. The listener accepts a line that is `activate` or starts with `activate `, parses the rest with `parse_fragment`, and answers `ok`.

- [ ] **Step 1: Write the failing tests** (unix test module):
  - Update `second_claim_hands_over` to `take().is_some()` / `is_none()`.
  - `hand_over_carries_the_page`: the second `claim_at(.., Some(&Target::Page(51)))` → `take()` == `Some(Request { target: Some(Page(51)) })`.
  - `hand_over_carries_a_named_place`: the same with `Named("sec 2")`.
  - `last_hand_over_wins`: `Page(1)` then `Page(5)` before `take` → `Page(5)`. `Page(5)` then a plain `activate` → still `Page(5)`.

- [ ] **Step 2: Run** `cargo test -p gui-egui instance` → FAIL.

- [ ] **Step 3: Implement**; fix the `claim` call in `main.rs` (pass `None` for now) and the `activation_raises_window` harness test's `request()` call (`request(None)`), and `come_forward` (`take().is_some()`).

- [ ] **Step 4: Run** `cargo test --workspace` → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Hand-over: carry the page or named place to the open window"`

### Task 5: Arriving at the destination in the window

**Files:**
- Modify: `crates/frontends/gui-egui/src/app.rs` (+ Harness tests)

**Interfaces:**
- Consumes: `Activation::take() -> Option<Request>` (Task 4); `Links::resolve`, `Links::cancel_named`, `Links::receive -> Option<Named>` (Task 3); `noido_core::Target`.
- Produces:
  - `pub fn opening_at(self, target: Option<Target>) -> Self` (builder; called after `with_persistence` in `main`). It stores an arrival.
  - Private `arrival: Option<(Target, bool /* leave current place first */)>`, applied in `show` right after `come_forward` and before input handling.
  - `opening_at` sets `leave` = something was restored (`persistence.saved.is_some()`). A hand-over always sets `leave = true`.
  - Applying `Page(i)`: optionally `leave_here()`, then `view.go_to_page(min(i, last))`, `chosen = Some(that)`, `search_state.reader_moved()`, and `links.cancel_named()`. This is the sidebar-click path at `app.rs` ~1214, not `follow`: no flash.
  - Applying `Named(name)`: `links.resolve(name)` and keep `leave` for when the answer comes. `Found(dest)`: optionally `leave_here()`, then `view.go_to_target(&dest)`, `chosen = Some(dest.page.min(last))`, `reader_moved()`. `Unknown(name)`: `eprintln!("noido: no place named {name:?} in the document")`, nothing else.

- [ ] **Step 1: Write the failing tests** (Harness):
  - `opens_at_the_linked_page`: `Harness::build(|a| a.opening_at(Some(Target::Page(5))))` → `centre().page == 5`, `history.back_entries()` empty (nothing restored).
  - `opening_past_the_end_goes_to_the_last_page`: `Page(99)` on 10 pages → page 9.
  - `opening_at_a_page_keeps_the_remembered_place_for_back`: restore at page 7 via `with_persistence` (as the test at ~2135 does) then `opening_at(Page(2))` → page 2. Alt+← → page 7. The remembered zoom is unchanged.
  - `hand_over_jumps_and_back_returns`: `with_activation(a)`; `go_to(&mut h, 4)`; `a.request(Some(Page(8)))`; one frame → page 8 and `Focus` sent. Alt+← → page 4.
  - `hand_over_without_a_page_only_comes_forward`: page and history unchanged.
  - `named_place_is_followed`: a `TextEngine` variant (or a small engine in the tests) whose `named_destination("sec")` gives page 6, via `linking(..)` plus `opening_at(Some(Named("sec")))`. Run frames until settled → page 6, no flash (`links.flash(now)` is `None`).
  - `unknown_named_place_stays`: `Named("nope")` → the page is unchanged and history is empty.
  - `newer_arrival_drops_a_pending_named_place`: `Named("sec")` via hand-over, then `Page(2)` via hand-over in the same frame batch before the answer → ends on page 2.

- [ ] **Step 2: Run** `cargo test -p gui-egui <name>` → FAIL.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** `cargo test --workspace` → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Links: open at the linked page or place, also in the window already open"`

### Task 6: The command line

**Files:**
- Modify: `crates/frontends/gui-egui/src/main.rs` (+ its tests)

**Interfaces:**
- Consumes: `noido_core::{parse_argument, Opening, Target}`, `instance::claim(path, target)`, `NoidoApp::opening_at`.
- Produces: `Args.path: Option<OsString>` (help text "PDF file, or a file: or noido: link, to open"). `Args.page: Option<u32>`, `#[arg(long, value_parser = clap::value_parser!(u32).range(1..), conflicts_with_all = ["install", "uninstall"])]`, help "Page to open at, from 1". Also `fn opening(args: &Args) -> Result<Opening, String>`, where `--page` replaces the link's target with `Page(n - 1)`.

- [ ] **Step 1: Write the failing tests:**
  - `page_flag`: `noido a.pdf --page 3` → `Opening { "a.pdf", Some(Page(2)) }`.
  - `page_flag_wins_over_the_link`: `noido 'noido:///b/a.pdf#page=9' --page 3` → `Page(2)`.
  - `page_zero_is_rejected`: `try_parse_from([.., "--page", "0"])` is `Err`.
  - `page_conflicts_with_install`.
  - `handed_over` message: with `Some(Page(50))`, it ends `"…asked its window to come forward at page 51"`. With `Named("x")`, it ends `"…at \"x\""`. With `None`, it is unchanged.

- [ ] **Step 2: Run** `cargo test -p gui-egui tests::page` → FAIL.

- [ ] **Step 3: Implement.** `run(opening: Opening)` claims with the target, loads `opening.path`, and calls `.opening_at(opening.target)` after `with_persistence`. Errors from `parse_argument` go out as `noido: <message>` like other errors.

- [ ] **Step 4: Run** `cargo test --workspace` → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Command line: open a file: or noido: link, and --page"`

### Task 7: Copy link and Copy Markdown link

**Files:**
- Modify: `crates/noido-core/src/keymap.rs`, `crates/noido-core/src/menu.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs` (+ Harness), `main.rs`

**Interfaces:**
- Consumes: `page_link`, `markdown_page_link` (Task 2).
- Produces:
  - `Action::CopyLink` and `Action::CopyMarkdownLink`, bound to `command(Key::L)` and `shift(command(Key::L))`.
  - A menu section right after Back/Forward: `entry(Action::CopyLink, "Copy link to page")` and `entry(Action::CopyMarkdownLink, "Copy Markdown link to page")`.
  - `NoidoApp::with_link_path(self, canonical: PathBuf) -> Self`. `main` passes `std::fs::canonicalize(&path)` when it succeeds.
  - A Harness field `copied: Vec<String>`, collected from `egui::OutputCommand::CopyText`.

- [ ] **Step 1: Write the failing tests:**
  - `menu_sections` gets the new section; `every_menu_action_has_a_key` covers it.
  - A keymap test: Ctrl+L → `CopyLink`, Shift+Ctrl+L → `CopyMarkdownLink`, and the shown key text is "Ctrl+L" / "Shift+Ctrl+L" on Other.
  - Harness `copy_link_copies_the_centre_page`: `with_name("Some Book.pdf")` and `with_link_path("/b/Some Book.pdf")`; `go_to(&mut h, 4)`; Ctrl+L → `copied == ["noido:///b/Some%20Book.pdf#page=5"]`. Shift+Ctrl+L → `"[Some Book, p. 5](noido:///b/Some%20Book.pdf#page=5)"`.
  - `copy_without_a_path_does_nothing`: no `with_link_path` → `copied` is empty.

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement.** In `apply`, the page is `self.view.centre_page()` (the title bar's number). With no page or no path, do nothing. Otherwise `ctx.copy_text(link)`.

- [ ] **Step 4: Run** `cargo test --workspace` → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Links: Copy link to page (Ctrl+L) and Copy Markdown link (Shift+Ctrl+L)"`

### Task 8: Desktop entry takes links

**Files:**
- Modify: `crates/frontends/gui-egui/src/desktop.rs` (+ tests)

- [ ] **Step 1: Update the tests:**
  - `entry_launches_the_binary_for_pdfs` expects `Exec=/opt/noido/noido %u` and `MimeType=application/pdf;x-scheme-handler/noido;`.
  - The two `Exec` quoting tests expect ` %u`.

- [ ] **Step 2: Run** `cargo test -p gui-egui desktop` → FAIL.

- [ ] **Step 3: Implement**: the two lines in `desktop_entry`.

- [ ] **Step 4: Run** → PASS; clippy.

- [ ] **Step 5: Commit** `jj commit -m "Desktop entry: take links (%u) and handle noido: links"`

### Task 9: Review, release build, hand-check list

- [ ] **Step 1:** Review the whole change with a fresh subagent on the most capable model, against the spec and this plan. Fix Critical/Important findings test-first. Minors go to one issue: `Links into noido: review minors (YYYY-MM-DD)`, label `review-minor`, a checklist in the body.
- [ ] **Step 2:** `cargo build --release`. Check with a scratch `XDG_STATE_HOME`: `target/release/noido 'noido:///…/fixtures/two-pages.pdf#page=2'` opens on page 2. Stop it with `pgrep -x noido | xargs -r kill -INT`.
- [ ] **Step 3:** Tell the user to run `target/release/noido --install` (it points the desktop entry at that binary), then check by hand:
  - `gio open 'noido:///…#page=N'` on a closed PDF and on an open one; Back returns.
  - Clicking a `noido:` link in a Markdown editor and in Firefox.
  - Ctrl+L and Shift+Ctrl+L, then pasting both and clicking.
  - `noido book.pdf --page 51`.
- [ ] **Step 4:** Delete the ledger `.superpowers/sdd/2026-10-30-links-into-noido/`.
