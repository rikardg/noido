# EPUB images (E4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw an EPUB book's images on its typeset pages: the cover, figures, chapter-title art and inline images.

**Architecture:** The comic engine's image code moves into a new crate, `noido-image`, which both the comic engine and the typesetter use. `noido-blocks` records each image's attributes. `noido-epub` reads an image's size and bytes from the archive on demand and names the cover's image. `noido-typeset` resolves each image when it lays a book out:
- an image alone in its paragraph, or a large one inside a paragraph, becomes a figure paragraph of one line;
- a small one becomes an inline atom;
- the cover becomes a page of its own.

Pixels are decoded and resized only when a tile is drawn, through a cache bounded by size.

**Tech Stack:** Rust 2024, `image` 0.25, `fast_image_resize` 6.1, `tiny-skia` 0.12, `zip` 9, `parley` 0.13.

**Spec:** `docs/superpowers/specs/2026-11-21-epub-images-design.md` (E1's spec, `docs/superpowers/specs/2026-11-19-epub-design.md`, for the parts it builds on).

## Global Constraints

- **Locations don't change.** An image stays one U+FFFC object character with a zero-length source span. A part's character count and its ids' places stay exactly as they were under E1.
- **The image's glyph carries the stand-in text:** the alt text, or `[image]` when the alt is empty or is the file's name.
- **Natural size:** 1 px = 0.75 pt. A plain-number `width`/`height` attribute (CSS px) wins over the pixel size; when only one is given, the aspect ratio is kept. Anything else (`50%`, `3em`, `100px`) is ignored.
- **Figure:** shrunk (aspect kept, never enlarged) to `MEASURE` (270 pt) wide and `LINES as f32 * LEADING` (434 pt) tall, centred on the measure. `LEADING / 2.0` of space before and after, dropped at the top of a page.
- **When an image becomes a figure:**
  - it is its paragraph's only drawn content (whitespace aside), or
  - its natural height is over 28 pt (`2.0 * LEADING`). The paragraph then splits around it, and images side by side stack.
- **Inline image:** shrunk (aspect kept, never enlarged) to the ascent of the font of the text it sits in; it sits on the baseline.
- **Cover:** fitted to the whole page (`PAPER`, margins ignored), enlarged or shrunk, aspect kept, centred. A cover without a readable image keeps E1's title page.
- **Limits (from `engine-comic`):**
  - 32 Mi pixels per image (`32 * 1024 * 1024`);
  - entry reads capped at 256 MB;
  - header reads capped at 8 MB.
- **Decoded and resized pixels:** in one cache shared by the book's threads, bounded at 64 MB, least recently used out first.
- **When an image can't be shown:**
  - a header that can't be read (missing, unknown format, corrupt, over the pixel limit) leaves E1's stand-in text;
  - a decode that fails at draw time gives a thin grey frame of the laid-out size.
- **`LAYOUT_KEY`** becomes `"b-format 11/14 images"`.
- **Markdown images keep their stand-in** (the engine has no directory to read them from).
- **Commits:** jj, `prefix: what changed` (crate short names: `image`, `blocks`, `epub`, `typeset`, `comic`, `egui`). TDD throughout. `just check` is clean at each commit.
- **No wall-clock assertions.** Count work with `#[cfg(test)]` counters.

## Review Focus

1. **An image whose header claims huge dimensions** (60000 × 60000 in a few bytes) is refused at layout and keeps its stand-in; nothing is ever decoded. Test in Task 3.
2. **A tile cut through an image** shows the same pixels as the whole page drawn at once. The image's pixel rectangle is rounded from page coordinates, not tile coordinates. Test in Task 5.
3. **Transparent PNGs/GIFs** draw over white correctly: the pixels are premultiplied once, not twice. Test in Task 5.
4. **`width="0"`, a 0 × 0 image, or `height` alone** never divide by zero or give an empty glyph. A zero size falls back to the stand-in. Test in Task 4.
5. **Several render threads drawing one image at once** decode it once and don't deadlock. The cache lock is not held while another lock is taken. Test in Task 5 (counter after two threads).

---

## File structure

- Create `crates/noido-image/` (`Cargo.toml`, `src/lib.rs`): limits, header read, decode, resize.
- Modify `crates/engine-comic/src/lib.rs`, `Cargo.toml`: use `noido-image`.
- Modify `crates/noido-blocks/src/text.rs` (`Style::Image`), `xhtml.rs` (attributes, svg images, `first_image`), `markdown.rs` (new fields).
- Modify `crates/noido-epub/src/lib.rs`: `image_size`, `image_data`, `PartKind::Cover { image }`, `PartKind::Image`.
- Create `crates/noido-typeset/src/images.rs`: the image store and cache.
- Modify in `crates/noido-typeset/src/`:
  - `paragraphs.rs`: figure paragraphs and splitting;
  - `lines.rs`: figure lines and inline atoms;
  - `book.rs`: flows, glyphs, cover page, wiring;
  - `draw.rs`: drawing images;
  - `engine.rs`: passing the bytes;
  - `house.rs`: the key.
- Modify `fixtures/make_fixtures.py` (new `images.epub`; `book.epub`'s cover becomes 30 × 40), and regenerate.
- Modify `Cargo.toml` (workspace member and dependency) and `CLAUDE.md` (crate list).

---

### Task 1: `noido-image`, shared by the comic engine

**Files:**
- Create: `crates/noido-image/Cargo.toml`, `crates/noido-image/src/lib.rs`
- Modify: `Cargo.toml` (members, `noido-image` in `[workspace.dependencies]`, `fast_image_resize = "6.1"` moved to the workspace), `crates/engine-comic/Cargo.toml`, `crates/engine-comic/src/lib.rs`, `CLAUDE.md` (crate list: "`noido-image`: reading, decoding and resizing images, bounded")

**Interfaces:**
- Produces (`noido_image`):
  - `pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;`
  - `pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;`
  - `pub const MAX_HEADER_BYTES: u64 = 8 * 1024 * 1024;`
  - `pub fn read_header(entry: &mut impl Read) -> (Vec<u8>, Option<(u32, u32)>)`: reads in the comic engine's steps (4 KB, 64 KB, 4 MB, then up to `MAX_HEADER_BYTES` in all) until the dimensions parse. Returns the bytes read and the dimensions.
  - `pub fn dimensions(data: &[u8]) -> Option<(u32, u32)>`
  - `pub fn decode(data: &[u8]) -> Result<image::RgbaImage, DecodeError>`: RGBA8, premultiplied, refused over `MAX_PIXELS`.
  - `pub enum DecodeError { Image(String), TooManyPixels(u64) }`, with `Display`.
  - `pub fn resized(image: image::RgbaImage, width: u32, height: u32) -> image::RgbaImage`: Catmull-Rom, alpha already premultiplied.
  - `pub fn premultiply(image: &mut image::RgbaImage)`
- The comic engine keeps `sniff`, `looks_like_image_name`, `junk`, its own error messages and `MAX_ENTRIES` / `MAX_COMIC_INFO_BYTES`.

- [ ] **Step 1: Move the comic engine's tests of these functions into `noido-image` and add new ones**
  - The header-cap test moves as `read_header_stops_at_the_header_cap` (position == `MAX_HEADER_BYTES` on zeros).
  - `decode_premultiplies`: a 1 × 1 PNG of `(200, 100, 50, 128)` decodes to `[100, 50, 25, 128]` (±1).
  - `decode_refuses_too_many_pixels`: a PNG whose IHDR says 8000 × 8000 gives `Err(DecodeError::TooManyPixels(_))` or `Image(_)`, never `Ok`. Build it in the test the way `comic_huge` does in the fixture script.
  - `resized_keeps_a_solid_colour`: 4 × 4 solid → 2 × 2 has the same colour.
- [ ] **Step 2: Run them before the crate's code exists**
  Run: `cargo test -p noido-image`. Expected: FAIL (no such functions).
- [ ] **Step 3: Move the code; `engine-comic` calls `noido_image`.** Its `decode` maps `DecodeError` to its existing messages ("page N is M megapixels, too large", "page N: …").
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-image -p engine-comic`. Expected: all pass, and the comic tests pass unchanged.
- [ ] **Step 5: Commit**
  `just check`, then `jj commit -m "image: one crate reads, decodes and resizes images for comics and books"`.

### Task 2: `noido-blocks`, image attributes and svg images

**Files:**
- Modify: `crates/noido-blocks/src/text.rs`, `crates/noido-blocks/src/xhtml.rs`, `crates/noido-blocks/src/markdown.rs`, and every match on `Style::Image` (use `..`).

**Interfaces:**
- Produces:
  - `Style::Image { src: String, alt: String, width: Option<u32>, height: Option<u32> }`. The size is from plain-number attributes only (`"100"` → `Some(100)`; `"100px"`, `"50%"`, `"0"` → `None`); markdown always gives `None`.
  - `Xhtml::first_image: Option<String>`: the first `<img src>` or svg `<image>` `href` / `xlink:href` in the body, as written. It replaces `svg_image`, and `noido-epub`'s cover rule uses `first_image.is_some()`.
- An `<svg>` holding an `<image>` draws one `Style::Image` object character at the svg's place, with `src` from the href and an empty alt. Its text is still counted and not drawn, as now, so the character count is unchanged. A block-level svg makes a paragraph of its own.

- [ ] **Step 1: Write the failing tests in `xhtml.rs`**
  - `img_size_attributes_are_plain_numbers`: `<img src="a.png" width="100" height="50%"/>` gives `width: Some(100), height: None`; `width="0"` gives `None`.
  - `an_svg_image_is_an_image`: `<div><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><title>t</title><image xlink:href="../i/c.jpg"/></svg></div><p>after</p>`. The tree has a paragraph holding one `Style::Image { src: "../i/c.jpg", alt: "", .. }`. The `after` paragraph's source span starts at 1 (the `t` is counted).
  - `the_first_image_is_noted`: `first_image` is `Some("a.png")` for `<p><img src="a.png"/></p>`, `Some("c.jpg")` for the svg form, and `None` for `<p>x</p>`.
  - Rewrite `an_svg_image_is_noted` onto `first_image`.
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-blocks`. Expected: the new tests fail.
- [ ] **Step 3: Implement.** Adjust `noido-epub`'s cover rule (`x.svg_image || has_image(&x)` becomes `x.first_image.is_some()`) so the workspace builds.
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-blocks -p noido-epub`. Expected: pass.
- [ ] **Step 5: Commit**
  `jj commit -m "blocks: an image's size attributes, and svg images as images"`.

### Task 3: `noido-epub`, reading images and naming the cover's

**Files:**
- Modify: `crates/noido-epub/src/lib.rs`, `crates/noido-epub/Cargo.toml` (`noido-image`), `crates/noido-epub/tests/open.rs`, `fixtures/make_fixtures.py`, `fixtures/*.epub` (regenerated)

**Interfaces:**
- Produces:
  - `pub fn image_size(bytes: &[u8], path: &str) -> Option<(u32, u32)>`: the entry at the container path, refused when declared over `MAX_ENTRY_BYTES`. Its header is read with `noido_image::read_header`. `None` when it can't be read, a dimension is 0, or the pixel count is over `MAX_PIXELS`.
  - `pub fn image_data(bytes: &[u8], path: &str) -> Option<Vec<u8>>`: the whole entry, read no further than `MAX_ENTRY_BYTES`.
  - `PartKind::Cover { image: Option<String> }`: the cover's image as a container path. In order:
    1. the spine item itself, when it is an image;
    2. else the part's `first_image`, resolved against the part's path with `resolve`;
    3. else the manifest item with `properties="cover-image"`;
    4. else the item named by `<meta name="cover" content="ID"/>`.
  - `PartKind::Image`: an image straight in the spine, not first. It was `Unreadable`.
- Fixtures:
  - In `book.epub`, `cover.png` becomes 30 × 40, `(200, 0, 0)`.
  - New `images.epub` (EPUB 3, language `en`, title `Pictures`). Spine: `text/a.xhtml`, then `img/plate.png` straight in the spine. Every image is a solid colour:
    - `img/wide.png` 800 × 100, `(0, 0, 200)`;
    - `img/tall.png` 100 × 1000, `(0, 150, 0)`;
    - `img/small.png` 20 × 20, `(0, 120, 200)`;
    - `img/dot.png` 8 × 8, `(0, 0, 0)`;
    - `img/alpha.png` 2 × 2 RGBA `(200, 100, 50, 128)`;
    - `img/plate.png` 60 × 60, `(120, 60, 0)`;
    - `img/broken.png`: a PNG signature followed by junk;
    - `img/cut.png`: a valid IHDR of 10 × 10 with a corrupt IDAT (its header reads, its decode fails);
    - `img/huge.png`: an IHDR of 60000 × 60000 and no data.

    `a.xhtml` holds, in order:
    1. `<h1><img src="../img/wide.png" alt="Title Art"/></h1>`
    2. `<p>Before the figure.</p>`
    3. `<p><img src="../img/tall.png" alt=""/></p>`
    4. `<p><a href="#end"><img src="../img/small.png" alt="small"/></a></p>`
    5. `<p>A <img src="../img/dot.png" alt="dot"/> bullet in text.</p>`
    6. `<p>Chart: <img src="../img/wide.png" alt="chart"/> and its caption.</p>`
    7. `<p><img src="../img/small.png" width="40"/><img src="../img/small.png" width="40"/></p>`
    8. `<p><img src="../img/alpha.png" alt="alpha" width="200"/></p>`
    9. `<p><img src="../img/broken.png" alt="broken"/></p>`
    10. `<p><img src="../img/cut.png" alt="cut"/></p>`
    11. `<p><img src="../img/huge.png" alt="huge"/></p>`
    12. `<p><img src="../img/missing.png" alt="missing"/></p>`
    13. `<div><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="../img/small.png"/></svg></div>`
    14. `<p id="end">The end.</p>`

- [ ] **Step 1: Write the failing tests in `tests/open.rs`**
  - `the_cover_names_its_image`: `book.epub`'s part 0 is `Cover { image: Some("OEBPS/cover.png") }`.
  - `an_image_in_the_spine_is_an_image_part`: `images.epub`'s part 1 is `PartKind::Image` with path `img/plate.png` (in its directory).
  - `image_size_reads_only_the_header`: `image_size(bytes, ".../wide.png") == Some((800, 100))`. `huge.png`, `broken.png` and `missing.png` give `None`.
  - `image_data_reads_the_entry`: `image_data` of `small.png` starts with the PNG signature.
  - A cover fixture without any image (an existing marked cover) gives `Cover { image: None }`. Update the existing cover tests to the new shape.
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-epub`. Expected: fail.
- [ ] **Step 3: Implement**, update the fixture script, and run `python3 fixtures/make_fixtures.py`. Adjust `noido-typeset`'s match on `PartKind` so it builds: `Cover { .. }` still gives the title page, and `Image` gives the unreadable notice until Task 7.
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-epub -p noido-typeset`. Expected: pass.
- [ ] **Step 5: Commit**
  `jj commit -m "epub: read an image's size and bytes, and name the cover's image"`.

### Task 4: `noido-typeset`, figures laid out

**Files:**
- Create: `crates/noido-typeset/src/images.rs`
- Modify: `crates/noido-typeset/src/{book.rs,paragraphs.rs,lines.rs,engine.rs,lib.rs}`, `crates/noido-typeset/Cargo.toml` (`noido-image`, `image`, `fast_image_resize`), `crates/noido-typeset/tests/epub.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct ImageId(u32)` (Copy, Eq, Hash).
  - `pub(crate) struct Images`: `fn none() -> Self` for markdown, and `fn of_epub(bytes: PdfBytes) -> Self`.
    - `fn size(&mut self, path: &str) -> Option<(ImageId, (u32, u32))>`, memoised per path. `None` for markdown, or when `image_size` gives `None`.
    - `fn pixels(&self, id: ImageId, width: u32, height: u32) -> Option<Arc<image::RgbaImage>>` comes in Task 5.
  - `fn natural(px: (u32, u32), width: Option<u32>, height: Option<u32>) -> Option<(f32, f32)>`: in points, with the attribute rule. `None` if a side is 0.
  - `fn fit(size: (f32, f32), max: (f32, f32), enlarge: bool) -> (f32, f32)`: aspect kept.
  - `paragraphs(tree, resolve: &mut dyn FnMut(&str) -> Option<(ImageId, (f32, f32))>)`: the closure maps an image's `src` as written to its id and natural size in points. For EPUB it resolves `src` against the part's path, then calls `Images::size` and `natural`; for markdown it always gives `None`.
  - `Role::Figure { image: ImageId, width: f32, height: f32, heading: Option<u8> }`. The `Para` text is the image's single object character, carrying its `Style::Image` and any wrapping `Style::Link`.
  - `SetGlyph::image: Option<(ImageId, f32)>`: the image and its drawn height. A figure's line has one glyph with `x = (MEASURE - width) / 2.0`, `advance = width`, `text` = the stand-in text, and `drawn` = the object character's range. The line's `height` and `ascent` are both the figure's height.
  - `typeset_epub(epub: &Epub, bytes: PdfBytes) -> Book`. `TypesetEngine::open` passes its bytes; the tests pass `PdfBytes::from(bytes)`.
  - `Book::images: Images`, kept for drawing.

What `paragraphs` does:
- A body paragraph whose only drawn content is one resolvable image (whitespace aside) becomes a `Figure`.
- So does a heading's, with `heading: Some(level)` and the heading's `new_page`.
- A resolvable image of natural height over `2.0 * LEADING` inside a longer paragraph splits it into:
  1. the text before (if not just whitespace), as the paragraph's role;
  2. the figure;
  3. the text after (if not just whitespace), unindented.

  Each piece's `Text` is the original's drawn sub-range, with its segments and styles clipped and kept, so source spans are untouched. A link style that spans the image goes with each piece it covers.
- An image that doesn't resolve stays in its paragraph as E1's stand-in.

`flow` for a figure gives one line of its height, `before` and `after` of `LEADING / 2.0`, `heading` set when `heading.is_some()`, and `new_page` from the heading. `glyphs_of` gives an image glyph the quad of its rectangle: `x0 .. x0 + advance`, `top .. top + height`.

- [ ] **Step 1: Write the failing tests**
  - `images.rs` unit tests:
    - `natural_is_three_quarters_of_a_pixel`: `natural((800, 100), None, None) == Some((600.0, 75.0))`.
    - `one_attribute_keeps_the_aspect`: `natural((20, 20), Some(40), None) == Some((30.0, 30.0))`.
    - `a_zero_side_has_no_size`: `natural((0, 5), None, None) == None`.
    - `fit_shrinks_and_never_enlarges`: `fit((600.0, 75.0), (270.0, 434.0), false) == (270.0, 33.75)` and `fit((15.0, 15.0), (270.0, 434.0), false) == (15.0, 15.0)`.
  - `tests/epub.rs` on `images.epub`:
    - `a_lone_image_is_a_centred_figure`: the glyph whose text is `[image]` (`tall.png`) has a quad of height 434 and width 43.4, centred: `x0 = LEFT + (270 - 43.4) / 2`, within 0.01.
    - `a_figure_that_does_not_fit_starts_the_next_page`: `tall.png`'s glyph is on a later page than "Before the figure.", and at the text block's top (`TOP` + 0, since space before is dropped).
    - `a_heading_image_is_a_figure_that_starts_a_page`: "Title Art" is the first glyph on part 0's first page, 270 wide.
    - `a_large_image_in_text_splits_its_paragraph`: "Chart:" and "and its caption." are on lines above and below the "chart" glyph, and the "chart" glyph is 270 wide.
    - `side_by_side_images_stack`: the two 40-px images (30 pt figures) are on different lines, one above the other.
    - `unreadable_images_keep_their_stand_in`: the page text contains "broken", "huge" and "missing" as glyph text, each at text height (quad height < 20).
    - `locations_do_not_move`: the last run of part 0 (`locations().last_line_of(0)`) ends at `Xhtml::chars` for `a.xhtml`. The `#end` link from the small figure lands on the line holding "The end.".
    - `a_linked_figure_is_a_link_over_the_image`: a link whose area contains the centre of `small.png`'s glyph.
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-typeset`. Expected: the new tests fail.
- [ ] **Step 3: Implement** `images.rs` (sizes only), figure paragraphs, splitting, figure lines, flows, glyph quads and the wiring above.
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-typeset`. Expected: pass; the existing tests pass unchanged except where `typeset_epub` now takes the bytes.
- [ ] **Step 5: Commit**
  `jj commit -m "typeset: lay out an EPUB's images as figures"`.

### Task 5: `noido-typeset`, drawing images

**Files:**
- Modify: `crates/noido-typeset/src/{images.rs,draw.rs,book.rs}`

**Interfaces:**
- Consumes: `noido_epub::image_data`, `noido_image::{decode, resized}`.
- Produces:
  - `PageInk` gains `images: Vec<ImageInk { id: ImageId, x: f32, y: f32, width: f32, height: f32 }>`, in page points, added by `add_line` for an image glyph.
  - `Images::pixels(&self, id, width, height) -> Option<Arc<image::RgbaImage>>`. The decoded image is cached, then the resized one, both in one LRU of 64 MB counted in pixel bytes. It is `None` when the decode fails, and that failure is remembered, so a broken image isn't decoded again.
  - `#[cfg(test)] decodes: AtomicUsize` counts decodes.
- In `render_tile`:
  - Each image's rectangle in page pixels is `x0 = (x * scale).round()` and `x1 = ((x + width) * scale).round()` (likewise for y). It's resized to `x1 - x0` by `y1 - y0` and blitted at `x0 - dx`, `y0 - dy` with `draw_pixmap` (`PixmapPaint` defaults, `FilterQuality::Nearest`, since the pixels are already at size).
  - Images are drawn under the text and bars.
  - When `pixels` gives `None`, the rectangle gets a `HAIRLINE` grey (`#999999`) frame.
  - An image rectangle over `MAX_PIXELS` at this scale is drawn as the frame.

- [ ] **Step 1: Write the failing tests in `draw.rs` (with `images.epub` through `typeset_epub`)**
  - `a_figure_is_drawn_in_its_rectangle`: on the whole page holding the `small.png` figure, at scale 2, the pixel at the glyph rectangle's centre is `(0, 120, 200)` (±2), and a pixel 3 pt outside it is white.
  - `a_tile_through_an_image_matches_the_page`: tiles of 37 × 41 px across `tall.png`'s page at scale 1.7, compared with `whole` byte for byte.
  - `a_transparent_image_draws_over_white`: the centre of `alpha.png` is `(228, 177, 152)` (±2), which is `(200, 100, 50)` at half alpha over white.
  - `an_image_is_decoded_once`: drawing its page twice, and from two threads at once, leaves `decodes == 1` for that image.
  - `a_broken_image_draws_a_frame`: `cut.png` (laid out as a 7.5 pt figure, since its header reads) has grey pixels on its rectangle's edge and a white centre.
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-typeset draw`. Expected: fail.
- [ ] **Step 3: Implement** the ink, the cache and the drawing.
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-typeset`. Expected: pass.
- [ ] **Step 5: Commit**
  `jj commit -m "typeset: draw a book's images, decoded once and kept"`.

### Task 6: `noido-typeset`, inline images

**Files:**
- Modify: `crates/noido-typeset/src/{lines.rs,paragraphs.rs}`

**Interfaces:**
- Consumes: `resolve` from Task 4, `SetGlyph::image`.
- What changes:
  - `paragraphs` gives a resolvable image that stays in its paragraph to the shaper through `Style::Image` plus a new `Para::images: Vec<(Range<usize>, ImageId, (f32, f32))>`: the drawn range, the id and the natural size in points.
  - In `Shape`, such an image is shaped as one U+FFFC, which is kept out of the stand-in list.
  - Its atom becomes one image glyph sized by `fit(natural, (MEASURE, ascent), false)`, where `ascent` is the ascent `Metrics` gives for the run's font and size; `advance` is the fitted width.
  - The glyph's `rise` is 0, its quad runs from the baseline minus the image's height to the baseline, and its `text` is the stand-in text.

- [ ] **Step 1: Write the failing tests**
  - `an_inline_image_sits_on_the_baseline` (tests/epub.rs): the "dot" glyph is on the same line as "bullet". Its quad's bottom is that line's baseline (±0.01), and its height is ≤ the body font's ascent at 11 pt and equals 6 (8 px at 0.75 pt).
  - `an_inline_image_takes_its_width_in_the_line`: the x of the glyph after the "dot" glyph's following space is greater than the dot's `x0 + 6`.
  - `markdown_images_keep_their_stand_in` (unit test in `book.rs`): `typeset("![a cat](cat.png)\n")` draws "a cat" in italic glyphs and has no image glyph.
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-typeset`. Expected: the first two fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests**
  Run: `cargo test -p noido-typeset`. Expected: pass.
- [ ] **Step 5: Commit**
  `jj commit -m "typeset: draw small images in the line"`.

### Task 7: the cover, image parts and the layout key

**Files:**
- Modify: `crates/noido-typeset/src/{book.rs,house.rs,links.rs}`, `crates/noido-typeset/tests/epub.rs`, `crates/frontends/gui-egui` (only if a test there pins the old layout key)

**Interfaces:**
- Consumes: `PartKind::Cover { image }`, `PartKind::Image`, `Images::size`, `fit`.
- Produces:
  - `Content::Cover(ImageId, (f32, f32))`: one page holding one image glyph fitted to `PAPER` (`fit(natural, (PAPER.width, PAPER.height), true)`), centred on the paper. It has no source, and a zero-length run at character 0 of the part, as the title page has. If the image doesn't resolve, the part is `Content::Drawn(title_page(epub))`, as in E1.
  - `PartKind::Image` is a part holding one figure paragraph (`Role::Figure`, `heading: None`). If its image doesn't resolve, it is the unreadable notice.
  - `LAYOUT_KEY = "b-format 11/14 images"`.

- [ ] **Step 1: Write the failing tests in `tests/epub.rs`**
  - Replace `the_cover_is_a_title_page` with `the_cover_is_its_image`: `book.epub`'s page 0 has exactly one glyph, the image glyph. Its quad is 365.67 × 487.56 (30 × 40 px is 22.5 × 30 pt, bound by the width), centred on the paper: top `(561.26 - 487.56) / 2`, within 0.01. Drawing page 0 at scale 1 gives red `(200, 0, 0)` at the page centre.
  - `a_cover_without_an_image_is_a_title_page`: the existing marked-cover fixture still gives "title\nauthor" text.
  - `an_image_part_is_a_figure`: `images.epub`'s last page holds one image glyph (plate.png, 45 × 45 pt, centred on the measure), and `locations().page_start` for that part answers.
  - `the_layout_key_names_images`: `TypesetEngine::open(...).layout_key() == Some("b-format 11/14 images")` (in `engine.rs`).
- [ ] **Step 2: Run them**
  Run: `cargo test -p noido-typeset`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all tests**
  Run: `just check`. Expected: clean. Then `just build`.
- [ ] **Step 5: Commit**
  `jj commit -m "typeset: draw the cover as its image, and images in the spine as pages"`.
