//! The left sidebar: a column of page thumbnails, as wide as the sidebar, or
//! while searching, the list of matches. Visible thumbnails are rendered
//! sharp by a renderer of their own; the low-res thumbnails stand in until
//! those arrive. The outline will appear here too.

use std::collections::HashMap;
use std::ops::Range;

use eframe::egui::scroll_area::{DragScroll, ScrollSource};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Rect, ScrollArea, Sense, Stroke, vec2};
use noido_core::{
    Column, Hits, Layout, MAX_RENDER_SIDE, RenderTracker, SIDEBAR_MIN_WIDTH, SidebarState,
    max_width, render_scale,
};
use noido_render::{RenderRequest, Renderer};

use crate::image::PageImage;
use crate::search::HIGHLIGHT_FILL;

/// How far the current page's highlight reaches around its thumbnail, and
/// its corner radius, in points.
pub const HIGHLIGHT: f32 = 6.0;
/// Space between a highlighted page number and the edge of its pill.
const PILL_PADDING: egui::Vec2 = egui::vec2(8.0, 2.0);
/// Space between the current page's highlight and its number's pill.
const PILL_GAP: f32 = 4.0;
/// Height of a row in the list of matches.
const RESULT_ROW_HEIGHT: f32 = 28.0;
/// Space around the list of matches and inside the current row's fill.
const RESULT_MARGIN: f32 = 8.0;

/// What the sidebar needs from the app each frame.
pub struct Input<'a> {
    pub layout: &'a Layout,
    /// The low-res thumbnails, by page.
    pub lowres: Option<&'a HashMap<usize, PageImage>>,
    /// The page the reader is at, which the sidebar keeps in view.
    pub current: Option<usize>,
    /// The pages shown as current: the current page, or its spread.
    pub highlighted: Range<usize>,
    /// While searching: the matches, shown instead of the thumbnails.
    pub results: Option<Results<'a>>,
}

/// The matches of a search, for the sidebar.
pub struct Results<'a> {
    pub hits: &'a Hits,
    /// Which query they belong to.
    pub generation: u64,
    /// A line above them: progress, or "No matches".
    pub status: String,
}

/// What the reader did in the sidebar.
#[derive(Default)]
pub struct Output {
    /// A thumbnail was clicked.
    pub clicked: Option<usize>,
    /// The reader scrolled the sidebar; the page at its middle.
    pub browsing: Option<usize>,
    /// A match was clicked: its index.
    pub hit: Option<usize>,
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
    /// The list of matches: its visible rows' screen rects last frame, the
    /// current match it last scrolled to, and its scroll offset.
    result_rows: Vec<(usize, Rect)>,
    followed_hit: Option<(u64, usize)>,
    results_offset: f32,
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
            result_rows: Vec::new(),
            followed_hit: None,
            results_offset: 0.0,
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
        let max = max_width(window);
        let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(0.0);
        if input.results.is_some() {
            // The thumbnails are out of sight: no sharp renders for them.
            self.closed();
        } else {
            self.result_rows.clear();
            self.followed_hit = None;
        }
        let response = egui::Panel::left("sidebar")
            .resizable(true)
            .frame(frame)
            .default_size(state.clamped(window).width)
            .size_range(SIDEBAR_MIN_WIDTH..=max)
            .show(ui, |ui| match &input.results {
                Some(results) => Output {
                    hit: self.results(ui, results),
                    ..Output::default()
                },
                None => self.scrolled(ui, input),
            });
        state.width = response.response.rect.width();
        self.shown = true;
        response.inner
    }

    /// The scrolling column, kept on the current page.
    fn scrolled(&mut self, ui: &mut egui::Ui, input: &Input) -> Output {
        // egui fades the edges of scroll areas; on pages it reads as a shadow.
        ui.spacing_mut().scroll.fade.strength = 0.0;
        // No overscroll at the ends, as on the pages; it would also undo the
        // offsets set to follow the current page.
        ui.spacing_mut().scroll.kinetic.rubber_band = false;
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
        // Left drag scrolls, as on the pages.
        let source = ScrollSource {
            drag: DragScroll::Always,
            ..Default::default()
        };
        let mut area = ScrollArea::vertical()
            .auto_shrink(false)
            .scroll_source(source);
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
            ..Output::default()
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
        let font = egui::TextStyle::Body.resolve(ui.style());
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
            // Centred in the number's row below the highlight, clear of it.
            let centre = content.min
                + vec2(
                    label.x + label.width / 2.0,
                    label.y + (label.height + HIGHLIGHT + PILL_GAP) / 2.0,
                );
            let colour = if current {
                selection.stroke.color
            } else {
                text
            };
            let galley = painter.layout_no_wrap((page + 1).to_string(), font.clone(), colour);
            let number = Rect::from_center_size(centre, galley.size());
            if current {
                let pill = number.expand2(PILL_PADDING);
                painter.rect_filled(pill, pill.height() / 2.0, selection.bg_fill);
            }
            painter.galley(number.min, galley, colour);
            self.drawn.push((page, rect));
        }
        clicked
    }

    /// The list of matches, kept on the current one; returns the match
    /// clicked, if any.
    fn results(&mut self, ui: &mut egui::Ui, results: &Results) -> Option<usize> {
        ui.spacing_mut().scroll.fade.strength = 0.0;
        ui.spacing_mut().scroll.kinetic.rubber_band = false;
        if !results.status.is_empty() {
            ui.add_space(RESULT_MARGIN);
            ui.horizontal(|ui| {
                ui.add_space(RESULT_MARGIN);
                ui.weak(&results.status);
            });
            ui.add_space(RESULT_MARGIN / 2.0);
        }
        let hits = results.hits;
        let height = ui.available_height();
        let current = hits.current();
        // Scrolled to the current match when it changes, the least distance.
        let target = current
            .filter(|&c| self.followed_hit != Some((results.generation, c)))
            .and_then(|c| {
                self.followed_hit = Some((results.generation, c));
                let top = RESULT_MARGIN + c as f32 * RESULT_ROW_HEIGHT;
                let bottom = top + RESULT_ROW_HEIGHT;
                if top < self.results_offset {
                    Some(top - RESULT_MARGIN)
                } else if bottom > self.results_offset + height {
                    Some(bottom + RESULT_MARGIN - height)
                } else {
                    None
                }
            });
        let source = ScrollSource {
            drag: DragScroll::Always,
            ..Default::default()
        };
        let mut area = ScrollArea::vertical()
            // Its own scroll position, not the thumbnails'.
            .id_salt("results")
            .auto_shrink(false)
            .scroll_source(source);
        if let Some(target) = target {
            area = area.vertical_scroll_offset(target.max(0.0));
        }
        self.result_rows.clear();
        let shown = area.show_viewport(ui, |ui, viewport| {
            let content = vec2(
                ui.available_width(),
                2.0 * RESULT_MARGIN + hits.len() as f32 * RESULT_ROW_HEIGHT,
            );
            let (area, _) = ui.allocate_exact_size(content, Sense::hover());
            let first = ((viewport.min.y - RESULT_MARGIN) / RESULT_ROW_HEIGHT)
                .floor()
                .max(0.0);
            let last = ((viewport.max.y - RESULT_MARGIN) / RESULT_ROW_HEIGHT)
                .ceil()
                .max(0.0);
            let mut clicked = None;
            for index in first as usize..(last as usize).min(hits.len()) {
                let row = Rect::from_min_size(
                    area.min + vec2(0.0, RESULT_MARGIN + index as f32 * RESULT_ROW_HEIGHT),
                    vec2(area.width(), RESULT_ROW_HEIGHT),
                );
                let id = ui.scope_id().with(("match", index));
                if ui.interact(row, id, Sense::click()).clicked() {
                    clicked = Some(index);
                }
                self.result_row(ui, row, hits, index, current == Some(index));
                self.result_rows.push((index, row));
            }
            clicked
        });
        self.results_offset = shown.state.clamped_offset().y;
        shown.inner
    }

    /// One match: its page number, then its context with the match marked.
    fn result_row(&self, ui: &egui::Ui, row: Rect, hits: &Hits, index: usize, current: bool) {
        let Some(hit) = hits.get(index) else {
            return;
        };
        let visuals = ui.visuals();
        let inner = row.shrink2(vec2(RESULT_MARGIN, 1.0));
        if current {
            ui.painter()
                .rect_filled(inner, RESULT_MARGIN / 2.0, visuals.selection.bg_fill);
        }
        let font = egui::TextStyle::Body.resolve(ui.style());
        let (text, weak) = if current {
            (
                visuals.selection.stroke.color,
                visuals.selection.stroke.color,
            )
        } else {
            (visuals.text_color(), visuals.weak_text_color())
        };
        let format = |color| TextFormat::simple(font.clone(), color);
        let mut job = LayoutJob::default();
        job.append(&format!("p. {}  ", hit.page + 1), 0.0, format(weak));
        let context: Vec<char> = hit.text.context.text.chars().collect();
        let bold = &hit.text.context.bold;
        let part = |range: Range<usize>| context[range].iter().collect::<String>();
        job.append(&part(0..bold.start), 0.0, format(text));
        // egui's default fonts have no bold: the match stands out in the
        // strong text colour on a faint highlight.
        job.append(
            &part(bold.clone()),
            0.0,
            TextFormat {
                background: HIGHLIGHT_FILL,
                ..format(if current {
                    text
                } else {
                    visuals.strong_text_color()
                })
            },
        );
        job.append(&part(bold.end..context.len()), 0.0, format(text));
        job.wrap.max_width = inner.width() - RESULT_MARGIN;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let at = egui::pos2(
            inner.min.x + RESULT_MARGIN / 2.0,
            inner.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(at, galley, text);
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
    pub fn result_rows(&self) -> &[(usize, Rect)] {
        &self.result_rows
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
