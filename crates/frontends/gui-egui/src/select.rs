//! The window's text selection: the select tool, the selection, and the
//! drag under way.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2, vec2};
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

#[derive(Default)]
pub struct State {
    /// With the select tool on, a plain drag selects.
    pub tool: bool,
    pub selection: Option<Selection>,
    /// The highlight being edited: its index in [`Highlights::items`].
    pub editing: Option<usize>,
    pub drag: Option<Drag>,
    /// A highlight being edited was clicked: the page point clicked,
    /// projected to screen each frame into [`State::bar_at`].
    bar_doc: Option<DocPoint>,
    /// Where the bar hangs from on screen, in the same coordinates as
    /// `caret_lines`; `None` hangs it from the selection's end.
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

    /// Places the carets, handles of the selection on screen for the
    /// pages' content at `content`, showing `viewport`.
    pub fn place(&mut self, texts: &Texts, layout: &Layout, content: Rect, viewport: Rect) {
        self.carets = [None, None];
        self.caret_lines = [None, None];
        self.handles = [None, None];
        // The click the bar hangs from, moved with the page it was made
        // on: scrolling, zooming or flipping pages takes it along.
        self.bar_at = self
            .bar_doc
            .and_then(|p| layout.content_point(p))
            .map(|(x, y)| content.min + vec2(x, y));
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
    /// a click selects the highlight under it, or clears the selection when
    /// there is none or `on_link` gives the click to a link. Also notes
    /// whether the pointer is over text, and places selection ends waiting
    /// for their page's text.
    pub fn update(
        &mut self,
        texts: &Texts,
        layout: &Layout,
        ui: &egui::Ui,
        content: Rect,
        highlights: &mut Highlights,
        on_link: bool,
    ) {
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
                    self.bar_doc = None;
                    self.bar_at = None;
                    selection.anchor = Spot::At(if end == 0 { last } else { start });
                }
                if let Some(focus) = pointer.and_then(|pos| spot(pos - grab)) {
                    selection.focus = focus;
                }
            }
            if drag.kind == DragKind::Select && drag.moved {
                self.editing = None;
                self.bar_doc = None;
                self.bar_at = None;
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
                self.bar_doc = None;
                self.bar_at = None;
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
                // Shift+click extends, tool on or off. The bar goes back to
                // the focus caret it now hangs from.
                if let (Some(selection), Some(to)) = (&mut self.selection, spot(drag.origin)) {
                    selection.extend_to(to);
                }
                self.bar_doc = None;
                self.bar_at = None;
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
                    // The bar comes up under the pointer, not at the
                    // highlight's far end.
                    self.bar_doc = Some(p);
                }
            } else {
                let empty = self
                    .selection
                    .is_some_and(|s| s.is_empty(|page| texts.get(page)));
                if drag.kind != DragKind::Pan && empty {
                    self.selection = None;
                }
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

    #[test]
    fn each_colour_fades_differently_in_light_and_dark_mode() {
        for colour in HighlightColour::ALL {
            let (light, dark) = (highlight_fill(colour, false), highlight_fill(colour, true));
            assert!(light.a() < 255 && dark.a() < 255, "opaque {colour:?}");
            assert_ne!(light, dark, "{colour:?}");
        }
    }
}
