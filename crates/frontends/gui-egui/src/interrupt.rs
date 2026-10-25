//! Ctrl-C and termination signals close the window normally, so view state
//! and the window are saved. A second Ctrl-C quits at once.

use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::{self, ViewportCommand};

/// Exit status after a second interrupt, as for a process killed by SIGINT.
const INTERRUPTED: i32 = 130;

/// Installs the handler. Failing to install it is reported, not fatal.
pub fn close_on_interrupt(ctx: egui::Context) {
    let interrupts = AtomicUsize::new(0);
    if let Err(e) = ctrlc::set_handler(move || on_interrupt(&ctx, &interrupts)) {
        eprintln!("noido: cannot handle Ctrl-C: {e}");
    }
}

fn on_interrupt(ctx: &egui::Context, interrupts: &AtomicUsize) {
    if interrupts.fetch_add(1, Ordering::Relaxed) > 0 {
        std::process::exit(INTERRUPTED);
    }
    ctx.send_viewport_cmd(ViewportCommand::Close);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use eframe::egui::{RawInput, ViewportId};

    use super::*;

    #[test]
    fn first_interrupt_closes_the_window() {
        let ctx = egui::Context::default();
        on_interrupt(&ctx, &AtomicUsize::new(0));
        let output = ctx.run_ui(RawInput::default(), |_| {});
        let commands = &output.viewport_output[&ViewportId::ROOT].commands;
        assert!(commands.contains(&ViewportCommand::Close), "{commands:?}");
        output.drop_without_applying_deltas();
    }
}
