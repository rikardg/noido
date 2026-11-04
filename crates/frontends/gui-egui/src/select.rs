//! The window's text selection: the select tool, the selection, and the
//! drag under way.

use eframe::egui::{self, Pos2, Rect, Vec2, vec2};
use noido_core::{Caret, DragKind, Layout, PageTextState, Selection, Spot, spot_at};

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
}

/// The radius of a handle's knob, in points.
pub const HANDLE_RADIUS: f32 = 6.0;
/// How near a handle's knob a press takes the handle, in points.
pub const HANDLE_REACH: f32 = 12.0;

/// How far the pointer moves before a press is a drag, in points.
pub const DRAG_THRESHOLD: f32 = 3.0;

#[derive(Default)]
pub struct State {
    /// With the select tool on, a plain drag selects.
    pub tool: bool,
    pub selection: Option<Selection>,
    pub drag: Option<Drag>,
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
    /// The pages (not a scroll bar, the sidebar or a popup over them) are
    /// under the pointer.
    pub pages_hovered: bool,
    /// The selection Copy was asked for while a page's text in it was
    /// still to come; dropped when the selection changes.
    pub copy_pending: Option<Selection>,
}

impl State {
    /// Drops the selection and any drag: a new version of the document.
    pub fn clear(&mut self) {
        self.selection = None;
        self.drag = None;
        self.copy_pending = None;
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

    /// Places the carets, handles of the selection on screen for the
    /// pages' content at `content`, showing `viewport`.
    pub fn place(&mut self, texts: &Texts, layout: &Layout, content: Rect, viewport: Rect) {
        self.carets = [None, None];
        self.caret_lines = [None, None];
        self.handles = [None, None];
        let Some(selection) = self.selection else {
            return;
        };
        let Some((start, end)) = selection.carets() else {
            return;
        };
        if selection.is_empty(|page| texts.get(page)) {
            return;
        }
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

    /// Follows the drag under way on the pages, whose content starts at
    /// `content`'s top left: a selecting drag moves the selection's focus;
    /// a click clears the selection. Also notes whether the pointer is
    /// over text, and places selection ends waiting for their page's text.
    pub fn update(&mut self, texts: &Texts, layout: &Layout, ui: &egui::Ui, content: Rect) {
        let spot = |pos: egui::Pos2| {
            let p = layout.doc_point(pos.x - content.min.x, pos.y - content.min.y)?;
            Some(spot_at(layout, p, texts.get(p.page)))
        };
        let (pointer, released) =
            ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_released()));
        if let Some(drag) = &mut self.drag {
            if let Some(pos) = pointer
                && (pos - drag.origin).length() > DRAG_THRESHOLD
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
            let empty = self
                .selection
                .is_some_and(|s| s.is_empty(|page| texts.get(page)));
            let click = !drag.moved && drag.kind != DragKind::Handle;
            if click || (drag.kind != DragKind::Pan && empty) {
                self.selection = None;
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
