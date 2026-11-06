//! The menu bar: noido, File, Edit and Window, in place of winit's;
//! and the Dock icon's click with no window showing, which goes to the
//! app delegate (winit's, lacking the method; it is added to that class
//! here, as `open_urls` does).
//!
//! AppKit's actions (Quit, Close Window, the Edit and Window items) go
//! along the responder chain as in any Mac app: Quit reaches eframe's
//! `on_exit` through `applicationWillTerminate:`, Close Window becomes
//! the window's close request. The Edit items find no taker in a document
//! window (winit's view has no `copy:`), so AppKit disables them there
//! and ⌘C and the rest go on to egui; in the Open panel's fields they
//! work. The rest is queued for the shell, which [`take_commands`]
//! drains; About and Settings, which go to the focused document window,
//! are disabled while none is key.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::encode::Encode;
use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Imp, Sel};
use objc2::{AnyThread, ClassType, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSDocumentController, NSEventModifierFlags, NSMenu, NSMenuItem, NSPanel,
    NSWindow,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString, NSURL};

/// What a menu item asks of noido's shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    About,
    Settings,
    Open,
    /// The Dock icon was clicked, or the app opened again, with no
    /// window showing.
    Reopen,
    /// The recent files' "Clear Menu" was chosen.
    ClearRecent,
}

/// What a menu item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Does {
    /// Asks the shell.
    Shell(MenuCommand),
    /// AppKit's own action, sent along the responder chain.
    AppKit(&'static CStr),
    /// A separator, not an item.
    Separator,
    /// AppKit's Services submenu.
    Services,
    /// The recently opened files' submenu, filled by [`set_recent`].
    Recent,
}

/// A modifier key that goes with ⌘ in an item's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Also {
    Nothing,
    /// ⌥
    Option,
    /// ⌃
    Control,
}

/// One item of the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// The menu it is in.
    pub menu: &'static str,
    pub title: &'static str,
    /// The key that goes with ⌘; empty for none.
    pub key: &'static str,
    /// What goes with ⌘ besides.
    pub also: Also,
    pub does: Does,
}

/// The menu bar, menu by menu in order. AppKit lists the windows at the
/// end of the Window menu.
pub fn menu_items() -> Vec<Item> {
    use Does::{AppKit, Recent, Separator, Services, Shell};
    use MenuCommand::{About, Open, Settings};
    let item = |menu, title, key, does| Item {
        menu,
        title,
        key,
        also: Also::Nothing,
        does,
    };
    let separator = |menu| item(menu, "", "", Separator);
    vec![
        item("noido", "About noido", "", Shell(About)),
        separator("noido"),
        item("noido", "Settings…", ",", Shell(Settings)),
        separator("noido"),
        item("noido", "Services", "", Services),
        separator("noido"),
        item("noido", "Hide noido", "h", AppKit(c"hide:")),
        Item {
            also: Also::Option,
            ..item(
                "noido",
                "Hide Others",
                "h",
                AppKit(c"hideOtherApplications:"),
            )
        },
        item("noido", "Show All", "", AppKit(c"unhideAllApplications:")),
        separator("noido"),
        item("noido", "Quit noido", "q", AppKit(c"terminate:")),
        item("File", "Open…", "o", Shell(Open)),
        item("File", "Open Recent", "", Recent),
        item("File", "Close Window", "w", AppKit(c"performClose:")),
        item("Edit", "Cut", "x", AppKit(c"cut:")),
        item("Edit", "Copy", "c", AppKit(c"copy:")),
        item("Edit", "Paste", "v", AppKit(c"paste:")),
        item("Edit", "Select All", "a", AppKit(c"selectAll:")),
        item("Window", "Minimize", "m", AppKit(c"performMiniaturize:")),
        item("Window", "Zoom", "", AppKit(c"performZoom:")),
        // AppKit retitles it Exit Full Screen while in full screen.
        Item {
            also: Also::Control,
            ..item(
                "Window",
                "Enter Full Screen",
                "f",
                AppKit(c"toggleFullScreen:"),
            )
        },
        separator("Window"),
        item(
            "Window",
            "Bring All to Front",
            "",
            AppKit(c"arrangeInFront:"),
        ),
    ]
}

/// The modifiers of `item`'s key: ⌘, and what goes with it.
fn modifier_mask(item: &Item) -> NSEventModifierFlags {
    match item.also {
        Also::Nothing => NSEventModifierFlags::Command,
        Also::Option => NSEventModifierFlags::Command | NSEventModifierFlags::Option,
        Also::Control => NSEventModifierFlags::Command | NSEventModifierFlags::Control,
    }
}

static QUEUE: Mutex<Vec<MenuCommand>> = Mutex::new(Vec::new());
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The commands a shell item can carry, by its tag.
const COMMANDS: [MenuCommand; 5] = [
    MenuCommand::About,
    MenuCommand::Settings,
    MenuCommand::Open,
    MenuCommand::Reopen,
    MenuCommand::ClearRecent,
];

fn tag_of(command: MenuCommand) -> isize {
    COMMANDS.iter().position(|c| *c == command).unwrap_or(0) as isize
}

/// The command of the item tagged `tag`.
fn command_of(tag: isize) -> Option<MenuCommand> {
    usize::try_from(tag)
        .ok()
        .and_then(|tag| COMMANDS.get(tag))
        .copied()
}

/// The item tagged `tag` was chosen: queues its command.
fn chosen(tag: isize) {
    if let Some(command) = command_of(tag) {
        queue(command);
    }
}

/// Whether the item for `command` is enabled, with or without a
/// document window `key`: About and Settings are the document window's.
fn enabled(command: MenuCommand, key: bool) -> bool {
    match command {
        MenuCommand::About | MenuCommand::Settings => key,
        MenuCommand::Open | MenuCommand::Reopen | MenuCommand::ClearRecent => true,
    }
}

/// Whether a document window is key: the root is never shown, so any
/// key window but a panel (the Open panel) is one.
fn document_window_key() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    NSApplication::sharedApplication(mtm)
        .keyWindow()
        .is_some_and(|window| !window.isKindOfClass(NSPanel::class()))
}

/// The app was opened again (a click on the Dock icon), with or without
/// a window `visible`: with none, the shell decides what to show.
fn reopened(visible: bool) {
    if !visible {
        queue(MenuCommand::Reopen);
    }
}

/// Queues `command` for [`take_commands`] and wakes the shell.
fn queue(command: MenuCommand) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(command);
    }
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Target has no
    // Drop.
    #[unsafe(super = NSObject)]
    struct Target;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        /// Queues the command the item's tag names.
        // SAFETY: A menu item sends its action with itself as the sender.
        #[unsafe(method(noidoCommand:))]
        fn command(&self, sender: &NSMenuItem) {
            chosen(sender.tag());
        }

        /// Opens the file the item carries, as Finder's would: through
        /// the opened-URL queue.
        // SAFETY: A menu item sends its action with itself as the sender.
        #[unsafe(method(noidoOpenRecent:))]
        fn open_recent(&self, sender: &NSMenuItem) {
            let Some(object) = sender.representedObject() else {
                return;
            };
            let Ok(url) = object.downcast::<NSURL>() else {
                return;
            };
            if let Some(text) = url.absoluteString() {
                crate::open_urls::hand_over(text.to_string());
            }
        }

        /// Disables About and Settings with no document window to
        /// carry them out; AppKit asks before showing the menu and
        /// before a key equivalent.
        #[unsafe(method(validateMenuItem:))]
        fn validate(&self, item: &NSMenuItem) -> bool {
            command_of(item.tag()).is_none_or(|command| enabled(command, document_window_key()))
        }
    }
);

impl Target {
    fn new() -> Retained<Self> {
        // SAFETY: The signature of NSObject's init is correct.
        #[allow(unsafe_code)]
        unsafe {
            msg_send![Self::alloc(), init]
        }
    }
}

/// `applicationShouldHandleReopen:hasVisibleWindows:`: with no window
/// showing, the shell decides what a click on the Dock icon brings.
/// AppKit goes on with its own handling (bringing back a minimized
/// window) either way.
extern "C-unwind" fn should_handle_reopen(
    _this: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    visible: Bool,
) -> Bool {
    reopened(visible.as_bool());
    Bool::YES
}

/// The menu item for `item`, sent to `target` if it is the shell's.
fn menu_item(
    mtm: MainThreadMarker,
    item: &Item,
    target: &Retained<Target>,
) -> Retained<NSMenuItem> {
    let action = match item.does {
        Does::Separator => return NSMenuItem::separatorItem(mtm),
        Does::Recent => {
            let submenu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(item.title));
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str(item.title));
            holder.setSubmenu(Some(&submenu));
            // Nothing to offer until set_recent fills it.
            holder.setEnabled(false);
            RECENT.with(|recent| {
                *recent.borrow_mut() = Some(RecentMenu {
                    holder: holder.clone(),
                    submenu,
                    target: target.clone(),
                });
            });
            return holder;
        }
        Does::Services => {
            let services = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(item.title));
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str(item.title));
            holder.setSubmenu(Some(&services));
            // AppKit fills it.
            NSApplication::sharedApplication(mtm).setServicesMenu(Some(&services));
            return holder;
        }
        Does::Shell(_) => sel!(noidoCommand:),
        Does::AppKit(name) => Sel::register(name),
    };
    // SAFETY: The action is a selector taking the sender, which the
    // target (or the responder chain, for AppKit's) answers.
    #[allow(unsafe_code)]
    let menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(item.title),
            Some(action),
            &NSString::from_str(item.key),
        )
    };
    menu_item.setKeyEquivalentModifierMask(modifier_mask(item));
    if let Does::Shell(command) = item.does {
        menu_item.setTag(tag_of(command));
        // SAFETY: The target answers `noidoCommand:`; it lives as long as
        // the process.
        #[allow(unsafe_code)]
        unsafe {
            menu_item.setTarget(Some(target));
        }
    }
    menu_item
}

/// The File menu's Open Recent item and its submenu, and the target that
/// answers them. Main thread only, so a thread-local.
struct RecentMenu {
    holder: Retained<NSMenuItem>,
    submenu: Retained<NSMenu>,
    target: Retained<Target>,
}

thread_local! {
    static RECENT: RefCell<Option<RecentMenu>> = const { RefCell::new(None) };
}

/// Fills the File menu's Open Recent submenu with `entries`, newest
/// first, and `Clear Menu` last; with none, the item is disabled. Main
/// thread only; before [`install_menu`] it does nothing.
pub fn set_recent(entries: &[(String, PathBuf)]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    RECENT.with(|recent| {
        let Ok(recent) = recent.try_borrow() else {
            return;
        };
        let Some(menu) = recent.as_ref() else {
            return;
        };
        menu.submenu.removeAllItems();
        for (label, path) in entries {
            // SAFETY: The target answers `noidoOpenRecent:`, and keeps
            // the URL it is given; both live as long as the process.
            #[allow(unsafe_code)]
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(label),
                    Some(sel!(noidoOpenRecent:)),
                    &NSString::from_str(""),
                );
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                item.setRepresentedObject(Some(&url));
                item.setTarget(Some(&menu.target));
                item
            };
            // Not a shell command: `validateMenuItem:` leaves it be.
            item.setTag(-1);
            menu.submenu.addItem(&item);
        }
        if !entries.is_empty() {
            menu.submenu.addItem(&NSMenuItem::separatorItem(mtm));
            // SAFETY: The target answers `noidoCommand:`.
            #[allow(unsafe_code)]
            let clear = unsafe {
                let clear = NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str("Clear Menu"),
                    Some(sel!(noidoCommand:)),
                    &NSString::from_str(""),
                );
                clear.setTarget(Some(&menu.target));
                clear
            };
            clear.setTag(tag_of(MenuCommand::ClearRecent));
            menu.submenu.addItem(&clear);
        }
        menu.holder.setEnabled(!entries.is_empty());
    });
}

/// Tells the system the file was opened, so the Dock icon's recents and
/// the system's Recent Items list it. Main thread only.
pub fn note_recent(path: &Path) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
}

/// Forgets the recent files, in the system's list too. Main thread only.
pub fn clear_recent() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: The sender is only passed on to the delegate, and there is
    // none.
    #[allow(unsafe_code)]
    unsafe {
        NSDocumentController::sharedDocumentController(mtm).clearRecentDocuments(None);
    }
}

/// Puts noido's menu bar in place of winit's (build the event loop
/// without winit's default menu), turns window tabbing off, and has a
/// click on the Dock icon with no window showing queue
/// [`MenuCommand::Reopen`]. Each queued command calls `wake`. Main thread
/// only; call it before the first window, so AppKit lists every window
/// in the Window menu (and the Dock's), and after winit's event loop is
/// built, which sets the app delegate. A second call does nothing.
pub fn install_menu(wake: impl Fn() + Send + Sync + 'static) {
    let mtm = MainThreadMarker::new().expect("the menu is installed on the main thread");
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = WAKE.set(Box::new(wake));
    let app = NSApplication::sharedApplication(mtm);
    NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);

    let target = Target::new();
    let bar = NSMenu::new(mtm);
    let mut menus: Vec<(&'static str, Retained<NSMenu>)> = Vec::new();
    for item in menu_items() {
        if menus.last().is_none_or(|(name, _)| *name != item.menu) {
            let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(item.menu));
            let holder = NSMenuItem::new(mtm);
            holder.setSubmenu(Some(&menu));
            bar.addItem(&holder);
            menus.push((item.menu, menu));
        }
        if let Some((_, menu)) = menus.last() {
            menu.addItem(&menu_item(mtm, &item, &target));
        }
    }
    // A menu item does not retain its target; this one lives as long as
    // the process.
    std::mem::forget(target);
    app.setMainMenu(Some(&bar));
    if let Some((_, window)) = menus.iter().find(|(name, _)| *name == "Window") {
        app.setWindowsMenu(Some(window));
    }

    let Some(delegate) = app.delegate() else {
        eprintln!("noido: no app delegate; a click on the Dock icon will not open anything");
        return;
    };
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    let types = CString::new(format!(
        "{}{}{}{}{}",
        Bool::ENCODING,
        <&AnyObject>::ENCODING,
        Sel::ENCODING,
        <&AnyObject>::ENCODING,
        Bool::ENCODING
    ))
    .unwrap_or_default();
    // SAFETY: `should_handle_reopen` has the signature `types` describes
    // (BOOL; self, _cmd, application, BOOL), and the class stays alive as
    // long as the delegate does.
    #[allow(unsafe_code)]
    let added = unsafe {
        class_addMethod(
            class as *const _ as *mut _,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, Bool) -> Bool,
                Imp,
            >(should_handle_reopen),
            types.as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("noido: cannot add the reopen handler; a click on the Dock icon will do nothing");
    }
}

/// The commands queued since the last call, in order.
pub fn take_commands() -> Vec<MenuCommand> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(menu: &'static str, title: &'static str, key: &'static str, does: Does) -> Item {
        Item {
            menu,
            title,
            key,
            also: Also::Nothing,
            does,
        }
    }

    /// `item` with ⌥ as well as ⌘.
    fn with_option(item: Item) -> Item {
        Item {
            also: Also::Option,
            ..item
        }
    }

    /// `item` with ⌃ as well as ⌘.
    fn with_control(item: Item) -> Item {
        Item {
            also: Also::Control,
            ..item
        }
    }

    fn separator(menu: &'static str) -> Item {
        item(menu, "", "", Does::Separator)
    }

    #[test]
    fn shell_items_and_a_reopen_with_no_window_queue_their_command() {
        // Off the main thread here, so without AppKit: what the item's
        // action and the delegate method call.
        chosen(tag_of(MenuCommand::Settings));
        chosen(-1);
        chosen(COMMANDS.len() as isize);
        reopened(true);
        reopened(false);
        assert_eq!(
            take_commands(),
            [MenuCommand::Settings, MenuCommand::Reopen]
        );
        assert!(take_commands().is_empty());
    }

    #[test]
    fn about_and_settings_need_a_document_window() {
        use MenuCommand::{About, Open, Reopen, Settings};
        for command in [About, Settings] {
            assert!(enabled(command, true), "{command:?}");
            assert!(!enabled(command, false), "{command:?}");
        }
        for command in [Open, Reopen] {
            assert!(enabled(command, true), "{command:?}");
            assert!(enabled(command, false), "{command:?}");
        }
    }

    #[test]
    fn menu_items_are_the_minimal_menu() {
        use Does::{AppKit, Shell};
        use MenuCommand::{About, Open, Settings};
        assert_eq!(
            menu_items(),
            vec![
                item("noido", "About noido", "", Shell(About)),
                separator("noido"),
                item("noido", "Settings…", ",", Shell(Settings)),
                separator("noido"),
                item("noido", "Services", "", Does::Services),
                separator("noido"),
                item("noido", "Hide noido", "h", AppKit(c"hide:")),
                with_option(item(
                    "noido",
                    "Hide Others",
                    "h",
                    AppKit(c"hideOtherApplications:")
                )),
                item("noido", "Show All", "", AppKit(c"unhideAllApplications:")),
                separator("noido"),
                item("noido", "Quit noido", "q", AppKit(c"terminate:")),
                item("File", "Open…", "o", Shell(Open)),
                item("File", "Open Recent", "", Does::Recent),
                item("File", "Close Window", "w", AppKit(c"performClose:")),
                item("Edit", "Cut", "x", AppKit(c"cut:")),
                item("Edit", "Copy", "c", AppKit(c"copy:")),
                item("Edit", "Paste", "v", AppKit(c"paste:")),
                item("Edit", "Select All", "a", AppKit(c"selectAll:")),
                item("Window", "Minimize", "m", AppKit(c"performMiniaturize:")),
                item("Window", "Zoom", "", AppKit(c"performZoom:")),
                // AppKit retitles it Exit Full Screen while in full screen.
                with_control(item(
                    "Window",
                    "Enter Full Screen",
                    "f",
                    AppKit(c"toggleFullScreen:")
                )),
                separator("Window"),
                item(
                    "Window",
                    "Bring All to Front",
                    "",
                    AppKit(c"arrangeInFront:")
                ),
            ]
        );
    }

    #[test]
    fn modifier_masks_are_command_and_the_extra_key() {
        use NSEventModifierFlags as M;
        let plain = item("File", "Open…", "o", Does::Shell(MenuCommand::Open));
        assert_eq!(modifier_mask(&plain), M::Command);
        assert_eq!(modifier_mask(&with_option(plain)), M::Command | M::Option);
        assert_eq!(modifier_mask(&with_control(plain)), M::Command | M::Control);
    }
}
