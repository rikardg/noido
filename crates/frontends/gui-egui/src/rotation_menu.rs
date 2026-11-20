//! The right-click menu on a page: the ways to link it, and a submenu of
//! the four absolute angles that turn it, clockwise from the page as the
//! PDF shows it.

use eframe::egui::{self, Color32, RichText};
use noido_core::{Action, Keymap, Platform, Rotation, shortcut_label};

/// What the reader chose from a page's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Turn the page to this absolute angle.
    Rotation(Rotation),
    /// Copy the link to the page, as Markdown when `markdown`.
    CopyLink { markdown: bool },
}

/// The menu's entries, in `ui`; gives what was chosen.
fn page_menu(ui: &mut egui::Ui, current: Rotation, keymap: &Keymap) -> Option<Choice> {
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
    let mut chosen = None;
    for (markdown, label, action) in [
        (false, "Copy link to page", Action::CopyLink),
        (true, "Copy Markdown link to page", Action::CopyMarkdownLink),
    ] {
        if ui
            .add(
                egui::Button::new(RichText::new(label).color(text)).shortcut_text(shortcut(action)),
            )
            .clicked()
        {
            chosen = Some(Choice::CopyLink { markdown });
            ui.close();
        }
    }
    ui.separator();
    // The angles live in a submenu: turning is rarer than linking, and
    // four rows of degrees would keep the links apart.
    let angles = ui.menu_button(RichText::new("Rotation").color(text), |ui| {
        let mut chosen = None;
        for rotation in Rotation::ALL {
            let mark = RichText::new(crate::titlebar::MARK).color(if rotation == current {
                check
            } else {
                Color32::TRANSPARENT
            });
            // The checks stand at the submenu's left, the degrees at its
            // right.
            let label = RichText::new(format!("{}°", rotation.degrees())).color(text);
            if ui.add(egui::Button::new(mark).right_text(label)).clicked() {
                if rotation != current {
                    chosen = Some(rotation);
                }
                ui.close();
            }
        }
        chosen
    });
    if let Some(Some(rotation)) = angles.inner {
        chosen = Some(Choice::Rotation(rotation));
    }
    chosen
}

/// The menu on a right-click of `response`. `None` while it is shut;
/// otherwise what was chosen this frame, if anything. It closes on a click
/// outside it, on one of its entries, or on an angle.
pub fn on(response: &egui::Response, current: Rotation, keymap: &Keymap) -> Option<Option<Choice>> {
    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| page_menu(ui, current, keymap))
        .map(|shown| shown.inner)
}
