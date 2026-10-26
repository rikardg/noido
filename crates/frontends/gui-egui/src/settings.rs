//! The settings page: what a document noido hasn't seen opens with.

use eframe::egui;
use noido_core::{Defaults, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT, StartZoom, ViewState};
use noido_settings::Settings;

/// The open settings page.
#[derive(Default)]
pub struct Page {
    /// Why the last change couldn't be saved.
    pub error: Option<String>,
}

pub struct Output {
    /// The settings changed: save them.
    pub changed: bool,
    pub close: bool,
}

/// Shows the page over the window. `current` is the open document's view,
/// for "Set from current view".
pub fn show(
    page: &Page,
    ctx: &egui::Context,
    settings: &mut Settings,
    current: Option<ViewState>,
) -> Output {
    let before = *settings;
    let new = &mut settings.new_documents;
    let builtin = Defaults::default();
    let on_off = |on: bool| if on { "on" } else { "off" };
    let mut close = false;
    let response = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading("Settings");
        ui.add_space(4.0);
        ui.label("New documents open with:");
        ui.add_space(8.0);

        ui.label(format!("Zoom (default: {})", zoom_name(builtin.zoom)));
        ui.indent("zoom", |ui| {
            ui.radio_value(&mut new.zoom, StartZoom::FitWidth, "Fit width");
            ui.radio_value(&mut new.zoom, StartZoom::FitPage, "Fit page");
            ui.horizontal(|ui| {
                ui.radio_value(&mut new.zoom, StartZoom::Fixed, "Fixed");
                ui.add_enabled(
                    new.zoom == StartZoom::Fixed,
                    egui::DragValue::new(&mut new.zoom_percent)
                        .range(MIN_ZOOM_PERCENT..=MAX_ZOOM_PERCENT)
                        .suffix(" %"),
                );
            });
        });
        ui.add_space(4.0);
        let label = |name: &str, on: bool| format!("{name} (default: {})", on_off(on));
        ui.checkbox(&mut new.continuous, label("Continuous", builtin.continuous));
        ui.checkbox(&mut new.dual, label("Dual page", builtin.dual));
        ui.checkbox(&mut new.odd_left, label("Odd pages left", builtin.odd_left));
        ui.add_space(8.0);

        let from_view = ui.add_enabled(
            current.is_some(),
            egui::Button::new("Set from current view"),
        );
        if from_view.clicked()
            && let Some(current) = &current
        {
            *new = Defaults::from_view(current);
        }
        if let Some(error) = &page.error {
            ui.add_space(4.0);
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button("Close").clicked();
        });
    });
    Output {
        changed: *settings != before,
        close: close || response.should_close(),
    }
}

fn zoom_name(zoom: StartZoom) -> &'static str {
    match zoom {
        StartZoom::FitWidth => "fit width",
        StartZoom::FitPage => "fit page",
        StartZoom::Fixed => "fixed",
    }
}
