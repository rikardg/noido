//! The right-click menu on a page: the ways to link it, how the pages are
//! laid out, and a submenu of the four absolute angles that turn it,
//! clockwise from the page as the PDF shows it.

use eframe::egui::{self, Color32, RichText};
use noido_core::{
    Action, Check, Keymap, Platform, Rotation, ViewMode, shortcut_label, view_entries,
};

/// What the page menu shows: the page's turn and how the pages are laid
/// out.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub rotation: Rotation,
    pub mode: ViewMode,
    pub odd_left: bool,
    pub gapless: bool,
    pub scale_to_fit: bool,
}

impl State {
    /// Whether the entry with `check` is checked in this state.
    fn checked(&self, check: Option<Check>) -> bool {
        match check {
            Some(Check::Continuous) => self.mode.continuous,
            Some(Check::Dual) => self.mode.dual,
            Some(Check::OddLeft) => self.odd_left,
            Some(Check::Gapless) => self.gapless,
            Some(Check::ScaleToFit) => self.scale_to_fit,
            Some(Check::Sidebar | Check::SelectTool | Check::Fullscreen) | None => false,
        }
    }
}

/// What the reader chose from a page's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Turn the page to this absolute angle.
    Rotation(Rotation),
    /// Lay the pages out this way.
    ViewMode(ViewMode),
    /// Carry out an action from the title bar's view section.
    Action(Action),
    /// Copy the link to the page, as Markdown when `markdown`.
    CopyLink { markdown: bool },
}

/// The side of a View mode submenu button, in points.
const VIEW_MODE_BUTTON: f32 = 36.0;
/// The icon's side within it: twice what the title bar's buttons give.
const VIEW_MODE_GLYPH: f32 = 30.0;

/// The View mode submenu's button for `mode`: its identity between frames,
/// and how a test finds it.
pub(crate) fn view_mode_button_id(mode: ViewMode) -> egui::Id {
    egui::Id::unique(("view-mode", mode.dual, mode.continuous))
}

/// What the icon button for `mode` says on hover.
fn view_mode_hint(mode: ViewMode) -> &'static str {
    match (mode.dual, mode.continuous) {
        (true, true) => "Spread, continuous scrolling",
        (true, false) => "Spread, one page at a time",
        (false, true) => "Single page, continuous scrolling",
        (false, false) => "Single page, one at a time",
    }
}

/// The x a menu entry's label starts at, from the Ui's left edge: the
/// check mark, the gap after it and the button's own padding.
fn label_inset(ui: &egui::Ui) -> f32 {
    let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
    let colour = ui.visuals().widgets.inactive.fg_stroke.color;
    let ctx = ui.ctx().clone();
    let mark = ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(crate::titlebar::MARK.to_owned(), font, colour)
            .rect
            .width()
    });
    ui.spacing().button_padding.x + mark + ui.spacing().item_spacing.x
}

/// The four view modes in one row of icons, the current one filled;
/// gives the one chosen.
fn view_mode_row(ui: &mut egui::Ui, current: ViewMode) -> Option<ViewMode> {
    // Start the row where the menu's labels do, not their marks.
    let inset = label_inset(ui);
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.add_space(inset);
        for mode in ViewMode::ALL {
            let response = crate::icons::button_with_glyph(
                ui,
                view_mode_button_id(mode),
                VIEW_MODE_BUTTON,
                VIEW_MODE_GLYPH,
                mode == current,
                |painter, rect, colour| {
                    crate::icons::paint_view_mode_icon(painter, rect, colour, mode);
                },
            );
            let response = crate::theme::hover_text(response, view_mode_hint(mode));
            if response.clicked() {
                chosen = Some(mode);
            }
        }
    });
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

/// The turn entry's label: the page's current angle, when it has one.
fn turn_label(rotation: Rotation) -> String {
    match rotation {
        Rotation::R0 => "Rotate page".to_owned(),
        turned => format!("Rotate page ({}°)", turned.degrees()),
    }
}

/// The menu's entries, in `ui`; gives what was chosen.
fn page_menu(ui: &mut egui::Ui, state: State, keymap: &Keymap) -> Option<Choice> {
    let text = ui.visuals().widgets.inactive.fg_stroke.color;
    // Solid, as the title bar's menu draws it: see `titlebar::MARK`.
    let check = text.to_opaque();
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Other
    };
    let shortcut = |action| {
        keymap
            .key_for(action)
            .map(|press| shortcut_label(press, platform))
            .unwrap_or_default()
    };
    // Every entry has a mark, so the labels line up whether they can be
    // checked or not.
    let mark = |checked: bool| {
        RichText::new(crate::titlebar::MARK).color(if checked {
            check
        } else {
            Color32::TRANSPARENT
        })
    };
    let mut chosen = None;
    for (markdown, label, action) in [
        (false, "Copy link to page", Action::CopyLink),
        (true, "Copy Markdown link to page", Action::CopyMarkdownLink),
    ] {
        if ui
            .add(
                egui::Button::new((mark(false), RichText::new(label).color(text)))
                    .shortcut_text(shortcut(action)),
            )
            .clicked()
        {
            chosen = Some(Choice::CopyLink { markdown });
            ui.close();
        }
    }
    ui.separator();
    if let Some(mode) = view_mode_row(ui, state.mode) {
        chosen = Some(Choice::ViewMode(mode));
    }
    // The title bar's view section, below the grid: the same entries and
    // shortcuts, for now.
    for entry in view_entries() {
        let label = RichText::new(entry.label).color(text);
        let button = egui::Button::new((mark(state.checked(entry.check)), label))
            .shortcut_text(shortcut(entry.action));
        if ui.add(button).clicked() {
            chosen = Some(Choice::Action(entry.action));
            ui.close();
        }
    }
    ui.separator();
    // The angles live in a submenu: turning is rarer than linking, and
    // four rows of degrees would keep the links apart.
    let angles = ui.menu_button(
        (
            mark(false),
            RichText::new(turn_label(state.rotation)).color(text),
        ),
        |ui| {
            let mut chosen = None;
            for rotation in Rotation::ALL {
                // The checks stand at the submenu's left, the degrees at
                // its right.
                let label = RichText::new(format!("{}°", rotation.degrees())).color(text);
                let button = egui::Button::new(mark(rotation == state.rotation)).right_text(label);
                if ui.add(button).clicked() {
                    if rotation != state.rotation {
                        chosen = Some(rotation);
                    }
                    ui.close();
                }
            }
            chosen
        },
    );
    if let Some(Some(rotation)) = angles.inner {
        chosen = Some(Choice::Rotation(rotation));
    }
    chosen
}

/// The menu on a right-click of `response`. `None` while it is shut;
/// otherwise what was chosen this frame, if anything. It closes on a click
/// outside it or on a choice.
pub fn on(response: &egui::Response, state: State, keymap: &Keymap) -> Option<Option<Choice>> {
    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| page_menu(ui, state, keymap))
        .map(|shown| shown.inner)
}
