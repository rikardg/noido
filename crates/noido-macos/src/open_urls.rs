//! Documents Finder sends to a running noido. Launch Services delivers
//! them as `application:openURLs:` to the app delegate, which is winit's
//! and lacks the method; it is added to that class here, once.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSArray, NSURL};

static QUEUE: Mutex<Vec<OsString>> = Mutex::new(Vec::new());
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

extern "C-unwind" fn open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    if let Ok(mut queue) = QUEUE.lock() {
        for url in urls.iter() {
            if let Some(text) = url.absoluteString() {
                queue.push(OsString::from(text.to_string()));
            }
        }
    }
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

/// Makes the app delegate accept `application:openURLs:`, queueing each
/// URL for [`take_opened`] and calling `wake`. Main thread only; the
/// delegate must be set (winit does that before the first frame). A
/// second call does nothing.
pub fn install_open_urls(wake: impl Fn() + Send + Sync + 'static) {
    let mtm = MainThreadMarker::new().expect("open URLs are installed on the main thread");
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = WAKE.set(Box::new(wake));
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
