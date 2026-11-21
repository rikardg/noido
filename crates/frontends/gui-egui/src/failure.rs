//! The window a launch that can't open its document shows: what it was
//! asked to open, and why it couldn't.

use eframe::egui;

/// The size of the window. A message that doesn't fit its width wraps.
const WINDOW_SIZE: egui::Vec2 = egui::vec2(480.0, 200.0);

/// The margin around the window's contents.
const MARGIN: i8 = 16;

/// The window's heading: what went wrong, above the document and reason.
const HEADING: &str = "Cannot open";

/// Shows `message` in a window of its own, and returns once it is closed.
/// A window that can't be made is noted on stderr and nothing else: the
/// message itself is on its way there too.
pub fn report(message: &str) {
    let ctx = egui::Context::default();
    crate::configure(&ctx);
    // Ctrl-C closes it, as it closes a document's window.
    crate::interrupt::close_on_interrupt(ctx.clone());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("noido")
            // Ties the window to noido.desktop, as a document's window is.
            .with_app_id("noido")
            .with_icon(crate::icon::window_icon())
            .with_inner_size(WINDOW_SIZE)
            .with_resizable(false),
        persist_window: false,
        ..Default::default()
    };
    let failure = Failure {
        message: message.to_owned(),
    };
    if let Err(e) = eframe::run_native_ext(
        "noido",
        options,
        Some(ctx),
        Box::new(move |_cc| Ok(Box::new(failure))),
    ) {
        eprintln!("noido: cannot show the error: {e}");
    }
}

/// Draws the window's contents; `true` when it is to be closed.
fn show(ui: &mut egui::Ui, message: &str) -> bool {
    let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    // eframe clears a window in its own dark grey, which a light theme's
    // text would sit unreadable on: paint the theme's background instead.
    let window = ui.max_rect();
    ui.painter()
        .rect_filled(window, 0.0, ui.visuals().panel_fill);
    egui::Frame::NONE.inner_margin(MARGIN).show(ui, |ui| {
        // The button goes in first, at the window's foot: a message taller
        // than the window then wraps above it rather than pushing it out.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
            close |= ui.button("Close").clicked();
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(ui.available_width());
                // A reason taller than the room left scrolls, rather than
                // painting over the button below it.
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.heading(HEADING);
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(message).wrap());
                    });
            });
        });
    });
    close
}

/// The window's app: one message, until it is closed.
struct Failure {
    message: String,
}

impl Failure {
    /// Draws the window, and closes it when it was asked to.
    fn drawn(&mut self, ui: &mut egui::Ui) {
        if show(ui, &self.message) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for Failure {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drawn(ui);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Rect, pos2};

    use super::*;

    /// The window drawn on its own, in the size and frame `report` gives
    /// it, one frame per call.
    struct Screen {
        ctx: egui::Context,
        failure: Failure,
        /// Everything the app asked of its window so far.
        commands: Vec<egui::ViewportCommand>,
        shapes: Vec<egui::epaint::ClippedShape>,
    }

    impl Screen {
        fn new(message: &str) -> Self {
            Self::new_in(message, egui::ThemePreference::Dark)
        }

        fn new_in(message: &str, theme: egui::ThemePreference) -> Self {
            let ctx = egui::Context::default();
            crate::theme::install_preset(&ctx, "adwaita");
            ctx.set_theme(theme);
            let mut screen = Self {
                ctx,
                failure: Failure {
                    message: message.to_owned(),
                },
                commands: Vec::new(),
                shapes: Vec::new(),
            };
            screen.frames(3, vec![]);
            screen
        }

        fn frames(&mut self, count: usize, events: Vec<Event>) {
            let mut events = events;
            for _ in 0..count {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), WINDOW_SIZE)),
                    events: std::mem::take(&mut events),
                    ..Default::default()
                };
                let failure = &mut self.failure;
                let output = self.ctx.run_ui(input, |ui| failure.drawn(ui));
                self.shapes = output.shapes.clone();
                if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                    self.commands.extend(viewport.commands.iter().cloned());
                }
                output.drop_without_applying_deltas();
            }
        }

        /// Every text the last frame painted.
        fn texts(&self) -> Vec<String> {
            fn collect(shape: &egui::Shape, into: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => into.push(t.galley.text().to_owned()),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, into)),
                    _ => {}
                }
            }
            let mut texts = Vec::new();
            for shape in &self.shapes {
                collect(&shape.shape, &mut texts);
            }
            texts
        }

        /// Whether the app asked its window to close, as a click on its
        /// own close button would.
        fn closed(&self) -> bool {
            self.commands.contains(&egui::ViewportCommand::Close)
        }

        /// The text painted over `rect` last frame, clipped parts aside:
        /// what a reader would see on top of it. The button's own label is
        /// not counted.
        fn texts_over(&self, rect: Rect) -> Vec<String> {
            fn walk(shape: &egui::Shape, clip: Rect, rect: Rect, into: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => {
                        let drawn = Rect::from_min_size(t.pos, t.galley.size()).intersect(clip);
                        let text = t.galley.text();
                        if drawn.intersects(rect) && text != "Close" {
                            into.push(text.to_owned());
                        }
                    }
                    egui::Shape::Vec(shapes) => {
                        shapes.iter().for_each(|s| walk(s, clip, rect, into))
                    }
                    _ => {}
                }
            }
            let mut into = Vec::new();
            for clipped in &self.shapes {
                walk(&clipped.shape, clipped.clip_rect, rect, &mut into);
            }
            into
        }

        /// The colour the whole window is painted in, as the last frame
        /// left it: what the theme's text sits on.
        fn background(&self) -> Option<egui::Color32> {
            fn find(shape: &egui::Shape, window: Rect) -> Option<egui::Color32> {
                match shape {
                    egui::Shape::Rect(r) if r.rect.contains_rect(window) && r.fill.a() > 0 => {
                        Some(r.fill)
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, window)),
                    _ => None,
                }
            }
            let window = Rect::from_min_size(pos2(0.0, 0.0), WINDOW_SIZE);
            self.shapes.iter().find_map(|c| find(&c.shape, window))
        }

        /// How many lines the text of `text` was laid out in.
        fn rows_of(&self, text: &str) -> Option<usize> {
            fn find(shape: &egui::Shape, text: &str) -> Option<usize> {
                match shape {
                    egui::Shape::Text(t) if t.galley.text() == text => Some(t.galley.rows.len()),
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                    _ => None,
                }
            }
            self.shapes.iter().find_map(|c| find(&c.shape, text))
        }

        /// Where text exactly equal to `text` was drawn last frame.
        fn text_rect(&self, text: &str) -> Option<Rect> {
            fn find(shape: &egui::Shape, text: &str) -> Option<Rect> {
                match shape {
                    egui::Shape::Text(t) if t.galley.text() == text => {
                        Some(Rect::from_min_size(t.pos, t.galley.size()))
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                    _ => None,
                }
            }
            self.shapes.iter().find_map(|c| find(&c.shape, text))
        }

        /// Clicks the primary button at `at`.
        fn click(&mut self, at: egui::Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frames(1, vec![Event::PointerMoved(at)]);
            self.frames(1, vec![button(true)]);
            self.frames(1, vec![button(false)]);
        }

        fn close_button(&self) -> Rect {
            self.text_rect("Close").expect("the Close button is drawn")
        }
    }

    /// A reason longer than the window is wide.
    const LONG: &str = "/books/a very long directory name, named so that \
                        the reason has to wrap/network analysis.pdf: not a \
                        readable document";

    #[test]
    fn the_heading_says_what_went_wrong() {
        let screen = Screen::new("/books/x.pdf: no such file");
        assert!(screen.texts().iter().any(|text| text == HEADING));
    }

    #[test]
    fn the_reason_is_drawn_as_it_was_given() {
        let message = "/books/x.pdf: no such file";
        let screen = Screen::new(message);
        assert_eq!(screen.rows_of(message), Some(1), "not drawn as one line");
    }

    #[test]
    fn a_reason_too_wide_for_the_window_wraps() {
        let screen = Screen::new(LONG);
        let rows = screen.rows_of(LONG).expect("the reason is drawn");
        assert!(
            rows > 1,
            "drawn on one line, so cut off at the window's side"
        );
    }

    /// A reason tall enough not to fit the window's height.
    fn towering() -> String {
        let mut message = String::new();
        for _ in 0..40 {
            message.push_str("/books/a long directory name");
        }
        message.push_str("/x.pdf: not a readable document");
        message
    }

    #[test]
    fn a_reason_taller_than_the_window_keeps_close_clear() {
        let screen = Screen::new(&towering());
        let button = screen.close_button();
        let window = Rect::from_min_size(pos2(0.0, 0.0), WINDOW_SIZE);
        assert!(
            window.contains_rect(button),
            "the Close button is at {button:?}, outside {window:?}"
        );
        let over = screen.texts_over(button);
        assert!(
            over.is_empty(),
            "the reason is painted over Close: {over:?}"
        );
    }

    #[test]
    fn the_window_is_filled_with_the_themes_background() {
        // eframe clears a window in its own dark grey; the theme's text
        // would sit unreadable on it in a light theme.
        for theme in [egui::ThemePreference::Dark, egui::ThemePreference::Light] {
            let screen = Screen::new_in("/books/x.pdf: no such file", theme);
            let fill = screen.ctx.style_of(screen.ctx.theme()).visuals.panel_fill;
            assert_eq!(screen.background(), Some(fill), "{theme:?}");
        }
    }

    #[test]
    fn close_closes_the_window() {
        let mut screen = Screen::new("/books/x.pdf: no such file");
        assert!(!screen.closed(), "closed before anything was asked");
        screen.click(screen.close_button().center());
        assert!(screen.closed(), "Close did not close the window");
    }

    #[test]
    fn escape_closes_the_window() {
        let mut screen = Screen::new("/books/x.pdf: no such file");
        let key = |pressed| Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        screen.frames(1, vec![key(true), key(false)]);
        assert!(screen.closed(), "Escape did not close the window");
    }
}
