//! The window's text selection: the select tool, the selection, and the
//! drag under way.

use eframe::egui::{self, Pos2, Rect};
use noido_core::{DragKind, Layout, PageTextState, Selection, spot_at};

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
}

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
}

impl State {
    /// Drops the selection and any drag: a new version of the document.
    pub fn clear(&mut self) {
        self.selection = None;
        self.drag = None;
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
            if !drag.moved || (drag.kind == DragKind::Select && empty) {
                self.selection = None;
            }
        }
        self.over_text = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| {
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
