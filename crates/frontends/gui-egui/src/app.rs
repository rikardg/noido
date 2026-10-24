use std::collections::HashMap;

use eframe::egui::{
    self, Color32, ColorImage, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, pos2, vec2,
};
use noido_core::{Layout, MAX_RENDER_SIDE, RenderTracker, render_scale};
use noido_engine::Size;
use noido_render::{DocumentInfo, RenderRequest, Renderer};

/// Pages beyond the visible ones that are rendered ahead, on each side.
const PREFETCH: usize = 1;
/// Pages beyond the visible ones whose images are kept, on each side.
const KEEP: usize = 2;

pub struct NoidoApp {
    renderer: Renderer,
    page_sizes: Vec<Size>,
    layout: Layout,
    tracker: RenderTracker,
    textures: HashMap<usize, TextureHandle>,
}

impl NoidoApp {
    pub fn new(renderer: Renderer, info: DocumentInfo) -> Self {
        let page_count = info.page_sizes.len();
        Self {
            renderer,
            layout: Layout::new(info.page_sizes.clone()),
            page_sizes: info.page_sizes,
            tracker: RenderTracker::new(page_count),
            textures: HashMap::new(),
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
        self.layout.set_available_width(ui.available_width());
        let (content, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), self.layout.content_height()),
            Sense::hover(),
        );

        let page_count = self.page_sizes.len();
        let visible = self.layout.visible_pages(viewport.min.y, viewport.max.y);
        let around = |extra: usize| {
            visible.start.saturating_sub(extra)..(visible.end + extra).min(page_count)
        };

        // Set before requesting: the render thread skips pages outside this
        // range, and the tracker forgets them below, so none stays pending.
        let keep = around(KEEP);
        self.renderer.set_keep(keep.clone());

        let display_scale = self.layout.scale() * ui.ctx().pixels_per_point();
        if display_scale > 0.0 {
            let sizes = &self.page_sizes;
            let wanted = self.tracker.wanted(around(PREFETCH), |page| {
                render_scale(sizes[page], display_scale, MAX_RENDER_SIDE)
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
            let Some(r) = self.layout.page_rect(page) else {
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
        self.receive_renders(ui.ctx());
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ScrollArea::vertical()
                .auto_shrink(false)
                .show_viewport(ui, |ui, viewport| self.pages(ui, viewport));
        });
    }
}
