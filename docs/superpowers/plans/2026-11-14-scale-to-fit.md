# Plan: Scale to fit

Spec: `docs/superpowers/specs/2026-11-14-scale-to-fit-design.md`

## Global constraints

- TDD: write the failing test first, run it and see it fail, then
  implement. `just lint` (clippy) must be clean, `just fmt` applied,
  `just test` green before each commit.
- jj, colocated with git. Commit with `jj commit -m "<message>"`. Never
  move the `main` bookmark. Message form `prefix: what changed, in plain
  words`, title ≤ 72 characters; prefix is the crate's short name
  (`core`, `render`, `state`, `egui`).
- Tests use only the generated fixtures in `fixtures/` or synthetic
  data; nothing from `../testfiles/`.
- Never send synthetic input to the desktop; GUI behaviour is tested in
  the headless `Harness` in `crates/frontends/gui-egui/src/app.rs`.
- The reference box: median width and median height, each taken on its
  own over the pages' sizes *as shown before scaling* (the PDF's
  `/Rotate` and noido's rotation applied). The median of `n` sorted
  values is the one at index `n / 2` (integer division): for odd `n` the
  middle, for even `n` the larger of the two middle values.
- A page's factor is `min(1, box_w / w, box_h / h)`. Pages only shrink.
  A page with a zero width or height gets factor 1.
- Order of transforms on a page: rotation first, then the scale.
- The toggle is saved per document as `scale_to_fit` on
  `PageTransforms`, not on `ViewState`; it must not become a default for
  new documents. Off unless the state file says otherwise.

## Task 1: core — the scale on a page transform, and the fit scales

Crate `noido-core`, file `crates/noido-core/src/transforms.rs` (and
re-exports in `lib.rs` as needed).

- `PageTransform` gains `scale: f32`; its `Default` has rotation `R0`
  and scale `1.0` (write `Default` by hand). It can no longer derive
  `Eq` (f32); keep `PartialEq`, fix whatever needed `Eq`.
  `is_identity` stays "equals the default".
- Methods on `PageTransform`, each taking the engine's (unrotated,
  unscaled) page size `engine: Size`:
  - `size(self, engine) -> Size`: `rotation.size(engine)`, both sides
    times `scale`.
  - `point(self, p: Point, engine) -> Point`: `rotation.point(p, engine)`
    times `scale`.
  - `rect(self, r: Rect, engine) -> Rect` and `quad(self, q: Quad,
    engine) -> Quad` likewise (use `Rotation::rect` / `Rotation::quad`,
    then multiply every coordinate and extent by `scale`).
  - `remap(p: Point, from: PageTransform, to: PageTransform, engine:
    Size) -> Point` (associated fn): a point on the page shown with
    `from` to the same spot shown with `to`: divide by `from.scale`,
    `Rotation::remap(…, from.rotation, to.rotation, engine)`, multiply
    by `to.scale`.
- `pub fn fit_scales(sizes: &[Size]) -> Vec<f32>`: the reference box and
  per-page factors as in Global constraints. Empty in, empty out.
- `PageTransforms` keeps its per-page map and gains a document-wide
  `scale_to_fit: bool` (default false), with `scale_to_fit(&self) ->
  bool` and `set_scale_to_fit(&mut self, on: bool)`. Equality includes
  it.
- `PageTransforms::refit(&mut self, engine_sizes: &[Size])`: sets every
  page's `scale` (keeping its rotation): when `scale_to_fit` is on, to
  `fit_scales` of the rotated sizes (`rotation.size(engine)` per page);
  when off, to `1.0`. Pages that become identity drop out of the map, as
  `set` already does. `set_scale_to_fit` does not refit by itself; the
  caller refits with the sizes.
- Tests (in the module): `fit_scales` for an odd count; an even count
  (two 600×800 and two 800×600 → all 1.0); one 1200×1600 page among
  four 600×800 (→ 0.5 for it, 1.0 for the rest); a landscape 842×595
  among portrait 595×842 pages (→ 595/842); a zero-size page (→ 1.0);
  empty. `refit` with a page rotated 90° counting by its turned shape;
  `refit` with the toggle off resets scales to 1 and drops identities;
  `PageTransform::size`/`point`/`remap` for rotation + scale together
  (a 90°, 0.5 transform on a 600×800 page: size 400×300; remap from
  {R0, 1} to {R90, 0.5} and back is the identity on a point).

Commit: `core: give a page transform a scale, and fit scales to the median page`

## Task 2: render — the wrapper applies the scale

Crate `noido-render`: `src/transformed.rs`, `src/lib.rs`, tests in
`tests/transformed.rs`.

- In `Transformed` (all `Document` methods), use the page's full
  `PageTransform` (rotation, then scale):
  - `page_size`: `transform.size(inner size)`.
  - `render_tile(page, scale, rect)`: the shown page at `scale` is the
    rotated page at `scale × transform.scale`; render through the
    existing rotation path with that product. Return early to the inner
    document only when the transform is the identity.
  - `render_page`: unchanged in meaning (it is given pixel dimensions);
    keep it working for scaled pages.
  - `text`: glyph `origin` and `quad` through the transform; glyph
    `size` (font size, points) times the scale.
  - `links`: areas through `transform.rect`; internal destinations
    through the target page's transform (x and y each times its scale
    after the turn; a missing coordinate stays missing).
  - `named_destination` and `outline`: as now, but through the full
    transform.
- `Transforms` (the shared `Arc<RwLock<…>>`) gains
  `scale_to_fit() -> bool`, `set_scale_to_fit(bool)` and
  `refit(engine_sizes: &[Size])`, each forwarding to `PageTransforms`.
  `Transformed` gains `engine_sizes(&self) -> Result<Vec<Size>,
  EngineError>`: the inner document's page sizes.
- In `lib.rs` `open(…)`: after `open_transformed`, when
  `transforms.scale_to_fit()`, call `transforms.refit(engine sizes)`
  before reading `page_sizes`, so a document opened (or reloaded) with
  the toggle on starts with fitted scales.
- Tests: page size, a glyph's quad/origin/size, a link area and an
  internal destination, for a scaled page and for a page rotated 90°
  and scaled 0.5; a rendered tile of a scaled page has the size asked
  for and matches the unscaled page rendered at the product scale;
  opening with `scale_to_fit` on gives fitted sizes in
  `DocumentInfo::page_sizes` (use a fixture with pages of two sizes;
  if no fixture has one, add a small one to `fixtures/make_fixtures.py`
  — e.g. `mixed_sizes.pdf`: three 595×842 pages and one 1190×1684 —
  regenerate, and commit it with this task).

Commit: `render: scale pages in the transform wrapper`

## Task 3: state — save the toggle

Crate `noido-state`, `src/lib.rs`.

- The document's TOML gets a top-level `scale_to_fit` boolean,
  `#[serde(default)]`, written only when true
  (`skip_serializing_if`). Loaded into `DocState.transforms` via
  `set_scale_to_fit`. Per-page records stay rotation-only: a page whose
  only non-identity part is its scale is not written; a rotated page is
  written with its rotation only.
- Tests: round trip with the toggle on; a file without the key loads as
  off; a file written with it off has no `scale_to_fit` key; a
  transforms map with a scaled-only page writes no `[[pages]]` entry.

Commit: `state: save whether a document scales its pages to fit`

## Task 4: core — the action and the menu entry

Crate `noido-core`: `keymap.rs` (the `Action` enum), `menu.rs`.

- `Action::ToggleScaleToFit`, no key binding. `Check::ScaleToFit`.
- Menu: in the layout section after "Gapless mode", a check entry
  `"Scale to fit"` with `Check::ScaleToFit`. Update `menu_sections` and
  any other menu tests.
- `View::apply` (or wherever actions reach the view) must leave this
  action to the frontend, as it does `ToggleSidebar`.
- If the macOS menu (`crates/noido-macos`) or titlebar mirror the menu
  by enumerating `Check`, make the code compile; wiring the check's value
  is Task 5.

Commit: `core: add a Scale to fit menu entry`

## Task 5: egui — toggling, refitting, and the check

Crate `frontends/gui-egui`: `app.rs`, `titlebar.rs` (and the macOS
menu's check, wherever `Check::Gapless` is read).

- Generalise `turn_page` into one routine that moves the app from the
  current `PageTransforms` to new ones: for every page whose transform
  changed, the layout gets `transform.size(engine size)` (engine size
  recovered from the shown size and the old transform), view anchor and
  history points are mapped with `PageTransform::remap`, and that page's
  textures, thumbnail, text, search hits and pagelet are dropped as
  `turn_page` does now. With many pages changing at once (a toggle on a
  long document) the view must re-arrange once, not once per page:
  add a `View` method in `noido-core` that sets several page sizes with
  one remap closure taking the page, anchored as `set_page_size` is
  (test it in `view.rs` first). `set_rotation` goes through the same
  routine; while scale to fit is on, a rotation refits all pages.
- `Action::ToggleScaleToFit` flips the toggle, refits with the engine
  sizes, and applies the result through that routine. It is saved with
  the document's state.
- The titlebar menu (and the macOS menu, if it shows checks) shows the
  check from the toggle.
- Harness tests (fake engine with mixed page sizes, e.g. pages 0–3
  595×842 and page 4 1190×1684): the action toggles the check; with it
  on, page 4's layout size is 595×842 and fit-width gives page 0 the
  same width as before while page 4 no longer is wider; the view stays
  on its page across the toggle (anchor on page 4 and on page 2);
  toggling off restores the original sizes; rotating page 4 while on
  refits; the toggle survives saving and reopening the document state;
  a highlight on page 4 draws on the same text rect, scaled, after the
  toggle.

Commit: `egui: toggle scale to fit from the menu`
