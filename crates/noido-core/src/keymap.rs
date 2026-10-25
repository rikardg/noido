/// Keys, independent of any UI toolkit. Frontends translate their own key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    PageUp,
    PageDown,
    Plus,
    Equals,
    Minus,
    W,
    F,
    C,
    D,
    Q,
    F9,
}

/// A key with the state of the platform's primary modifier (`command`: Ctrl,
/// or Cmd on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: Key,
    pub command: bool,
}

/// Viewer actions that keys (and later menus) trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ScrollScreenUp,
    ScrollScreenDown,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    ToggleContinuous,
    ToggleDual,
    ToggleSidebar,
    FocusSearch,
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
        };
        let command = |key| KeyPress { key, command: true };
        let bindings = vec![
            (plain(Key::PageUp), Action::ScrollScreenUp),
            (plain(Key::PageDown), Action::ScrollScreenDown),
            (command(Key::Equals), Action::ZoomIn),
            (command(Key::Plus), Action::ZoomIn),
            (command(Key::Minus), Action::ZoomOut),
            (plain(Key::W), Action::FitWidth),
            (plain(Key::F), Action::FitPage),
            (plain(Key::C), Action::ToggleContinuous),
            (plain(Key::D), Action::ToggleDual),
            (plain(Key::F9), Action::ToggleSidebar),
            (command(Key::F), Action::FocusSearch),
            (command(Key::Q), Action::Quit),
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
    press.command || matches!(press.key, Key::F9 | Key::PageUp | Key::PageDown)
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
        Key::Plus => "+",
        Key::Equals => "=",
        Key::Minus => "-",
        Key::W => "W",
        Key::F => "F",
        Key::C => "C",
        Key::D => "D",
        Key::Q => "Q",
        Key::F9 => "F9",
    };
    match (press.command, platform) {
        (false, _) => key.to_owned(),
        (true, Platform::Mac) => format!("⌘{key}"),
        (true, Platform::Other) => format!("Ctrl+{key}"),
    }
}

/// Fraction of the visible height that stays on screen when scrolling a screenful.
pub const SCREEN_OVERLAP: f32 = 0.1;

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
        KeyPress { key, command }
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
    fn w_with_mod_does_nothing() {
        assert_eq!(Keymap::native().action(press(Key::W, true), false), None);
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
        assert_eq!(
            Keymap::native().action(press(Key::Q, true), false),
            Some(Action::Quit)
        );
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
            900.0
        );
    }

    #[test]
    fn screen_up_from_middle() {
        assert_eq!(
            scroll_target(Action::ScrollScreenUp, 2000.0, 1000.0, 5000.0),
            1100.0
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
