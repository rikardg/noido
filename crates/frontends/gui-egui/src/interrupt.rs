//! Ctrl-C and termination signals close the window normally (on macOS,
//! quit the app), so view state and windows are saved. A second Ctrl-C
//! quits at once.

use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::{self, ViewportCommand};

use crate::shell::{Request, Requests};

/// Exit status after a second interrupt, as for a process killed by SIGINT.
const INTERRUPTED: i32 = 130;

/// Installs the handler that closes the window. Failing to install it is
/// reported, not fatal.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn close_on_interrupt(ctx: egui::Context) {
    install(move || ctx.send_viewport_cmd(ViewportCommand::Close));
}

/// Installs the handler that asks the shell to quit.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn quit_on_interrupt(requests: Requests) {
    install(move || requests.send(Request::Quit));
}

fn install(first: impl Fn() + Send + 'static) {
    let interrupts = AtomicUsize::new(0);
    if let Err(e) = ctrlc::set_handler(move || on_interrupt(&interrupts, &first)) {
        eprintln!("noido: cannot handle Ctrl-C: {e}");
    }
}

/// The first interrupt calls `first`; the next one exits.
fn on_interrupt(interrupts: &AtomicUsize, first: &impl Fn()) {
    if interrupts.fetch_add(1, Ordering::Relaxed) > 0 {
        std::process::exit(INTERRUPTED);
    }
    first();
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};

    use eframe::egui::{RawInput, ViewportId};

    use super::*;

    #[test]
    fn first_interrupt_closes_the_window() {
        let ctx = egui::Context::default();
        on_interrupt(&AtomicUsize::new(0), &|| {
            ctx.send_viewport_cmd(ViewportCommand::Close)
        });
        let output = ctx.run_ui(RawInput::default(), |_| {});
        let commands = &output.viewport_output[&ViewportId::ROOT].commands;
        assert!(commands.contains(&ViewportCommand::Close), "{commands:?}");
        output.drop_without_applying_deltas();
    }

    #[test]
    fn first_interrupt_calls_the_first_action_once() {
        let calls = AtomicUsize::new(0);
        on_interrupt(&AtomicUsize::new(0), &|| {
            calls.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn first_interrupt_asks_the_shell_to_quit() {
        let (tx, rx) = mpsc::channel();
        let requests = Requests::new(tx, Arc::new(|| {}));
        on_interrupt(&AtomicUsize::new(0), &|| requests.send(Request::Quit));
        assert!(matches!(rx.try_recv(), Ok(Request::Quit)));
    }
}
