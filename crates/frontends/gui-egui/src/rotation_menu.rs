//! The right-click menu that turns a page: four absolute angles, clockwise
//! from the page as the PDF shows it.

use eframe::egui::{self, Color32, RichText};
use noido_core::Rotation;

/// Draws the four angles into `ui` under a "Rotation" heading, a check by
/// `current`. Gives the one
/// chosen, unless it is `current` (choosing it again changes nothing).
fn rotation_menu(ui: &mut egui::Ui, current: Rotation) -> Option<Rotation> {
    let text = ui.visuals().widgets.inactive.fg_stroke.color;
    // Solid, as the title bar's menu draws it: see `titlebar::MARK`.
    let check = text.to_opaque();
    let mut chosen = None;
    // A heading, not an entry: what the angles below are for.
    ui.label(RichText::new("Rotation").color(ui.visuals().weak_text_color()));
    ui.separator();
    for rotation in Rotation::ALL {
        let mark = RichText::new(crate::titlebar::MARK).color(if rotation == current {
            check
        } else {
            Color32::TRANSPARENT
        });
        let label = RichText::new(format!("{}°", rotation.degrees())).color(text);
        if ui.add(egui::Button::new((mark, label))).clicked() {
            chosen = Some(rotation);
            ui.close();
        }
    }
    chosen.filter(|&rotation| rotation != current)
}

/// The menu on a right-click of `response`. `None` while it is shut;
/// otherwise the angle chosen this frame, if any. It closes on a click
/// outside it or on an angle, not on its heading.
pub fn on(response: &egui::Response, current: Rotation) -> Option<Option<Rotation>> {
    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| rotation_menu(ui, current))
        .map(|shown| shown.inner)
}
