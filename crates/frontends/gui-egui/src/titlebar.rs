//! noido's own title bar: sidebar toggle, search field, the document's name
//! with page and zoom, and the menu.

use eframe::egui::{
    self, Align, Color32, Layout, Painter, Rect, Response, RichText, Sense, Stroke, TextStyle,
    TextWrapMode, WidgetText, pos2, vec2,
};
use noido_core::{Action, Check, Keymap, Platform, SidebarState, menu, shortcut_label};

/// Height of the bar, in UI points.
pub const TITLE_BAR_HEIGHT: f32 = 40.0;
/// The narrowest the search field gets.
const SEARCH_MIN_WIDTH: f32 = 160.0;
/// The search field's width while the sidebar is hidden.
const SEARCH_HIDDEN_WIDTH: f32 = 200.0;
/// Side of the square icon buttons.
const ICON_BUTTON: f32 = 28.0;
/// Space between the name and the page and zoom text.
const STATUS_GAP: f32 = 8.0;

/// What the bar shows.
pub struct Bar<'a> {
    pub name: &'a str,
    /// Page and zoom, see [`status_text`].
    pub status: String,
    pub sidebar: SidebarState,
    pub continuous: bool,
    pub dual: bool,
    pub keymap: &'a Keymap,
}

/// Where the bar's parts were drawn last frame.
#[derive(Clone, Copy, Debug)]
pub struct Rects {
    pub search: Rect,
    pub menu: Rect,
}

impl Default for Rects {
    fn default() -> Self {
        Self {
            search: Rect::NOTHING,
            menu: Rect::NOTHING,
        }
    }
}

/// What the reader did in the bar this frame.
pub struct Output {
    /// From the sidebar toggle and the menu.
    pub actions: Vec<Action>,
    /// Enter was pressed in the search field.
    pub submitted: bool,
    /// Esc was pressed in the search field.
    pub cancelled: bool,
    pub rects: Rects,
}

/// "3 / 10 · 106 %" for `page` (from 0) of `count` at `scale`.
pub fn status_text(page: usize, count: usize, scale: f32) -> String {
    format!("{} / {count} · {} %", page + 1, (scale * 100.0).round())
}

/// Draws the bar's contents into `ui`, the title bar panel. `focus` puts the
/// cursor in the search field.
pub fn show(ui: &mut egui::Ui, bar: &Bar, search: &mut String, focus: bool) -> Output {
    let mut output = Output {
        actions: vec![],
        submitted: false,
        cancelled: false,
        rects: Rects::default(),
    };
    let full = ui.max_rect();
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        let toggle =
            icon_button(ui, bar.sidebar.open, paint_sidebar_icon).on_hover_text("Sidebar (F9)");
        if toggle.clicked() {
            output.actions.push(Action::ToggleSidebar);
        }

        let width = if bar.sidebar.open {
            // Ends where the sidebar ends, so it sits above the results.
            let right = full.left() + bar.sidebar.width;
            right - ui.cursor().left() - ui.spacing().item_spacing.x
        } else {
            SEARCH_HIDDEN_WIDTH
        };
        let field = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("Search")
                .desired_width(width.max(SEARCH_MIN_WIDTH)),
        );
        let (enter, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if field.lost_focus() && enter {
            output.submitted = true;
            // Stays in the field: Enter will step through hits.
            field.request_focus();
        }
        output.cancelled = field.lost_focus() && escape;
        if focus {
            field.request_focus();
        }
        output.rects.search = field.rect;

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let menu_button = icon_button(ui, false, paint_menu_icon).on_hover_text("Menu");
            output.rects.menu = menu_button.rect;
            egui::Popup::menu(&menu_button).show(|ui| {
                menu_entries(ui, bar, &mut output.actions);
            });

            let rest = ui.available_rect_before_wrap();
            paint_title(ui, rest, full.center().x, bar);
        });
    });
    output
}

fn menu_entries(ui: &mut egui::Ui, bar: &Bar, actions: &mut Vec<Action>) {
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Other
    };
    for (i, section) in menu().iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        for entry in section {
            let checked = match entry.check {
                Some(Check::Continuous) => bar.continuous,
                Some(Check::Dual) => bar.dual,
                Some(Check::Sidebar) => bar.sidebar.open,
                None => false,
            };
            // An invisible mark keeps all labels aligned.
            let mark = RichText::new("✔").color(if checked {
                ui.visuals().text_color()
            } else {
                Color32::TRANSPARENT
            });
            let shortcut = bar
                .keymap
                .key_for(entry.action)
                .map(|press| shortcut_label(press, platform))
                .unwrap_or_default();
            if ui
                .add(egui::Button::new((mark, entry.label)).shortcut_text(shortcut))
                .clicked()
            {
                actions.push(entry.action);
            }
        }
    }
}

/// The name, elided to fit, followed by the muted page and zoom; centred on
/// `centre` (the window's centre) where `rect` allows.
fn paint_title(ui: &egui::Ui, rect: Rect, centre: f32, bar: &Bar) {
    let status = WidgetText::from(RichText::new(&bar.status).weak()).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    let name_width = (rect.width() - status.size().x - STATUS_GAP).max(0.0);
    let name = WidgetText::from(bar.name).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        name_width,
        TextStyle::Body,
    );
    let total = name.size().x + STATUS_GAP + status.size().x;
    let left = (centre - total / 2.0).clamp(rect.left(), (rect.right() - total).max(rect.left()));
    let painter = ui.painter_at(rect);
    let colour = ui.visuals().text_color();
    let top = |height: f32| rect.center().y - height / 2.0;
    let status_left = left + name.size().x + STATUS_GAP;
    painter.galley(pos2(left, top(name.size().y)), name, colour);
    painter.galley(pos2(status_left, top(status.size().y)), status, colour);
}

/// A square button with a painted icon, highlighted while `selected`.
pub fn icon_button(
    ui: &mut egui::Ui,
    selected: bool,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ICON_BUTTON, ICON_BUTTON), Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter();
    if selected || response.hovered() || response.has_focus() {
        painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    paint(painter, rect.shrink(8.0), visuals.fg_stroke.color);
    response
}

/// A panel outline with a bar down its left side.
fn paint_sidebar_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    let x = rect.left() + rect.width() * 0.35;
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
}

/// Three horizontal lines.
fn paint_menu_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    for y in [rect.top() + 2.0, rect.center().y, rect.bottom() - 2.0] {
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
    }
}
