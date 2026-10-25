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

/// How far the current page's highlight reaches around its thumbnail, and
/// its corner radius, in points.
const HIGHLIGHT: f32 = 4.0;

/// What the sidebar needs from the app each frame.
pub struct Input<'a> {
    pub layout: &'a Layout,
    /// The low-res thumbnails, by page.
    pub lowres: Option<&'a HashMap<usize, PageImage>>,
    /// The page the reader is at, which the sidebar keeps in view.
    pub current: Option<usize>,
    /// The pages shown as current: the current page, or its spread.
    pub highlighted: Range<usize>,
}

/// What the reader did in the sidebar.
#[derive(Default)]
pub struct Output {
    /// A thumbnail was clicked.
    pub clicked: Option<usize>,
    /// The reader scrolled the sidebar; the page at its middle.
    pub browsing: Option<usize>,
}

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
    /// The current page the sidebar last scrolled to (or that was clicked).
    followed: Option<usize>,
    /// The scroll offset at the end of last frame.
    offset: f32,
    /// The column's width last frame.
    width: f32,
    /// Whether the sidebar was shown last frame.
    shown: bool,
    highlighted: Range<usize>,
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
            followed: None,
            offset: 0.0,
            width: 0.0,
            shown: false,
            highlighted: 0..0,
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
            // Scrolled away or closed meanwhile: the tracker has forgotten
            // the page, so it is asked for again when it comes back.
            if !self.visible.contains(&rendered.page) {
                continue;
            }
            match rendered.result {
                Ok(image) => {
                    self.tracker.mark_ready(rendered.page, rendered.scale);
                    let name = format!("sidebar-{}", rendered.page);
                    let size = layout.page_size(rendered.page);
                    let image = PageImage::load(ctx, name, &image, size, rendered.scale);
                    self.sharp.insert(rendered.page, image);
                }
                Err(e) => {
                    eprintln!("noido: thumbnail of page {}: {e}", rendered.page + 1);
                    self.sharp.remove(&rendered.page);
                    self.tracker.mark_failed(rendered.page, rendered.scale);
                }
            }
        }
    }

    /// Renders with `renderer` from now on, for a new version of the
    /// document with `page_count` pages. Sharp images stay until new ones
    /// replace them, except past the new page count.
    pub fn replace(&mut self, renderer: Renderer, page_count: usize) {
        self.renderer = Some(renderer);
        self.tracker = RenderTracker::new(page_count);
        self.sharp.retain(|&page, _| page < page_count);
    }

    /// Forgets the sharp renders while the sidebar is closed.
    pub fn closed(&mut self) {
        if !self.shown {
            return;
        }
        self.shown = false;
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
            .show(ui, |ui| self.scrolled(ui, input));
        state.width = response.response.rect.width();
        self.shown = true;
        response.inner
    }

    /// The scrolling column, kept on the current page.
    fn scrolled(&mut self, ui: &mut egui::Ui, input: &Input) -> Output {
        let width = ui.available_width();
        let height = ui.available_height();
        let column = Column::new(input.layout.page_sizes(), width);
        let resized = (width - self.width).abs() > 0.5;
        self.width = width;
        // The sidebar's own scroll position stands until the page changes.
        let follow = input
            .current
            .filter(|&current| !self.shown || resized || self.followed != Some(current));
        let target = follow.and_then(|current| {
            self.followed = Some(current);
            column.reveal(current, self.offset, height)
        });
        let mut area = ScrollArea::vertical().auto_shrink(false);
        if let Some(target) = target {
            area = area.vertical_scroll_offset(target);
        }
        let shown =
            area.show_viewport(ui, |ui, viewport| self.column(ui, viewport, &column, input));
        let offset = shown.state.clamped_offset().y;
        let scrolled = target.is_none() && !resized && (offset - self.offset).abs() > 0.5;
        self.offset = offset;
        let mut output = Output {
            clicked: shown.inner,
            browsing: None,
        };
        if scrolled {
            output.browsing = column.page_at(offset + height / 2.0);
        }
        if let Some(page) = output.clicked {
            // Already in view: going there mustn't scroll the sidebar.
            self.followed = Some(page);
        }
        output
    }

    /// The thumbnails in the scroll area's `viewport` (content coordinates);
    /// returns the page clicked, if any.
    fn column(
        &mut self,
        ui: &mut egui::Ui,
        viewport: Rect,
        column: &Column,
        input: &Input,
    ) -> Option<usize> {
        self.thumb_width = column.thumb_width();
        self.highlighted = input.highlighted.clone();
        let (content, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), column.content_height()),
            Sense::hover(),
        );
        let visible = column.visible(viewport.min.y, viewport.max.y);
        self.request(column, &visible, ui.ctx().pixels_per_point(), input.layout);
        self.visible = visible.clone();

        let outline = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let selection = ui.visuals().selection;
        let text = ui.visuals().text_color();
        let font = egui::TextStyle::Small.resolve(ui.style());
        let mut clicked = None;
        self.drawn.clear();
        for page in visible {
            let (Some(thumb), Some(label)) = (column.thumb(page), column.label(page)) else {
                continue;
            };
            let rect = Rect::from_min_size(
                content.min + vec2(thumb.x, thumb.y),
                vec2(thumb.width, thumb.height),
            );
            let with_label = rect.with_max_y(rect.max.y + label.height);
            let id = ui.scope_id().with(("thumbnail", page));
            if ui.interact(with_label, id, Sense::click()).clicked() {
                clicked = Some(page);
            }
            let current = input.highlighted.contains(&page);
            let painter = ui.painter();
            if current {
                painter.rect_filled(rect.expand(HIGHLIGHT), HIGHLIGHT, selection.bg_fill);
            }
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
                Stroke::new(1.0, outline),
                egui::StrokeKind::Outside,
            );
            painter.text(
                content.min + vec2(label.x + label.width / 2.0, label.y + label.height / 2.0),
                egui::Align2::CENTER_CENTER,
                (page + 1).to_string(),
                font.clone(),
                if current {
                    selection.stroke.color
                } else {
                    text
                },
            );
            self.drawn.push((page, rect));
        }
        clicked
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
    pub fn highlighted(&self) -> Range<usize> {
        self.highlighted.clone()
    }

    #[cfg(test)]
    pub fn thumb_width(&self) -> f32 {
        self.thumb_width
    }
}
