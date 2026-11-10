# Pins: the arrow opens the pagelet, the card goes there — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A card click navigates on both sidebar tabs, and an arrow on the thumbnail's right strip opens or closes the pagelet on that page without moving the document.

**Architecture:** The sidebar draws a hover-only arrow strip over each card as its own `ui.interact`, the way the pin chip already works: a press on it sets a new `Output.pagelet` and cancels the card's `clicked`. The app toggles the single pagelet through the existing `open_pagelet`/`close_pagelet`. The Pins tab's card click joins the Pages tab's navigation path, which deletes the pending-press/double-click machinery. The pin chip moves to the top centre.

**Tech Stack:** Rust 2024, egui 0.37, the headless `Harness` in `crates/frontends/gui-egui/src/app.rs`.

**Spec:** `docs/superpowers/specs/2026-11-10-pins-arrows-design.md`

## Global Constraints

- TDD: the failing test first, watched failing, then the least code that passes it.
- `just fmt` and `just check` (fmt, clippy `-D warnings`, every test) green before each commit.
- Commit with `jj commit -m "<prefix>: <what changed>"`; the prefix is `egui` for all three tasks. Never move the `main` bookmark.
- No synthetic input to the desktop; behaviour is driven through the harness.
- An arrow or pin press never navigates and never scrolls; a card press never opens the pagelet.

## Review Focus

- **A press on the strip must not navigate.** The strip is a later widget over the card; if the card's `clicked` is not cleared, an arrow press both opens the pagelet and jumps the document.
- **The arrow hides when the pointer leaves the thumbnail.** It must not linger over a card from a previous frame, including on the Pins tab right after an unpin reshapes the column.
- **The pagelet's page decides open vs close.** Pressing the arrow of the page the pagelet already shows must close it; pressing another page's arrow must swap the page and keep the rect.
- **A card click leaves an open pagelet where it is**, on its own page, just as before.
- **The strip and the pin chip stay clear of each other** at the narrowest sidebar, where the thumbnail is smallest.

---

### Task 1: The arrow strip opens and closes the pagelet

**Files:**
- Modify: `crates/frontends/gui-egui/src/sidebar.rs` (constants near line 72; `Input` near line 100; `Output` near line 120; `SidebarView` fields near line 210; accessors near line 1366; `column` near line 829; `pin_rect`/`paint_pin` near line 1446)
- Modify: `crates/frontends/gui-egui/src/app.rs` (`Input` built near line 2697; `Output` handled near line 2723; tests near line 7769)
- Test: `crates/frontends/gui-egui/src/app.rs`, `mod tests`

**Interfaces:**
- Consumes: `Input`/`Output` as they are today; `open_pagelet(page, view, bounds, pointer)` and `close_pagelet()`; `pin_preview.page`.
- Produces:
  - `sidebar::Input.preview: Option<usize>` — the page the pagelet shows, if it is open.
  - `sidebar::Output.pagelet: Option<usize>` — a press on a card's arrow strip.
  - `pub const ARROW_SIZE: f32 = 16.0;`, `ARROW_PADDING: f32 = 4.0;`, `ARROW_ROUNDING: f32 = 4.0;`, `ARROW_STRIP: f32 = ARROW_SIZE + 2.0 * ARROW_PADDING;`
  - `fn arrow_rect(thumb: Rect) -> Rect` — the strip, inset from the thumbnail's right edge.
  - `fn paint_arrow(ui: &egui::Ui, rect: Rect, open: bool, hovered: bool) -> Color32`
  - Test accessors `SidebarView::arrow_icons(&self) -> &[(usize, Rect)]`, `arrow_inks(&self) -> &[(usize, Color32)]`.
  - Test helper in `app.rs`: `fn arrow_rect(h: &Harness, page: usize) -> Rect` (must panic when the page draws no arrow); `fn open_pagelet(h: &mut Harness, page: usize)` (hover the card, press its arrow, wait for the pagelet).

- [ ] **Step 1: Write the failing tests**

In `mod tests` in `app.rs`, next to `thumb_rect`/`pin_rect`:

```rust
/// Page `page`'s arrow strip as drawn last frame, which must be in view
/// and under the pointer.
fn arrow_rect(h: &Harness, page: usize) -> Rect {
    h.app
        .sidebar_view
        .arrow_icons()
        .iter()
        .find(|(p, _)| *p == page)
        .map(|(_, rect)| *rect)
        .unwrap_or_else(|| panic!("page {page}'s arrow not drawn"))
}

/// Hovers `page`'s card, presses its arrow and waits for the pagelet.
fn open_pagelet(h: &mut Harness, page: usize) {
    let card = thumb_rect(h, page);
    h.frames(1, vec![Event::PointerMoved(card.center())]);
    let arrow = arrow_rect(h, page);
    h.click(arrow.center());
    wait_for_pagelet(h, page);
}
```

Add the tests (each one hovers before it reads the strip):

```rust
#[test]
fn hovering_a_card_draws_an_arrow_on_its_right_edge() {
    let mut h = sidebar_on(Arc::new(BlankEngine));
    let thumb = thumb_rect(&h, 2);
    h.frames(1, vec![Event::PointerMoved(thumb.center())]);
    let arrow = arrow_rect(&h, 2);
    assert!((arrow.max.x - thumb.max.x).abs() < 0.5, "{arrow:?} vs {thumb:?}");
    assert!((arrow.width() - 24.0).abs() < 0.5, "not one icon and padding: {arrow:?}");
    assert!((arrow.min.y - thumb.min.y).abs() < 0.5);
    assert!((arrow.center().y - thumb.center().y).abs() < 0.5);
}

#[test]
fn the_arrow_is_drawn_only_while_the_pointer_is_over_the_card() {
    let mut h = sidebar_on(Arc::new(BlankEngine));
    let thumb = thumb_rect(&h, 2);
    h.frames(1, vec![Event::PointerMoved(thumb.center())]);
    assert_eq!(arrow_rect(&h, 2).max.x, thumb.max.x);
    h.frames(1, vec![Event::PointerMoved(egui::pos2(4.0, 4.0))]);
    assert!(
        h.app.sidebar_view.arrow_icons().iter().all(|(p, _)| *p != 2),
        "the arrow outlived the pointer"
    );
}

#[test]
fn an_arrow_press_opens_the_pagelet_without_moving() {
    let mut h = sidebar_on(Arc::new(BlankEngine));
    open_pagelet(&mut h, 3);
    assert!(pagelet_on(&h, 3));
    assert_eq!(h.app.current_page(), Some(0), "it navigated");
    assert_eq!(h.app.chosen, None, "it navigated");
}

#[test]
fn a_second_arrow_press_closes_the_pagelet() {
    let mut h = sidebar_on(Arc::new(BlankEngine));
    open_pagelet(&mut h, 3);
    let rect = h.app.pin_preview.as_ref().unwrap().rect;
    let arrow = arrow_rect(&h, 3);
    h.click(arrow.center());
    h.frames(2, vec![]);
    assert!(h.app.pin_preview.is_none(), "the arrow did not close it");
    let card = thumb_rect(&h, 3);
    h.frames(1, vec![Event::PointerMoved(card.center())]);
    let arrow = arrow_rect(&h, 3);
    h.click(arrow.center());
    wait_for_pagelet(&mut h, 3);
    assert_eq!(h.app.pin_preview.as_ref().unwrap().rect, rect, "the place moved");
}

#[test]
fn a_pins_card_arrow_opens_the_pagelet() {
    let mut h = pinned_on(&[5]);
    open_pagelet(&mut h, 5);
    assert!(pagelet_on(&h, 5));
    assert_eq!(h.app.current_page(), Some(0), "it navigated");
}

#[test]
fn the_arrows_ink_marks_the_pagelet_page() {
    let mut h = sidebar_on(Arc::new(BlankEngine));
    open_pagelet(&mut h, 3);
    let arrow_ink = |h: &Harness, page: usize| {
        h.app
            .sidebar_view
            .arrow_inks()
            .iter()
            .find(|(p, _)| *p == page)
            .map(|(_, colour)| *colour)
            .unwrap_or_else(|| panic!("page {page}'s arrow ink not recorded"))
    };
    let accent = h.ctx.global_style().visuals.selection.stroke.color;
    h.frames(1, vec![Event::PointerMoved(thumb_rect(&h, 4).center())]);
    h.frames(1, vec![]);
    assert_eq!(arrow_ink(&h, 3), accent, "the open page's arrow");
    assert_ne!(arrow_ink(&h, 4), accent, "another page's arrow");
}

#[test]
fn the_arrow_and_the_pin_stay_clear_of_each_other() {
    for mut h in [sidebar_on(Arc::new(BlankEngine)), pinned_on(&[2, 7])] {
        let thumb = thumb_rect(&h, 2);
        h.frames(1, vec![Event::PointerMoved(thumb.center())]);
        let arrow = arrow_rect(&h, 2);
        let pin = pin_rect(&h, 2);
        assert!(!arrow.intersects(pin), "{arrow:?} meets {pin:?}");
    }
}
```

`pinned_on` exists; `pagelet_on`/`wait_for_pagelet` exist at line ~8430.

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test -p gui-egui hovering_a_card_draws_an_arrow`
Expected: compile error — no method `arrow_icons` and no fn `arrow_rect`.

- [ ] **Step 3: Add the strip to `sidebar.rs`**

Constants beside `PIN_SIZE`:

```rust
/// The arrow's icon, its padding inside the strip, and the strip's width.
pub const ARROW_SIZE: f32 = 16.0;
pub const ARROW_PADDING: f32 = 4.0;
pub const ARROW_STRIP: f32 = ARROW_SIZE + 2.0 * ARROW_PADDING;
const ARROW_ROUNDING: f32 = 4.0;
```

`Input` gains `pub preview: Option<usize>`; `Output` gains `pub pagelet: Option<usize>`. `SidebarView` gains

```rust
/// Each visible thumbnail's arrow strip last frame, and its ink.
#[cfg(test)]
arrow_icons: Vec<(usize, Rect)>,
#[cfg(test)]
arrow_inks: Vec<(usize, Color32)>,
```

initialised empty in `new`, cleared with `pin_icons`/`pin_inks` at the top of `column`.

```rust
/// The arrow's strip in `thumb`: its right edge, one icon and padding wide,
/// the full height of the page image.
fn arrow_rect(thumb: Rect) -> Rect {
    Rect::from_min_max(
        egui::pos2(thumb.max.x - ARROW_STRIP, thumb.min.y),
        thumb.max,
    )
}

/// Draws `rect`'s arrow chip and glyph; returns the ink it used.
fn paint_arrow(ui: &egui::Ui, rect: Rect, open: bool, hovered: bool) -> Color32 {
    let visuals = ui.visuals();
    let ink = if open {
        visuals.selection.stroke.color
    } else {
        visuals.weak_text_color()
    };
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let chip = if hovered { 1.0 } else { 0.85 };
    painter.rect_filled(rect, ARROW_ROUNDING, visuals.panel_fill.gamma_multiply(chip));
    let c = rect.center();
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(c.x - 3.5, c.y - 5.0),
            egui::pos2(c.x + 3.5, c.y),
            egui::pos2(c.x - 3.5, c.y + 5.0),
        ],
        Stroke::new(1.5, ink),
    ));
    ink
}
```

In `column`'s card loop, after the pin block and before `self.drawn.push((page, rect))`:

```rust
// The arrow on the page's right edge, shown while the pointer is on
// the page image: a press opens or closes the pagelet on that page and
// does not click the card.
if ui.rect_contains_pointer(rect) {
    let strip = arrow_rect(rect);
    let response = ui.interact(strip, id.with("arrow"), Sense::click());
    if response.clicked() {
        pagelet = Some(page);
    }
    if response.hovered() {
        ui.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    #[cfg(test)]
    {
        let ink = paint_arrow(ui, strip, input.preview == Some(page), response.hovered());
        self.arrow_icons.push((page, strip));
        self.arrow_inks.push((page, ink));
    }
    #[cfg(not(test))]
    paint_arrow(ui, strip, input.preview == Some(page), response.hovered());
}
```

with `let mut pagelet = None;` beside `clicked`/`toggled`. Extend the existing suppression:

```rust
if pagelet.is_some() {
    // The strip sits over the card: its press is not a card click.
    clicked = None;
}
```

Return `pagelet` in the `Output` literal at the end of `column`.

Add the accessors beside `pin_icons`/`pin_inks`:

```rust
#[cfg(test)]
pub fn arrow_icons(&self) -> &[(usize, Rect)] {
    &self.arrow_icons
}

#[cfg(test)]
pub fn arrow_inks(&self) -> &[(usize, Color32)] {
    &self.arrow_inks
}
```

- [ ] **Step 4: Wire it in `app.rs`**

The `Input` literal gains `preview: self.pin_preview.as_ref().map(|preview| preview.page),`.

The output handling, beside `toggle_pin`:

```rust
if let Some(page) = output.pagelet {
    if self
        .pin_preview
        .as_ref()
        .is_some_and(|preview| preview.page == page)
    {
        self.close_pagelet();
    } else if page < self.view.layout().page_count() {
        let view = self
            .last_scroll
            .map_or_else(|| ctx.content_rect(), |last| last.inner);
        let bounds = Rect::from_min_max(pos2(ctx.content_rect().min.x, view.min.y), view.max);
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        self.open_pagelet(page, view, bounds, pointer);
    }
}
```

- [ ] **Step 5: Run the task's tests**

Run: `cargo test -p gui-egui arrow` and `cargo test -p gui-egui pagelet`
Expected: the new tests pass; the existing ones still pass (`a_single_click_opens_the_pagelet_after_the_double_click_window` still opens from a card press).

- [ ] **Step 6: `just fmt`, `just check`, commit**

```bash
jj commit -m "egui: an arrow on each card opens the pagelet" -m "The thumbnail's right strip, shown while the pointer is over the page,
presses the pagelet open or shut on that page without moving the
document. A press there is not a card click."
```

---

### Task 2: The card click goes there, and the double-click wait goes

**Files:**
- Modify: `crates/frontends/gui-egui/src/sidebar.rs` (`Output` near line 120; `pins()` near line 988)
- Modify: `crates/frontends/gui-egui/src/app.rs` (field near line 144; `Output` handling near line 2723; the pending branch near line 2794; tests from line 8430 on)
- Test: `crates/frontends/gui-egui/src/app.rs`, `mod tests`

**Interfaces:**
- Consumes: `Output.pagelet` and `open_pagelet` from Task 1.
- Produces: `Output.clicked` is emitted by both tabs; `Output.pin_clicked`, `NoidoApp::pending_pin`, and the double-click branches are gone.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_pins_card_click_goes_there_at_once() {
    let mut h = pinned_on(&[5]);
    let card = thumb_rect(&h, 5);
    h.click(card.center());
    h.frames(1, vec![]);
    assert_eq!(h.app.current_page(), Some(5), "it did not navigate at once");
    assert_eq!(h.app.chosen, Some(5));
    assert!(h.app.pin_preview.is_none(), "it opened the pagelet");
}

#[test]
fn a_card_click_leaves_the_pagelet_open() {
    let mut h = pinned_on(&[2, 5]);
    open_pagelet(&mut h, 2);
    let rect = h.app.pin_preview.as_ref().unwrap().rect;
    let card = thumb_rect(&h, 5);
    h.click(card.center());
    h.frames(2, vec![]);
    assert!(pagelet_on(&h, 2), "the pagelet closed or changed page");
    assert_eq!(h.app.pin_preview.as_ref().unwrap().rect, rect);
    assert_eq!(h.app.current_page(), Some(5));
}

#[test]
fn an_arrow_on_another_page_swaps_the_page_and_keeps_the_place() {
    let mut h = pinned_on(&[2, 5]);
    open_pagelet(&mut h, 2);
    let rect = h.app.pin_preview.as_ref().unwrap().rect;
    open_pagelet(&mut h, 5);
    assert!(pagelet_on(&h, 5));
    assert_eq!(h.app.pin_preview.as_ref().unwrap().rect, rect);
}
```

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test -p gui-egui a_pins_card_click_goes_there_at_once`
Expected: FAIL — `current_page` is still `Some(0)` (the press waits out the double-click window).

- [ ] **Step 3: Delete the machinery**

In `sidebar.rs`: delete `Output::pin_clicked`; in `pins()`, return `clicked: shown.inner.clicked` instead.

In `app.rs`: delete the `pending_pin` field and its initialiser; delete the whole `if let Some(page) = output.pin_clicked { ... }` block and the whole "A pin click waits out egui's double-click window" block that follows the sidebar `if`/`else`.

- [ ] **Step 4: Move every pagelet test to the arrow**

Replace each `h.click(thumb_rect(&h, N).center());` followed by `wait_for_pagelet(&mut h, N);` with `open_pagelet(&mut h, N);`, and the close-chip test's first two lines likewise. Delete `a_single_click_opens_the_pagelet_after_the_double_click_window` and `a_double_click_goes_there_and_never_opens_the_pagelet`; rename `a_double_click_leaves_an_open_pagelet` to `a_card_click_leaves_an_open_pagelet` (it is the new test above; keep one).

- [ ] **Step 5: Run the suite**

Run: `cargo test -p gui-egui`
Expected: every test passes.

- [ ] **Step 6: `just fmt`, `just check`, commit**

```bash
jj commit -m "egui: a card click goes there and the double-click wait goes" -m "Both tabs' cards navigate on one press, so the pending press, its
repaint scheduling and the double-click branches are gone. A press on a
card's pin or arrow still does neither."
```

---

### Task 3: The pin sits at the top centre

**Files:**
- Modify: `crates/frontends/gui-egui/src/sidebar.rs` (`pin_rect`, line ~1446)
- Test: `crates/frontends/gui-egui/src/app.rs` (`every_visible_thumbnail_has_a_pin_in_its_corner`, line ~7791)

**Interfaces:**
- Produces: `fn pin_rect(thumb: Rect) -> Rect`, centred on `thumb`'s top edge.

- [ ] **Step 1: Write the failing test**

Replace `every_visible_thumbnail_has_a_pin_in_its_corner` with

```rust
#[test]
fn every_visible_thumbnail_has_a_pin_at_its_top_centre() {
    let h = sidebar_on(Arc::new(BlankEngine));
    let drawn = drawn_pages(&h);
    assert!(!drawn.is_empty());
    for page in drawn {
        let (thumb, pin) = (thumb_rect(&h, page), pin_rect(&h, page));
        assert!(thumb.contains_rect(pin), "{page}: {pin:?} in {thumb:?}");
        assert!(
            (pin.center().x - thumb.center().x).abs() < 0.5,
            "{page}: not centred: {pin:?} over {thumb:?}"
        );
        assert!(
            (pin.min.y - thumb.min.y).abs() < 0.5,
            "{page}: not at the top: {pin:?} over {thumb:?}"
        );
    }
}
```

- [ ] **Step 2: Run it to watch it fail**

Run: `cargo test -p gui-egui every_visible_thumbnail_has_a_pin_at_its_top_centre`
Expected: FAIL — `not centred` (the chip is at the right).

- [ ] **Step 3: Centre it**

```rust
fn pin_rect(thumb: Rect) -> Rect {
    let side = vec2(PIN_SIZE, PIN_SIZE);
    let min = egui::pos2(
        thumb.center().x - PIN_SIZE / 2.0,
        thumb.min.y + PIN_INSET,
    );
    Rect::from_min_size(min, side)
}
```

- [ ] **Step 4: Run the suite**

Run: `cargo test -p gui-egui`
Expected: every test passes, including `clicking_a_pin_pins_without_navigating` and `a_card_click_pins_nothing` (the card's middle is clear of a centred pin).

- [ ] **Step 5: `just fmt`, `just check`, commit**

```bash
jj commit -m "egui: the pin sits at the top centre of the card" -m "The arrow's strip needs the right edge, and the pin reads as the card's
own mark at the top centre."
```
