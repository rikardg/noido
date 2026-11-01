/// Keys, independent of any UI toolkit. Frontends translate their own key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    PageUp,
    PageDown,
    Home,
    End,
    Plus,
    Equals,
    Minus,
    W,
    F,
    C,
    D,
    O,
    Q,
    G,
    I,
    L,
    Comma,
    F3,
    F9,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

impl Key {
    pub fn is_arrow(self) -> bool {
        matches!(
            self,
            Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
        )
    }
}

/// A key with the state of the platform's primary modifier (`command`: Ctrl,
/// or Cmd on macOS), Shift and Alt (Option on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Viewer actions that keys (and later menus) trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ScrollScreenUp,
    ScrollScreenDown,
    /// A small step, or a flip where the wheel flips.
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    ToggleContinuous,
    ToggleDual,
    ToggleOddLeft,
    ToggleGapless,
    ToggleSidebar,
    FocusSearch,
    NextHit,
    PreviousHit,
    Back,
    Forward,
    /// Puts a link to the current page on the clipboard.
    CopyLink,
    /// The same as a Markdown link.
    CopyMarkdownLink,
    /// Opens another document, picked in the desktop's file dialog.
    OpenDocument,
    ShowDocumentInfo,
    OpenSettings,
    /// Shows the first page.
    FirstPage,
    /// Shows the last page.
    LastPage,
    /// Closes the window; for now the same as quitting.
    CloseWindow,
    Quit,
}

/// Maps key presses to actions.
pub struct Keymap {
    /// In declaration order: the first binding of an action is the one menus show.
    bindings: Vec<(KeyPress, Action)>,
}

impl Keymap {
    /// The platform-conventional bindings, always active.
    pub fn native() -> Self {
        let plain = |key| KeyPress {
            key,
            command: false,
            shift: false,
            alt: false,
        };
        let command = |key| KeyPress {
            command: true,
            ..plain(key)
        };
        let shift = |press: KeyPress| KeyPress {
            shift: true,
            ..press
        };
        let alt = |key| KeyPress {
            alt: true,
            ..plain(key)
        };
        let bindings = vec![
            (plain(Key::PageUp), Action::ScrollScreenUp),
            (plain(Key::PageDown), Action::ScrollScreenDown),
            (plain(Key::Home), Action::FirstPage),
            (plain(Key::End), Action::LastPage),
            (plain(Key::ArrowUp), Action::ScrollUp),
            (plain(Key::ArrowDown), Action::ScrollDown),
            (plain(Key::ArrowLeft), Action::ScrollLeft),
            (plain(Key::ArrowRight), Action::ScrollRight),
            (command(Key::Equals), Action::ZoomIn),
            (command(Key::Plus), Action::ZoomIn),
            (command(Key::Minus), Action::ZoomOut),
            (plain(Key::W), Action::FitWidth),
            (plain(Key::F), Action::FitPage),
            (plain(Key::C), Action::ToggleContinuous),
            (plain(Key::D), Action::ToggleDual),
            (plain(Key::O), Action::ToggleOddLeft),
            (plain(Key::G), Action::ToggleGapless),
            (plain(Key::F9), Action::ToggleSidebar),
            (command(Key::F), Action::FocusSearch),
            (command(Key::G), Action::NextHit),
            (shift(command(Key::G)), Action::PreviousHit),
            (plain(Key::F3), Action::NextHit),
            (shift(plain(Key::F3)), Action::PreviousHit),
            (alt(Key::ArrowLeft), Action::Back),
            (alt(Key::ArrowRight), Action::Forward),
            (command(Key::L), Action::CopyLink),
            (shift(command(Key::L)), Action::CopyMarkdownLink),
            (command(Key::I), Action::ShowDocumentInfo),
            (command(Key::Comma), Action::OpenSettings),
            (command(Key::O), Action::OpenDocument),
            (command(Key::Q), Action::Quit),
            (command(Key::W), Action::CloseWindow),
        ];
        Self { bindings }
    }

    /// The action bound to `press`. While `editing` text, only presses that
    /// can't be typed act: with Mod, function keys and Page Up/Down.
    pub fn action(&self, press: KeyPress, editing: bool) -> Option<Action> {
        if editing && !acts_while_editing(press) {
            return None;
        }
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == press)
            .map(|(_, action)| *action)
    }

    /// The key shown for `action`: its first binding.
    pub fn key_for(&self, action: Action) -> Option<KeyPress> {
        self.bindings
            .iter()
            .find(|(_, bound)| *bound == action)
            .map(|(press, _)| *press)
    }
}

fn acts_while_editing(press: KeyPress) -> bool {
    press.command || matches!(press.key, Key::F3 | Key::F9 | Key::PageUp | Key::PageDown)
}

/// How shortcuts are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Other,
}

/// `press` as menus show it: "Ctrl+=" or "⌘=".
pub fn shortcut_label(press: KeyPress, platform: Platform) -> String {
    let key = match press.key {
        Key::PageUp => "Page Up",
        Key::PageDown => "Page Down",
        Key::Home => "Home",
        Key::End => "End",
        Key::Plus => "+",
        Key::Equals => "=",
        Key::Minus => "-",
        Key::W => "W",
        Key::F => "F",
        Key::C => "C",
        Key::D => "D",
        Key::O => "O",
        Key::Q => "Q",
        Key::G => "G",
        Key::I => "I",
        Key::L => "L",
        Key::Comma => ",",
        Key::F3 => "F3",
        Key::F9 => "F9",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
    };
    let (shift, alt, command) = match platform {
        Platform::Mac => ("⇧", "⌥", "⌘"),
        Platform::Other => ("Shift+", "Alt+", "Ctrl+"),
    };
    let shift = if press.shift { shift } else { "" };
    let alt = if press.alt { alt } else { "" };
    let command = if press.command { command } else { "" };
    format!("{shift}{alt}{command}{key}")
}

/// Fraction of the visible height that stays on screen when scrolling a screenful.
pub const SCREEN_OVERLAP: f32 = 0.02;

/// The scroll offset after `action`, clamped to the document.
pub fn scroll_target(
    action: Action,
    offset: f32,
    viewport_height: f32,
    content_height: f32,
) -> f32 {
    let step = viewport_height * (1.0 - SCREEN_OVERLAP);
    let target = match action {
        Action::ScrollScreenUp => offset - step,
        Action::ScrollScreenDown => offset + step,
        _ => offset,
    };
    target.clamp(0.0, (content_height - viewport_height).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, command: bool) -> KeyPress {
        KeyPress {
            key,
            command,
            shift: false,
            alt: false,
        }
    }

    fn shifted(key: Key, command: bool) -> KeyPress {
        KeyPress {
            shift: true,
            ..press(key, command)
        }
    }

    fn alted(key: Key) -> KeyPress {
        KeyPress {
            alt: true,
            ..press(key, false)
        }
    }

    #[test]
    fn alt_arrows_go_back_and_forward() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(alted(Key::ArrowLeft), false),
            Some(Action::Back)
        );
        assert_eq!(
            keymap.action(alted(Key::ArrowRight), false),
            Some(Action::Forward)
        );
        assert_eq!(
            keymap.action(press(Key::ArrowLeft, false), false),
            Some(Action::ScrollLeft)
        );
    }

    #[test]
    fn alt_arrows_go_to_the_field_while_editing() {
        let keymap = Keymap::native();
        assert_eq!(keymap.action(alted(Key::ArrowLeft), true), None);
        assert_eq!(keymap.action(alted(Key::ArrowRight), true), None);
    }

    #[test]
    fn shortcut_label_shows_alt() {
        assert_eq!(
            shortcut_label(alted(Key::ArrowLeft), Platform::Other),
            "Alt+←"
        );
        assert_eq!(shortcut_label(alted(Key::ArrowLeft), Platform::Mac), "⌥←");
    }

    #[test]
    fn arrows_scroll_but_not_while_editing() {
        let keymap = Keymap::native();
        for (key, action) in [
            (Key::ArrowUp, Action::ScrollUp),
            (Key::ArrowDown, Action::ScrollDown),
            (Key::ArrowLeft, Action::ScrollLeft),
            (Key::ArrowRight, Action::ScrollRight),
        ] {
            assert_eq!(keymap.action(press(key, false), false), Some(action));
            assert_eq!(keymap.action(press(key, false), true), None);
            assert_eq!(keymap.action(shifted(key, false), false), None);
        }
        assert_eq!(
            shortcut_label(press(Key::ArrowDown, false), Platform::Other),
            "↓"
        );
    }

    #[test]
    fn o_toggles_odd_pages_left() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::O, false), false),
            Some(Action::ToggleOddLeft)
        );
        assert_eq!(keymap.action(press(Key::O, false), true), None);
        assert_eq!(shortcut_label(press(Key::O, false), Platform::Other), "O");
    }

    #[test]
    fn g_toggles_gapless() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::G, false), false),
            Some(Action::ToggleGapless)
        );
        assert_eq!(keymap.action(press(Key::G, false), true), None);
        assert_eq!(shortcut_label(press(Key::G, false), Platform::Other), "G");
    }

    #[test]
    fn mod_o_opens_a_document() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::O, true), false),
            Some(Action::OpenDocument)
        );
        // Also while a text field has focus, like every Mod key.
        assert_eq!(
            keymap.action(press(Key::O, true), true),
            Some(Action::OpenDocument)
        );
    }

    #[test]
    fn mod_g_and_f3_go_to_next_hit() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::G, true), false),
            Some(Action::NextHit)
        );
        assert_eq!(
            keymap.action(press(Key::F3, false), false),
            Some(Action::NextHit)
        );
    }

    #[test]
    fn shift_selects_previous_hit() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(shifted(Key::G, true), false),
            Some(Action::PreviousHit)
        );
        assert_eq!(
            keymap.action(shifted(Key::F3, false), false),
            Some(Action::PreviousHit)
        );
    }

    #[test]
    fn f3_acts_while_editing() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::F3, false), true),
            Some(Action::NextHit)
        );
        assert_eq!(
            keymap.action(shifted(Key::F3, false), true),
            Some(Action::PreviousHit)
        );
    }

    #[test]
    fn shift_c_does_not_toggle() {
        let keymap = Keymap::native();
        assert_eq!(keymap.action(shifted(Key::C, false), false), None);
        assert_eq!(keymap.action(shifted(Key::D, false), false), None);
    }

    #[test]
    fn mod_l_copies_links() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::L, true), false),
            Some(Action::CopyLink)
        );
        assert_eq!(
            keymap.action(shifted(Key::L, true), false),
            Some(Action::CopyMarkdownLink)
        );
        assert_eq!(keymap.action(press(Key::L, false), false), None);
        assert_eq!(
            shortcut_label(shifted(Key::L, true), Platform::Other),
            "Shift+Ctrl+L"
        );
    }

    #[test]
    fn shortcut_label_shows_shift() {
        assert_eq!(
            shortcut_label(shifted(Key::G, true), Platform::Other),
            "Shift+Ctrl+G"
        );
        assert_eq!(shortcut_label(shifted(Key::G, true), Platform::Mac), "⇧⌘G");
        assert_eq!(
            shortcut_label(shifted(Key::F3, false), Platform::Other),
            "Shift+F3"
        );
    }

    #[test]
    fn home_and_end_go_to_the_ends_of_the_document() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::Home, false), false),
            Some(Action::FirstPage)
        );
        assert_eq!(
            keymap.action(press(Key::End, false), false),
            Some(Action::LastPage)
        );
        // While typing they move the caret, like the arrows.
        assert_eq!(keymap.action(press(Key::Home, false), true), None);
        assert_eq!(keymap.action(press(Key::End, false), true), None);
        assert_eq!(
            shortcut_label(press(Key::Home, false), Platform::Other),
            "Home"
        );
        assert_eq!(shortcut_label(press(Key::End, false), Platform::Mac), "End");
    }

    #[test]
    fn native_maps_page_keys() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::PageDown, false), false),
            Some(Action::ScrollScreenDown)
        );
        assert_eq!(
            keymap.action(press(Key::PageUp, false), false),
            Some(Action::ScrollScreenUp)
        );
    }

    #[test]
    fn mod_plus_and_equals_zoom_in() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::Plus, true), false),
            Some(Action::ZoomIn)
        );
        assert_eq!(
            keymap.action(press(Key::Equals, true), false),
            Some(Action::ZoomIn)
        );
    }

    #[test]
    fn mod_minus_zooms_out() {
        assert_eq!(
            Keymap::native().action(press(Key::Minus, true), false),
            Some(Action::ZoomOut)
        );
    }

    #[test]
    fn plain_w_and_f_fit() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::W, false), false),
            Some(Action::FitWidth)
        );
        assert_eq!(
            keymap.action(press(Key::F, false), false),
            Some(Action::FitPage)
        );
    }

    #[test]
    fn plain_c_and_d_toggle_modes() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::C, false), false),
            Some(Action::ToggleContinuous)
        );
        assert_eq!(
            keymap.action(press(Key::D, false), false),
            Some(Action::ToggleDual)
        );
    }

    #[test]
    fn c_with_mod_does_nothing() {
        assert_eq!(Keymap::native().action(press(Key::C, true), false), None);
    }

    #[test]
    fn plus_without_mod_does_nothing() {
        assert_eq!(
            Keymap::native().action(press(Key::Plus, false), false),
            None
        );
    }

    #[test]
    fn mod_i_shows_document_info() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::I, true), false),
            Some(Action::ShowDocumentInfo)
        );
        assert_eq!(keymap.action(press(Key::I, false), false), None);
        assert_eq!(
            shortcut_label(press(Key::I, true), Platform::Other),
            "Ctrl+I"
        );
        assert_eq!(shortcut_label(press(Key::I, true), Platform::Mac), "⌘I");
    }

    #[test]
    fn mod_comma_opens_settings() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::Comma, true), false),
            Some(Action::OpenSettings)
        );
        assert_eq!(keymap.action(press(Key::Comma, false), false), None);
        assert_eq!(
            shortcut_label(press(Key::Comma, true), Platform::Other),
            "Ctrl+,"
        );
        assert_eq!(shortcut_label(press(Key::Comma, true), Platform::Mac), "⌘,");
    }

    #[test]
    fn mod_w_closes_the_window() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::W, true), false),
            Some(Action::CloseWindow)
        );
        assert_eq!(
            keymap.key_for(Action::CloseWindow),
            Some(press(Key::W, true))
        );
    }

    #[test]
    fn f9_toggles_sidebar() {
        assert_eq!(
            Keymap::native().action(press(Key::F9, false), false),
            Some(Action::ToggleSidebar)
        );
    }

    #[test]
    fn mod_f_focuses_search() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::F, true), false),
            Some(Action::FocusSearch)
        );
        assert_eq!(
            keymap.action(press(Key::F, false), false),
            Some(Action::FitPage)
        );
    }

    #[test]
    fn mod_q_quits() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::Q, true), false),
            Some(Action::Quit)
        );
        assert_eq!(keymap.key_for(Action::Quit), Some(press(Key::Q, true)));
    }

    #[test]
    fn editing_blocks_plain_keys() {
        let keymap = Keymap::native();
        for key in [Key::W, Key::F, Key::C, Key::D] {
            assert_eq!(keymap.action(press(key, false), true), None, "{key:?}");
        }
    }

    #[test]
    fn editing_keeps_mod_function_and_page_keys() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::Equals, true), true),
            Some(Action::ZoomIn)
        );
        assert_eq!(
            keymap.action(press(Key::F9, false), true),
            Some(Action::ToggleSidebar)
        );
        assert_eq!(
            keymap.action(press(Key::PageDown, false), true),
            Some(Action::ScrollScreenDown)
        );
    }

    #[test]
    fn key_for_zoom_in_is_mod_equals() {
        assert_eq!(
            Keymap::native().key_for(Action::ZoomIn),
            Some(press(Key::Equals, true))
        );
    }

    #[test]
    fn labels_per_platform() {
        let mod_equals = press(Key::Equals, true);
        assert_eq!(shortcut_label(mod_equals, Platform::Other), "Ctrl+=");
        assert_eq!(shortcut_label(mod_equals, Platform::Mac), "⌘=");
        for platform in [Platform::Other, Platform::Mac] {
            assert_eq!(shortcut_label(press(Key::F9, false), platform), "F9");
            assert_eq!(shortcut_label(press(Key::W, false), platform), "W");
        }
        assert_eq!(
            shortcut_label(press(Key::Q, true), Platform::Other),
            "Ctrl+Q"
        );
        assert_eq!(shortcut_label(press(Key::Q, true), Platform::Mac), "⌘Q");
        assert_eq!(
            shortcut_label(press(Key::PageDown, false), Platform::Other),
            "Page Down"
        );
    }

    #[test]
    fn screen_down_keeps_overlap() {
        assert_eq!(
            scroll_target(Action::ScrollScreenDown, 0.0, 1000.0, 5000.0),
            980.0
        );
    }

    #[test]
    fn screen_up_from_middle() {
        assert_eq!(
            scroll_target(Action::ScrollScreenUp, 2000.0, 1000.0, 5000.0),
            1020.0
        );
    }

    #[test]
    fn clamps_at_top() {
        assert_eq!(
            scroll_target(Action::ScrollScreenUp, 300.0, 1000.0, 5000.0),
            0.0
        );
    }

    #[test]
    fn clamps_at_bottom() {
        assert_eq!(
            scroll_target(Action::ScrollScreenDown, 3900.0, 1000.0, 5000.0),
            4000.0
        );
    }

    #[test]
    fn short_document_never_scrolls() {
        assert_eq!(
            scroll_target(Action::ScrollScreenDown, 0.0, 1000.0, 500.0),
            0.0
        );
    }
}
