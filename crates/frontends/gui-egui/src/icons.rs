//! The icons noido paints itself: a few strokes each, in the button's own
//! colour. No icon font, no sprite sheet.

use eframe::egui::{self, Color32, Painter, Rect, Stroke, pos2, vec2};
use noido_core::ViewMode;

/// A square button `side` points across, with `paint`'s icon in it,
/// highlighted while `selected` or hovered. The response carries `id`, so
/// a test can find the button.
pub fn button(
    ui: &mut egui::Ui,
    id: egui::Id,
    side: f32,
    selected: bool,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> egui::Response {
    // The title bar's share of the button for its icon: 8 of 28 points.
    button_with_glyph(ui, id, side, side * 12.0 / 28.0, selected, paint)
}

/// [`button`] with the icon in a `glyph`-point square at its centre, for
/// buttons whose icons fill more of them.
pub fn button_with_glyph(
    ui: &mut egui::Ui,
    id: egui::Id,
    side: f32,
    glyph: f32,
    selected: bool,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> egui::Response {
    let (_, rect) = ui.allocate_space(vec2(side, side));
    // Senses drags too, so a drag that starts on the button isn't passed on
    // to what is underneath, like the title bar's.
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let glyph = Rect::from_center_size(rect.center(), vec2(glyph, glyph));
    paint(ui.painter(), glyph, visuals.fg_stroke.color);
    response
}

/// Two sheets, the front one with its top-right corner folded: copy.
pub fn paint_copy_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let side = rect.width();
    let at = |x: f32, y: f32| pos2(rect.left() + x * side, rect.top() + y * side);
    // The back sheet: what shows beside and above the front one.
    painter.add(egui::Shape::line(
        vec![
            at(0.38, 0.62),
            at(0.0, 0.62),
            at(0.0, 0.0),
            at(0.62, 0.0),
            at(0.62, 0.35),
        ],
        stroke,
    ));
    // The front sheet, cut where its corner folds.
    painter.add(egui::Shape::line(
        vec![
            at(0.38, 0.35),
            at(0.75, 0.35),
            at(1.0, 0.6),
            at(1.0, 1.0),
            at(0.38, 1.0),
            at(0.38, 0.35),
        ],
        stroke,
    ));
    // The fold, laid back over the sheet.
    painter.add(egui::Shape::line(
        vec![at(0.75, 0.35), at(0.75, 0.6), at(1.0, 0.6)],
        stroke,
    ));
}

/// A folder: its tab across the top left, the body under it.
pub fn paint_folder_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let side = rect.width();
    let at = |x: f32, y: f32| pos2(rect.left() + x * side, rect.top() + y * side);
    painter.add(egui::Shape::line(
        vec![
            at(0.0, 1.0),
            at(0.0, 0.15),
            at(0.42, 0.15),
            at(0.56, 0.32),
            at(1.0, 0.32),
            at(1.0, 1.0),
            at(0.0, 1.0),
        ],
        stroke,
    ));
}

/// Where a single page starts across a view mode glyph, as a fraction of
/// it.
pub(crate) const SINGLE_PAGE_LEFT: f32 = 0.27;

/// A page or spread, with two chevrons under it when the pages scroll:
/// the four view modes in one glyph.
pub fn paint_view_mode_icon(painter: &Painter, rect: Rect, colour: Color32, mode: ViewMode) {
    let stroke = Stroke::new(1.5, colour);
    let side = rect.width();
    let at = |x: f32, y: f32| pos2(rect.left() + x * side, rect.top() + y * side);
    let page = |left: f32, right: f32| {
        painter.rect_stroke(
            Rect::from_min_max(at(left, 0.02), at(right, 0.58)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    if mode.dual {
        page(0.02, 0.44);
        page(0.56, 0.98);
    } else {
        page(SINGLE_PAGE_LEFT, 1.0 - SINGLE_PAGE_LEFT);
    }
    if mode.continuous {
        // Two chevrons down, as Lucide's `chevrons-down`: the strip goes on.
        for y in [0.68, 0.84] {
            painter.add(egui::Shape::line(
                vec![at(0.36, y), at(0.5, y + 0.10), at(0.64, y)],
                stroke,
            ));
        }
    }
}
