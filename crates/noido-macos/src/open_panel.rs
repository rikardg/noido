//! The Open panel: AppKit's own, filtering by the types noido.app claims,
//! not by the extension.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSModalResponse, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{MainThreadMarker, NSArray, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

/// The PDF type, the one noido.app claims in its Info.plist.
fn pdf_type() -> Option<Retained<UTType>> {
    UTType::typeWithIdentifier(&NSString::from_str("com.adobe.pdf"))
}

/// A CBZ, filtered by its extension. Info.plist declares the UTI that makes
/// Finder offer noido; the panel can work from the extension even when the
/// bundle is not installed, as in tests.
const CBZ_UTI: &str = "io.github.rikardg.noido.cbz";

fn cbz_type() -> Option<Retained<UTType>> {
    UTType::typeWithFilenameExtension(&NSString::from_str("cbz"))
        .or_else(|| UTType::typeWithIdentifier(&NSString::from_str(CBZ_UTI)))
}

/// The file a picked URL names.
fn path_of(url: &NSURL) -> Option<PathBuf> {
    url.path().map(|path| PathBuf::from(path.to_string()))
}

/// Whether to show a new panel: not while `showing` holds one, which
/// `front` brings forward instead.
fn new_panel<T>(showing: &RefCell<Option<T>>, front: impl FnOnce(&T)) -> bool {
    match &*showing.borrow() {
        Some(panel) => {
            front(panel);
            false
        }
        None => true,
    }
}

thread_local! {
    /// The Open panel showing, until it has answered. Main thread only.
    static SHOWING: RefCell<Option<Retained<NSOpenPanel>>> = const { RefCell::new(None) };
}

/// Asks for PDFs and comics in the Open panel, in front, starting in
/// `directory` when given; `on_picked` gets the picked files, and is not
/// called on Cancel. Several can be picked. The panel is not modal: this
/// returns at once, so it is safe from inside an event handler, and the
/// windows keep running while it is up. While one is up, it comes forward
/// instead of a second. Main thread only.
pub fn pick_documents(directory: Option<&Path>, on_picked: impl Fn(Vec<PathBuf>) + 'static) {
    let mtm = MainThreadMarker::new().expect("the Open panel runs on the main thread");
    // NSApp.activate needs macOS 14; noido.app supports 11.
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    if !SHOWING.with(|showing| new_panel(showing, |panel| panel.makeKeyAndOrderFront(None))) {
        return;
    }
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setAllowsMultipleSelection(true);
    panel.setCanChooseDirectories(false);
    let types: Vec<Retained<UTType>> = [pdf_type(), cbz_type()].into_iter().flatten().collect();
    if !types.is_empty() {
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
    }
    if let Some(directory) = directory {
        let url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&directory.to_string_lossy()),
            true,
        );
        panel.setDirectoryURL(Some(&url));
    }
    // The handler holds the panel until it has answered.
    let held = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        SHOWING.with(|showing| showing.replace(None));
        if response != NSModalResponseOK {
            return;
        }
        let picked: Vec<PathBuf> = held.URLs().iter().filter_map(|url| path_of(&url)).collect();
        if !picked.is_empty() {
            on_picked(picked);
        }
    });
    SHOWING.with(|showing| showing.replace(Some(panel.clone())));
    panel.beginWithCompletionHandler(&handler);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_panel_filters_by_the_pdf_type_the_bundle_claims() {
        let pdf = pdf_type().expect("the system knows the PDF type");
        assert_eq!(pdf.identifier().to_string(), "com.adobe.pdf");
        let extension = pdf.preferredFilenameExtension().map(|e| e.to_string());
        assert_eq!(extension.as_deref(), Some("pdf"));
    }

    #[test]
    fn the_open_panel_offers_cbz_as_well_as_pdf() {
        let cbz = cbz_type().expect("the system knows cbz");
        let extension = cbz.preferredFilenameExtension().map(|e| e.to_string());
        assert_eq!(extension.as_deref(), Some("cbz"));
        assert!(pdf_type().is_some());
    }

    #[test]
    fn a_second_ask_while_a_panel_is_up_brings_that_one_forward() {
        let showing = RefCell::new(None);
        let mut fronted = Vec::new();
        assert!(new_panel(&showing, |p| fronted.push(*p)));
        showing.replace(Some(1));
        assert!(!new_panel(&showing, |p| fronted.push(*p)));
        assert_eq!(fronted, [1]);
        // Answered: the next ask shows a new one.
        showing.replace(None);
        assert!(new_panel(&showing, |p| fronted.push(*p)));
        assert_eq!(fronted, [1]);
    }

    #[test]
    fn a_picked_url_is_its_file_exactly() {
        // As the panel gives it: the name as it is on disk, escaped.
        let url = NSURL::URLWithString(&NSString::from_str("file:///b/a%20b%20%C3%A9%23%25.pdf"))
            .unwrap();
        assert_eq!(path_of(&url), Some(PathBuf::from("/b/a b é#%.pdf")));
    }
}
