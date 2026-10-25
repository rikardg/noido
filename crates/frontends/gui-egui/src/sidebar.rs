//! The left sidebar. Empty for now; search results, thumbnails and the
//! outline will appear in it.

use eframe::egui;
use noido_core::{SIDEBAR_MAX_FRACTION, SIDEBAR_MIN_WIDTH, SidebarState};

const PLACEHOLDER: &str = "Search results, thumbnails and the outline will appear here";

/// Draws the open sidebar at the left of `ui` and records its width, which
/// the reader may have changed by dragging its edge.
pub fn show(ui: &mut egui::Ui, sidebar: &mut SidebarState) {
    let window = ui.max_rect().width();
    let max = (window * SIDEBAR_MAX_FRACTION).max(SIDEBAR_MIN_WIDTH);
    let response = egui::Panel::left("sidebar")
        .resizable(true)
        .default_size(sidebar.clamped(window).width)
        .size_range(SIDEBAR_MIN_WIDTH..=max)
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(PLACEHOLDER).weak()).wrap());
            // A resizable panel needs its contents to fill it.
            ui.take_available_space();
        });
    sidebar.width = response.response.rect.width();
}
