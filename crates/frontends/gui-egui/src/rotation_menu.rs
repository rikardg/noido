//! The right-click menu that turns a page: four absolute angles, clockwise
//! from the page as the PDF shows it.

use eframe::egui::{self, Color32, RichText};
use noido_core::Rotation;

/// Draws the four angles into `ui`, a check by `current`. Gives the one
/// chosen, unless it is `current` (choosing it again changes nothing).
pub fn rotation_menu(ui: &mut egui::Ui, current: Rotation) -> Option<Rotation> {
    let text = ui.visuals().widgets.inactive.fg_stroke.color;
    let mut chosen = None;
    for rotation in Rotation::ALL {
        let mark = RichText::new("✔").color(if rotation == current {
            text
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
