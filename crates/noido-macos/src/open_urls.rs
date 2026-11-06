//! Documents Finder sends to noido, those that start it included.
//! Launch Services delivers them as `application:openURLs:` to the app
//! delegate, which is winit's and lacks the method; it is added to that
//! class here, once. A link (`noido:`) comes as a `GURL` Apple event,
//! which gets a handler of its own.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{AnyThread, define_class, msg_send, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSObject,
    NSObjectProtocol, NSURL,
};

static QUEUE: Mutex<Vec<OsString>> = Mutex::new(Vec::new());
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The Apple event a link arrives as: class and id are both `GURL`.
const GURL: u32 = u32::from_be_bytes(*b"GURL");
/// Where the event carries the link's text (`keyDirectObject`).
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Links has no Drop.
    #[unsafe(super = NSObject)]
    struct Links;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for Links {}

    impl Links {
        /// Queues the link a `GURL` event carries, like an opened URL.
        // SAFETY: The signature is the one NSAppleEventManager calls a
        // handler's selector with: the event and its reply, which may be
        // nil.
        #[unsafe(method(handleGetURL:withReplyEvent:))]
        fn handle_get_url(
            &self,
            event: &NSAppleEventDescriptor,
            _reply: Option<&NSAppleEventDescriptor>,
        ) {
            let link = event
                .paramDescriptorForKeyword(DIRECT_OBJECT)
                .and_then(|link| link.stringValue());
            if let Some(link) = link {
                queue([link.to_string()]);
            }
        }
    }
);

impl Links {
    fn new() -> Retained<Self> {
        // SAFETY: The signature of NSObject's init is correct.
        #[allow(unsafe_code)]
        unsafe {
            msg_send![Self::alloc(), init]
        }
    }
}

extern "C-unwind" fn open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    queue(
        urls.iter()
            .filter_map(|url| url.absoluteString().map(|text| text.to_string())),
    );
}

/// Hands `text` over as if Finder had opened it: a file URL, or a path.
pub fn hand_over(text: String) {
    queue([text]);
}

/// Queues `texts` for [`take_opened`] and wakes the GUI.
fn queue(texts: impl IntoIterator<Item = String>) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.extend(texts.into_iter().map(OsString::from));
    }
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// Makes the app delegate accept `application:openURLs:`, and links
/// their `GURL` event, queueing each URL for [`take_opened`] and calling
/// `wake`. Main thread only. Call it after winit's event loop is built,
/// which sets the delegate, and before it runs: AppKit hands over the
/// documents that started the app while it finishes launching, and
/// drops them if the method is not there yet. A second call does
/// nothing.
pub fn install_open_urls(wake: impl Fn() + Send + Sync + 'static) {
    let mtm = MainThreadMarker::new().expect("open URLs are installed on the main thread");
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = WAKE.set(Box::new(wake));
    let links = Links::new();
    // SAFETY: Links answers the selector with the signature
    // NSAppleEventManager calls it with.
    #[allow(unsafe_code)]
    unsafe {
        NSAppleEventManager::sharedAppleEventManager()
            .setEventHandler_andSelector_forEventClass_andEventID(
                &links,
                sel!(handleGetURL:withReplyEvent:),
                GURL,
                GURL,
            );
    }
    // NSAppleEventManager does not retain its handler; this one lives as
    // long as the process.
    std::mem::forget(links);
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        eprintln!("noido: no app delegate; documents sent by Finder will not open");
        return;
    };
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    // SAFETY: `open_urls` has the signature the type encoding "v@:@@"
    // describes (void; self, _cmd, application, urls), and the class stays
    // alive as long as the delegate does.
    #[allow(unsafe_code)]
    let added = unsafe {
        class_addMethod(
            class as *const _ as *mut _,
            sel!(application:openURLs:),
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, &NSArray<NSURL>),
                Imp,
            >(open_urls),
            c"v@:@@".as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("noido: cannot add the open handler; documents sent by Finder will not open");
    }
}

/// The URLs received since the last call, in arrival order.
pub fn take_opened() -> Vec<OsString> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSAppleEventDescriptor, NSString};

    #[test]
    fn a_link_event_queues_its_url() {
        let event = NSAppleEventDescriptor::appleEventWithEventClass_eventID_targetDescriptor_returnID_transactionID(
            GURL, GURL, None, -1, 0,
        );
        let link = NSAppleEventDescriptor::descriptorWithString(&NSString::from_str(
            "noido:///x.pdf#page=2",
        ));
        event.setParamDescriptor_forKeyword(&link, DIRECT_OBJECT);
        let links = Links::new();
        // No reply wanted: AppKit may pass nil.
        let reply: Option<&NSAppleEventDescriptor> = None;
        // SAFETY: Links answers this selector with these argument types.
        #[allow(unsafe_code)]
        unsafe {
            let _: () = msg_send![&*links, handleGetURL: &*event, withReplyEvent: reply];
        }
        assert_eq!(take_opened(), [OsString::from("noido:///x.pdf#page=2")]);
    }
}
