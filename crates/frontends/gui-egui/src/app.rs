use std::collections::HashMap;

use eframe::egui::{
    self, Color32, ColorImage, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, Vec2, pos2,
    vec2,
};
use noido_core::{Key, KeyPress, Keymap, MAX_RENDER_SIDE, RenderTracker, View, render_scale};
use noido_render::{DocumentInfo, RenderRequest, Renderer};

/// Pages beyond the visible ones that are rendered ahead, on each side.
const PREFETCH: usize = 1;
/// Pages beyond the visible ones whose images are kept, on each side.
const KEEP: usize = 2;
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
        }
    }

    /// Brings the view up to date with what egui did last frame: window size
    /// and any scrolling by wheel, drag or scrollbar.
    fn sync_from_egui(&mut self) {
        if let Some(last) = self.last_scroll {
            self.view
                .set_viewport_size(last.inner.width(), last.inner.height());
            self.view.scrolled_to(last.offset.x, last.offset.y);
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        let (presses, zoom, pointer) = ctx.input(|i| {
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
            (presses, i.zoom_delta(), i.pointer.hover_pos())
        });

        for press in presses {
            if let Some(action) = self.keymap.action(press) {
                self.view.apply(action);
            }
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
        let around = |extra: usize| {
            visible.start.saturating_sub(extra)..(visible.end + extra).min(page_count)
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
            let mut scroll_area = ScrollArea::both().auto_shrink(false);
            if move_scroll {
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

fn core_key(key: egui::Key) -> Option<Key> {
    match key {
        egui::Key::PageUp => Some(Key::PageUp),
        egui::Key::PageDown => Some(Key::PageDown),
        egui::Key::Plus => Some(Key::Plus),
        egui::Key::Equals => Some(Key::Equals),
        egui::Key::Minus => Some(Key::Minus),
        egui::Key::W => Some(Key::W),
        egui::Key::F => Some(Key::F),
        _ => None,
    }
}
