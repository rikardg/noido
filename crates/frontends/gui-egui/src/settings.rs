//! The settings page: the paper colour, and what a document noido hasn't
//! seen opens with.

use eframe::egui;
use noido_core::{Defaults, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT, Paper, StartZoom};
use noido_settings::Settings;

use crate::image::paper_colour;

/// The side of a paper colour square.
const SWATCH: f32 = 20.0;
/// The alpha of the black that dims the document behind the page: egui's.
const SCRIM_ALPHA: f32 = 100.0;
/// How long the scrim takes to clear, or come back.
const SCRIM_FADE: f32 = 0.15;

/// The open settings page.
#[derive(Default)]
pub struct Page {
    /// Why the last change couldn't be saved.
    pub error: Option<String>,
    /// The pointer was over the Paper row last frame: the scrim clears, so
    /// the paper can be judged.
    paper_hovered: bool,
}

pub struct Output {
    /// The settings changed: save them.
    pub changed: bool,
    pub close: bool,
}

/// Shows the page over the window. `current` is how the open document is
/// shown, for "Set from current view".
pub fn show(
    page: &mut Page,
    ctx: &egui::Context,
    settings: &mut Settings,
    current: Option<Defaults>,
) -> Output {
    let before = *settings;
    let paper = &mut settings.paper;
    let new = &mut settings.new_documents;
    let builtin = Defaults::default();
    let on_off = |on: bool| if on { "on" } else { "off" };
    let mut close = false;
    let id = egui::Id::unique("settings");
    let clear = ctx.animate_bool_with_time(id.with("scrim"), page.paper_hovered, SCRIM_FADE);
    let scrim = egui::Color32::from_black_alpha((SCRIM_ALPHA * (1.0 - clear)).round() as u8);
    let modal = egui::Modal::new(id).backdrop_color(scrim);
    let mut paper_hovered = false;
    let response = modal.show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading("Settings");
        ui.add_space(4.0);
        let row = ui.horizontal(|ui| {
            ui.label("Paper");
            for choice in Paper::ALL {
                swatch(ui, paper, choice);
            }
        });
        paper_hovered = row.response.contains_pointer();
        ui.add_space(12.0);
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
        ui.checkbox(&mut new.sidebar, label("Show sidebar", builtin.sidebar));
        ui.add_space(8.0);

        let from_view = ui.add_enabled(
            current.is_some(),
            egui::Button::new("Set from current view"),
        );
        if from_view.clicked()
            && let Some(current) = current
        {
            *new = current;
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
    page.paper_hovered = paper_hovered;
    Output {
        changed: *settings != before,
        close: close || response.should_close(),
    }
}

/// A square of `choice`'s colour that chooses it, framed when chosen.
fn swatch(ui: &mut egui::Ui, paper: &mut Paper, choice: Paper) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::click());
    let chosen = *paper == choice;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, true, chosen, choice.name())
    });
    if response.clicked() {
        *paper = choice;
    }
    let visuals = ui.visuals();
    let stroke = if chosen {
        egui::Stroke::new(2.0, visuals.selection.bg_fill)
    } else {
        // The text colour, brighter on hover: visible on light and dark.
        egui::Stroke::new(1.0, ui.style().interact(&response).fg_stroke.color)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, paper_colour(choice));
    painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);
    response.on_hover_text(choice.name());
}

fn zoom_name(zoom: StartZoom) -> &'static str {
    match zoom {
        StartZoom::FitWidth => "fit width",
        StartZoom::FitPage => "fit page",
        StartZoom::Fixed => "fixed",
    }
}
