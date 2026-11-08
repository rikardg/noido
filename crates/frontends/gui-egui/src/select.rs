//! The window's text selection: the select tool, the selection, and the
//! drag under way.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Vec2, pos2, vec2};
use noido_core::{
    Caret, DocPoint, DragKind, HighlightColour, Highlights, Layout, PageTextState, Selection, Spot,
    spot_at,
};

use crate::texts::Texts;

/// A press on the pages and what it does.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub kind: DragKind,
    pub origin: Pos2,
    /// The pointer moved away from the origin: a drag, not a click.
    pub moved: bool,
    /// For a selecting drag, whether its anchor was placed.
    pub started: bool,
    /// For a handle drag, which end (0 start, 1 end), and the pointer's
    /// offset from that end's caret at the press.
    pub handle: Option<(usize, Vec2)>,
    /// Shift was held at the press.
    pub shift: bool,
}

/// The radius of a handle's knob, in points.
pub const HANDLE_RADIUS: f32 = 6.0;
/// How near a handle's knob a press takes the handle, in points.
pub const HANDLE_REACH: f32 = 12.0;
/// The size of a highlight colour's button in the selection's bar, in
/// points.
pub const SWATCH_SIZE: f32 = 27.0;
/// How many colour buttons the bar has: one per colour.
pub const SWATCH_COUNT: usize = HighlightColour::ALL.len();
/// The radius of the colour circle drawn in that button, in points.
pub const SWATCH_RADIUS: f32 = 9.0;
/// The alpha of a highlight over the page's text, in light and dark mode:
/// the fill is stronger in dark mode, where the page sits in darker chrome.
const HIGHLIGHT_ALPHA_LIGHT: u8 = 89;
const HIGHLIGHT_ALPHA_DARK: u8 = 115;

/// The hue of `colour`'s highlight, before it is faded over the page.
fn hue(colour: HighlightColour) -> (u8, u8, u8) {
    match colour {
        HighlightColour::Yellow => (0xff, 0xd8, 0x4d),
        HighlightColour::Green => (0x76, 0xc7, 0x5a),
        HighlightColour::Blue => (0x5a, 0xa9, 0xe8),
        HighlightColour::Pink => (0xec, 0x7f, 0xa9),
        HighlightColour::Purple => (0xa9, 0x8b, 0xd8),
    }
}

/// The colour of `colour`'s button in the selection's bar: the hue the page
/// gets, at full strength.
pub fn swatch_colour(colour: HighlightColour) -> Color32 {
    let (r, g, b) = hue(colour);
    Color32::from_rgb(r, g, b)
}

/// The name of `colour`'s button in the selection's bar, for its tooltip.
pub fn colour_name(colour: HighlightColour) -> &'static str {
    match colour {
        HighlightColour::Yellow => "Yellow",
        HighlightColour::Green => "Green",
        HighlightColour::Blue => "Blue",
        HighlightColour::Pink => "Pink",
        HighlightColour::Purple => "Purple",
    }
}

/// The fill a highlight of `colour` is drawn with over the page's text,
/// `dark` for a dark window theme.
pub fn highlight_fill(colour: HighlightColour, dark: bool) -> Color32 {
    let (r, g, b) = hue(colour);
    let alpha = if dark {
        HIGHLIGHT_ALPHA_DARK
    } else {
        HIGHLIGHT_ALPHA_LIGHT
    };
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// How far clear of a knob the bar moves when it has no room for more.
pub const BAR_CLEARANCE: f32 = 2.0;

/// Which end of `selection` its focus is (0 start, 1 end): where the mouse let
/// go, before a highlight's range forgets it.
fn focus_end(selection: &Selection) -> usize {
    let page = |spot: Spot| match spot {
        Spot::At(caret) => caret.page,
        Spot::Waiting(point) => point.page,
    };
    match selection.carets() {
        Some((start, _)) => usize::from(selection.focus != Spot::At(start)),
        None => usize::from(page(selection.focus) >= page(selection.anchor)),
    }
}

/// Where the bar goes, as a position and what of the bar is at it, hanging
/// from the anchor's top and bottom, `gap` off them. It keeps off the knobs of
/// the selection's ends, whose caret lines are `lines` (start, end), since a
/// press on the bar would not reach them, and off the anchor, where a click
/// would land on it. `window` is what egui pushes the bar back inside.
///
/// With an end `moved` by the mouse (0 start, 1 end), the bar goes outside the
/// selection there if it can. Otherwise it goes by the anchor and leaves only
/// for a knob.
pub fn bar_place(
    (top, bottom): (Pos2, Pos2),
    lines: [Option<(Pos2, Pos2)>; 2],
    view: Rect,
    window: Rect,
    size: Vec2,
    gap: f32,
    moved: Option<usize>,
) -> (Pos2, Align2) {
    let x = top
        .x
        .min(window.right() - size.x / 2.0)
        .max(window.left() + size.x / 2.0);
    let above = |y: f32| (pos2(x, y), Align2::CENTER_BOTTOM);
    let below = |y: f32| (pos2(x, y), Align2::CENTER_TOP);
    let under_knob = 2.0 * HANDLE_RADIUS + gap;
    let knobs = lines.map(|line| {
        line.map(|(_, bottom)| {
            let centre = bottom + vec2(0.0, HANDLE_RADIUS);
            Rect::from_center_size(centre, Vec2::splat(2.0 * HANDLE_RADIUS))
        })
    });
    let rect = |(pos, pivot): (Pos2, Align2)| pivot.anchor_size(pos, size);
    let covers = |place, end: usize| knobs[end].is_some_and(|knob| rect(place).intersects(knob));
    let free = |place: &(Pos2, Align2)| {
        let bar = rect(*place);
        bar.top() >= view.top()
            && bar.bottom() <= view.bottom()
            && !covers(*place, 0)
            && !covers(*place, 1)
            && !bar.contains(top)
            && !bar.contains(bottom)
    };

    // Past the selection, and past an anchor beyond it.
    let outside = |end: usize| {
        lines[end].map(|(line_top, line_bottom)| match end {
            0 => above(line_top.y.min(top.y) - gap),
            _ => below(line_bottom.y.max(bottom.y) + under_knob),
        })
    };
    // Which end to go outside. On one line, above it, unless the anchor is
    // beyond the line and its knob. Otherwise the end let go of, which the
    // pixels can't tell: in tight lines one line's knob reaches the next.
    let from_line = |line: Option<(Pos2, Pos2)>| {
        line.map_or(f32::MAX, |(t, b)| (t.y - top.y).max(top.y - b.y).max(0.0))
    };
    let one_line = match lines {
        [Some((a, b)), Some((c, _))] => (a.y - c.y).abs() < (b.y - a.y) / 2.0,
        _ => false,
    };
    let near = match (one_line, moved) {
        (true, _) => usize::from(lines[0].is_some_and(|(_, b)| top.y > b.y + 2.0 * HANDLE_RADIUS)),
        (false, Some(end)) => end,
        (false, None) => usize::from(from_line(lines[1]) < from_line(lines[0])),
    };

    if moved.is_some()
        && let Some(place) = outside(near).filter(free)
    {
        return place;
    }
    let first = if top.y - gap - size.y >= view.top() {
        above(top.y - gap)
    } else {
        below(bottom.y + under_knob)
    };
    let from_mouse = |end: usize| knobs[end].map_or(f32::MAX, |k| (k.center().y - top.y).abs());
    let covered = match (covers(first, 0), covers(first, 1)) {
        (false, false) => return first,
        (true, false) => 0,
        (false, true) => 1,
        (true, true) => usize::from(from_mouse(1) < from_mouse(0)),
    };
    // Just clear of the knob. Down off the start's, past an anchor that would
    // then be under the bar; up off the end's, past it would be where `first`
    // already failed.
    let clear = |end: usize| {
        knobs[end].map(|knob| match end {
            0 => {
                let y = knob.bottom() + BAR_CLEARANCE;
                below(if (y..=y + size.y).contains(&bottom.y) {
                    bottom.y + under_knob
                } else {
                    y
                })
            }
            _ => above(knob.top() - BAR_CLEARANCE),
        })
    };
    [outside(near), clear(covered), outside(1 - near)]
        .into_iter()
        .flatten()
        .find(free)
        .unwrap_or(first)
}

/// The page point the bar hangs from.
#[derive(Clone, Copy)]
struct BarPoint {
    doc: DocPoint,
    /// The end of the selection the mouse let go of; `None` for a click on a
    /// highlight, which keeps the bar by the mouse.
    end: Option<usize>,
}

/// The pages as they stand this frame.
#[derive(Clone, Copy)]
pub struct Pages<'a> {
    pub texts: &'a Texts,
    pub layout: &'a Layout,
    /// Where the pages' content starts: its top left is their origin.
    pub content: Rect,
    /// The part of the content in view, on screen.
    pub viewport: Rect,
}

#[derive(Default)]
pub struct State {
    /// With the select tool on, a plain drag selects.
    pub tool: bool,
    pub selection: Option<Selection>,
    /// The highlight being edited: its index in [`Highlights::items`].
    pub editing: Option<usize>,
    pub drag: Option<Drag>,
    /// Where the mouse let go, projected to screen each frame into
    /// [`State::bar_at`].
    bar_doc: Option<BarPoint>,
    /// Where the bar hangs from on screen, in the same coordinates as
    /// `caret_lines`, while something is selected; `None` hangs it from
    /// the selection's end.
    pub bar_at: Option<Pos2>,
    /// The pointer is over text on the pages.
    pub over_text: bool,
    /// A widget had keyboard focus at the end of the last frame.
    pub widget_focused: bool,
    /// The middle of each end's caret on screen, last frame (start, end).
    pub carets: [Option<Pos2>; 2],
    /// Each end's caret top and bottom on screen, last frame.
    pub caret_lines: [Option<(Pos2, Pos2)>; 2],
    /// Each end's handle knob on screen, where in view.
    pub handles: [Option<Pos2>; 2],
    /// Where the bar was drawn last frame.
    pub bar_rect: Option<Rect>,
    /// The bar's size when it was last drawn.
    pub bar_size: Option<Vec2>,
    /// Where the bar's colour swatches were drawn last frame, in
    /// [`HighlightColour::ALL`] order.
    pub swatches: [Option<Rect>; SWATCH_COUNT],
    /// Where the bar's delete button was drawn last frame.
    pub delete_rect: Option<Rect>,
    /// The pages (not a scroll bar, the sidebar or a popup over them) are
    /// under the pointer.
    pub pages_hovered: bool,
    /// The selection Copy was asked for while a page's text in it was
    /// still to come; dropped when the selection changes.
    pub copy_pending: Option<Selection>,
    /// When a selection drag reached the band along the pages' edge, for
    /// the edge scroll's speed-up; `None` outside it.
    pub edge_since: Option<f64>,
}

impl State {
    /// Drops the selection and any drag: a new version of the document.
    pub fn clear(&mut self) {
        self.selection = None;
        self.editing = None;
        self.bar_doc = None;
        self.bar_at = None;
        self.drag = None;
        self.copy_pending = None;
    }

    /// The end of the selection the mouse let go of, which the bar goes
    /// outside; `None` for a click on a highlight, which keeps it by the
    /// mouse. With no point to hang from, the bar hangs from the later end.
    pub fn bar_end(&self) -> Option<usize> {
        self.bar_doc.map_or(Some(1), |point| point.end)
    }

    /// Keeps the highlight being edited in step with the selection: its
    /// new range while the selection holds characters, its stored one when
    /// the selection has collapsed.
    fn settle(&mut self, texts: &Texts, highlights: &mut Highlights) {
        let Some(index) = self.editing else {
            return;
        };
        let Some(highlight) = highlights.get(index) else {
            self.editing = None;
            self.bar_doc = None;
            self.bar_at = None;
            return;
        };
        let selected = self
            .selection
            .filter(|s| !s.is_empty(|page| texts.get(page)))
            .and_then(|s| s.carets());
        match selected {
            Some((start, end)) => {
                highlights.set_range(index, start, end);
                self.selection = Some(Selection {
                    anchor: Spot::At(start),
                    focus: Spot::At(end),
                });
            }
            None => {
                self.selection = Some(Selection {
                    anchor: Spot::At(highlight.start),
                    focus: Spot::At(highlight.end),
                });
            }
        }
    }

    /// The handle a press at `pos` takes, with the press's offset from its
    /// caret.
    pub fn handle_at(&self, pos: Pos2) -> Option<(usize, Vec2)> {
        (0..2).find_map(|i| {
            let knob = self.handles[i]?;
            let caret = self.carets[i]?;
            ((knob - pos).abs().max_elem() <= HANDLE_REACH).then_some((i, pos - caret))
        })
    }

    /// Places the carets, handles of the selection on screen.
    pub fn place(&mut self, pages: &Pages) {
        let Pages {
            texts,
            layout,
            content,
            viewport,
        } = *pages;
        self.carets = [None, None];
        self.caret_lines = [None, None];
        self.handles = [None, None];
        self.bar_at = None;
        let Some(selection) = self.selection else {
            return;
        };
        let Some((start, end)) = selection.carets() else {
            return;
        };
        if selection.is_empty(|page| texts.get(page)) {
            return;
        }
        // The point the bar hangs from, moved with the page it was made
        // on: scrolling, zooming or flipping pages takes it along.
        self.bar_at = self
            .bar_doc
            .and_then(|point| layout.content_point(point.doc))
            .map(|(x, y)| content.min + vec2(x, y));
        for (i, caret) in [start, end].into_iter().enumerate() {
            let Some((page, char)) = shown_caret(texts, caret, i == 1) else {
                continue;
            };
            let (Some(r), PageTextState::Known(text)) = (layout.page_rect(page), texts.get(page))
            else {
                continue;
            };
            let Some(c) = text.caret_rect(char) else {
                continue;
            };
            let scale = layout.scale();
            let top = content.min + vec2(r.x + c.x * scale, r.y + c.y * scale);
            let bottom = top + vec2(0.0, c.height * scale);
            self.carets[i] = Some(top.lerp(bottom, 0.5));
            self.caret_lines[i] = Some((top, bottom));
            let knob = bottom + vec2(0.0, HANDLE_RADIUS);
            if viewport.contains(knob) {
                self.handles[i] = Some(knob);
            }
        }
    }

    /// Follows the drag under way on the pages: a selecting drag moves the
    /// selection's focus; a click selects the highlight under it, or clears
    /// the selection when there is none or `on_link` gives the click to a
    /// link. Also notes whether the pointer is over text, and places
    /// selection ends waiting for their page's text.
    pub fn update(
        &mut self,
        pages: &Pages,
        ui: &egui::Ui,
        highlights: &mut Highlights,
        on_link: bool,
    ) {
        let Pages {
            texts,
            layout,
            content,
            viewport,
        } = *pages;
        let spot = |pos: egui::Pos2| {
            let p = layout.doc_point(pos.x - content.min.x, pos.y - content.min.y)?;
            Some(spot_at(layout, p, texts.get(p.page)))
        };
        let (pointer, released) =
            ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_released()));
        // A release that never came (the window lost the pointer) ends the
        // drag too.
        let released = released || !ui.input(|i| i.pointer.primary_down());
        // egui's own click distance: a press that moves less is a click,
        // as it is for links.
        let threshold = ui.ctx().options(|o| o.input_options.max_click_dist);
        if let Some(drag) = &mut self.drag {
            if let Some(pos) = pointer
                && (pos - drag.origin).length() > threshold
            {
                drag.moved = true;
            }
            if drag.kind == DragKind::Handle
                && drag.moved
                && let (Some((end, grab)), Some(selection)) = (drag.handle, &mut self.selection)
            {
                if !drag.started
                    && let Some((start, last)) = selection.carets()
                {
                    drag.started = true;
                    selection.anchor = Spot::At(if end == 0 { last } else { start });
                }
                if let Some(focus) = pointer.and_then(|pos| spot(pos - grab)) {
                    selection.focus = focus;
                }
            }
            if drag.kind == DragKind::Select && drag.moved {
                self.editing = None;
                if !drag.started
                    && let Some(anchor) = spot(drag.origin)
                {
                    drag.started = true;
                    self.selection = Some(Selection {
                        anchor,
                        focus: anchor,
                    });
                }
                if let (Some(selection), Some(focus)) =
                    (&mut self.selection, pointer.and_then(spot))
                {
                    selection.focus = focus;
                }
            }
        }
        if let Some(selection) = &mut self.selection {
            selection.resolve(|page| texts.get(page));
        }
        if released && let Some(drag) = self.drag.take() {
            let let_go = self.selection.as_ref().map_or(1, focus_end);
            // Read before a highlight's range is restored: such a resize
            // changed nothing, so the bar stays.
            let collapsed = self
                .selection
                .is_some_and(|s| s.is_empty(|page| texts.get(page)));
            let click = !drag.moved && drag.kind != DragKind::Handle;
            let (double, triple) = ui.input(|i| {
                (
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary),
                    i.pointer
                        .button_triple_clicked(egui::PointerButton::Primary),
                )
            });
            let point =
                layout.doc_point(drag.origin.x - content.min.x, drag.origin.y - content.min.y);
            if drag.kind == DragKind::Handle && self.editing.is_some() {
                // The handles resized the highlight being edited.
                self.settle(texts, highlights);
            } else if click && drag.kind == DragKind::Select && (double || triple) {
                self.editing = None;
                self.bar_doc = point.map(|doc| BarPoint { doc, end: Some(1) });
                self.selection = point.and_then(|p| {
                    let PageTextState::Known(text) = texts.get(p.page) else {
                        return None;
                    };
                    let range = if triple {
                        text.line_range_at(p.x, p.y)
                    } else {
                        text.word_at(p.x, p.y)
                    }?;
                    let at = |char| Spot::At(Caret { page: p.page, char });
                    Some(Selection {
                        anchor: at(range.start),
                        focus: at(range.end),
                    })
                });
            } else if click && drag.shift && self.selection.is_some() {
                // Shift+click extends, tool on or off.
                if let (Some(selection), Some(to)) = (&mut self.selection, spot(drag.origin)) {
                    selection.extend_to(to);
                }
                let end = self.selection.as_ref().map(focus_end);
                self.bar_doc = point.map(|doc| BarPoint { doc, end });
                if self.editing.is_some() {
                    self.settle(texts, highlights);
                }
            } else if click {
                // A plain click: on a link it follows it, on a highlight it
                // selects it, elsewhere it clears.
                self.selection = None;
                self.editing = None;
                self.bar_doc = None;
                self.bar_at = None;
                if !(on_link && !drag.shift)
                    && let Some(p) = point
                    && let PageTextState::Known(text) = texts.get(p.page)
                    && let Some(index) = highlights.at_point(p, text)
                    && let Some(highlight) = highlights.get(index)
                {
                    self.selection = Some(Selection {
                        anchor: Spot::At(highlight.start),
                        focus: Spot::At(highlight.end),
                    });
                    self.editing = Some(index);
                    self.bar_doc = Some(BarPoint { doc: p, end: None });
                }
            } else {
                let empty = self
                    .selection
                    .is_some_and(|s| s.is_empty(|page| texts.get(page)));
                if drag.kind != DragKind::Pan && empty {
                    self.selection = None;
                }
            }
            // A drag that moved leaves the bar where it let go, or the
            // nearest it can be seen: out of view it would not show at all.
            if drag.kind != DragKind::Pan && drag.moved && !collapsed {
                self.bar_doc = pointer
                    .and_then(|pos| {
                        let pos = viewport.shrink(1.0).clamp(pos);
                        layout.doc_point(pos.x - content.min.x, pos.y - content.min.y)
                    })
                    .map(|doc| BarPoint {
                        doc,
                        end: Some(let_go),
                    });
            }
        }
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|_| self.pages_hovered);
        self.over_text = hover.is_some_and(|pos| {
            layout
                .doc_point(pos.x - content.min.x, pos.y - content.min.y)
                .is_some_and(|p| match texts.get(p.page) {
                    PageTextState::Known(text) => {
                        text.rects(0..text.char_count()).iter().any(|r| {
                            r.x <= p.x
                                && p.x <= r.x + r.width
                                && r.y <= p.y
                                && p.y <= r.y + r.height
                        })
                    }
                    PageTextState::Unknown => false,
                })
        });
    }
}

/// Where `caret` is drawn: an end at the start of a page after the
/// selected text is drawn at the end of the page before it.
fn shown_caret(texts: &Texts, caret: Caret, end: bool) -> Option<(usize, usize)> {
    if end && caret.char == 0 && caret.page > 0 {
        let page = caret.page - 1;
        if let PageTextState::Known(text) = texts.get(page) {
            return Some((page, text.char_count()));
        }
    }
    Some((caret.page, caret.char))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: f32 = 8.0;
    const SIZE: Vec2 = vec2(350.0, 50.0);

    const X: f32 = 520.0;

    /// A caret centred on `y`, 20 tall: its knob spans `y + 10` to `y + 22`.
    fn line(y: f32) -> Option<(Pos2, Pos2)> {
        Some((pos2(500.0, y - 10.0), pos2(500.0, y + 10.0)))
    }

    /// Where the bar goes for a click on a highlight with the mouse at `y`,
    /// in a view 1000 tall.
    fn placed(mouse: f32, start: f32, end: f32) -> Rect {
        placed_in(1000.0, mouse, line(start), line(end))
    }

    /// Where it goes for a selection just made, whose `moved` end the mouse
    /// let go of.
    fn made(mouse: f32, start: f32, end: f32, moved: usize) -> Rect {
        place_bar(
            pos2(X, mouse),
            [line(start), line(end)],
            1000.0,
            Some(moved),
        )
    }

    fn placed_in(
        height: f32,
        mouse: f32,
        start: Option<(Pos2, Pos2)>,
        end: Option<(Pos2, Pos2)>,
    ) -> Rect {
        place_bar(pos2(X, mouse), [start, end], height, None)
    }

    fn place_bar(
        at: Pos2,
        lines: [Option<(Pos2, Pos2)>; 2],
        height: f32,
        outside: Option<usize>,
    ) -> Rect {
        let view = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, height));
        let (pos, pivot) = bar_place((at, at), lines, view, view, SIZE, GAP, outside);
        pivot.anchor_size(pos, SIZE)
    }

    #[test]
    fn made_selection_puts_the_bar_past_the_end_let_go() {
        assert_eq!(made(490.0, 100.0, 500.0, 1).top(), 530.0);
        assert_eq!(made(110.0, 100.0, 500.0, 0).bottom(), 82.0);
    }

    #[test]
    fn made_selection_in_tight_lines_goes_by_the_end_not_the_pixels() {
        // Lines 18 apart: the mouse at 322 is on the second line, and also
        // on the first line's knob, 310 to 322.
        assert_eq!(made(322.0, 300.0, 318.0, 1).top(), 348.0);
        assert_eq!(made(322.0, 300.0, 318.0, 0).bottom(), 282.0);
    }

    #[test]
    fn made_selection_ending_higher_than_it_starts_goes_past_the_end() {
        // Across columns or pages: the end's line above the start's.
        assert_eq!(made(300.0, 500.0, 300.0, 1).top(), 330.0);
    }

    #[test]
    fn made_selection_on_one_line_puts_the_bar_above_it() {
        assert_eq!(made(300.0, 300.0, 300.0, 1).bottom(), 282.0);
        // On the knob, as when a handle is let go.
        assert_eq!(made(315.0, 300.0, 300.0, 0).bottom(), 282.0);
        assert_eq!(made(315.0, 300.0, 300.0, 1).bottom(), 282.0);
    }

    #[test]
    fn bar_past_the_selection_goes_past_a_mouse_beyond_it_too() {
        assert_eq!(made(340.0, 300.0, 300.0, 1).top(), 360.0);
        assert_eq!(made(80.0, 300.0, 600.0, 0).bottom(), 72.0);
    }

    #[test]
    fn made_selection_without_room_outside_keeps_the_bar_by_the_mouse() {
        assert_eq!(made(965.0, 100.0, 970.0, 1).bottom(), 957.0);
    }

    #[test]
    fn bar_is_not_moved_clear_of_a_knob_onto_the_mouse() {
        // Down off the start's knob, 50 to 62, it would span 64 to 114.
        assert_eq!(placed(100.0, 40.0, 600.0).top(), 120.0);
    }

    #[test]
    fn bar_pushed_in_from_the_windows_edge_still_clears_a_knob() {
        // Centred on the mouse it would miss the knob at 340; pushed inside
        // the window it spans 0 to 350.
        let start = Some((pos2(340.0, 290.0), pos2(340.0, 310.0)));
        let bar = place_bar(pos2(20.0, 340.0), [start, line(800.0)], 1000.0, None);
        assert_eq!(bar.left(), 0.0);
        assert_eq!(bar.bottom(), 282.0);
    }

    #[test]
    fn bar_stays_over_the_mouse_when_it_covers_no_handle() {
        let bar = placed(300.0, 100.0, 500.0);
        assert_eq!(bar.bottom(), 292.0);
        assert_eq!(bar.center().x, X);
    }

    #[test]
    fn bar_ignores_a_handle_to_its_side() {
        let far = Some((pos2(900.0, 290.0), pos2(900.0, 310.0)));
        let bar = placed_in(1000.0, 340.0, line(100.0), far);
        assert_eq!(bar.bottom(), 332.0);
    }

    #[test]
    fn bar_goes_below_a_covered_end_handle() {
        // Over the mouse it would span 282 to 332, the knob 310 to 322.
        let bar = placed(340.0, 100.0, 300.0);
        assert_eq!(bar.top(), 360.0);
        assert_eq!(bar.center().x, X);
    }

    #[test]
    fn bar_goes_above_a_covered_start_handle() {
        let bar = placed(340.0, 300.0, 600.0);
        assert_eq!(bar.bottom(), 282.0);
    }

    #[test]
    fn bar_covering_the_start_handle_goes_past_the_end_nearer_the_mouse() {
        // A click on the last line: over the mouse the bar would span 292 to
        // 342, the start's knob 310 to 322.
        let bar = placed(350.0, 300.0, 345.0);
        assert_eq!(bar.top(), 375.0);
    }

    #[test]
    fn bar_covering_both_handles_goes_past_the_one_nearer_the_mouse() {
        let bar = placed(350.0, 300.0, 320.0);
        assert_eq!(bar.top(), 370.0);
    }

    #[test]
    fn bar_with_no_room_below_moves_up_just_clear_of_the_end_handle() {
        // The knob spans 980 to 992; below it the bar would leave the view.
        let bar = placed(998.0, 100.0, 970.0);
        assert_eq!(bar.bottom(), 980.0 - BAR_CLEARANCE);
    }

    #[test]
    fn bar_with_no_room_above_moves_down_just_clear_of_the_start_handle() {
        // Under the mouse at the view's top it would span 40 to 90, the
        // knob 50 to 62; above the line it would leave the view.
        let bar = placed(20.0, 40.0, 600.0);
        assert_eq!(bar.top(), 62.0 + BAR_CLEARANCE);
    }

    #[test]
    fn bar_on_a_short_selection_at_the_bottom_goes_past_the_other_end() {
        // Moving up off the end's knob lands on the start's, 950 to 962.
        let bar = placed(995.0, 940.0, 970.0);
        assert_eq!(bar.bottom(), 922.0);
    }

    #[test]
    fn bar_pushed_in_from_the_windows_other_edge_still_clears_a_knob() {
        let start = Some((pos2(660.0, 290.0), pos2(660.0, 310.0)));
        let bar = place_bar(pos2(980.0, 340.0), [start, line(800.0)], 1000.0, None);
        assert_eq!(bar.right(), 1000.0);
        assert_eq!(bar.bottom(), 282.0);
    }

    #[test]
    fn bar_on_a_short_selection_at_the_top_goes_past_the_other_end() {
        // Moving down off the start's knob lands on the end's, 80 to 92.
        assert_eq!(placed(20.0, 40.0, 70.0).top(), 100.0);
    }

    #[test]
    fn bar_covering_both_handles_clears_the_one_nearer_the_mouse() {
        // Over the mouse it covers both knobs; past the start it lands on
        // the end's knob, so it moves off the start's, and past the mouse.
        let span = |top, bottom| Some((pos2(500.0, top), pos2(500.0, bottom)));
        let bar = placed_in(1000.0, 315.0, span(271.8, 279.9), span(241.7, 249.8));
        assert_eq!(bar.top(), 335.0);
        // Below the mouse it would span 50 to 100, over both knobs: the
        // start's 95 to 107 and the end's 90 to 102. The end's is nearer, so
        // it moves up off that one.
        assert_eq!(placed(30.0, 85.0, 80.0).bottom(), 88.0);
    }

    #[test]
    fn bar_is_not_moved_clear_of_a_knob_onto_the_mouse_going_up() {
        // Below the mouse it would span 50 to 100, on the end's knob 60 to
        // 72. Moved up clear of that, it would cover the mouse; past either
        // end it doesn't fit, so it stays.
        assert_eq!(placed(30.0, 100.0, 50.0).top(), 50.0);
    }

    #[test]
    fn a_knob_centred_out_of_view_still_counts() {
        // The start's knob spans -7 to 5: no handle shows, but its lower part
        // is in view, and the bar over the mouse would span 2 to 52.
        let bar = placed_in(1000.0, 60.0, line(-17.0), line(500.0));
        assert_eq!(bar.top(), 5.0 + BAR_CLEARANCE);
    }

    #[test]
    fn bar_below_a_caret_as_the_anchor_goes_from_its_bottom() {
        // No page point to hang from: the anchor is a caret, 10 to 30.
        let (top, bottom) = (pos2(X, 10.0), pos2(X, 30.0));
        let view = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0));
        let (pos, pivot) = bar_place((top, bottom), [None, None], view, view, SIZE, GAP, None);
        assert_eq!(pivot.anchor_size(pos, SIZE).top(), 50.0);
    }

    #[test]
    fn bar_stays_put_when_nothing_fits() {
        let bar = placed_in(60.0, 30.0, line(45.0), line(55.0));
        assert_eq!(bar.top(), 50.0);
    }

    #[test]
    fn each_colour_fades_differently_in_light_and_dark_mode() {
        for colour in HighlightColour::ALL {
            let (light, dark) = (highlight_fill(colour, false), highlight_fill(colour, true));
            assert!(light.a() < 255 && dark.a() < 255, "opaque {colour:?}");
            assert_ne!(light, dark, "{colour:?}");
        }
    }
}
