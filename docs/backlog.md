# Backlog

Known issues and deferred work, roughly in priority order. Remove an entry
when it is fixed.

## Next up

(Nothing queued.)

## Known issues

- **Try the desktop integration by hand (user).** Run `noido --install`
  from the binary you want launched, then in Nautilus: "Open With" on a
  PDF lists noido and opens it; several selected PDFs open one window
  each; the window gets focus on GNOME Wayland (egui-winit 0.37 passes
  `XDG_ACTIVATION_TOKEN` on); the dock shows "noido" with the document
  icon. `noido --uninstall` removes it again.

- **Try links by hand (user).** In the network analysis paper: contents
  entries and footnote markers jump and flash the target line; the
  networkx URL on printed page 14 opens the browser; a drag starting on a
  link pans; in paged mode (`c`) a link shows the target's spread. Vaesen's
  contents links go to whole pages, so the flash lands on the page's top
  line (the running head), as agreed.

- **Try history by hand (user).** In the network analysis paper: follow a
  footnote or contents link, read on a few pages, then Alt+← (and the
  mouse's back button) returns to where you were; Alt+→ (forward button)
  goes to where you read on to, not where the link landed. Close and
  reopen the file: Back still works. Search for a word, step with Ctrl+G,
  then one Back leaves the search. Back and Forward are in the menu.

- **Try document info by hand (user).** Ctrl+I (or "Document info…" in
  the menu) on the network analysis paper: title, author, LuaTeX producer,
  dates with +02:00, Source Serif 4 / Caption / Subhead as three families.
  On Vaesen: Intervogue with seven styles, Minion Pro, Mrs Eaves OT.
  Hovering a family shows the raw names; names can be selected and copied;
  a long list scrolls; Esc and Close close it.

- **Try the LaTeX loop by hand (user).** Run `latexmk -pvc` (or repeated
  `pdflatex`) with noido open: it should follow each compile, keep the
  place, and survive a failing build.

- **Zero-page PDF opens a blank window.** hayro also reports 0 pages for some
  malformed page trees. Treat it as an error ("no pages") before the window
  opens. (`crates/frontends/gui-egui/src/main.rs`)
- **Failed page looks like a loading page.** Both draw as a white rectangle.
  Draw failed pages differently, e.g. grey fill or an outline with a message.
  (`crates/frontends/gui-egui/src/app.rs`)
- **Raw error text for unreadable files.** A non-PDF shows
  `not a readable PDF: Invalid` (Debug output of hayro's `LoadPdfError`). Map
  errors to readable messages, with a distinct one for encrypted PDFs.
  (`crates/engine-hayro/src/lib.rs`)
- **Render size cap is hardcoded.** `MAX_RENDER_SIDE` (8192) matches
  egui-wgpu's texture limit today; use
  `min(MAX_RENDER_SIDE, ctx.input(|i| i.max_texture_side))` so another backend
  cannot exceed its limit. (`crates/frontends/gui-egui/src/app.rs`)

- **Minimizing can lose the position.** Some platforms report a 0×0 viewport
  while minimized; `View::set_viewport_size(0, 0)` refits to 0.1 and anchors
  at (0, 0). Ignore non-positive sizes after the first real one.
  (`crates/noido-core/src/view.rs`)
- **Kinetic scroll velocity survives a zoom.** A coasting touch drag keeps
  going in the new content coordinates after a zoom. Low impact.

- **More egui-level tests for view modes.** The headless harness doesn't yet
  cover PageUp flipping into a page of a different height, or a mode toggle in
  fit-page mode. Needs a `BlankDocument` with configurable page sizes (use a
  static `Mutex`, not `thread_local`: the render thread opens the document).
- **Prefetched pages in paged fit-page mode render twice** when the next
  spread fits at a different scale: they are rendered at the current spread's
  scale, then again after the flip. Wasted work only.

## View state storage (review minors, 2026-10-25)

- A failed or interrupted state write leaves a `<hash>.tmp-<pid>` file in
  `docs/` that prune never removes; no fsync before the rename.
- Window files without a state file (noido killed within 2 s of opening a new
  document) are never pruned.
- Two instances on one document can both write its window file (eframe's
  write isn't atomic); a broken file falls back to the default window.
- Path-fallback ties within the same second are picked arbitrarily.

## Title bar and sidebar (review minors, 2026-10-25)

- The comment on calling `frame::resize_edges` first is misleading: order
  doesn't keep presses from widgets underneath; the page view's 8 pt margin
  does.
- A double-click in the top 5 points both starts resizes and maximises.
- After a mouse click an icon button keeps focus, so Space or Enter presses
  it again.

## Reload and one window per file (review minors, 2026-10-25)

- A second launch while the first is still starting (reading the file,
  loading state, parsing) waits 1 s, gets no answer and opens a second
  window: the first only serves after `Renderer::start`. Serve right after
  `claim`.
- A write between reading the file and `Watcher::start` is missed until
  the next change. Arm the debounce once at start (`open_new` skips an
  unchanged hash anyway).
- Watcher overflow or rescan events carry no paths, so they're dropped;
  treat `event.need_rescan()` as a change.
- Without `XDG_RUNTIME_DIR` the socket is `/tmp/noido-<hash>.sock`, which
  another local user can predict and squat (every launch on that file then
  exits silently). Use a per-user 0700 directory and check its owner.
- `prune` trusts the window name in a state file: a hand-edited
  `window = "../../x"` makes it delete `docs/../../x.window.ron`. Accept only
  64 hex characters, as `files_with` does.
## Sidebar thumbnails (review minors, 2026-10-25)

- A fling of the page view keeps gliding after a thumbnail click (egui keeps
  the scroll velocity when the offset is set), and the gliding then clears
  the clicked page. Reset the page view's scroll velocity after
  `go_to_page`. Keys during a fling already behave this way.
- A vertical window resize, or a reload to fewer pages, can re-clamp the
  sidebar's offset and count as browsing, moving the low-res window. Count
  a height change as a resize too.
- After a reload a kept sharp thumbnail is stretched to a page whose aspect
  changed until its new render arrives, and for good if the new sidebar
  renderer fails to open. Drop images whose page size changed.
- `reload_swaps_sidebar_renderer` is weak: only pages 0–2 are visible, so
  "sharp pages below 5" holds anyway, and it doesn't check the place is
  kept. Scroll the sidebar to pages 6–9 first.
## Deferred features

- Vim key layer (zathura-style), with key sequences and counts.
- **Our window frame on Windows, checked on Windows:** snapping, resize
  edges, the window buttons, Alt+F4.
- **GNOME's button layout:** GNOME can put the window buttons on the left
  (`org.gnome.desktop.wm.preferences button-layout`); we always put them on
  the right.
- **The system's accent colour.** Selection highlights (the sidebar's
  current-page frame and number pill) use egui's built-in colours (dark:
  RGB 0, 92, 128; light: 144, 209, 255). Follow the system accent instead,
  with a fixed noido colour as the fallback: on Linux the desktop portal's
  `org.freedesktop.appearance` `accent-color` (GNOME 47+, KDE; e.g. via
  `ashpd`); macOS and Windows have their own APIs.
- **Proper window activation on Wayland.** winit's focus call does
  nothing on Wayland, and it can't activate an existing window with a
  token from another process. A second launch should pass its
  `XDG_ACTIVATION_TOKEN` to the running noido, which activates with
  `xdg_activation_v1` itself (bound on winit's connection via the raw
  window handles). Only helps when the launcher supplies a token (file
  managers and the desktop do; terminals and editors often don't).
  Today the running window only asks for attention.
- **Releases with dist:** versioning, a hand-written changelog, released
  from GitHub's Actions page after a dry run. After the macOS port. See
  `docs/investigations/2026-10-26-versioning-and-releases.md`.
- **Bringing the window forward on Windows, checked on Windows:** the
  foreground rules may need `AllowSetForegroundWindow` from the second
  launch. The handover itself no longer relies on pipe timeouts (unsupported
  there) but has never run on Windows.
- **Notes in a right sidebar.** The left sidebar holds search results,
  thumbnails and the outline; the right one holds the reader's notes:
  bookmarks, freeform notes, lasso captures (a region of the page) and text
  selections. Later: combine these, e.g. a selection or lasso capture with a
  note attached, or a bookmark with a comment.
- **ActualText in hayro.** hayro reads `/Span <</ActualText …>>` but doesn't
  pass it to the device, so books typeset with ligature fonts lose letters in
  extracted text (Vaesen: ~3.5 % of words; no other test file uses it). See
  `docs/investigations/2026-10-25-text-extraction.md`. Upstream issue or PR,
  or wait for hayro; the text API should let a glyph run carry replacement
  text so this slots in later.
- **Link preview.** Something on hovering an internal link: the target
  region, a footnote's text, a citation's entry? Unclear what yet.
- **Highlight all links.** A key or button that shows every link on the
  visible pages. Think about whether it's wanted; overlaps with drawing
  borders below.
- **Link borders as the spec says.** Follow `/Border` (default `[0 0 1]`,
  a 1 pt box), `/BS`, `/C`, `/H` (click look) and `/AP` appearance streams,
  maybe behind a setting, since hyperref's default red boxes are often
  unwanted.
- **More link kinds.** Links to another file (GoToR), named actions
  (NextPage, PrevPage, FirstPage, LastPage), Launch. Today they do nothing.

- **Font names from the font files.** Read the family name embedded in each
  font program (Type 1 `FamilyName`, CFF and TrueType `name` tables) for
  the document info page, falling back to the name rules in
  `noido-core/src/fonts.rs`. Would fix run-together names
  (`PlayfairDisplay`), non-ASCII names and TeX fonts the table doesn't
  know; needs three font formats parsed (see what hayro exposes); nothing
  for fonts that aren't embedded, and writers put junk there.

## Far future

- **Our own OCR** for scanned PDFs without a usable text layer, so search,
  selection and notes work on them too. Example: `vehicles-…-annas-archive.pdf`
  in testfiles is 600 dpi page scans; only the cover has an (invisible, poor)
  OCR layer. Options: run an OCR engine (e.g. Tesseract) in the background and
  keep the result in a sidecar, or suggest `ocrmypdf` to the user.

## Ideas and thoughts

- **Dark paper with light text.** Tinting (the paper colour setting) can
  only darken, so a dark page needs the pixels remapped: on the render
  thread after hayro rasterises (cheap next to rasterising), or in a
  shader. Plain inversion turns photos and colour figures into negatives;
  zathura's "keep hue" flips only lightness, which helps but still looks
  odd on photos. Decide what to do with images first.
- **Paper colour per document?** It is global for now; not yet settled
  whether a document should remember its own (a scan may want white while
  books want sepia).
- **Rendered previews of search hits.** Instead of (or next to) a line of
  text, each result in the sidebar shows a small rendering of the page region
  around the match, with the match highlighted, as macOS Preview can. Helps
  where extracted text reads poorly: formulas, figures, OCR'd scans. Would need
  a low-priority render queue for the regions and a cache, so a long result
  list doesn't hold up the page view or the thumbnails.
- **Search across punctuation between words.** "divinity original" finds
  nothing in "Divinity: Original Sin"; the words are adjacent, so it
  should. Proposed rule (not yet agreed): a space in the query matches any
  gap of whitespace and Unicode punctuation (categories P*) in the text;
  punctuation typed in the query must be there, as with accents ("gödel"
  doesn't find "godel"). Within a word nothing changes ("dont" doesn't find
  "don't"). Symbols (S*: `+ = → © •`) stay significant: "x y" shouldn't
  find "x + y". The highlight covers the punctuation. Preview doesn't do
  this either.
- **Fuzzy search, fzf-style?** Worth investigating, but unclear whether
  it can be made intuitive in a document viewer: scattered matches across
  a page are hard to highlight and to rank, and a reader usually searches
  for words they expect to be there.

## Text layer (review minors, 2026-10-25)

- **Context lines show raw text:** "spe- cifically" across a line-end
  hyphen, double spaces from OCR layers. Collapse whitespace and join
  hyphenated breaks in `context()`, keeping the bold range right.
- **An unjoined mark lands after an inserted space** ("a ¨b" rather than
  "a¨ b"). Cosmetic. (`crates/noido-text/src/page.rs`)
## Search UI (review minors, 2026-10-25)

- **A reload during the typing delay never jumps:** the re-run uses the
  no-jump mode even when the text differs from the last query searched.
  Re-run with a jump then. (`crates/frontends/gui-egui/src/search.rs`)
- **Enter, Mod+G or F3 during the typing delay step through the old query's
  hits,** then the new query jumps. Start the pending search at once on a
  step.
- **The first letter typed shows thumbnails for 0.2 s** before the results
  replace them. A brief flicker; show the results list as soon as there is
  text.
- **Hits on multi-column pages** are ordered by text but compared by their
  top for "next from the view"; the two can disagree.
- **Paged fit modes render the neighbouring hits' pages at the current
  spread's scale.** With mixed page sizes the page is rendered again at its
  own fit scale after the step, and the page left behind again at the new
  one's: two extra renders per step. The prefetch window does the same.
- **The prefetched hit targets shift while a search is still running**
  (until a hit below the view arrives); a little wasted render work.
- **`results_are_cards_of_one_height` is weak:** `> 4 × body` holds mostly
  by the padding; assert the height from the font metrics, and add a test
  that a match near the end of a long context is inside the shown rows.

## Arrow keys (review minors, 2026-10-26)

- **Holding an arrow in fit-page paged mode flips ~30 spreads a second**, as
  fast as key repeat; check that renders and prefetch don't churn.
## Odd pages left (review minors, 2026-10-26)

- **An empty document with page 1 alone has one empty spread**
  (`(0 + 1).div_ceil(2)`): a 24 pt content height instead of 0. Return 0
  spreads for no pages.
- **A wide lone page counts double in `widest_spread`:** a landscape cover
  with page 1 alone (or a lone last page) lowers fit width for the whole
  document. Count a lone page as its width + gap + a neighbour's width, or
  leave it out.
- **Pages of different widths don't line up at the seam** when one is alone
  (same as the lone last page today).
- **Test gaps:** flipping / Page Up/Down / go to page reaching the lone
  first spread; `doc_point` in the empty left half; mixed sizes; fit-page
  scale of the lone spread; the sidebar highlight with page 1 alone.

## Desktop install (review minors, 2026-10-26)

- **`TryExec=`**: the entry names the resolved binary (e.g.
  `target/release/noido`); after a `cargo clean` it stays in "Open With"
  but fails. A `TryExec=` with the same path makes desktops hide it.
- `--uninstall` removes any `noido.desktop`, also one noido didn't write;
  a marker key (`X-Noido-Installed=true`) could tell them apart.
- Symlinks: a dangling `noido.desktop` link counts as "not installed";
  writing follows a link into its target (dotfiles managers).
- Control characters (newline, tab) in the binary's path aren't escaped,
  making an invalid file; escape or refuse them.
- The file is written in place, not via a temp file and rename.
- A plan that goes stale while the prompt waits: a file removed meanwhile
  makes `Remove` fail; treat not-found as done.
- Non-UTF-8 input at the prompt is an error rather than a no.
- Tests: the glue (`proceed`, `Places::here`, the `XDG_DATA_HOME` rules)
  is untested; quoting tests check substrings rather than parsing back;
  temp dirs aren't cleaned up.

## Settings (review minors, 2026-10-26)

- Test module: the `/// Puts the cursor in the search field.` doc comment
  now sits above `open_settings`; it belongs on `focus_search`.
- The fixed-zoom `DragValue` saves the file on every drag frame and every
  keystroke (typing 150 saves 10, 15, 150). Save on drag stop / focus loss.
- A field of the wrong type (`zoom_percent = 150.0`, `dual = "yes"`) fails
  the whole load, so every field falls back and the next change overwrites
  the file. Could fall back field by field via `toml::Value`.
- A failed settings load (bad TOML, newer `version`) is only on stderr;
  the page could say its values are defaults, not the file's. A newer
  version file is quietly downgraded on the next change.
- Ctrl+Q does nothing while the settings page is open (`handle_input`
  returns early); Quit could be let through.
- `view_keys_do_nothing_while_open` doesn't hover the pages before the
  wheel event, so it doesn't prove egui's ScrollArea is blocked by the modal.
- Sidebar row: "Set from current view" copies a sidebar that search opened
  by itself (`sidebar_opened_by_search`), saving `sidebar = true` though
  the reader never chose it. Could use `open && !opened_by_search`.
- Sidebar row: no settings test can fail on a file without `sidebar`
  (its default is also `bool::default()`); `new_document_opens_with_sidebar_setting`
  checks the flag, not that the sidebar is drawn.

## Hand cursor (review minors, 2026-10-26)

- `no_hand_under_settings_page` asserts `!= Grab`; `== Default` would also
  catch a stray Grabbing.
- No test for a drag that starts outside the pages (sidebar scroll bar,
  search text selection) and moves over them: it should show no hand.

## Links (review minors, 2026-10-26)

- **A click that closes a menu also follows a link** under the pointer, and
  the hand and outline show while a menu is open. Skip link hover and
  click while `Popup::is_any_open`. (`gui-egui/src/app.rs`, `pages`)
- **A click that stops a fling follows the link.** Clicking to stop kinetic
  scrolling counts as a click on whatever link is under it.
- **A click from the frame before a reload is followed in the new
  version.** Clear `clicked_link` and `hovered_link` in `reload()`.
- **Non-finite coordinates.** Huge numbers in `/XYZ` or `/Rect` become `inf`
  after the f32 cast; filter with `is_finite()` in `engine-hayro/src/links.rs`
  (`to_page`, `page_rect`).
- **Long URIs make huge tooltips.** Truncate the shown text (about 200
  chars with an ellipsis); the opened URL stays whole.
- **QuadPoints with non-numbers** are misread (`iter::<f64>()` skips them
  and shifts the rest). Fall back to `/Rect` when any entry isn't a number.
- **Leading whitespace in a URI** defeats the scheme check (it then does
  nothing, which is safe). Trim before checking.
- **An unresolvable `/Dest` doesn't fall back to `/A`.** Matches the spec;
  try `/A` too if real files need it.
- **No cropped-page fixture.** A page with `/CropBox [50 50 562 742]` in
  `links.pdf` would lock in that areas and targets follow the crop box.

## Document info (review minors, 2026-10-26)

- **Non-ASCII font names are garbled.** `BaseFont` is a name, not a text
  string: a UTF-8 name (宋体) is read as PDFDocEncoding. Try UTF-8 first.
  (`engine-hayro/src/properties.rs`, `font`)
- **An absurd XMP offset overflows** `num(h)? * 60` (`+9999:00`): a panic in
  debug, a wrong offset in release. Check `h < 24`, `m < 60` first.
- **XMP dates keep no precision.** "2024" shows as 2024-01-01 00:00; an
  offset without a colon (`+0100`) loses the whole date.
- **"Reading…" forever** if the properties thread panics or can't start:
  `Page::receive` ignores `Disconnected`. Show "Couldn't read" instead.
- **Reloads stack up reads.** With the page open, each reload starts a new
  full walk and the old ones run to the end. Re-read only once the current
  read is done.
- **Fonts outside page and form resources are missed**: annotation
  appearance streams (form fields), Type 3 glyph procedures, patterns;
  Type 3 fonts without `/BaseFont` are skipped silently.
- **Run-together names split wrongly** when a family ends in a style-ish
  word: `PlayfairDisplay` becomes Playfair / Display while
  `PlayfairDisplay-Bold` is Playfair Display. Peel only weight and slant
  words off names without a separator.
- **`Code2000` becomes "Code 2000"**: a number after a small letter is
  split off, which is right for "Source Serif 4" but not here.
- **"1000 kB"** for 999 999 bytes: pick the unit after rounding.
- **Test gaps**: no unit tests of their own for `xmp_date` and `xmp_value`;
  none for malformed XMP, a BOM'd UTF-16 string beyond the fixture, or Esc
  in the info page keeping the search text.
- **Encryption isn't shown**: hayro doesn't expose the trailer. Revisit if
  it does.

## TeX font names (review minors, 2026-10-26)

- **"Slanted" doesn't count as slanted** in `slanted()` (only "Slant"), so
  Slanted, Bold Extended Slanted and Slanted Typewriter sort among the
  upright styles. Add it, with a `families()` test on CMSL10 and CMR10.
- **Roman isn't first** among Computer Modern's styles: most tie at weight
  400 and sort by name, so "Caps and Small Caps" and "Math Italic" come
  first. Rank Roman (and Regular) first among equals.
- **`CMINCH` never matches**: it has no design size, and the rule wants one.
  Drop it or allow it without.
- **Missing common TeX fonts**: `LASY` (LaTeX symbols), `CMEXB`, `RSFS`,
  `STMARY`, `WASY`, `LCIRCLE`/`LINE`; CM-Super `SFSO` (check the cm-super
  README).
- **`EUEX`** is family "Euler", style "Extension", unlike the other Euler
  entries; "Euler Extension" / Regular would match them.
- **Lowercase TeX names** (`cmr10`, from old dvips bitmap output) stay raw;
  compare ignoring ASCII case.
## Document info jump fix (review minors, 2026-10-26)

- **A vertical jump is still possible**: the modal centres on last frame's
  size, so when the properties arrive (and on a second opening, which
  starts from the last height) it may draw a frame at the old top, then
  re-centre. The test checks only x. Look by hand; a fixed minimum height
  for the contents would stop it.
- `section` sets `item_spacing.y` on the shared scroll area Ui, so the
  "File" heading is spaced unlike the later ones. Use `ui.scope` or set it
  once in `contents`. (`gui-egui/src/info.rs`)
- `info_stays_still_when_the_properties_arrive`: the long `title` is never
  shown (`BookDocument` ignores the bytes); and 20 × 5 ms may be short on a
  loaded machine. Wait until "Intervogue" shows, with a deadline.

## Document info scroll bar (review minors, 2026-10-26)

- A hovered floating bar is 10 px wide but the margin is 6, so it covers
  about 4 px of text; widen the right margin to the bar width, or accept.
## Screen edge flash (review minors, 2026-10-26)

- `reload()` doesn't clear `screen_edge`, unlike the link flash: a reload
  within the fade keeps the line on the new version. Clear it, with a test
  like `reload_drops_links_and_flash`.
- A finished flash whose page isn't shown (paged mode, jumped away) stays
  in `screen_edge` until a page key; check the time before the page rect.
- Holding PageDown to the document's end flashes nothing on release (the
  last repeat moves nothing and clears the edge); a no-op press also
  clears a fading line. Keep the last real edge if wanted.
- Other moves (Home, a link) don't clear a showing or pending edge; held
  PageDown then Home flashes off screen. Maybe clear it in `reader_moved`.
- PageUp from inside the top margin draws the line in the margin above
  page 0 (negative y); skip it when `y < 0`.
- An edge in a gap between rows is a page point past the page's bottom;
  the gap doesn't scale with zoom, so a zoom during the fade can move the
  line out of the gap.
- Tests: no GUI test for PageUp, a dual spread (two segments), an edge in
  a gap, a no-op press at the end, or a flip clearing a showing flash;
  `edge_lines` matches any line of the flash width.
- Opening settings or info (by mouse) while a page key repeats: the
  repeats aren't seen behind the modal, so the edge flashes on release.
  Record the flag before the modal's early return, or clear the edge.
- No test for PageUp held then PageDown tapped (no flash), or a hold at
  the document's end followed by a tap (flashes).

## Page step 98% (review minors, 2026-10-26)

- `next_after_scrolling_goes_from_view` reaches page 6 with `go_to`, which
  presses PageDown, so it depends on the step size; it now checks the hit
  is visible rather than centred. Placing the view directly with
  `scrolled_to` would make it independent and bring back the centred
  check.
## Paper colour (review minors, 2026-10-27)

- **No focus ring on the swatches.** The chosen square always has its
  2px selection frame, so tabbing onto it shows nothing new; unchosen ones
  only brighten their outline slightly. Draw a focus stroke outside the
  square when `response.has_focus()` (`settings.rs`, `swatch`).
- **Other windows keep the old paper.** On Linux, one process per file:
  choosing sepia in one window leaves the others white until relaunched or
  their settings page is opened (which rereads the file). Reread settings
  on focus, or watch `settings.toml`.
- **A misplaced hand edit is silently ignored.** `paper = "sepia"` written
  after `[new_documents]` is read as `new_documents.paper` and dropped.
  Names are case-sensitive, and `"gray"` is not accepted. Accept `gray` and
  any case; maybe warn on stderr for an unknown name.
- **The "Paper" label isn't tied to the squares** for screen readers:
  `labelled_by` the label, or group them.
- **Test helper `sepia()`** in the app tests duplicates
  `image::paper_colour(Paper::Sepia)`.

## Paper row clears the scrim (review minors, 2026-10-27)

- **Keyboard focus on a paper square doesn't clear the scrim**; only the
  pointer does (by the spec). Clear it while a square has focus too.
- **Closed while the scrim was clear, the scrim fades back in on reopening**
  over 0.15 s instead of showing at once: the animation lives in the egui
  context, not in `settings::Page`. Cosmetic; the dialog fades in too.
  Reset that animation id when the page opens.

## Paper hover preview (review minors, 2026-10-27)

- **No test that the sidebar thumbnails follow the preview.** The page
  images do; the thumbnails get the same colour through
  `sidebar::Input::paper`, untested on hover.

## Paper row without gaps, fading (review minors, 2026-10-27)

- **The gap between "Paper" and the first square doubled**, to about two
  item spacings: the label's spacing, `add_space(gap / 2)` and the first
  cell's half-gap padding (`settings.rs`). Meant to stay one; drop the
  `add_space`.
- **Misleading assert message** in `between_paper_squares_previews_one_of_them`:
  "squares touch" sits on a check that the coloured squares are apart.

## History (review minors, 2026-10-28)

- **The mouse's back/forward buttons can pan the page or move the window**
  if the mouse moves while the button is down: egui's `ScrollArea` drag
  (`DragScroll::Always`) and the title bar's `drag_started()` react to any
  button. Pan and move only on the primary button. The "Left drag pans"
  comment in `app.rs` isn't what egui does.
- **Clicking the thumbnail of the current page adds an entry.** Plausible
  (Back returns to the mid-page spot), but repeated clicks pile up. Could
  skip `leave_here` when the page is already current.
- **One damaged `[[back]]`/`[[forward]]` table loses the whole state
  entry**, view and sidebar too. Deserialize the lists leniently.
- **`doc_state` clones the history every frame** for the save check (up to
  200 `ViewState`s). Negligible now; a generation counter would avoid it.
- **A link's flash keeps running after Back/Forward**, usually off screen.
- **Alt+arrows do nothing while a non-text widget has Tab focus** (the
  `egui_arrows` check ignores modifiers). The mouse buttons still work.
