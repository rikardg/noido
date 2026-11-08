//! The left sidebar: tabs over a column of page thumbnails, as wide as the
//! sidebar, the document's outline as a tree, or the list of matches of a
//! search. Visible thumbnails are rendered sharp by a renderer of their own;
//! the low-res thumbnails stand in until those arrive.

use std::collections::HashMap;
use std::ops::Range;

use eframe::egui::scroll_area::{DragScroll, ScrollSource};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Rect, ScrollArea, Sense, Stroke, vec2};
use noido_core::{
    Column, Eased, Highlight, Hits, Layout, MAX_RENDER_SIDE, OutlineRow, OutlineTree, Pace,
    PageNumber, PageNumbers, PageTextState, RenderTracker, SIDEBAR_MIN_WIDTH, SidebarState,
    SidebarTab, max_width, render_scale, reveal_row,
};
use noido_engine::LinkTarget;
use noido_render::{RenderRequest, Renderer};

use crate::image::{Fades, PageImage, paint_page};
use crate::select::highlight_fill;
use crate::texts::Texts;
use crate::theme;

/// How far the current page's highlight reaches around its thumbnail, and
/// its corner radius, in points.
pub const HIGHLIGHT: f32 = 6.0;
/// Space between a highlighted page number and the edge of its pill.
const PILL_PADDING: egui::Vec2 = egui::vec2(8.0, 2.0);
/// Space between the current page's highlight and its number's pill.
const PILL_GAP: f32 = 4.0;
/// Space around the list of matches and inside the current card's fill.
const RESULT_MARGIN: f32 = 8.0;
/// Space above and below a card's text, inside its fill.
const CARD_PADDING: f32 = 4.0;
/// Space between a card's header and its excerpt.
const CARD_HEADER_GAP: f32 = 2.0;
/// Space between cards.
const CARD_GAP: f32 = 2.0;
/// Space kept between a card's page and its count of matches, and for the
/// word "Page".
const CARD_HEADER_ROOM: f32 = 48.0;
/// Lines of text around a match in its card.
const EXCERPT_ROWS: usize = 3;
/// How many thumbnails each side of the visible ones are rendered sharp
/// ahead: the next spread either way in dual mode.
const MARGIN: usize = 2;

/// Space above and below a tab's label, inside its fill.
const TAB_PADDING: f32 = 4.0;
/// Space between the strip of tabs and what is below.
const TAB_GAP: f32 = 4.0;
/// A tab's corner radius.
const TAB_ROUNDING: f32 = 4.0;
/// Space left and right of an outline row's contents.
const OUTLINE_MARGIN: f32 = 4.0;
/// Space above and below an outline row's text.
const OUTLINE_PADDING: f32 = 3.0;
/// How far each level of the outline is indented, in points.
const OUTLINE_INDENT: f32 = 12.0;
/// The most of a row's width its indent may take, as a fraction: deeper
/// levels line up, so a deep title keeps its chevron, number and some room.
const OUTLINE_MAX_INDENT: f32 = 0.25;
/// The most of a row's width its page number may take, as a fraction.
const OUTLINE_NUMBER_SHARE: f32 = 1.0 / 3.0;
/// Space between an outline row's title and its page number.
const OUTLINE_GAP: f32 = 6.0;
/// What the sidebar needs from the app each frame.
pub struct Input<'a> {
    pub layout: &'a Layout,
    /// The low-res thumbnails, by page.
    pub lowres: Option<&'a HashMap<usize, PageImage>>,
    /// The page the reader is at, which the sidebar keeps in view.
    pub current: Option<usize>,
    /// The pages shown as current: the current page, or its spread.
    pub highlighted: Range<usize>,
    /// The tint thumbnails are drawn with: the paper colour.
    pub paper: Color32,
    /// While searching: the matches, shown instead of the thumbnails.
    pub results: Option<Results<'a>>,
    /// The pages are moving by wheel, flick or drag: following them, the
    /// thumb catches up gently.
    pub scrolling: bool,
    /// The document's highlights, drawn on the thumbnails they cover.
    pub highlights: &'a [Highlight],
    /// The pages' text layouts, for placing the highlights.
    pub texts: &'a mut Texts,
    /// How to ask the text thread for a page's text.
    pub request_text: &'a dyn Fn(usize),
    /// How pages are numbered.
    pub numbers: &'a PageNumbers,
    /// The document's outline; `None` until it is known.
    pub outline: Option<&'a mut OutlineTree>,
    /// The outline entry the reader is in.
    pub current_entry: Option<usize>,
}

/// The matches of a search, for the sidebar.
pub struct Results<'a> {
    pub hits: &'a Hits,
    /// Which query they belong to.
    pub generation: u64,
    /// A line above them: progress, or "No matches".
    pub status: String,
    /// How pages are numbered.
    pub numbers: &'a PageNumbers,
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
    /// An outline entry with a target was clicked: its index.
    pub outline_entry: Option<usize>,
}

/// A visible outline row, for the tests.
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct OutlineRowRects {
    pub entry: usize,
    pub row: Rect,
    /// The chevron's hit area, on a row with children.
    pub chevron: Option<Rect>,
    /// The whole title, which may be drawn elided.
    pub title: String,
    /// The number as drawn; empty for none.
    pub number: String,
}

/// The sidebar's thumbnails and their sharp renders.
pub struct SidebarView {
    /// `None` draws low-res thumbnails only.
    renderer: Option<Renderer>,
    tracker: RenderTracker,
    sharp: HashMap<usize, PageImage>,
    /// Sharp renders fading in over the low-res thumbnails.
    fades: Fades,
    /// The pages visible last frame; empty while closed.
    visible: Range<usize>,
    /// The visible pages and the margin around them, whose sharp renders
    /// are asked for and kept.
    kept: Range<usize>,
    /// Visible thumbnails' screen rects last frame.
    drawn: Vec<(usize, Rect)>,
    thumb_width: f32,
    /// The current page the sidebar last scrolled to (or that was clicked).
    followed: Option<usize>,
    /// The scroll offset at the end of last frame.
    offset: f32,
    /// The thumb of a solid bar, as drawn: it catches up after the page.
    thumb: Eased,
    /// The column's width last frame.
    width: f32,
    /// The thumbnails' scroll area's id, once shown.
    scroll_id: Option<egui::Id>,
    /// Whether the sidebar was shown last frame.
    shown: bool,
    highlighted: Range<usize>,
    /// The list of matches: its visible rows' screen rects last frame, the
    /// current match it last scrolled to, and its scroll offset.
    result_rows: Vec<(usize, Rect)>,
    followed_hit: Option<(u64, usize)>,
    results_offset: f32,
    /// The list's scroll area's id, once shown.
    results_id: Option<egui::Id>,
    /// The tabs' screen rects last frame.
    tabs: Vec<(SidebarTab, Rect)>,
    /// The outline's visible rows last frame.
    #[cfg(test)]
    outline_rows: Vec<OutlineRowRects>,
    /// The entry the outline last scrolled to (or that was clicked).
    followed_entry: Option<usize>,
    /// The outline's scroll offset at the end of last frame.
    outline_offset: f32,
    /// The outline's width last frame.
    outline_width: f32,
    /// The outline's scroll area's id, once shown.
    outline_id: Option<egui::Id>,
    /// Whether the outline was drawn last frame.
    outline_shown: bool,
}

impl SidebarView {
    pub fn new(page_count: usize) -> Self {
        Self {
            renderer: None,
            tracker: RenderTracker::new(page_count),
            sharp: HashMap::new(),
            fades: Fades::default(),
            visible: 0..0,
            kept: 0..0,
            drawn: Vec::new(),
            thumb_width: 0.0,
            followed: None,
            offset: 0.0,
            thumb: Eased::default(),
            width: 0.0,
            scroll_id: None,
            shown: false,
            highlighted: 0..0,
            result_rows: Vec::new(),
            followed_hit: None,
            results_offset: 0.0,
            results_id: None,
            tabs: Vec::new(),
            #[cfg(test)]
            outline_rows: Vec::new(),
            followed_entry: None,
            outline_offset: 0.0,
            outline_width: 0.0,
            outline_id: None,
            outline_shown: false,
        }
    }

    /// The sidebar is closed: no sharp renders, and the outline follows the
    /// reader again when it opens.
    pub fn hidden(&mut self) {
        self.closed();
        self.outline_shown = false;
    }

    /// A different outline: its folding started over and its rows moved, so
    /// the outline follows the reader again.
    pub fn outline_changed(&mut self) {
        self.outline_shown = false;
    }

    /// Renders visible thumbnails sharp with `renderer`.
    pub fn with_renderer(mut self, renderer: Renderer) -> Self {
        self.renderer = Some(renderer);
        self
    }

    /// Takes in sharp renders; those over a `lowres` thumbnail fade in.
    pub fn receive(&mut self, ctx: &egui::Context, lowres: Option<&HashMap<usize, PageImage>>) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        let now = ctx.input(|i| i.time);
        self.fades.forget_finished(now);
        while let Some(rendered) = renderer.try_recv() {
            // Scrolled away or closed meanwhile: the tracker has forgotten
            // the page, so it is asked for again when it comes back.
            if !self.kept.contains(&rendered.page) {
                continue;
            }
            match rendered.result {
                Ok(image) => {
                    self.tracker.mark_ready(rendered.page, rendered.scale);
                    let name = format!("sidebar-{}", rendered.page);
                    let image = PageImage::load(ctx, name, &image);
                    // A margin page's render is there before it shows.
                    let stand_in = self.visible.contains(&rendered.page)
                        && lowres.is_some_and(|l| l.contains_key(&rendered.page));
                    if self.sharp.insert(rendered.page, image).is_none() && stand_in {
                        self.fades.start(rendered.page, now);
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

    /// Renders with `renderer` from now on, for a new version of the
    /// document with `page_count` pages. Sharp images stay until new ones
    /// replace them, except past the new page count.
    pub fn replace(&mut self, renderer: Renderer, page_count: usize) {
        self.renderer = Some(renderer);
        self.tracker = RenderTracker::new(page_count);
        self.sharp.retain(|&page, _| page < page_count);
    }

    /// Forgets the sharp renders while the sidebar is closed, and where its
    /// thumb was drawn: it is drawn in place when the thumbnails show again.
    pub fn closed(&mut self) {
        if !self.shown {
            return;
        }
        self.shown = false;
        self.thumb = Eased::default();
        self.visible = 0..0;
        self.kept = 0..0;
        self.sharp.clear();
        self.drawn.clear();
        self.tracker.forget_outside(0..0);
        if let Some(renderer) = &self.renderer {
            renderer.set_keep(0..0);
        }
    }

    /// Draws the open sidebar at the left of `ui` and records its width,
    /// which the reader may have changed by dragging its edge.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut SidebarState,
        input: &mut Input,
    ) -> Output {
        let window = ui.max_rect().width();
        let max = max_width(window);
        let frame = theme::Theme::current(ui.ctx()).frame(
            ui.ctx().theme(),
            theme::Surface::Sidebar,
            ui.style(),
        );
        let response = egui::Panel::left("sidebar")
            .resizable(true)
            .frame(frame)
            .default_size(state.clamped(window).width)
            .size_range(SIDEBAR_MIN_WIDTH..=max)
            .show(ui, |ui| {
                theme::scope(ui, theme::Role::Sidebar, |ui| {
                    // The strip first: the areas below take what is left.
                    if let Some(tab) = self.strip(ui, state.tab) {
                        state.choose(tab);
                    }
                    if state.tab != SidebarTab::Results {
                        self.result_rows.clear();
                        self.followed_hit = None;
                    }
                    if state.tab != SidebarTab::Outline {
                        #[cfg(test)]
                        self.outline_rows.clear();
                        self.outline_shown = false;
                    }
                    match state.tab {
                        SidebarTab::Thumbnails => self.scrolled(ui, input),
                        SidebarTab::Results => {
                            // The thumbnails are out of sight: no sharp
                            // renders for them.
                            self.closed();
                            match &input.results {
                                Some(results) => Output {
                                    hit: self.results(ui, results),
                                    ..Output::default()
                                },
                                None => {
                                    muted_note(ui, "No search");
                                    Output::default()
                                }
                            }
                        }
                        SidebarTab::Outline => {
                            self.closed();
                            self.outline(ui, input)
                        }
                    }
                })
            });
        state.width = response.response.rect.width();
        // Off the thumbnails tab the thumbnails count as not shown, so they
        // follow the current page when they come back.
        self.shown = state.tab == SidebarTab::Thumbnails;
        response.inner
    }

    /// The row of tabs; returns the one clicked.
    fn strip(&mut self, ui: &mut egui::Ui, active: SidebarTab) -> Option<SidebarTab> {
        const TABS: [(SidebarTab, &str); 3] = [
            (SidebarTab::Thumbnails, "Pages"),
            (SidebarTab::Outline, "Outline"),
            (SidebarTab::Results, "Search"),
        ];
        let font = egui::TextStyle::Body.resolve(ui.style());
        let height = font.size + 2.0 * TAB_PADDING;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.add_space(TAB_GAP);
        let width = row.width() / TABS.len() as f32;
        let selection = ui.visuals().selection;
        let text = ui.visuals().text_color();
        let mut clicked = None;
        self.tabs.clear();
        for (i, (tab, label)) in TABS.into_iter().enumerate() {
            let rect =
                Rect::from_min_size(row.min + vec2(i as f32 * width, 0.0), vec2(width, height));
            self.tabs.push((tab, rect));
            let response = ui.interact(rect, ui.scope_id().with(("tab", i)), Sense::click());
            if response.clicked() {
                clicked = Some(tab);
            }
            let colour = if tab == active {
                ui.painter()
                    .rect_filled(rect.shrink(1.0), TAB_ROUNDING, selection.bg_fill);
                selection.stroke.color
            } else {
                text
            };
            if response.hovered() {
                ui.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                font.clone(),
                colour,
            );
        }
        clicked
    }

    /// The outline as a tree, kept on the current entry; returns the entry
    /// clicked, if it goes somewhere.
    fn outline(&mut self, ui: &mut egui::Ui, input: &mut Input) -> Output {
        #[cfg(test)]
        self.outline_rows.clear();
        let Some(tree) = input.outline.as_deref_mut() else {
            // Not known yet: nothing to say.
            self.outline_shown = false;
            return Output::default();
        };
        if tree.is_empty() {
            muted_note(ui, "This document has no outline");
            self.outline_shown = true;
            return Output::default();
        }
        ui.spacing_mut().scroll.fade.strength = 0.0;
        ui.spacing_mut().scroll.kinetic.rubber_band = false;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0 * OUTLINE_PADDING;
        let content_height = tree.rows().len() as f32 * row_height;
        let outer = (ui.available_width(), ui.available_height());
        // A solid bar, when the rows need one; their height doesn't depend
        // on the width.
        let solid = theme::solid_bar(ui).map(|bar| {
            let bars = noido_core::scroll_bars(outer, bar, |(width, _)| (width, content_height));
            (bars, bars.inner(outer, bar))
        });
        let (width, height) = solid.map_or(outer, |(_, room)| room);
        // An overlay bar covers the rows' right end: keep clear of it when
        // the list overflows.
        let covered = match solid {
            None if content_height > height => theme::floating_bar(ui).unwrap_or(0.0),
            _ => 0.0,
        };
        let width = width - covered;
        let resized = (width - self.outline_width).abs() > 0.5;
        self.outline_width = width;
        let marked = input.current_entry.and_then(|e| tree.marked_row(e));
        // Scrolled to the marked row, the least distance, when the current
        // entry changes, the tab shows or the width changes. Folding keeps
        // the entry; the reader's own scrolling stands.
        let target = (!self.outline_shown || resized || self.followed_entry != input.current_entry)
            .then(|| {
                self.followed_entry = input.current_entry;
                marked.and_then(|row| reveal_row(row, row_height, self.outline_offset, height))
            })
            .flatten();
        self.outline_shown = true;
        let source = ScrollSource {
            drag: DragScroll::Always,
            ..Default::default()
        };
        let mut area = ScrollArea::vertical()
            // Its own scroll position, not the thumbnails'.
            .id_salt("outline")
            .auto_shrink(false)
            .scroll_source(source);
        if let Some(target) = target {
            area = area.vertical_scroll_offset(target);
        }
        if let Some((bars, _)) = solid {
            area = theme::with_solid_bars(ui.ctx(), area, bars, self.outline_id);
        }
        let last = input.layout.page_count().saturating_sub(1);
        let numbers = input.numbers;
        // The rows' colours are the panel's, as the cards' are.
        let panel = ui.style().clone();
        let shown = theme::scope(ui, theme::Role::Scrollbar, |ui| {
            if let Some((bars, _)) = solid {
                theme::paint_grooves(ui, ui.available_rect_before_wrap(), bars);
            }
            let shown = area.show_viewport(ui, |ui, viewport| {
                ui.set_style(panel);
                let (area, _) = ui.allocate_exact_size(
                    vec2(ui.available_width() - covered, content_height),
                    Sense::hover(),
                );
                let rows = tree.rows();
                let first = (viewport.min.y / row_height).floor().max(0.0) as usize;
                let end = ((viewport.max.y / row_height).ceil().max(0.0) as usize).min(rows.len());
                let visible: Vec<OutlineRow> = rows[first.min(end)..end].to_vec();
                let mut clicked = None;
                let mut toggled = None;
                for (i, row) in (first..).zip(&visible) {
                    let rect = Rect::from_min_size(
                        area.min + vec2(0.0, i as f32 * row_height),
                        vec2(area.width(), row_height),
                    );
                    let drawn = OutlineDraw {
                        tree,
                        numbers,
                        last,
                        font: &font,
                        marked: marked == Some(i),
                    };
                    match self.outline_row(ui, rect, row, &drawn) {
                        RowClick::Go => clicked = Some(row.entry),
                        RowClick::Toggle => toggled = Some(row.entry),
                        RowClick::None => {}
                    }
                }
                (clicked, toggled)
            });
            if let Some((bars, _)) = solid {
                theme::paint_thumbs(ui, &shown, bars);
            }
            shown
        });
        self.outline_id = Some(shown.id);
        self.outline_offset = shown.state.clamped_offset().y;
        let (clicked, toggled) = shown.inner;
        if let Some(entry) = toggled {
            tree.toggle(entry);
        }
        if let Some(entry) = clicked
            && matches!(tree.target(entry), Some(LinkTarget::Internal(_)))
        {
            // Already in view: going there mustn't scroll the outline.
            self.followed_entry = Some(entry);
        }
        Output {
            outline_entry: clicked,
            ..Output::default()
        }
    }

    /// One row of the outline in `rect`: the chevron, the title and the page
    /// number, filled if it is the marked one. Says what a click on it did.
    fn outline_row(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        row: &OutlineRow,
        drawn: &OutlineDraw,
    ) -> RowClick {
        let tree = drawn.tree;
        let target = tree.target(row.entry);
        // A row acts when a click on it goes somewhere or folds a branch.
        let goes = match target {
            Some(LinkTarget::Internal(_)) => true,
            Some(LinkTarget::Web(uri)) => crate::links::opens(uri),
            None => false,
        };
        let acts = goes || row.has_children;
        let id = ui.scope_id().with(("outline", row.entry));
        let response = ui.interact(rect, id, Sense::click());
        let mut click = RowClick::None;
        let indent = (row.depth as f32 * OUTLINE_INDENT).min(rect.width() * OUTLINE_MAX_INDENT);
        let chevron = Rect::from_min_size(
            egui::pos2(rect.min.x + OUTLINE_MARGIN + indent, rect.min.y),
            vec2(rect.height(), rect.height()),
        );
        if row.has_children {
            // Over the row: a press on it folds and doesn't jump.
            let toggle = ui.interact(chevron, id.with("chevron"), Sense::click());
            if toggle.clicked() {
                click = RowClick::Toggle;
            }
            if toggle.hovered() {
                ui.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if click == RowClick::None && response.clicked() {
            click = if goes {
                RowClick::Go
            } else if row.has_children {
                RowClick::Toggle
            } else {
                RowClick::None
            };
        }
        if acts && response.hovered() {
            ui.set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let visuals = ui.visuals();
        let (text, weak) = if drawn.marked {
            (
                visuals.selection.stroke.color,
                visuals.selection.stroke.color,
            )
        } else {
            (visuals.text_color(), visuals.weak_text_color())
        };
        if drawn.marked {
            ui.painter()
                .rect_filled(rect, 2.0, ui.visuals().selection.bg_fill);
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if row.has_children {
            paint_chevron(&painter, chevron, row.open, text);
        }
        // The number: a third of the row at most, at the right.
        let number = match target {
            Some(LinkTarget::Internal(d)) => {
                let number = drawn.numbers.short_number(d.page.min(drawn.last));
                number_text(
                    ui,
                    &number,
                    drawn.font,
                    (rect.width() - 2.0 * OUTLINE_MARGIN) * OUTLINE_NUMBER_SHARE,
                )
            }
            _ => String::new(),
        };
        let mut right = rect.max.x - OUTLINE_MARGIN;
        if !number.is_empty() {
            let galley = ui
                .fonts_mut(|fonts| fonts.layout_no_wrap(number.clone(), drawn.font.clone(), weak));
            right -= galley.size().x;
            let at = egui::pos2(right, rect.center().y - galley.size().y / 2.0);
            painter.galley(at, galley, weak);
            right -= OUTLINE_GAP;
        }
        let left = chevron.max.x + 2.0;
        let title = tree.title(row.entry);
        let mut job = LayoutJob::simple_singleline(title.to_owned(), drawn.font.clone(), text);
        job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        if galley.elided {
            response.on_hover_text(title);
        }
        let at = egui::pos2(left, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, text);
        #[cfg(test)]
        self.outline_rows.push(OutlineRowRects {
            entry: row.entry,
            row: rect,
            chevron: row.has_children.then_some(chevron),
            title: title.to_owned(),
            number,
        });
        click
    }

    /// The scrolling column, kept on the current page.
    fn scrolled(&mut self, ui: &mut egui::Ui, input: &mut Input) -> Output {
        // egui fades the edges of scroll areas; on pages it reads as a shadow.
        ui.spacing_mut().scroll.fade.strength = 0.0;
        // No overscroll at the ends, as on the pages; it would also undo the
        // offsets set to follow the current page.
        ui.spacing_mut().scroll.kinetic.rubber_band = false;
        let outer = (ui.available_width(), ui.available_height());
        // A solid bar takes room from the thumbnails: lay them out beside it
        // when they need one.
        let solid = theme::solid_bar(ui).map(|bar| {
            let bars = noido_core::scroll_bars(outer, bar, |(width, _)| {
                let column = Column::new(input.layout.page_sizes(), width);
                (width, column.content_height())
            });
            (bars, bars.inner(outer, bar))
        });
        let (width, height) = solid.map_or(outer, |(_, room)| room);
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
        if let Some((bars, _)) = solid {
            area = theme::with_solid_bars(ui.ctx(), area, bars, self.scroll_id);
        }
        let shown = theme::scope(ui, theme::Role::Scrollbar, |ui| {
            if let Some((bars, _)) = solid {
                theme::paint_grooves(ui, ui.available_rect_before_wrap(), bars);
            }
            let shown =
                area.show_viewport(ui, |ui, viewport| self.column(ui, viewport, &column, input));
            if let Some((bars, _)) = solid {
                // Following the page, the thumbnails jump and the thumb
                // eases after them, gently while the pages scroll on; it
                // goes with the reader's scrolling.
                let offset = shown.state.clamped_offset();
                let follows = target.is_some()
                    && self.shown
                    && !resized
                    && !theme::Theme::current(ui.ctx()).reduce_motion();
                let pace = if input.scrolling {
                    Pace::Gentle
                } else {
                    Pace::Quick
                };
                let follow = follows.then_some(pace);
                // Under the pointer it is where egui takes presses on it.
                let inner = shown.inner_rect;
                let groove = Rect::from_min_max(
                    egui::pos2(inner.max.x, inner.min.y),
                    egui::pos2(
                        inner.max.x + ui.spacing().scroll.allocated_width(),
                        inner.max.y,
                    ),
                );
                if ui.rect_contains_pointer(groove) {
                    self.thumb = Eased::default();
                }
                // A frame's time, not a gap since an idle one.
                let dt = ui.input(|input| input.stable_dt).min(0.1);
                let y = self.thumb.update(offset.y, follow, dt);
                if self.thumb.moving() {
                    ui.ctx().request_repaint();
                }
                theme::paint_thumbs_at(ui, &shown, bars, egui::vec2(offset.x, y));
            }
            shown
        });
        self.scroll_id = Some(shown.id);
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
        input: &mut Input,
    ) -> Option<usize> {
        self.thumb_width = column.thumb_width();
        self.highlighted = input.highlighted.clone();
        let (content, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), column.content_height()),
            Sense::hover(),
        );
        let visible = column.visible(viewport.min.y, viewport.max.y);
        self.request(column, &visible, ui.ctx().pixels_per_point(), input.layout);
        let pages = with_margin(&visible, input.layout.page_count());
        want_texts(input, pages);
        self.visible = visible.clone();

        let outline = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let selection = ui.visuals().selection;
        let text = ui.visuals().text_color();
        let dark = ui.visuals().dark_mode;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut clicked = None;
        let now = ui.input(|i| i.time);
        let mut fading = false;
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
            fading |= paint_page(
                painter,
                rect,
                self.sharp.get(&page),
                input.lowres.and_then(|lowres| lowres.get(&page)),
                self.fades.alpha(page, now),
                input.paper,
            );
            // The document's highlights, as on the page.
            if let PageTextState::Known(text) = input.texts.get(page)
                && let Some(size) = input.layout.page_size(page)
                && size.width > 0.0
            {
                for highlight in input.highlights {
                    let Some(range) = highlight.on_page(page, text.char_count()) else {
                        continue;
                    };
                    let fill = highlight_fill(highlight.colour, dark);
                    for area in text.rects(range) {
                        painter.rect_filled(mark_rect(rect, size.width, area), 0.0, fill);
                    }
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
            let shown = number_text(ui, &input.numbers.both(page), &font, label.width);
            let painter = ui.painter();
            let galley = painter.layout_no_wrap(shown, font.clone(), colour);
            let number = Rect::from_center_size(centre, galley.size());
            if current {
                let pill = number.expand2(PILL_PADDING);
                painter.rect_filled(pill, pill.height() / 2.0, selection.bg_fill);
            }
            painter.galley(number.min, galley, colour);
            self.drawn.push((page, rect));
        }
        if fading {
            ui.ctx().request_repaint();
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
        let card = card_height(ui);
        let outer = (ui.available_width(), ui.available_height());
        let content_height = 2.0 * RESULT_MARGIN + hits.len() as f32 * card;
        // A solid bar, when the matches need one, as beside the thumbnails;
        // their height doesn't depend on the width.
        let solid = theme::solid_bar(ui).map(|bar| {
            let bars = noido_core::scroll_bars(outer, bar, |(width, _)| (width, content_height));
            (bars, bars.inner(outer, bar))
        });
        let height = solid.map_or(outer.1, |(_, room)| room.1);
        let current = hits.current();
        // Scrolled to the current match when it changes, the least distance.
        let target = current
            .filter(|&c| self.followed_hit != Some((results.generation, c)))
            .and_then(|c| {
                self.followed_hit = Some((results.generation, c));
                let top = RESULT_MARGIN + c as f32 * card;
                let bottom = top + card;
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
        if let Some((bars, _)) = solid {
            area = theme::with_solid_bars(ui.ctx(), area, bars, self.results_id);
        }
        self.result_rows.clear();
        // The cards' colours are the panel's: the scrollbar role sets the
        // widget colours egui's bars use, strong text among them. egui
        // paints the bars with the outer style, so they keep the role.
        let panel = ui.style().clone();
        let shown = theme::scope(ui, theme::Role::Scrollbar, |ui| {
            if let Some((bars, _)) = solid {
                theme::paint_grooves(ui, ui.available_rect_before_wrap(), bars);
            }
            let shown = area.show_viewport(ui, |ui, viewport| {
                ui.set_style(panel);
                let content = vec2(ui.available_width(), content_height);
                let (area, _) = ui.allocate_exact_size(content, Sense::hover());
                let first = ((viewport.min.y - RESULT_MARGIN) / card).floor().max(0.0);
                let last = ((viewport.max.y - RESULT_MARGIN) / card).ceil().max(0.0);
                let mut clicked = None;
                for index in first as usize..(last as usize).min(hits.len()) {
                    let row = Rect::from_min_size(
                        area.min + vec2(0.0, RESULT_MARGIN + index as f32 * card),
                        vec2(area.width(), card),
                    );
                    let id = ui.scope_id().with(("match", index));
                    if ui.interact(row, id, Sense::click()).clicked() {
                        clicked = Some(index);
                    }
                    self.result_card(ui, row, results, index, current == Some(index));
                    self.result_rows.push((index, row));
                }
                clicked
            });
            if let Some((bars, _)) = solid {
                theme::paint_thumbs(ui, &shown, bars);
            }
            shown
        });
        self.results_id = Some(shown.id);
        self.results_offset = shown.state.clamped_offset().y;
        shown.inner
    }

    /// One match as a card: a header with its page and its number on the
    /// page, then the lines of its context around it, the match marked.
    fn result_card(
        &self,
        ui: &egui::Ui,
        row: Rect,
        results: &Results,
        index: usize,
        current: bool,
    ) {
        let hits = results.hits;
        let Some(hit) = hits.get(index) else {
            return;
        };
        let visuals = ui.visuals();
        let inner = row.shrink2(vec2(RESULT_MARGIN, CARD_GAP / 2.0));
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
        let left = inner.min.x + RESULT_MARGIN / 2.0;
        let width = inner.width() - RESULT_MARGIN;
        // The header, small and weak: the page, and which of its matches.
        let small = egui::TextStyle::Small.resolve(ui.style());
        let first = hits.first_on(hit.page).unwrap_or(index);
        let count = hits.on_page(hit.page).len();
        let header =
            |text: String| ui.fonts_mut(|fonts| fonts.layout_no_wrap(text, small.clone(), weak));
        let top = inner.min.y + CARD_PADDING;
        let number = header(hits_text(index - first + 1, count));
        let room = width - number.size().x - CARD_HEADER_ROOM;
        let page = header(format!(
            "Page {}",
            number_text(ui, &results.numbers.both(hit.page), &small, room)
        ));
        ui.painter()
            .galley(egui::pos2(left, top), page.clone(), weak);
        ui.painter().galley(
            egui::pos2(left + width - number.size().x, top),
            number,
            weak,
        );
        // The excerpt: the whole context laid out once, shifted up and
        // clipped to the lines around the match. Laying out only those
        // lines could wrap them differently, by a pixel.
        let context: Vec<char> = hit.text.context.text.chars().collect();
        let bold = &hit.text.context.bold;
        let format = |color| TextFormat::simple(font.clone(), color);
        let part = |range: Range<usize>| context[range].iter().collect::<String>();
        let mut job = LayoutJob::default();
        job.append(&part(0..bold.start), 0.0, format(text));
        // egui's default fonts have no bold: the match stands out in the
        // strong text colour on a faint highlight.
        job.append(
            &part(bold.clone()),
            0.0,
            TextFormat {
                background: crate::search::highlight_fill(ui.visuals().warn_fg_color),
                ..format(if current {
                    text
                } else {
                    visuals.strong_text_color()
                })
            },
        );
        job.append(&part(bold.end..context.len()), 0.0, format(text));
        job.wrap.max_width = width;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let rows: Vec<usize> = galley
            .rows
            .iter()
            .map(|row| row.char_count_including_newline().0)
            .collect();
        let shown = rows_around(&rows, bold.start, EXCERPT_ROWS);
        let (Some(first), Some(last)) = (
            galley.rows.get(shown.start),
            shown
                .end
                .checked_sub(1)
                .and_then(|row| galley.rows.get(row)),
        ) else {
            return;
        };
        let top = top + page.size().y + CARD_HEADER_GAP;
        let clip = Rect::from_min_max(
            egui::pos2(inner.min.x, top),
            egui::pos2(inner.max.x, top + last.rect().max.y - first.rect().min.y),
        );
        // Within the list's own clip, so it stays inside the scroll area.
        let clip = clip.intersect(ui.painter().clip_rect());
        let at = egui::pos2(left, top - first.rect().min.y);
        ui.painter().with_clip_rect(clip).galley(at, galley, text);
    }

    /// Asks for sharp renders of the `visible` pages and the margin around
    /// them, and drops the rest.
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
        let kept = with_margin(visible, layout.page_count());
        let order = render_order(visible, &kept);
        renderer.set_keep(kept.clone());
        renderer.set_priority(order.clone());
        let width = column.thumb_width() * pixels_per_point;
        for page in order {
            let wanted = self.tracker.wanted(page..page + 1, |page| {
                let size = layout.page_size(page).expect("page in range");
                render_scale(size, width / size.width, MAX_RENDER_SIDE)
            });
            for (page, scale) in wanted {
                renderer.request(RenderRequest { page, scale });
            }
        }
        self.tracker.forget_outside(kept.clone());
        self.sharp.retain(|page, _| kept.contains(page));
        self.kept = kept;
    }

    /// The tabs, with where they were drawn.
    #[cfg(test)]
    pub fn tab_rects(&self) -> &[(SidebarTab, Rect)] {
        &self.tabs
    }

    #[cfg(test)]
    pub fn kept_pages(&self) -> Range<usize> {
        self.kept.clone()
    }

    #[cfg(test)]
    pub fn drawn(&self) -> &[(usize, Rect)] {
        &self.drawn
    }

    #[cfg(test)]
    pub fn offset(&self) -> f32 {
        self.offset
    }

    #[cfg(test)]
    pub fn result_rows(&self) -> &[(usize, Rect)] {
        &self.result_rows
    }

    #[cfg(test)]
    pub fn outline_rows(&self) -> &[OutlineRowRects] {
        &self.outline_rows
    }

    #[cfg(test)]
    pub fn outline_offset(&self) -> f32 {
        self.outline_offset
    }

    #[cfg(test)]
    pub fn sharp_pages(&self) -> Vec<usize> {
        let mut pages: Vec<usize> = self.sharp.keys().copied().collect();
        pages.sort_unstable();
        pages
    }

    #[cfg(test)]
    pub fn fading(&self, page: usize) -> bool {
        self.fades.started(page)
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

/// What a click on an outline row did.
#[derive(Clone, Copy, PartialEq)]
enum RowClick {
    None,
    /// It went to the row's target.
    Go,
    /// It folded or unfolded the row.
    Toggle,
}

/// What drawing an outline row needs besides the row.
struct OutlineDraw<'a> {
    tree: &'a OutlineTree,
    numbers: &'a PageNumbers,
    /// The document's last page, which a target past the end goes to.
    last: usize,
    font: &'a egui::FontId,
    marked: bool,
}

/// `text` muted, wrapped inside a margin, so it never widens the sidebar.
fn muted_note(ui: &mut egui::Ui, text: &str) {
    let muted = ui.visuals().weak_text_color();
    ui.add_space(RESULT_MARGIN);
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(RESULT_MARGIN as i8, 0))
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).color(muted)).wrap());
        });
}

/// The fold chevron in `rect`, as two line segments: `>` when folded, `v`
/// when `open`.
fn paint_chevron(painter: &egui::Painter, rect: Rect, open: bool, colour: Color32) {
    let c = rect.center();
    let half = rect.height() * 0.14;
    let stroke = Stroke::new(1.5, colour);
    let points = if open {
        [
            egui::pos2(c.x - half, c.y - half * 0.6),
            egui::pos2(c.x, c.y + half * 0.6),
            egui::pos2(c.x + half, c.y - half * 0.6),
        ]
    } else {
        [
            egui::pos2(c.x - half * 0.6, c.y - half),
            egui::pos2(c.x + half * 0.6, c.y),
            egui::pos2(c.x - half * 0.6, c.y + half),
        ]
    };
    painter.line_segment([points[0], points[1]], stroke);
    painter.line_segment([points[1], points[2]], stroke);
}

/// `number` as text at most `max_width` wide: a label too long is elided,
/// and the file number beside it stays.
pub fn number_text(
    ui: &egui::Ui,
    number: &PageNumber,
    font: &egui::FontId,
    max_width: f32,
) -> String {
    let full = number.text();
    let Some(label) = number.label else {
        return full;
    };
    let width = |text: String| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text, font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        })
    };
    if width(full.clone()) <= max_width {
        return full;
    }
    let with_kept = |kept: usize| {
        let cut: String = label.chars().take(kept).collect();
        PageNumber {
            label: Some(&format!("{}…", cut.trim_end())),
            file: number.file.clone(),
        }
        .text()
    };
    // The most of the label that fits; none at all when nothing does.
    let (mut fits, mut over) = (0, label.chars().count());
    while over - fits > 1 {
        let middle = (fits + over) / 2;
        if width(with_kept(middle)) <= max_width {
            fits = middle;
        } else {
            over = middle;
        }
    }
    with_kept(fits)
}

/// The thumbnail rectangle a highlight's `area`, in page points, falls on:
/// `thumb`'s width over the page's `page_width`, with each side at least a
/// point so a small mark still shows at thumbnail size, and clipped to the
/// thumbnail.
fn mark_rect(thumb: Rect, page_width: f32, area: noido_text::Rect) -> Rect {
    let scale = thumb.width() / page_width;
    let scaled = Rect::from_min_size(
        thumb.min + vec2(area.x, area.y) * scale,
        vec2(area.width, area.height) * scale,
    );
    Rect::from_center_size(
        scaled.center(),
        vec2(scaled.width().max(1.0), scaled.height().max(1.0)),
    )
    .intersect(thumb)
}

/// Asks for the text of `pages` that hold a highlight, so their marks can be
/// placed.
fn want_texts(input: &mut Input, pages: Range<usize>) {
    let Input {
        texts,
        request_text,
        highlights,
        ..
    } = input;
    for page in pages {
        if highlights.iter().any(|h| h.pages().contains(&page)) {
            texts.want(page, *request_text);
        }
    }
}

/// The height of a match's card: its header, its excerpt and the space
/// around them.
fn card_height(ui: &egui::Ui) -> f32 {
    let small = egui::TextStyle::Small.resolve(ui.style());
    let body = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts_mut(|fonts| fonts.row_height(&small) + EXCERPT_ROWS as f32 * fonts.row_height(&body))
        + 2.0 * CARD_PADDING
        + CARD_HEADER_GAP
        + CARD_GAP
}

/// How many matches the page has, and which this is: "3/10 hits", or
/// "1 hit" when it is the only one.
fn hits_text(number: usize, count: usize) -> String {
    if count == 1 {
        "1 hit".to_owned()
    } else {
        format!("{number}/{count} hits")
    }
}

/// `visible` widened by [`MARGIN`] pages each side, within `page_count`.
fn with_margin(visible: &Range<usize>, page_count: usize) -> Range<usize> {
    if visible.is_empty() {
        return visible.clone();
    }
    visible.start.saturating_sub(MARGIN)..(visible.end + MARGIN).min(page_count)
}

/// The pages of `kept` in the order to render them: the `visible` ones,
/// then the margin, nearest first, the page after before the page before.
fn render_order(visible: &Range<usize>, kept: &Range<usize>) -> Vec<usize> {
    let mut order: Vec<usize> = visible.clone().collect();
    for distance in 1..=MARGIN {
        let after = visible.end + distance - 1;
        if after < kept.end {
            order.push(after);
        }
        if let Some(before) = visible.start.checked_sub(distance)
            && before >= kept.start
        {
            order.push(before);
        }
    }
    order
}

/// Up to `wanted` of the rows, of the lengths `rows`, with the row holding
/// character `at` in the middle, or as near as the first and last rows
/// allow.
fn rows_around(rows: &[usize], at: usize, wanted: usize) -> Range<usize> {
    let mut end = 0;
    let row = rows
        .iter()
        .position(|&len| {
            end += len;
            at < end
        })
        .unwrap_or(rows.len().saturating_sub(1));
    let first = row
        .saturating_sub(wanted / 2)
        .min(rows.len().saturating_sub(wanted));
    first..(first + wanted).min(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_order_is_visible_then_nearest_after_first() {
        let kept = with_margin(&(3..5), 10);
        assert_eq!(kept, 1..7);
        assert_eq!(render_order(&(3..5), &kept), [3, 4, 5, 2, 6, 1]);
    }

    #[test]
    fn render_order_stops_at_the_ends() {
        let kept = with_margin(&(0..2), 10);
        assert_eq!(kept, 0..4);
        assert_eq!(render_order(&(0..2), &kept), [0, 1, 2, 3]);
        let kept = with_margin(&(8..10), 10);
        assert_eq!(kept, 6..10);
        assert_eq!(render_order(&(8..10), &kept), [8, 9, 7, 6]);
        assert_eq!(with_margin(&(4..4), 10), 4..4);
        assert!(render_order(&(4..4), &(4..4)).is_empty());
    }

    #[test]
    fn hits_text_counts_on_page() {
        assert_eq!(hits_text(3, 10), "3/10 hits");
        assert_eq!(hits_text(1, 2), "1/2 hits");
        assert_eq!(hits_text(1, 1), "1 hit");
    }

    #[test]
    fn rows_around_puts_match_row_in_middle() {
        // Rows of 10, 12, 8, 9 and 11 characters; the match starts at 25,
        // in the third row (22..30).
        let rows = [10, 12, 8, 9, 11];
        assert_eq!(rows_around(&rows, 25, 3), 1..4);
    }

    #[test]
    fn rows_around_at_start_takes_first_rows() {
        let rows = [10, 12, 8, 9, 11];
        assert_eq!(rows_around(&rows, 3, 3), 0..3);
    }

    #[test]
    fn rows_around_at_end_takes_last_rows() {
        let rows = [10, 12, 8, 9, 11];
        assert_eq!(rows_around(&rows, 45, 3), 2..5);
    }

    #[test]
    fn rows_around_few_rows_takes_all() {
        assert_eq!(rows_around(&[10, 5], 12, 3), 0..2);
        assert_eq!(rows_around(&[], 0, 3), 0..0);
    }

    fn area(x: f32, y: f32, width: f32, height: f32) -> noido_text::Rect {
        noido_text::Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn mark_rect_scales_page_points_into_the_thumbnail() {
        let thumb = Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(100.0, 130.0));
        // A 500 pt wide page at 100 pt: one fifth.
        let mark = mark_rect(thumb, 500.0, area(250.0, 100.0, 20.0, 10.0));
        assert!((mark.min.x - 60.0).abs() < 0.01, "{mark:?}");
        assert!((mark.min.y - 40.0).abs() < 0.01, "{mark:?}");
        assert!((mark.width() - 4.0).abs() < 0.01, "{mark:?}");
        assert!((mark.height() - 2.0).abs() < 0.01, "{mark:?}");
    }

    #[test]
    fn mark_rect_grows_thin_marks_to_a_point_centred() {
        let thumb = Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(100.0, 130.0));
        let mark = mark_rect(thumb, 500.0, area(100.0, 50.0, 2.0, 0.5));
        // 0.4 pt wide and 0.1 pt tall at one fifth: a point each, centred on
        // the scaled mark's centre.
        assert!((mark.center().x - 30.2).abs() < 0.01, "{mark:?}");
        assert!((mark.center().y - 30.05).abs() < 0.01, "{mark:?}");
        assert!((mark.width() - 1.0).abs() < 0.01, "{mark:?}");
        assert!((mark.height() - 1.0).abs() < 0.01, "{mark:?}");
    }

    #[test]
    fn mark_rect_grows_only_the_thin_side() {
        let thumb = Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(100.0, 130.0));
        // 20 pt wide and 0.1 pt tall at one fifth: the width stands, the
        // height grows to a point.
        let mark = mark_rect(thumb, 500.0, area(0.0, 100.0, 100.0, 0.5));
        assert!((mark.width() - 20.0).abs() < 0.01, "{mark:?}");
        assert!((mark.height() - 1.0).abs() < 0.01, "{mark:?}");
    }

    #[test]
    fn mark_rect_stays_inside_the_thumbnail() {
        let thumb = Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(100.0, 130.0));
        // A line whose glyphs hang off the page's left edge, and a thin mark
        // there that the minimum size grows outwards: both clip to the page.
        let off_page = mark_rect(thumb, 500.0, area(-10.0, 0.0, 20.0, 10.0));
        assert!(thumb.contains_rect(off_page), "{off_page:?} in {thumb:?}");
        let thin = mark_rect(thumb, 500.0, area(0.0, 0.0, 0.1, 0.1));
        assert!(thumb.contains_rect(thin), "{thin:?} in {thumb:?}");
    }
}
