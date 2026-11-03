//! Re-reads the OS theme when it changes, on the UI thread.
//!
//! The watcher's callback only says that something changed: it runs off
//! the main thread, and macOS reads the scroller style (overlay or always
//! shown) only on the main thread, which on macOS is the UI thread.

use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use native_theme::watch::ThemeSubscription;

use super::Theme;

/// Reads the OS theme; an error keeps the current one.
type Loader = Box<dyn FnMut() -> Result<Theme, String> + Send>;

/// A running theme watcher and the changes it has signalled.
pub struct Live {
    changed: Receiver<()>,
    load: Loader,
    _subscription: Option<ThemeSubscription>,
}

impl Live {
    /// Starts watching the OS theme.
    pub fn start(ctx: &egui::Context) -> Self {
        let (live, tx) = Self::with_loader(Theme::try_from_system);
        let ctx = ctx.clone();
        let subscription = native_theme::watch::on_theme_change(move |_event| {
            native_theme::detect::invalidate_caches();
            let _ = tx.send(());
            ctx.request_repaint();
        })
        .inspect_err(|error| {
            eprintln!("noido: cannot watch the system theme ({error})");
        })
        .ok();
        Self {
            _subscription: subscription,
            ..live
        }
    }

    /// A watcher that reads the theme with `load`, and the sender that
    /// signals a change.
    pub fn with_loader(
        load: impl FnMut() -> Result<Theme, String> + Send + 'static,
    ) -> (Self, Sender<()>) {
        let (tx, changed) = mpsc::channel();
        let live = Self {
            changed,
            load: Box::new(load),
            _subscription: None,
        };
        (live, tx)
    }

    /// After a change, reads the theme and installs it; returns whether it
    /// did. Several changes since the last poll are one read.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        if self.changed.try_iter().count() == 0 {
            return false;
        }
        match (self.load)() {
            Ok(theme) => {
                theme.install(ctx);
                true
            }
            Err(error) => {
                eprintln!("noido: cannot read the system theme ({error}); keeping the current one");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_failed_read_keeps_the_current_theme() {
        let ctx = egui::Context::default();
        let (mut live, tx) = Live::with_loader(|| Err("no portal".into()));
        tx.send(()).unwrap();
        assert!(!live.poll(&ctx));
    }

    #[test]
    fn poll_installs_a_changed_theme_once() {
        let ctx = egui::Context::default();
        let (mut live, tx) = Live::with_loader(|| Ok(Theme::preset("adwaita").unwrap()));
        assert!(!live.poll(&ctx));
        tx.send(()).unwrap();
        assert!(live.poll(&ctx));
        assert!(!live.poll(&ctx));
        assert_eq!(
            Theme::current(&ctx).palette(egui::Theme::Dark).panel,
            egui::Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }

    /// The watcher's callback runs off the main thread, where macOS can't
    /// tell the scroller style; the UI thread reads the theme, once for a
    /// burst of changes.
    #[test]
    fn the_theme_is_read_on_the_polling_thread_once_per_burst() {
        let ctx = egui::Context::default();
        let reads = Arc::new(Mutex::new(Vec::new()));
        let log = reads.clone();
        let (mut live, tx) = Live::with_loader(move || {
            log.lock().unwrap().push(std::thread::current().id());
            Ok(Theme::preset("adwaita").unwrap())
        });
        std::thread::spawn(move || {
            for _ in 0..3 {
                tx.send(()).unwrap();
            }
        })
        .join()
        .unwrap();
        assert!(live.poll(&ctx));
        assert_eq!(*reads.lock().unwrap(), vec![std::thread::current().id()]);
    }
}
