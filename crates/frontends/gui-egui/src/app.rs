use std::collections::HashMap;

use eframe::egui::scroll_area::{DragScroll, ScrollSource};
use eframe::egui::{
    self, Color32, ColorImage, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, Vec2, pos2,
    vec2,
};
use noido_core::{Key, KeyPress, Keymap, MAX_RENDER_SIDE, RenderTracker, View, render_scale};
use noido_render::{DocumentInfo, RenderRequest, Renderer};

/// Spreads (pages in single mode) beyond the visible ones that are rendered
/// ahead, on each side.
const PREFETCH: usize = 1;
/// Spreads beyond the visible ones whose images are kept, on each side.
const KEEP: usize = 2;
/// Trackpad scroll distance, in UI points, that flips one page in paged mode
/// (about one mouse wheel step).
const TRACKPAD_FLIP_DISTANCE: f32 = 125.0;
/// Offset difference, in UI points, below which egui's scroll position is left alone.
const OFFSET_EPSILON: f32 = 0.5;

/// What egui's scroll area reported at the end of the last frame.
#[derive(Clone, Copy)]
struct ScrollState {
    /// The visible area, in screen coordinates.
    inner: Rect,
    offset: Vec2,
}

pub struct NoidoApp {
    renderer: Renderer,
    view: View,
    tracker: RenderTracker,
    textures: HashMap<usize, TextureHandle>,
    keymap: Keymap,
    last_scroll: Option<ScrollState>,
    /// Trackpad distance scrolled towards the next flip, positive downwards.
    flip_distance: f32,
}

impl NoidoApp {
    pub fn new(renderer: Renderer, info: DocumentInfo) -> Self {
        let page_count = info.page_sizes.len();
        Self {
            renderer,
            view: View::new(info.page_sizes),
            tracker: RenderTracker::new(page_count),
            textures: HashMap::new(),
            keymap: Keymap::native(),
            last_scroll: None,
            flip_distance: 0.0,
        }
    }

    /// Brings the view up to date with what egui did last frame: window size
    /// and any scrolling by wheel, drag or scrollbar.
    fn sync_from_egui(&mut self) {
        if let Some(last) = self.last_scroll {
            // egui's offset belongs to the old size: record it before resizing,
            // so the resize can re-anchor from it.
            self.view.scrolled_to(last.offset.x, last.offset.y);
            self.view
                .set_viewport_size(last.inner.width(), last.inner.height());
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        let (presses, wheel, zoom, pointer) = ctx.input(|i| {
            let presses: Vec<KeyPress> = i
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => core_key(*key).map(|key| KeyPress {
                        key,
                        command: modifiers.command,
                    }),
                    _ => None,
                })
                .collect();
            let wheel: Vec<WheelStep> = i
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        phase,
                        source,
                        modifiers,
                    } => Some(WheelStep {
                        unit: *unit,
                        down: -delta.y,
                        phase: *phase,
                        source: *source,
                        modifiers: *modifiers,
                    }),
                    _ => None,
                })
                .collect();
            (presses, wheel, i.zoom_delta(), i.pointer.hover_pos())
        });

        for press in presses {
            if let Some(action) = self.keymap.action(press) {
                self.view.apply(action);
            }
        }
        for step in wheel {
            self.wheel_flip(step);
        }
        if zoom != 1.0 {
            let anchor = self.last_scroll.and_then(|last| {
                pointer
                    .filter(|p| last.inner.contains(*p))
                    .map(|p| (p.x - last.inner.min.x, p.y - last.inner.min.y))
            });
            self.view.zoom_by(zoom, anchor);
        }
    }

    /// In paged mode with the page fitting the window, vertical wheel input
    /// flips pages: each mouse wheel step flips once, trackpad scrolling flips
    /// once per [`TRACKPAD_FLIP_DISTANCE`]. Momentum, zoom (Mod) and sideways
    /// (Shift, or horizontal movement) never flip.
    fn wheel_flip(&mut self, step: WheelStep) {
        use egui::{MouseWheelSource, MouseWheelUnit, TouchPhase};
        let ignored = step.modifiers.command
            || step.modifiers.shift
            || step.source == MouseWheelSource::Momentum
            || !self.view.wheel_flips();
        if ignored || step.phase == TouchPhase::Start {
            self.flip_distance = 0.0;
        }
        if ignored || step.down == 0.0 {
            return;
        }
        match step.unit {
            MouseWheelUnit::Line | MouseWheelUnit::Page => self.view.wheel_flip(step.down > 0.0),
            MouseWheelUnit::Point => {
                self.flip_distance += step.down;
                if self.flip_distance.abs() >= TRACKPAD_FLIP_DISTANCE {
                    self.view.wheel_flip(self.flip_distance > 0.0);
                    self.flip_distance = 0.0;
                }
            }
        }
        if matches!(step.phase, TouchPhase::End | TouchPhase::Cancel) {
            self.flip_distance = 0.0;
        }
    }

    fn receive_renders(&mut self, ctx: &egui::Context) {
        while let Some(rendered) = self.renderer.try_recv() {
            match rendered.result {
                Ok(image) => {
                    let size = [image.width() as usize, image.height() as usize];
                    let color_image = ColorImage::from_rgba_premultiplied(size, image.pixels());
                    let name = format!("page-{}", rendered.page);
                    let texture = ctx.load_texture(name, color_image, TextureOptions::LINEAR);
                    self.textures.insert(rendered.page, texture);
                    self.tracker.mark_ready(rendered.page, rendered.scale);
                }
                Err(e) => {
                    eprintln!("noido: page {}: {e}", rendered.page + 1);
                    self.tracker.mark_failed(rendered.page, rendered.scale);
                }
            }
        }
    }

    fn pages(&mut self, ui: &mut egui::Ui, viewport: Rect) {
        let layout = self.view.layout();
        let (content, _) = ui.allocate_exact_size(
            vec2(layout.content_width(), layout.content_height()),
            Sense::hover(),
        );

        // Until the first frame has measured the window, the layout is a placeholder.
        let (width, height) = self.view.viewport_size();
        if width <= 0.0 || height <= 0.0 {
            ui.ctx().request_repaint();
            return;
        }

        let page_count = layout.page_count();
        let visible = layout.visible_pages(viewport.min.y, viewport.max.y);
        // The visible pages widened by whole spreads, so a spread is never half rendered.
        let around = |extra: usize| {
            if visible.is_empty() || layout.spread_count() == 0 {
                return visible.clone();
            }
            let first = layout.spread_of(visible.start).saturating_sub(extra);
            let last = (layout.spread_of(visible.end - 1) + extra).min(layout.spread_count() - 1);
            layout.spread_pages(first).start..layout.spread_pages(last).end.min(page_count)
        };

        // Set before requesting: the render thread skips pages outside this
        // range, and the tracker forgets them below, so none stays pending.
        let keep = around(KEEP);
        self.renderer.set_keep(keep.clone());

        let display_scale = self.view.scale() * ui.ctx().pixels_per_point();
        if display_scale > 0.0 {
            let wanted = self.tracker.wanted(around(PREFETCH), |page| {
                let size = layout.page_size(page).expect("page in range");
                render_scale(size, display_scale, MAX_RENDER_SIDE)
            });
            for (page, scale) in wanted {
                self.renderer.request(RenderRequest { page, scale });
            }
        }
        for page in self.tracker.forget_outside(keep) {
            self.textures.remove(&page);
        }

        let painter = ui.painter();
        for page in visible {
            let Some(r) = layout.page_rect(page) else {
                continue;
            };
            let rect = Rect::from_min_size(content.min + vec2(r.x, r.y), vec2(r.width, r.height));
            match self.textures.get(&page) {
                Some(texture) => {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(texture.id(), rect, uv, Color32::WHITE);
                }
                None => {
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                }
            }
        }
    }
}

impl eframe::App for NoidoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl NoidoApp {
    /// One frame of the viewer.
    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.receive_renders(&ctx);
        self.sync_from_egui();
        self.handle_input(&ctx);

        let (x, y) = self.view.offset();
        let wanted = vec2(x, y);
        let move_scroll = self
            .last_scroll
            .is_some_and(|last| (last.offset - wanted).abs().max_elem() > OFFSET_EPSILON);

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            // egui fades the edges of scroll areas; on pages it reads as a shadow.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            // Left drag pans the document (until selection tools need the button).
            let source = ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            };
            let mut scroll_area = ScrollArea::both().auto_shrink(false).scroll_source(source);
            if move_scroll {
                // egui checks a new offset against last frame's content size;
                // past it, rubber-banding would spring the view away from the anchor.
                ui.spacing_mut().scroll.kinetic.rubber_band = false;
                scroll_area = scroll_area.scroll_offset(wanted);
            }
            let output = scroll_area.show_viewport(ui, |ui, viewport| self.pages(ui, viewport));
            self.last_scroll = Some(ScrollState {
                inner: output.inner_rect,
                offset: output.state.clamped_offset(),
            });
        });
    }
}

/// The parts of a mouse wheel event that decide whether it flips a page.
#[derive(Clone, Copy)]
struct WheelStep {
    unit: egui::MouseWheelUnit,
    /// Vertical movement, positive towards the end of the document.
    down: f32,
    phase: egui::TouchPhase,
    source: egui::MouseWheelSource,
    modifiers: egui::Modifiers,
}

fn core_key(key: egui::Key) -> Option<Key> {
    match key {
        egui::Key::PageUp => Some(Key::PageUp),
        egui::Key::PageDown => Some(Key::PageDown),
        egui::Key::Plus => Some(Key::Plus),
        egui::Key::Equals => Some(Key::Equals),
        egui::Key::Minus => Some(Key::Minus),
        egui::Key::W => Some(Key::W),
        egui::Key::F => Some(Key::F),
        egui::Key::C => Some(Key::C),
        egui::Key::D => Some(Key::D),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, Modifiers, RawInput, pos2};
    use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

    use super::*;

    /// Ten Letter pages that render as a single white pixel.
    struct BlankEngine;
    struct BlankDocument;

    impl PdfEngine for BlankEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(BlankDocument))
        }
    }

    impl Document for BlankDocument {
        fn page_count(&self) -> usize {
            10
        }
        fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 612.0,
                height: 792.0,
            })
        }
        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Ok(Rgba8Image::from_premultiplied(1, 1, vec![255; 4]).unwrap())
        }
    }

    /// Runs the app headlessly in egui, one frame per call.
    struct Harness {
        ctx: egui::Context,
        app: NoidoApp,
        size: Vec2,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let (renderer, info) =
                Renderer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}))
                    .unwrap();
            let mut harness = Self {
                ctx: egui::Context::default(),
                app: NoidoApp::new(renderer, info),
                size: vec2(900.0, 1000.0),
                time: 0.0,
            };
            harness.frames(3, vec![]);
            harness
        }

        fn frames(&mut self, count: usize, mut events: Vec<Event>) {
            for _ in 0..count {
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
                    time: Some(self.time),
                    predicted_dt: 1.0 / 60.0,
                    events: std::mem::take(&mut events),
                    ..Default::default()
                };
                self.time += 1.0 / 60.0;
                let app = &mut self.app;
                self.ctx
                    .run_ui(input, |ui| app.show(ui))
                    .drop_without_applying_deltas();
            }
        }

        fn press(&mut self, key: egui::Key, modifiers: Modifiers) {
            let event = Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            self.frames(1, vec![event]);
        }

        /// The document point at the viewport centre.
        fn centre(&self) -> noido_core::DocPoint {
            let view = &self.app.view;
            let (ox, oy) = view.offset();
            let (w, h) = view.viewport_size();
            view.layout().doc_point(ox + w / 2.0, oy + h / 2.0).unwrap()
        }

        /// Where document point `p` appears, relative to the viewport centre.
        fn distance_from_centre(&self, p: noido_core::DocPoint) -> f32 {
            let view = &self.app.view;
            let (cx, cy) = view.layout().content_point(p).unwrap();
            let (ox, oy) = view.offset();
            let (w, h) = view.viewport_size();
            (cx - ox - w / 2.0).abs().max((cy - oy - h / 2.0).abs())
        }
    }

    #[test]
    fn zoom_in_near_bottom_keeps_centre() {
        let mut h = Harness::new();
        for _ in 0..30 {
            h.press(egui::Key::PageDown, Modifiers::NONE);
        }
        h.press(egui::Key::PageUp, Modifiers::NONE);
        h.frames(5, vec![]);
        let centre = h.centre();
        h.press(egui::Key::Plus, Modifiers::COMMAND);
        h.frames(60, vec![]);
        assert!(
            h.distance_from_centre(centre) < 1.0,
            "drifted {}",
            h.distance_from_centre(centre)
        );
    }

    #[test]
    fn left_drag_pans_both_axes() {
        let mut h = Harness::new();
        h.press(egui::Key::Plus, Modifiers::COMMAND);
        h.press(egui::Key::Plus, Modifiers::COMMAND);
        h.frames(5, vec![]);
        let (x0, y0) = h.app.view.offset();

        let at = |x: f32, y: f32| Event::PointerMoved(pos2(x, y));
        let button = |x: f32, y: f32, pressed: bool| Event::PointerButton {
            pos: pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![at(500.0, 600.0)]);
        h.frames(1, vec![button(500.0, 600.0, true)]);
        for step in 1..=10 {
            let t = step as f32 / 10.0;
            h.frames(1, vec![at(500.0 - 100.0 * t, 600.0 - 200.0 * t)]);
        }
        h.frames(1, vec![button(400.0, 400.0, false)]);
        h.frames(5, vec![]);

        let (x1, y1) = h.app.view.offset();
        assert!(x1 > x0 + 50.0, "x {x0} -> {x1}");
        assert!(y1 > y0 + 150.0, "y {y0} -> {y1}");
    }

    #[test]
    fn c_switches_to_paged_and_page_down_flips() {
        let mut h = Harness::new();
        h.press(egui::Key::C, Modifiers::NONE);
        assert_eq!(h.app.view.current_spread(), Some(0));
        for _ in 0..3 {
            h.press(egui::Key::PageDown, Modifiers::NONE);
            h.frames(2, vec![]);
        }
        assert_eq!(h.app.view.current_spread(), Some(1));
    }

    #[test]
    fn dual_paged_prefetches_whole_next_spread() {
        let mut h = Harness::new();
        // Paged first, so continuous mode never rendered the later pages.
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::D, Modifiers::NONE);
        assert_eq!(h.app.view.current_spread(), Some(0));
        let ready = |h: &Harness| (0..4).all(|page| h.app.textures.contains_key(&page));
        for _ in 0..200 {
            if ready(&h) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.frames(1, vec![]);
        }
        assert!(ready(&h), "textures: {:?}", h.app.textures.keys());
    }

    #[test]
    fn d_switches_to_dual() {
        let mut h = Harness::new();
        h.press(egui::Key::D, Modifiers::NONE);
        assert!(h.app.view.dual());
        let layout = h.app.view.layout();
        assert!(layout.page_rect(1).unwrap().x > layout.page_rect(0).unwrap().x);
    }

    fn wheel(unit: egui::MouseWheelUnit, dy: f32, source: egui::MouseWheelSource) -> Event {
        Event::MouseWheel {
            unit,
            delta: vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source,
            modifiers: Modifiers::NONE,
        }
    }

    /// Paged mode with the page fitted, so the wheel flips.
    fn fitted_paged() -> Harness {
        let mut h = Harness::new();
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::F, Modifiers::NONE);
        h.frames(2, vec![]);
        h
    }

    #[test]
    fn wheel_step_flips_fitting_page() {
        let mut h = fitted_paged();
        let step = wheel(
            egui::MouseWheelUnit::Line,
            -1.0,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![step]);
        assert_eq!(h.app.view.current_spread(), Some(1));
        let back = wheel(
            egui::MouseWheelUnit::Line,
            1.0,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![back]);
        assert_eq!(h.app.view.current_spread(), Some(0));
    }

    #[test]
    fn trackpad_flips_after_enough_distance() {
        let mut h = fitted_paged();
        let swipe = |dy| {
            wheel(
                egui::MouseWheelUnit::Point,
                dy,
                egui::MouseWheelSource::Trackpad,
            )
        };
        h.frames(1, vec![swipe(-50.0)]);
        assert_eq!(h.app.view.current_spread(), Some(0));
        h.frames(1, vec![swipe(-100.0)]);
        assert_eq!(h.app.view.current_spread(), Some(1));
    }

    #[test]
    fn momentum_never_flips() {
        let mut h = fitted_paged();
        let coast = wheel(
            egui::MouseWheelUnit::Point,
            -500.0,
            egui::MouseWheelSource::Momentum,
        );
        h.frames(1, vec![coast]);
        assert_eq!(h.app.view.current_spread(), Some(0));
    }

    #[test]
    fn sideways_scrolling_never_flips() {
        let mut h = fitted_paged();
        let sideways = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(-3.0, 0.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        };
        let shifted = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::SHIFT,
        };
        h.frames(1, vec![sideways, shifted]);
        assert_eq!(h.app.view.current_spread(), Some(0));
    }

    #[test]
    fn wheel_scrolls_tall_page_without_flipping() {
        let mut h = Harness::new();
        h.press(egui::Key::C, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.view.wheel_flips());
        let step = wheel(
            egui::MouseWheelUnit::Line,
            -1.0,
            egui::MouseWheelSource::Wheel,
        );
        // egui scrolls the area under the pointer.
        h.frames(1, vec![Event::PointerMoved(pos2(450.0, 500.0))]);
        h.frames(10, vec![step]);
        assert_eq!(h.app.view.current_spread(), Some(0));
        assert!(h.app.view.offset().1 > 0.0);
    }

    #[test]
    fn resize_in_fit_width_keeps_centre() {
        let mut h = Harness::new();
        for _ in 0..3 {
            h.press(egui::Key::PageDown, Modifiers::NONE);
        }
        h.frames(5, vec![]);
        let centre = h.centre();
        h.size = vec2(1300.0, 1000.0);
        h.frames(60, vec![]);
        assert!(
            h.distance_from_centre(centre) < 1.0,
            "drifted {}",
            h.distance_from_centre(centre)
        );
    }
}
