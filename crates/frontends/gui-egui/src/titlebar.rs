//! noido's own title bar: sidebar toggle, search field, the document's name
//! with page and zoom, and the menu.

use std::path::PathBuf;

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::text_edit::TextEditState;
use eframe::egui::{
    self, Align, Align2, Color32, Layout, Painter, Rect, Response, RichText, Sense, Stroke,
    TextStyle, TextWrapMode, ViewportCommand, WidgetText, pos2, vec2,
};
use noido_core::{
    Action, Check, Keymap, PageNumbers, Platform, SidebarState, menu, shortcut_label,
};

use crate::theme;

/// Height of the bar, in UI points.
pub const TITLE_BAR_HEIGHT: f32 = 40.0;
/// The search field's height: egui's 20 pt and 2 pt more above and below.
const SEARCH_HEIGHT: f32 = 24.0;
/// The narrowest the search field gets.
const SEARCH_MIN_WIDTH: f32 = 160.0;
/// The search field's width while the sidebar is hidden.
const SEARCH_HIDDEN_WIDTH: f32 = 200.0;
/// Side of the square icon buttons.
const ICON_BUTTON: f32 = 28.0;
/// Space between the name and the page and zoom text.
const STATUS_GAP: f32 = 8.0;
/// Room left for macOS's window buttons at the bar's left.
const TRAFFIC_LIGHTS_INSET: f32 = 76.0;
/// The least room a scrolling menu keeps for itself, in points: enough for
/// a few entries, even when the window's edge is closer.
const MENU_LOWEST_ROOM: f32 = 96.0;

/// What the bar shows.
pub struct Bar<'a> {
    pub name: &'a str,
    /// Page and zoom, see [`status_text`].
    pub status: String,
    pub sidebar: SidebarState,
    pub continuous: bool,
    pub dual: bool,
    pub odd_left: bool,
    pub gapless: bool,
    /// The select tool is on.
    pub select_tool: bool,
    pub keymap: &'a Keymap,
    /// The match counter, shown beside the search field; empty for none.
    pub counter: String,
    /// The recently opened files, newest first.
    pub recent: &'a [PathBuf],
}

/// Where the bar's parts were drawn last frame.
#[derive(Clone, Copy, Debug)]
pub struct Rects {
    pub search: Rect,
    pub menu: Rect,
    /// The select tool's button.
    pub select: Rect,
    /// The close button; nothing on macOS, which has its own.
    pub close: Rect,
}

impl Default for Rects {
    fn default() -> Self {
        Self {
            search: Rect::NOTHING,
            menu: Rect::NOTHING,
            select: Rect::NOTHING,
            close: Rect::NOTHING,
        }
    }
}

/// What the reader did in the bar this frame.
pub struct Output {
    /// From the sidebar toggle and the menu.
    pub actions: Vec<Action>,
    /// Enter (true) or Shift+Enter (false) was pressed in the search field:
    /// go to the next or the previous hit.
    pub step: Option<bool>,
    /// Esc was pressed in the search field.
    pub cancelled: bool,
    /// The search field lost focus some other way, e.g. a click elsewhere.
    pub left: bool,
    /// The search field has focus.
    pub focused: bool,
    /// A recently opened file was chosen.
    pub recent: Option<PathBuf>,
    pub rects: Rects,
}

/// "3 / 10 · 106 %" for `page` (from 0) of `count` at `scale`, with the page's
/// label if it has one: "61 (79 / 300) · 106 %".
pub fn status_text(numbers: &PageNumbers, page: usize, count: usize, scale: f32) -> String {
    let position = numbers.title_bar(page, count);
    format!("{position} · {} %", (scale * 100.0).round())
}

/// Draws the bar's contents into `ui`, the title bar panel. `focus` puts the
/// cursor in the search field.
pub fn show(ui: &mut egui::Ui, bar: &Bar, search: &mut String, focus: bool) -> Output {
    let mut output = Output {
        actions: vec![],
        step: None,
        cancelled: false,
        left: false,
        focused: false,
        recent: None,
        rects: Rects::default(),
    };
    let full = ui.max_rect();
    let (maximized, fullscreen) = ui.input(|i| {
        let viewport = i.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    // Added before the controls, so they take presses first: what reaches
    // this is the empty part of the bar.
    let background = ui.interact(
        full,
        ui.scope_id().with("background"),
        Sense::click_and_drag(),
    );
    if background.drag_started() {
        ui.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if background.double_clicked() {
        ui.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        if cfg!(target_os = "macos") && !fullscreen {
            ui.add_space(TRAFFIC_LIGHTS_INSET);
        }
        let toggle =
            theme::hover_text(
            icon_button(ui, bar.sidebar.open, paint_sidebar_icon),
            "Sidebar (F9)",
        );
        if toggle.clicked() {
            output.actions.push(Action::ToggleSidebar);
        }
        let select = theme::hover_text(
            icon_button(ui, bar.select_tool, paint_select_icon),
            "Select text (S)",
        );
        output.rects.select = select.rect;
        if select.clicked() {
            output.actions.push(Action::ToggleSelectTool);
        }

        let width = if bar.sidebar.open {
            // Ends where the sidebar ends, so it sits above the results.
            let right = full.left() + bar.sidebar.width;
            right - ui.cursor().left() - ui.spacing().item_spacing.x
        } else {
            SEARCH_HIDDEN_WIDTH
        };
        let field = theme::scope(ui, theme::Role::Input, |ui| {
            // The native input height is taller than the compact title bar allows.
            ui.spacing_mut().interact_size.y = SEARCH_HEIGHT;
            ui.add(
                egui::TextEdit::singleline(search)
                    .hint_text("Search")
                    .desired_width(width.max(SEARCH_MIN_WIDTH))
                    .min_size(vec2(0.0, SEARCH_HEIGHT))
                    .align(Align2::LEFT_CENTER),
            )
        });
        let (enter, escape, shift) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                // From the key event itself: Shift as it was when Enter went down.
                i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if modifiers.shift)
                }),
            )
        });
        if field.lost_focus() && enter {
            output.step = Some(!shift);
            // Stays in the field: Enter will step through hits.
            field.request_focus();
        }
        output.cancelled = field.lost_focus() && escape;
        // Not when focus is being given back, as after Esc clears the field.
        output.left = field.lost_focus() && !enter && !escape && !focus;
        output.focused = field.has_focus();
        if focus {
            field.request_focus();
            // Selected, so typing replaces the last query.
            let mut state = TextEditState::load(ui.ctx(), field.id).unwrap_or_default();
            let all = CCursorRange::two(CCursor::new(0), CCursor::new(search.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), field.id);
        }
        output.rects.search = field.rect;
        if !bar.counter.is_empty() {
            ui.weak(&bar.counter);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !cfg!(target_os = "macos") {
                output.rects.close = window_buttons(ui, maximized);
                ui.add_space(ui.spacing().item_spacing.x * 2.0);
            }
            let menu_button = theme::hover_text(icon_button(ui, false, paint_menu_icon), "Menu");
            output.rects.menu = menu_button.rect;
            let menu_theme = theme::Theme::current(ui.ctx());
            let mode = ui.ctx().theme();
            egui::Popup::menu(&menu_button)
                .style(move |style: &mut egui::Style| {
                    egui::menu::menu_style(&mut *style);
                    menu_theme.apply_role(mode, style, theme::Role::Menu);
                })
                .show(|ui| {
                    // A long menu in a short window scrolls instead of
                    // running past the window's edge, where its last
                    // entries could not be reached.
                    let room = (ui.ctx().content_rect().height()
                        - 2.0 * f32::from(ui.spacing().menu_margin.top))
                        .max(MENU_LOWEST_ROOM);
                    ui.set_max_height(room);
                    egui::ScrollArea::vertical()
                        .max_height(room)
                        .auto_shrink([true, true])
                        .show(ui, |ui| {
                            menu_entries(ui, bar, &mut output.actions, &mut output.recent);
                        });
                });

            let rest = ui.available_rect_before_wrap();
            paint_title(ui, rest, full.center().x, bar);
        });
    });
    output
}

fn menu_entries(
    ui: &mut egui::Ui,
    bar: &Bar,
    actions: &mut Vec<Action>,
    recent: &mut Option<PathBuf>,
) {
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Other
    };
    let text = ui.visuals().widgets.inactive.fg_stroke.color;
    for (i, section) in menu().iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        for entry in section {
            let checked = match entry.check {
                Some(Check::Continuous) => bar.continuous,
                Some(Check::Dual) => bar.dual,
                Some(Check::OddLeft) => bar.odd_left,
                Some(Check::Gapless) => bar.gapless,
                Some(Check::Sidebar) => bar.sidebar.open,
                Some(Check::SelectTool) => bar.select_tool,
                None => false,
            };
            let mark = RichText::new("✔").color(if checked { text } else { Color32::TRANSPARENT });
            let label = RichText::new(entry.label).color(text);
            let shortcut = bar
                .keymap
                .key_for(entry.action)
                .map(|press| shortcut_label(press, platform))
                .unwrap_or_default();
            if ui
                .add(egui::Button::new((mark, label)).shortcut_text(shortcut))
                .clicked()
            {
                actions.push(entry.action);
            }
        }
        // Right under Open…, as the File menu has them, and in the same
        // section, so no rule separates them.
        if i == 0 {
            recent_submenu(ui, bar, actions, recent);
        }
    }
}

/// "Open Recent" and the files behind it: the whole names, as the
/// system's own recents show them, and "Clear recent" under them. With
/// none, the entry is there but offers nothing.
fn recent_submenu(
    ui: &mut egui::Ui,
    bar: &Bar,
    actions: &mut Vec<Action>,
    recent: &mut Option<PathBuf>,
) {
    let text = ui.visuals().widgets.inactive.fg_stroke.color;
    // The same invisible mark the other entries have, so the labels line
    // up.
    let mark = RichText::new("✔").color(Color32::TRANSPARENT);
    let entries = noido_core::recent(bar.recent);
    let room = label_room(ui, &mark);
    let width_of = width_of(ui);
    ui.add_enabled_ui(!entries.is_empty(), |ui| {
        let label = RichText::new("Open Recent").color(text);
        ui.menu_button((mark.clone(), label), |ui| {
            for entry in &entries {
                let name = noido_core::elide_to_width(&entry.label, room, &width_of);
                let label = RichText::new(&name).color(text);
                let button = ui
                    .add(egui::Button::new((mark.clone(), label)))
                    // A name cut by the screen still says where the file is.
                    .on_hover_text(entry.path.display().to_string());
                if button.clicked() {
                    *recent = Some(entry.path.clone());
                    ui.close();
                }
            }
            if entries.is_empty() {
                return;
            }
            ui.separator();
            let clear = RichText::new("Clear recent").color(text);
            if ui.add(egui::Button::new((mark.clone(), clear))).clicked() {
                actions.push(Action::ClearRecent);
                ui.close();
            }
        });
    });
}

/// How wide a label in the submenu may be, so that the submenu, which
/// opens beside the menu `ui` is drawing in, does not reach over the menu
/// nor past the screen's edge.
fn label_room(ui: &egui::Ui, mark: &RichText) -> f32 {
    let menu = ui.max_rect();
    let screen = ui.ctx().content_rect();
    let beside = (menu.left() - screen.left()).max(screen.right() - menu.right());
    let frame = egui::Frame::menu(ui.style()).total_margin();
    let spacing = ui.spacing().item_spacing.x;
    // The mark and the arrow on either side of the label, the frame
    // around them, and the gap egui leaves between the two popups: half
    // a frame each, and its own.
    let arrow = ui.spacing().icon_width + spacing;
    let gap = (frame.left + frame.right) * 0.5 + SUBMENU_GAP;
    let chrome = frame.left + frame.right + width_of(ui)(mark.text()) + spacing * 2.0 + arrow;
    // A little to spare: a submenu that misses the room by a hair opens
    // on the other side, over the menu instead.
    (beside - chrome - gap - SLACK).max(0.0)
}

/// The space egui leaves between a menu and its submenu, at least.
const SUBMENU_GAP: f32 = 2.0;
/// Room kept spare so the submenu is not placed on the menu's side.
const SLACK: f32 = 8.0;

/// How wide `text` is drawn in the menu's own text.
fn width_of(ui: &egui::Ui) -> impl Fn(&str) -> f32 + use<> {
    let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    let ctx = ui.ctx().clone();
    move |text: &str| {
        let galley =
            ctx.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), color));
        galley.rect.width()
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
    let palette = theme::Theme::current(ui.ctx()).palette(ui.ctx().theme());
    let top = |height: f32| rect.center().y - height / 2.0;
    let status_left = left + name.size().x + STATUS_GAP;
    painter.galley(pos2(left, top(name.size().y)), name, palette.title_text);
    painter.galley(
        pos2(status_left, top(status.size().y)),
        status,
        palette.muted,
    );
}

/// Minimise, maximise or restore, and close, right to left from the bar's
/// right end; returns the close button's rect.
fn window_buttons(ui: &mut egui::Ui, maximized: bool) -> Rect {
    let close = theme::hover_text(icon_button(ui, false, paint_close_icon), "Close");
    let palette = theme::Theme::current(ui.ctx()).palette(ui.ctx().theme());
    if close.hovered() {
        // Redrawn in the danger colour over the plain hover.
        ui.painter().rect_filled(
            close.rect,
            ui.visuals().widgets.hovered.corner_radius,
            palette.danger,
        );
        paint_close_icon(ui.painter(), close.rect.shrink(8.0), palette.danger_text);
    }
    if close.clicked() {
        ui.send_viewport_cmd(ViewportCommand::Close);
    }
    let (paint, hint): (fn(&Painter, Rect, Color32), _) = if maximized {
        (paint_restore_icon, "Restore")
    } else {
        (paint_maximise_icon, "Maximise")
    };
    if theme::hover_text(icon_button(ui, false, paint), hint).clicked() {
        ui.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    if theme::hover_text(icon_button(ui, false, paint_minimise_icon), "Minimise").clicked() {
        ui.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
    close.rect
}

/// A square button with a painted icon, highlighted while `selected`, as
/// wide as the bar's own buttons. A drag that starts on one isn't passed
/// on to the bar underneath, which would move the window.
fn icon_button(
    ui: &mut egui::Ui,
    selected: bool,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    crate::icons::button(ui, ui.next_auto_id(), ICON_BUTTON, selected, paint)
}

/// A panel outline with a bar down its left side.
fn paint_sidebar_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    let x = rect.left() + rect.width() * 0.35;
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
}

/// Three horizontal lines.
/// An I-beam: a stem with serifs at both ends.
fn paint_select_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let (top, bottom, x) = (rect.top() + 1.0, rect.bottom() - 1.0, rect.center().x);
    let serif = rect.width() * 0.2;
    painter.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
    for y in [top, bottom] {
        painter.line_segment([pos2(x - serif, y), pos2(x + serif, y)], stroke);
    }
}

fn paint_menu_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    for y in [rect.top() + 2.0, rect.center().y, rect.bottom() - 2.0] {
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
    }
}

fn paint_close_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let rect = rect.shrink(1.0);
    painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
}

fn paint_maximise_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    painter.rect_stroke(rect.shrink(1.0), 1.0, stroke, egui::StrokeKind::Inside);
}

/// Two overlapping squares.
fn paint_restore_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let side = rect.width() * 0.75;
    let front = Rect::from_min_size(pos2(rect.left(), rect.bottom() - side), vec2(side, side));
    let back = front.translate(vec2(rect.width() - side, side - rect.height()));
    painter.line_segment([back.left_top(), back.right_top()], stroke);
    painter.line_segment([back.right_top(), back.right_bottom()], stroke);
    painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
}

fn paint_minimise_icon(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.5, colour);
    let y = rect.bottom() - 2.0;
    painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
}
