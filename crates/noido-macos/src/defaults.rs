//! System settings noido follows.

use objc2_foundation::{NSString, NSUserDefaults};

/// Whether quitting an application keeps its windows for the next start:
/// System Settings > Desktop & Dock > "Close windows when quitting an
/// application" turned off. The key is only in the global domain when the
/// setting was changed from its default, which is on (windows close), so
/// absent reads as false.
pub fn keeps_windows() -> bool {
    NSUserDefaults::standardUserDefaults()
        .boolForKey(&NSString::from_str("NSQuitAlwaysKeepsWindows"))
}
