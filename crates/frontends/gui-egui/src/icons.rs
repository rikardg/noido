//! The icons noido paints itself: a few strokes each, in the button's own
//! colour. No icon font, no sprite sheet.

use eframe::egui::{self, Color32, Painter, Rect, Stroke, pos2, vec2};

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
    let (_, rect) = ui.allocate_space(vec2(side, side));
    // Senses drags too, so a drag that starts on the button isn't passed on
    // to what is underneath, like the title bar's.
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    // The title bar's share of the button for its icon: 8 of 28 points.
    let glyph = rect.shrink(side * 8.0 / 28.0);
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
