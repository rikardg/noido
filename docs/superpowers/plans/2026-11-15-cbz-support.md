# CBZ Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the approved CBZ spike into the reviewed CBZ reader: format-aware document info, macOS open/bundle integration, a hardened engine with edge-case fixtures, a documented archive-format decision, and the usual final review.

**Architecture:** `engine-comic` implements the existing engine traits; a frontend dispatcher picks hayro for `%PDF-` and the comic engine for `PK\x03\x04`. Pages are ZIP entries in natural order, decoded with `image` and downscaled with `fast_image_resize`; a render never exceeds the image's own pixels. A ZIP's state key hashes its ends, not the whole book.

**Tech Stack:** Rust 2024, `zip` 9.5 (`deflate-flate2`), `image` 0.25 (`jpeg`, `png`, `gif`, `webp`, `bmp`, `tiff`), `fast_image_resize` 6.1, `roxmltree`, egui/eframe 0.37.

**Spec:** No separate spec: an approved CBZ spike established the design; the Baseline below records its decisions, and the spike's code and tests begin this branch's CBZ commits.

## Baseline

The spike already implements the following; the tasks below modify it rather than rebuild it:

- `crates/engine-comic`: `ComicEngine`, ZIP page listing, natural order, junk/metadata filtering, ComicInfo properties, bounded decode, premultiplied RGBA, native-resolution cap, `fast_image_resize`.
- `crates/frontends/gui-egui/src/document.rs`: `AnyEngine` dispatch and the six worker threads.
- `crates/frontends/gui-egui/src/load.rs` and `crates/noido-state`: sampled ZIP state key (`length + first 4 KB + last 1 MB`).
- Frontend: `.cbz` in the rfd filter, Linux `.desktop` MIME type, CLI copy.
- `fixtures/comic.cbz` and its generator; engine tests; `examples/measure.rs`.

Measured on `testfiles/blame_vol1.cbz` (406 pages, 883 MB, stored CMYK JPEGs, 2456×3375):

| Measurement | Value |
|---|---|
| mmap load | ~0.2–1 ms |
| open + 406 header reads | ~2.3 ms |
| sampled key (1 MB tail) | ~3 ms |
| full-file key | ~2.2 s first pass, 0.4 s warm |
| native render | 19–30 ms |
| fit 1200 px with `fast_image_resize` | 31–42 ms |
| fit 1200 px with `image` CatmullRom | 70–80 ms |
| app RSS on the real book | ~100–340 MB, vs 962 MB with the full-file key |

Decisions locked by the spike:

- 1 image pixel = 1 page point. 100% is one image pixel per logical UI point; the info page shows pixels, not millimetres.
- Whole-page renders never upscale above the source; `render_page` may return fewer pixels than requested, and the renderer's cost accounting already allows it.
- No decoded-page cache. The texture budget keeps the pages a reader needs; a cache is a later measurement.
- Encrypted ZIPs, AVIF/JXL pages, CB7/CBR/CBA, ext4/reflink fallback and right-to-left manga are **not** in this plan.

Known shortcuts to fix: `MAX_PIXELS` is 64 MP (too high for six floor spreads), page errors name the archive entry rather than the comic page, the sampled key's 1 MB tail can miss a large central directory, and the spike has no fixtures for deflated, alpha, malformed, huge or image-less archives.

## Global Constraints

- TDD: write the failing test, run it and see it fail, then implement. `just check` (fmt-check, clippy, nextest) green before each commit.
- jj, colocated with git. Commit with `jj commit -m "<message>"`; never move `main`. Title ≤ 72 characters, prefix is the crate's short name (`comic`, `state`, `egui`, `Repo`, `Investigation`). A review fix is a `fixup:` commit, squashed before the work is reported done.
- Tests use only `fixtures/` or synthetic data; nothing from `../testfiles/`.
- Never send synthetic input to the desktop; GUI behaviour is tested in the headless `Harness`.
- Pure-Rust dependencies only; no C/C++ or system libraries. `zip` uses `default-features = false, features = ["deflate-flate2"]`; `image` uses the features listed above; `fast_image_resize` is 6.1.
- `MAX_PIXELS = 32 * 1024 * 1024`, `MAX_ENTRY_BYTES = 256 * 1024 * 1024`, `MAX_ENTRIES = 20_000`. A page over the pixel cap is refused before decoding.
- `DocKey::sampled` is for ZIPs only: length + first 4 KB + last 4 MB. PDFs keep the full content hash. A future TAR reader must not use the sampled key.
- Backlog candidates are recorded in the plan's last section and in the session TODO list; they are settled at the end of the work, not during these tasks.

## Review Focus

1. **Malformed or truncated ZIP**: a clear open error, not a panic; a bad page fails that page, not the document.
2. **Mixed entries and compression**: stored and deflated pages, nested folders, `ComicInfo.xml`, `notes.txt`, `__MACOSX`, `._*`; every real page appears once in natural order.
3. **Colour and alpha**: CMYK scans keep their colours and orientation; a transparent PNG's pixels are premultiplied, not straight.
4. **Resource limits**: a huge image header, a huge entry, or tens of thousands of entries opens or refuses without unbounded allocation.
5. **State identity**: editing a ZIP changes its key; an unchanged ZIP restores state; a PDF keeps the full-hash behaviour; the watcher uses the same key.

---

### Task 1: Format-aware Document Info

**Files:**
- Modify: `crates/noido-engine/src/types.rs`
- Modify: `crates/engine-hayro/src/properties.rs`
- Modify: `crates/engine-comic/src/lib.rs`
- Modify: `crates/frontends/gui-egui/src/info.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs` (the `PropsEngine` test fake)
- Test: in each file's `#[cfg(test)]` module

**Interfaces:**
- Consumes: the existing `Properties` struct and `info::page_size_text`.
- Produces: `noido_engine::Format::{Unknown, Pdf, Comic}` and `Properties::format: Format`; `info::page_size_text(sizes: &[Size], pixels: bool) -> String`.

- [ ] **Step 1: Write the failing engine test**

In `crates/noido-engine/src/types.rs` tests:

```rust
#[test]
fn a_document_without_a_format_is_unknown() {
    assert_eq!(Properties::default().format, Format::Unknown);
}
```

Run `cargo test -p noido-engine a_document_without_a_format_is_unknown`. Expected: compile error, `Format` not found.

- [ ] **Step 2: Add `Format` and the field**

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Unknown,
    Pdf,
    Comic,
}

pub struct Properties {
    pub format: Format,
    // existing fields
}
```

Run the test; expected PASS. Fix every `Properties { ... }` literal with `..Default` or an explicit format.

- [ ] **Step 3: Write the failing engine tests**

In `crates/engine-hayro/tests/properties.rs`, assert `properties().format == Format::Pdf`; in `crates/engine-comic/tests/comic.rs`, assert `open_fixture().properties().format == Format::Comic`. Run both; expected FAIL with `Unknown`.

- [ ] **Step 4: Set the format in both engines**

`engine-hayro/src/properties.rs` starts its `Properties` with `format: Format::Pdf`. In `engine-comic`, initialise `Properties { format: Format::Comic, ..Default::default() }` before parsing and have `parse_comic_info` return that same format on its success and malformed paths, so a missing or bad ComicInfo still says Comic. Run both tests; expected PASS.

- [ ] **Step 5: Write the failing info-page tests**

In `crates/frontends/gui-egui/src/info.rs` tests:

```rust
#[test]
fn comic_pages_are_shown_in_pixels() {
    assert_eq!(
        page_size_text(&[size(2456.0, 3375.0)], true),
        "2456 × 3375 px"
    );
    assert_eq!(page_size_text(&[size(10.0, 15.0), size(20.0, 30.0)], true), "mixed");
}

#[test]
fn comic_info_hides_pdf_rows() {
    let mut h = with_info_source_kind(Format::Comic);
    h.press(egui::Key::I, Modifiers::COMMAND);
    h.until(|h| h.text_rect("CBZ comic archive").is_some());
    assert!(h.text_rect("2456 × 3375 px").is_some());
    for text in ["PDF version", "Tagged", "Outline", "Fonts"] {
        assert!(h.text_rect(text).is_none(), "{text:?} drawn");
    }
}
```

Add `with_info_source_kind(format)` in `app.rs` tests (a `PropsEngine` variant with that `format` and 2456×3375 page sizes). Do not use the existing `show_info`, which waits for a font row; press Mod+I and wait for `"CBZ comic archive"` instead. Run; expected FAIL.

- [ ] **Step 6: Implement the info-page branch**

In `contents`:

```rust
let comic = properties.is_some_and(|p| p.format == Format::Comic);
```

Show `row(ui, "Format", Some("CBZ comic archive".into()))` only when `comic`; call
`page_size_text(sizes, comic)`; wrap the PDF version/tagged/outline rows and the
whole Fonts section in `if !comic`. Unknown formats keep today's PDF-shaped rows.

Run the tests; expected PASS.

- [ ] **Step 7: Run the full checks and commit**

Run: `just check`
Expected: every test passes, fmt-check is clean, clippy is clean.

```bash
jj commit -m "egui: show comic details in the info page"
```

---

### Task 2: macOS Open Panel and Finder

**Files:**
- Modify: `crates/frontends/gui-egui/src/app_bundle.rs`
- Modify: `crates/frontends/gui-egui/src/open.rs`
- Modify: `crates/frontends/gui-egui/src/app.rs`
- Modify: `crates/noido-macos/src/open_panel.rs`
- Modify: `crates/noido-macos/src/lib.rs`
- Test: `app_bundle.rs`, `open_panel.rs`, and the existing `shell.rs` call sites

**Interfaces:**
- Consumes: the CBZ MIME type `application/vnd.comicbook+zip` and the exported UTI.
- Produces: `app_bundle::CBZ_UTI = "io.github.rikardg.noido.cbz"`; `pick_document`/`pick_documents` (renamed from `pick_pdf`/`pick_pdfs`); an Open panel allowing PDF and CBZ.

- [ ] **Step 1: Write the failing plist test**

In `app_bundle.rs`, extend `plist_is_xml_and_declares_pdfs_and_the_scheme` (or add a new test) to require:

```rust
for wanted in [
    "<key>UTExportedTypeDeclarations</key>",
    "<string>io.github.rikardg.noido.cbz</string>",
    "<string>public.zip-archive</string>",
    "<string>application/vnd.comicbook+zip</string>",
    "<key>CFBundleTypeName</key><string>Comic book archive</string>",
] { assert!(xml.contains(wanted), "missing {wanted}"); }
```

Run it; expected FAIL.

- [ ] **Step 2: Add the UTI and the second document type**

Add `pub const CBZ_UTI: &str = "io.github.rikardg.noido.cbz";` and the plist blocks:

```xml
<key>UTExportedTypeDeclarations</key>
<array><dict>
  <key>UTTypeIdentifier</key><string>io.github.rikardg.noido.cbz</string>
  <key>UTTypeDescription</key><string>Comic book archive</string>
  <key>UTTypeConformsTo</key><array><string>public.zip-archive</string></array>
  <key>UTTypeTagSpecification</key><dict>
    <key>public.filename-extension</key><array><string>cbz</string></array>
    <key>public.mime-type</key><string>application/vnd.comicbook+zip</string>
  </dict>
</dict></array>
```

Add a second `CFBundleDocumentTypes` dict named `Comic book archive`, `Viewer`,
`Alternate`, with `LSItemContentTypes` = the CBZ UTI. Run the test; expected PASS.

- [ ] **Step 3: Write the failing Open panel test**

In `open_panel.rs`:

```rust
#[test]
fn the_open_panel_offers_cbz_as_well_as_pdf() {
    let cbz = cbz_type().expect("the system knows cbz");
    let extension = cbz.preferredFilenameExtension().map(|e| e.to_string());
    assert_eq!(extension.as_deref(), Some("cbz"));
    assert!(pdf_type().is_some());
}
```

Run `cargo test -p noido-macos the_open_panel_offers_cbz_as_well_as_pdf`; expected compile error.

- [ ] **Step 4: Add the CBZ type to the panel**

```rust
const CBZ_UTI: &str = "io.github.rikardg.noido.cbz";

fn cbz_type() -> Option<Retained<UTType>> {
    // The extension is what the panel can filter by; the UTI in Info.plist
    // is what makes Finder offer noido. A dynamic type from the extension
    // works even when the bundle is not installed (as in tests).
    UTType::typeWithFilenameExtension(&NSString::from_str("cbz"))
        .or_else(|| UTType::typeWithIdentifier(&NSString::from_str(CBZ_UTI)))
}
```

In `pick_documents`, collect the PDF and CBZ types and pass both to
`setAllowedContentTypes`. Rename `pick_pdfs` to `pick_documents` and update the
re-export; rename `open::pick_pdf` to `open::pick_document` and update
`app.rs`/`shell.rs`. Run the test; expected PASS.

- [ ] **Step 5: Check the non-macOS path and commit**

The rfd filter already lists `["pdf", "cbz"]`; update its doc comment to “PDF or
comic”. Run `just check`; commit:

```bash
jj commit -m "egui: open CBZ files from Finder and the Open panel"
```

---

### Task 3: Harden the Comic Engine and Fixtures

**Files:**
- Modify: `crates/engine-comic/src/lib.rs`
- Modify: `crates/engine-comic/tests/comic.rs`
- Modify: `fixtures/make_fixtures.py`
- Add: `fixtures/comic-deflated.cbz`, `fixtures/comic-alpha.cbz`, `fixtures/comic-broken.cbz`, `fixtures/comic-empty.cbz`, `fixtures/comic-huge.cbz`, `fixtures/comic-bad-info.cbz`
- Modify: `crates/noido-engine/src/engine.rs` (the `render_page` doc)
- Test: `crates/engine-comic/tests/comic.rs`

**Interfaces:**
- Consumes: the spike's `ComicEngine`/`ComicDocument`.
- Produces: `MAX_PIXELS = 32 * 1024 * 1024`; page errors naming 1-based comic pages; `render_page` documented as “at most” for raster engines; fixtures for deflate, alpha, corruption, empty and oversized pages.

- [ ] **Step 1: Write the failing deflate and alpha tests**

Add fixture cases first:

```python
# in comic(): parameterise compression and entries
def comic(compression=zipfile.ZIP_STORED, extra=b"") -> bytes: ...
# comic.cbz stays stored; comic-deflated.cbz uses ZIP_DEFLATED
```

Tests:

```rust
#[test]
fn opens_and_renders_a_deflated_archive() { /* page_count 4; render page 0 red */ }

#[test]
fn alpha_is_premultiplied() {
    let image = open_named("comic-alpha.cbz").render_page(0, 2, 2).unwrap();
    assert_eq!(image.pixel(0, 0), [100, 50, 25, 128]); // source 200,100,50,128
}
```

Run; expected FAIL (fixtures/tests missing).

- [ ] **Step 2: Generate the fixtures and implement the alpha path**

Extend `make_fixtures.py` with a 2×2 RGBA PNG and the deflated archive; run
`python3 fixtures/make_fixtures.py`. The existing premultiply loop should make
the alpha test pass; if it does not, fix the loop (integer `channel * alpha / 255`).
Run both tests; expected PASS.

- [ ] **Step 3: Write the failing page-number error test**

Add `comic-broken.cbz`: ComicInfo at index 0, a valid `1.png`, a `2.png` whose
IDAT is corrupted but whose IHDR is valid. In `comic.rs`:

```rust
#[test]
fn a_broken_page_names_the_comic_page() {
    let doc = open_named("comic-broken.cbz");
    let message = doc.render_page(1, 10, 15).unwrap_err().to_string();
    assert!(message.contains("page 2"), "{message}");
}
```

Run; expected FAIL: the spike uses the ZIP entry index, so it says “page 3”.

- [ ] **Step 4: Number pages after sorting**

Add `number: usize` to `Page`, set it after `pages.sort_by(...)` with
`for (number, page) in pages.iter_mut().enumerate()`, and make every per-page
error use `page.number + 1`. Run; expected PASS.

- [ ] **Step 5: Write the failing size-limit test**

Add `comic-huge.cbz`: one PNG whose IHDR declares 8000×8000, with a tiny IDAT
that never decodes. Change `MAX_PIXELS` to `32 * 1024 * 1024`. Test:

```rust
#[test]
fn a_page_over_the_pixel_cap_is_refused_before_decoding() {
    let doc = open_named("comic-huge.cbz");
    assert_eq!(doc.page_size(0).unwrap(), Size { width: 8000.0, height: 8000.0 });
    assert!(matches!(
        doc.render_page(0, 1, 1),
        Err(EngineError::TooLarge { width: 8000, height: 8000 })
    ));
}
```

Run; expected FAIL (the spike decodes first, so it reports a decode error).

- [ ] **Step 6: Check the known size before decoding**

Add `fn check_size(page: &Page) -> Result<(), EngineError>` using
`page.size`; call it at the top of `render_page` and `render_tile`, before
`decode`. Keep the post-decode pixel check as defence. Run; expected PASS.
Update the `Document::render_page` doc:

```rust
/// Renders the whole of `page` at about `width` × `height` pixels ... A
/// raster engine may return fewer pixels rather than upscale; the renderer
/// treats that as the page's render.
```

- [ ] **Step 7: Write the failing empty/malformed tests**

```rust
#[test]
fn an_archive_without_images_is_refused() {
    let error = ComicEngine.open(fixture_bytes("comic-empty.cbz")).unwrap_err();
    assert!(error.to_string().contains("no images"));
}

#[test]
fn a_malformed_zip_is_an_open_error() {
    let bytes = PdfBytes::from(b"PK\x03\x04not a zip".to_vec());
    assert!(ComicEngine.open(bytes).is_err());
}

#[test]
fn malformed_comic_info_leaves_defaults() {
    let properties = open_named("comic-bad-info.cbz").properties();
    assert_eq!(properties.format, Format::Comic);
    assert_eq!(properties.title, None);
}
```

Add `comic-empty.cbz` (ComicInfo + notes only) and `comic-bad-info.cbz`
(unclosed `<Title>`). Run; expected FAIL until the fixtures exist; the code
already handles them, so the tests should pass once the fixtures are generated.

- [ ] **Step 8: Add the entry-count test**

In `comic.rs`, build an in-memory ZIP with `zip::ZipWriter` containing
20,001 empty files (`MAX_ENTRIES + 1`; the constant is private), then:

```rust
let error = ComicEngine.open(PdfBytes::from(bytes)).unwrap_err();
assert!(error.to_string().contains("more than a comic"));
```

Run; expected PASS (the spike already has the check); this pins it.

- [ ] **Step 9: Run the full checks and commit**

Run: `just check`; then:

```bash
jj commit -m "comic: harden the archive reader"
```

---

### Task 4: Widen the Sampled ZIP Key

**Files:**
- Modify: `crates/noido-state/src/lib.rs`
- Modify: `crates/frontends/gui-egui/src/load.rs`
- Test: `crates/noido-state/src/lib.rs` `key_tests`, `load.rs` tests

**Interfaces:**
- Consumes: `DocKey::sampled` and `load::doc_key`.
- Produces: a 4 MiB tail sample, with tests for what it sees and what it deliberately does not.

- [ ] **Step 1: Write the failing boundary tests**

In `key_tests`:

```rust
#[test]
fn a_sampled_key_sees_a_tail_change_but_not_a_middle_one() {
    let path = Path::new("/papers/a.cbz");
    let bytes = vec![0u8; 8 * 1024 * 1024];
    let key = DocKey::sampled(&bytes, path).hex();
    let mut middle = bytes.clone();
    middle[2 * 1024 * 1024] = 1;                  // outside the first 4 KB and last 4 MiB
    assert_eq!(DocKey::sampled(&middle, path).hex(), key);
    let mut tail = bytes.clone();
    tail[7 * 1024 * 1024] = 1;                    // inside the last 4 MiB
    assert_ne!(DocKey::sampled(&tail, path).hex(), key);
    assert_ne!(DocKey::sampled(&[bytes, vec![0]].concat(), path).hex(), key);
}
```

Replace the existing `a_sampled_key_leaves_the_middle_alone` test and update
the `load.rs` ZIP/PDF test to 8 MiB and the same 2 MiB/7 MiB points.
Run; expected FAIL: the current 1 MiB tail sees the 2 MiB change.

- [ ] **Step 2: Widen the tail and pass**

Change `END` to `4 * 1024 * 1024`; update the doc comment (“first four
kilobytes and last four mebibytes”). Run both tests; expected PASS.

- [ ] **Step 3: Run the full checks and commit**

Run: `just check`; then:

```bash
jj commit -m "state: sample a wider end of a ZIP for its key"
```

---

### Task 5: Comic Archive Format Investigation

**Files:**
- Add: `docs/investigations/2026-11-15-comic-archive-formats.md`

**Interfaces:** none; this task produces a decision document, not code.

- [ ] **Step 1: Write the investigation**

Cover `.cbz`, `.cbt`, `.cb7`, `.cbr` and `.cba`, with one table row each:
container, Rust crate and version, licence, random access, commonality, and
recommendation. Use these facts:

- **CBZ / ZIP** — done; `zip` 9.5.1, stored and deflate. Most common.
- **CBT / TAR** — `tar` 0.4.46, MIT/Apache, pure Rust, ~290M downloads,
  uncompressed only. `Entry::raw_file_position()` and `size()` give a page's
  byte range in the mapped archive, so random access is cheap. Recommended
  next format; it needs an `Archive` abstraction and a **full** content key
  (TAR has no central directory or CRC).
- **CB7 / 7z** — `sevenz-rust2` 0.25.0, Apache-2.0, pure Rust, ~2.3M downloads.
  Solid blocks make a late page's read decode the block from its start; the
  crate documents that skipping is not possible. Needs a block cache or
  extraction design. Backlog.
- **CBR / RAR** — `unrar` 0.5.8, MIT/Apache wrapper around the C++
  `unrar_sys`; the UnRAR licence permits decompression but restricts a
  compressor and is not OSI. Common in scene scans. Backlog pending a
  licensing/optional-feature decision.
- **CBA / ACE** — proprietary; no maintained Rust decoder found. Drop unless
  a real sample and decoder appear.
- **Encrypted ZIP** — out of scope; `zip` is built without AES support.

End with a recommendation: ship CBZ in this plan; make CBT the next small
format; revisit CB7 only with a block cache; take CBR only with an explicit
licensing decision; drop CBA. Note that commonality is a judgement from format
and MIME data, not a sample set: ask for real CB7/CBR files before scheduling
either.

- [ ] **Step 2: Commit**

```bash
jj commit -m "Investigation: comic archive formats"
```

---

### Task 6: Repo and User-Facing Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `crates/frontends/gui-egui/src/main.rs`
- Modify: `crates/frontends/gui-egui/src/desktop.rs`
- Modify: `crates/frontends/gui-egui/src/app_bundle.rs` (user-facing strings)
- Modify: `crates/frontends/gui-egui/src/open.rs` (docs)
- Modify: `crates/engine-comic/src/lib.rs` (crate docs)

**Interfaces:** none; copy and comments only.

- [ ] **Step 1: Update the copy**

- README: “Early stage PDF and comic reader”; add a bullet for CBZ.
- CLAUDE.md: add `engine-comic` to the crate list (“the engine on ZIP comic
  archives”); change “Test PDFs” to “Test documents” and mention `comic.cbz`
  in `fixtures/`.
- `main.rs`: `--install` help says “for PDFs and comics”; keep the existing
  `about` line.
- `desktop.rs` and `app_bundle.rs`: “PDFs and comics” in the install/Finder
  messages.
- `open.rs`/`engine-comic`: “PDF or comic” in doc comments.

Do **not** add to `docs/backlog.md`; it is frozen.

- [ ] **Step 2: Run the full checks and commit**

Run: `just check`; then:

```bash
jj commit -m "Repo: document the comic reader"
```

---

### Task 7: Whole-Branch Review and Minor Settling

**Files:** whatever the review and the user's minor decisions require.

**Interfaces:** none.

- [ ] **Step 1: Verify the branch**

Run `just check`; run `target/release/noido --foreground fixtures/comic.cbz`
and the real `testfiles/blame_vol1.cbz` once more, with a scratch
`XDG_STATE_HOME`. Check page order, fit/zoom quality, rotation, dual spreads,
pins and state restore. Confirm the info page and the macOS Open panel.

- [ ] **Step 2: Dispatch a fresh reviewer**

Per `AGENTS.md`: a fresh subagent on `deepseek-v4.1-flash`, thinking `high`;
a second angle on `glm-5.3-flash` for the UI/infra parts if the first review
finds the design questionable. Review the whole branch against the Review
Focus section, not just the last task.

- [ ] **Step 3: Fix and settle**

Fix every Critical/Important finding test-first. Use `fixup: <crate>: ...`
commits, then squash them into the commits they correct. Present the remaining
minors to the user with a recommendation each, and settle them as fix now,
review-minors issue, or drop. Delete any `.superpowers/sdd/` ledger when done.

- [ ] **Step 4: Final state**

Review the Backlog Candidates table with the user and settle every row: fix,
file as an issue, or drop. `just check` green; `just build` release; report what
to try by hand and which binary to run. Do not move `main`.

---

## Backlog Candidates

Reviewed at the end of the work; none are implemented by this plan.

| ID | Candidate | Why it is parked |
|---|---|---|
| B1 | ext4/reflink fallback for `PdfBytes::mapped_in` | On a file system without `FICLONE`, a large book is read into RAM; revisit the mmap decision after the feature lands. |
| B2 | Manga/right-to-left spread order | `odd_left` only pairs pages; a true RTL reading order needs layout and key changes, and examples. |
| B3 | Encrypted ZIP support | Deliberately out of scope for this plan. |
| B4 | CBT (TAR) support | Recommended next format; see Task 5. Needs an `Archive` abstraction and a full content key. |
| B5 | CB7 (7z) support | Pure-Rust crate exists, but solid blocks make lazy page access expensive. |
| B6 | CBR (RAR) support | Common, but `unrar` is C++ and the UnRAR licence needs a decision. |
| B7 | CBA (ACE) support | No maintained decoder; drop unless a sample appears. |
| B8 | Textless search/selection UX | Search finds nothing and selection is inert on a comic; a generic “no text” affordance would also help scanned PDFs. |
| B9 | Rename `PdfEngine`/`PdfBytes` | The names are now document-neutral; a mechanical workspace rename. |
| B10 | Decoded-page cache / Linux re-measure | Only if page turns lag on the daily driver; the texture budget currently wins. |
| B11 | Auto-detect double-page spreads | A wide page should be shown alone; a layout feature, not a reader feature. |
| B12 | Page-size DPI mapping | 1 px = 1 pt is simple; 300 DPI scans might prefer `px × 72/300`. |
| B13 | Richer ComicInfo metadata | Series, number, publisher and manga direction are parsed but not surfaced. |
