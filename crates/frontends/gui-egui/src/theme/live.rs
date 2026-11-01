//! Re-reads the OS theme when it changes and hands it to the UI thread.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use native_theme::watch::ThemeSubscription;

use super::Theme;

/// A running theme watcher and the themes it has queued for the UI thread.
pub struct Live {
    rx: Receiver<Theme>,
    _subscription: Option<ThemeSubscription>,
}

impl Live {
    /// Starts watching the OS theme; the callback reads it off the UI thread.
    pub fn start(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let subscription = native_theme::watch::on_theme_change(move |_event| {
            native_theme::detect::invalidate_caches();
            if offer(&tx, Theme::try_from_system) {
                ctx.request_repaint();
            }
        })
        .inspect_err(|error| {
            eprintln!("noido: cannot watch the system theme ({error})");
        })
        .ok();
        Self {
            rx,
            _subscription: subscription,
        }
    }

    /// Installs the newest queued theme, returning whether there was one.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut installed = false;
        while let Ok(theme) = self.rx.try_recv() {
            theme.install(ctx);
            installed = true;
        }
        installed
    }

    /// A watcher whose queue the test feeds, and its sender.
    #[cfg(test)]
    pub fn for_test() -> (Self, std::sync::mpsc::Sender<Theme>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                rx,
                _subscription: None,
            },
            tx,
        )
    }
}

/// Sends `theme` only when the read succeeded; a failed read keeps the old one.
fn offer(
    tx: &std::sync::mpsc::Sender<Theme>,
    load: impl FnOnce() -> Result<Theme, String>,
) -> bool {
    match load() {
        Ok(theme) => {
            let _ = tx.send(theme);
            true
        }
        Err(error) => {
            eprintln!("noido: cannot read the system theme ({error}); keeping the current one");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn a_failed_read_is_not_queued() {
        let (tx, rx) = std::sync::mpsc::channel();
        offer(&tx, || Err("no portal".into()));
        assert!(rx.try_recv().is_err());
        offer(&tx, || Ok(Theme::preset("adwaita").unwrap()));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn poll_installs_the_queued_theme_once() {
        let ctx = egui::Context::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Theme::preset("adwaita").expect("adwaita")).unwrap();
        let mut live = Live {
            rx,
            _subscription: None,
        };
        assert!(live.poll(&ctx));
        assert!(!live.poll(&ctx));
        assert_eq!(
            Theme::current(&ctx).palette(egui::Theme::Dark).panel,
            egui::Color32::from_rgb(0x22, 0x22, 0x26)
        );
    }
}
