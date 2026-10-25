//! The left sidebar: a column of page thumbnails, as wide as the sidebar.
//! Visible thumbnails are rendered sharp by a renderer of their own; the
//! low-res thumbnails stand in until those arrive. Search results and the
//! outline will appear here too.

use std::collections::HashMap;
use std::ops::Range;

use eframe::egui::{self, Color32, Rect, ScrollArea, Sense, Stroke, vec2};
use noido_core::{
    Column, Layout, MAX_RENDER_SIDE, RenderTracker, SIDEBAR_MAX_FRACTION, SIDEBAR_MIN_WIDTH,
    SidebarState, render_scale,
};
use noido_render::{RenderRequest, Renderer};

use crate::image::PageImage;

/// What the sidebar needs from the app each frame.
pub struct Input<'a> {
    pub layout: &'a Layout,
    /// The low-res thumbnails, by page.
    pub lowres: Option<&'a HashMap<usize, PageImage>>,
}

/// What the reader did in the sidebar.
#[derive(Default)]
pub struct Output {}

/// The sidebar's thumbnails and their sharp renders.
pub struct SidebarView {
    /// `None` draws low-res thumbnails only.
    renderer: Option<Renderer>,
    tracker: RenderTracker,
    sharp: HashMap<usize, PageImage>,
    /// The pages visible last frame; empty while closed.
    visible: Range<usize>,
    /// Visible thumbnails' screen rects last frame.
    drawn: Vec<(usize, Rect)>,
    thumb_width: f32,
}

impl SidebarView {
    pub fn new(page_count: usize) -> Self {
        Self {
            renderer: None,
            tracker: RenderTracker::new(page_count),
            sharp: HashMap::new(),
            visible: 0..0,
            drawn: Vec::new(),
            thumb_width: 0.0,
        }
    }

    /// Renders visible thumbnails sharp with `renderer`.
    pub fn with_renderer(mut self, renderer: Renderer) -> Self {
        self.renderer = Some(renderer);
        self
    }

    /// Takes in sharp renders.
    pub fn receive(&mut self, ctx: &egui::Context, layout: &Layout) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        while let Some(rendered) = renderer.try_recv() {
            match rendered.result {
                Ok(image) => {
                    self.tracker.mark_ready(rendered.page, rendered.scale);
                    // Scrolled away or closed meanwhile.
                    if self.visible.contains(&rendered.page) {
                        let name = format!("sidebar-{}", rendered.page);
                        let size = layout.page_size(rendered.page);
                        let image = PageImage::load(ctx, name, &image, size, rendered.scale);
                        self.sharp.insert(rendered.page, image);
                    }
                }
                Err(e) => {
                    eprintln!("noido: thumbnail of page {}: {e}", rendered.page + 1);
                    self.sharp.remove(&rendered.page);
                    self.tracker.mark_failed(rendered.page, rendered.scale);
                }
            }
        }
    }

    /// Forgets the sharp renders while the sidebar is closed.
    pub fn closed(&mut self) {
        if self.visible.is_empty() && self.sharp.is_empty() && self.drawn.is_empty() {
            return;
        }
        self.visible = 0..0;
        self.sharp.clear();
        self.drawn.clear();
        self.tracker.forget_outside(0..0);
        if let Some(renderer) = &self.renderer {
            renderer.set_keep(0..0);
        }
    }

    /// Draws the open sidebar at the left of `ui` and records its width,
    /// which the reader may have changed by dragging its edge.
    pub fn show(&mut self, ui: &mut egui::Ui, state: &mut SidebarState, input: &Input) -> Output {
        let window = ui.max_rect().width();
        let max = (window * SIDEBAR_MAX_FRACTION).max(SIDEBAR_MIN_WIDTH);
        let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(0.0);
        let response = egui::Panel::left("sidebar")
            .resizable(true)
            .frame(frame)
            .default_size(state.clamped(window).width)
            .size_range(SIDEBAR_MIN_WIDTH..=max)
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink(false)
                    .show_viewport(ui, |ui, viewport| self.column(ui, viewport, input));
            });
        state.width = response.response.rect.width();
        Output::default()
    }

    /// The thumbnails in the scroll area's `viewport` (content coordinates).
    fn column(&mut self, ui: &mut egui::Ui, viewport: Rect, input: &Input) {
        let column = Column::new(input.layout.page_sizes(), ui.available_width());
        self.thumb_width = column.thumb_width();
        let (content, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), column.content_height()),
            Sense::hover(),
        );
        let visible = column.visible(viewport.min.y, viewport.max.y);
        self.request(&column, &visible, ui.ctx().pixels_per_point(), input.layout);
        self.visible = visible.clone();

        let painter = ui.painter();
        let outline = ui.visuals().widgets.noninteractive.bg_stroke;
        let text = ui.visuals().text_color();
        let font = egui::TextStyle::Small.resolve(ui.style());
        self.drawn.clear();
        for page in visible {
            let (Some(thumb), Some(label)) = (column.thumb(page), column.label(page)) else {
                continue;
            };
            let rect = Rect::from_min_size(
                content.min + vec2(thumb.x, thumb.y),
                vec2(thumb.width, thumb.height),
            );
            let image = self
                .sharp
                .get(&page)
                .or_else(|| input.lowres.and_then(|lowres| lowres.get(&page)));
            match image {
                Some(image) => {
                    painter.image(image.texture.id(), rect, image.uv, Color32::WHITE);
                }
                None => {
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                }
            }
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, outline.color),
                egui::StrokeKind::Outside,
            );
            let label_centre =
                content.min + vec2(label.x + label.width / 2.0, label.y + label.height / 2.0);
            painter.text(
                label_centre,
                egui::Align2::CENTER_CENTER,
                (page + 1).to_string(),
                font.clone(),
                text,
            );
            self.drawn.push((page, rect));
        }
    }

    /// Asks for sharp renders of the `visible` pages and drops the rest.
    fn request(
        &mut self,
        column: &Column,
        visible: &Range<usize>,
        pixels_per_point: f32,
        layout: &Layout,
    ) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        renderer.set_keep(visible.clone());
        renderer.set_priority(visible.clone().collect());
        let width = column.thumb_width() * pixels_per_point;
        let wanted = self.tracker.wanted(visible.clone(), |page| {
            let size = layout.page_size(page).expect("page in range");
            render_scale(size, width / size.width, MAX_RENDER_SIDE)
        });
        for (page, scale) in wanted {
            renderer.request(RenderRequest { page, scale });
        }
        self.tracker.forget_outside(visible.clone());
        self.sharp.retain(|page, _| visible.contains(page));
    }

    #[cfg(test)]
    pub fn drawn(&self) -> &[(usize, Rect)] {
        &self.drawn
    }

    #[cfg(test)]
    pub fn sharp_pages(&self) -> Vec<usize> {
        let mut pages: Vec<usize> = self.sharp.keys().copied().collect();
        pages.sort_unstable();
        pages
    }

    #[cfg(test)]
    pub fn thumb_width(&self) -> f32 {
        self.thumb_width
    }
}
