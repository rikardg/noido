//! The title bar of Preview and Safari for noido's windows: an empty
//! toolbar in the unified style makes AppKit draw the bar 52 pt tall,
//! with the window buttons centred in it, and keep them there through
//! resizes. noido draws the bar's contents itself.

use objc2::{ClassType, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSPanel, NSTitlebarSeparatorStyle, NSToolbar, NSWindowStyleMask,
    NSWindowToolbarStyle,
};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol};

/// Gives each of the app's windows that has a title bar, draws under it
/// (a full-size content view, as noido's own title bar asks for) and has
/// no toolbar yet an empty unified one, with no line under the bar.
/// AppKit refuses a toolbar to a window with no title bar. The toolbar
/// is hidden while its window is fullscreen, where AppKit would keep it
/// as a grey strip over noido's bar. Called on every frame; off the main
/// thread it does nothing.
pub fn unify_title_bars() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    for window in NSApplication::sharedApplication(mtm).windows() {
        let style = window.styleMask();
        if window.isKindOfClass(NSPanel::class())
            || !style.contains(NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView)
        {
            continue;
        }
        let toolbar = window.toolbar().unwrap_or_else(|| {
            let toolbar = NSToolbar::init(NSToolbar::alloc(mtm));
            window.setToolbar(Some(&toolbar));
            window.setToolbarStyle(NSWindowToolbarStyle::Unified);
            window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
            toolbar
        });
        let shown = !style.contains(NSWindowStyleMask::FullScreen);
        if toolbar.isVisible() != shown {
            toolbar.setVisible(shown);
        }
    }
}
