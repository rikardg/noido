use std::collections::HashMap;

/// Keys, independent of any UI toolkit. Frontends translate their own key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    PageUp,
    PageDown,
}

/// Viewer actions that keys (and later menus) trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ScrollScreenUp,
    ScrollScreenDown,
}

/// Maps keys to actions.
pub struct Keymap {
    bindings: HashMap<Key, Action>,
}

impl Keymap {
    /// The platform-conventional bindings, always active.
    pub fn native() -> Self {
        let bindings = HashMap::from([
            (Key::PageUp, Action::ScrollScreenUp),
            (Key::PageDown, Action::ScrollScreenDown),
        ]);
        Self { bindings }
    }

    pub fn action(&self, key: Key) -> Option<Action> {
        self.bindings.get(&key).copied()
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
    };
    target.clamp(0.0, (content_height - viewport_height).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_maps_page_keys() {
        let keymap = Keymap::native();
        assert_eq!(keymap.action(Key::PageDown), Some(Action::ScrollScreenDown));
        assert_eq!(keymap.action(Key::PageUp), Some(Action::ScrollScreenUp));
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
