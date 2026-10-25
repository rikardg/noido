use std::collections::HashMap;

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
}

/// Maps key presses to actions.
pub struct Keymap {
    bindings: HashMap<KeyPress, Action>,
}

impl Keymap {
    /// The platform-conventional bindings, always active.
    pub fn native() -> Self {
        let plain = |key| KeyPress {
            key,
            command: false,
        };
        let command = |key| KeyPress { key, command: true };
        let bindings = HashMap::from([
            (plain(Key::PageUp), Action::ScrollScreenUp),
            (plain(Key::PageDown), Action::ScrollScreenDown),
            (command(Key::Plus), Action::ZoomIn),
            (command(Key::Equals), Action::ZoomIn),
            (command(Key::Minus), Action::ZoomOut),
            (plain(Key::W), Action::FitWidth),
            (plain(Key::F), Action::FitPage),
            (plain(Key::C), Action::ToggleContinuous),
            (plain(Key::D), Action::ToggleDual),
        ]);
        Self { bindings }
    }

    pub fn action(&self, press: KeyPress) -> Option<Action> {
        self.bindings.get(&press).copied()
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
            keymap.action(press(Key::PageDown, false)),
            Some(Action::ScrollScreenDown)
        );
        assert_eq!(
            keymap.action(press(Key::PageUp, false)),
            Some(Action::ScrollScreenUp)
        );
    }

    #[test]
    fn mod_plus_and_equals_zoom_in() {
        let keymap = Keymap::native();
        assert_eq!(keymap.action(press(Key::Plus, true)), Some(Action::ZoomIn));
        assert_eq!(
            keymap.action(press(Key::Equals, true)),
            Some(Action::ZoomIn)
        );
    }

    #[test]
    fn mod_minus_zooms_out() {
        assert_eq!(
            Keymap::native().action(press(Key::Minus, true)),
            Some(Action::ZoomOut)
        );
    }

    #[test]
    fn plain_w_and_f_fit() {
        let keymap = Keymap::native();
        assert_eq!(keymap.action(press(Key::W, false)), Some(Action::FitWidth));
        assert_eq!(keymap.action(press(Key::F, false)), Some(Action::FitPage));
    }

    #[test]
    fn plain_c_and_d_toggle_modes() {
        let keymap = Keymap::native();
        assert_eq!(
            keymap.action(press(Key::C, false)),
            Some(Action::ToggleContinuous)
        );
        assert_eq!(
            keymap.action(press(Key::D, false)),
            Some(Action::ToggleDual)
        );
    }

    #[test]
    fn c_with_mod_does_nothing() {
        assert_eq!(Keymap::native().action(press(Key::C, true)), None);
    }

    #[test]
    fn plus_without_mod_does_nothing() {
        assert_eq!(Keymap::native().action(press(Key::Plus, false)), None);
    }

    #[test]
    fn w_with_mod_does_nothing() {
        assert_eq!(Keymap::native().action(press(Key::W, true)), None);
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
