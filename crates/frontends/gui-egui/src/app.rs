use std::collections::HashMap;
use std::time::Duration;

use eframe::egui::scroll_area::{DragScroll, ScrollSource};
use eframe::egui::{
    self, Color32, ColorImage, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, Vec2, pos2,
    vec2,
};
use noido_core::{
    Action, Direction, Key, KeyPress, Keymap, MAX_RENDER_SIDE, RenderTracker, SidebarState,
    THUMBNAIL_BUDGET, View, ViewState, render_scale, window_keep, window_order,
};
use noido_engine::{Rgba8Image, Size};
use noido_render::{DocumentInfo, RenderRequest, Renderer, Thumbnailer};
use noido_state::{DocState, Slot, Store};

use crate::instance::Activation;
use crate::titlebar::{self, TITLE_BAR_HEIGHT};
use crate::watch::{Reload, Watcher};
use crate::{frame, sidebar};

/// Spreads (pages in single mode) rendered at full resolution around the
/// current one, after the visible ones: the current spread, then six ahead
/// and three behind in the direction of travel (see [`window_order`]).
const PREFETCH_BUDGET: usize = 10;
/// Spreads on each side of the current one whose full renders are kept; more
/// than the window reaches ahead, so moving back and forth doesn't re-render.
const KEEP: usize = 8;
/// Trackpad scroll distance, in UI points, that flips one page in paged mode
/// (about one mouse wheel step).
const TRACKPAD_FLIP_DISTANCE: f32 = 125.0;
/// Offset difference, in UI points, below which egui's scroll position is left alone.
const OFFSET_EPSILON: f32 = 0.5;

/// Seconds the view must stay unchanged before its state is saved.
const SAVE_DELAY: f64 = 2.0;

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
    textures: HashMap<usize, PageImage>,
    keymap: Keymap,
    last_scroll: Option<ScrollState>,
    /// Trackpad distance scrolled towards the next flip, positive downwards.
    flip_distance: f32,
    persistence: Option<Persistence>,
    thumbnails: Option<Thumbnails>,
    /// The page at the window centre.
    focus: Option<usize>,
    /// The direction the focus last moved in.
    direction: Direction,
    sidebar: SidebarState,
    /// The document's file name, shown in the title bar.
    name: String,
    /// The search field's text.
    search: String,
    /// Mod+F was pressed: the search field takes focus next frame.
    search_focus_requested: bool,
    /// Enter in the search field opened the sidebar, so Esc closes it again.
    sidebar_opened_by_search: bool,
    /// Where the title bar's parts were drawn last frame.
    bar: titlebar::Rects,
    /// Brings new versions of the document when its file changes.
    watcher: Option<Watcher>,
    /// Set when a later launch on the same file asks the window forward.
    activation: Option<Activation>,
    /// Where the reader was before a reload to fewer pages moved them, and
    /// where that left them: a version cut short mid-write mustn't cost
    /// them their place, so the next reload returns there unless they moved.
    clamped: Option<(ViewState, ViewState)>,
}

/// Low-res images shown for pages whose full render hasn't arrived.
struct Thumbnails {
    thumbnailer: Thumbnailer,
    textures: HashMap<usize, PageImage>,
    /// The page the thumbnailer was last pointed at.
    sent: Option<usize>,
}

/// Saves the state of the open document.
struct Persistence {
    store: Store,
    slot: Slot,
    saved: Option<DocState>,
    /// A state not saved yet, and the time it was first seen.
    changed: Option<(DocState, f64)>,
    /// Whether a failed save was already reported.
    warned: bool,
}

impl Persistence {
    /// Saves `state` once it has been unchanged for [`SAVE_DELAY`] at `now`;
    /// otherwise returns how long until then.
    fn update(&mut self, state: DocState, now: f64) -> Option<f64> {
        if self.saved == Some(state) {
            self.changed = None;
            return None;
        }
        let since = match self.changed {
            Some((changed, since)) if changed == state => since,
            _ => {
                self.changed = Some((state, now));
                now
            }
        };
        let left = SAVE_DELAY - (now - since);
        if left > 0.0 {
            return Some(left);
        }
        self.save(state);
        None
    }

    fn save(&mut self, state: DocState) {
        if let Err(e) = self.store.save(&self.slot, &state)
            && !self.warned
        {
            eprintln!("noido: cannot save view state: {e}");
            self.warned = true;
        }
        // Also after a failure, so it isn't retried every frame.
        self.saved = Some(state);
        self.changed = None;
    }
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
            persistence: None,
            thumbnails: None,
            focus: None,
            direction: Direction::Forward,
            sidebar: SidebarState::default(),
            search_focus_requested: false,
            name: String::new(),
            search: String::new(),
            sidebar_opened_by_search: false,
            bar: titlebar::Rects::default(),
            watcher: None,
            activation: None,
            clamped: None,
        }
    }

    /// Shows `name` in the title bar.
    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    /// Shows thumbnails from `thumbnailer` until pages render.
    pub fn with_thumbnails(mut self, thumbnailer: Thumbnailer) -> Self {
        self.thumbnails = Some(Thumbnails {
            thumbnailer,
            textures: HashMap::new(),
            sent: None,
        });
        self
    }

    /// Follows the page at the window centre and the direction it moves in.
    fn update_focus(&mut self) {
        let Some(page) = self.view.centre_page() else {
            return;
        };
        if let Some(old) = self.focus
            && old != page
        {
            self.direction = if page > old {
                Direction::Forward
            } else {
                Direction::Backward
            };
        }
        self.focus = Some(page);
    }

    /// Points the thumbnailer at the current page and takes in its thumbnails.
    fn update_thumbnails(&mut self, ctx: &egui::Context) {
        let Some(thumbnails) = &mut self.thumbnails else {
            return;
        };
        let page_count = self.view.layout().page_count();
        if let Some(page) = self.focus
            && thumbnails.sent != Some(page)
        {
            thumbnails.sent = Some(page);
            thumbnails.thumbnailer.set_focus(page, self.direction);
            let keep = window_keep(page_count, page, THUMBNAIL_BUDGET);
            thumbnails.textures.retain(|page, _| keep.contains(page));
        }
        let keep = window_keep(page_count, thumbnails.sent.unwrap_or(0), THUMBNAIL_BUDGET);
        while let Some(thumbnail) = thumbnails.thumbnailer.try_recv() {
            // A failed thumbnail leaves the page white; its full render reports errors.
            if let Ok(image) = thumbnail.result
                && keep.contains(&thumbnail.page)
            {
                let name = format!("thumbnail-{}", thumbnail.page);
                let size = self.view.layout().page_size(thumbnail.page);
                let texture = PageImage::load(ctx, name, &image, size, thumbnail.scale);
                thumbnails.textures.insert(thumbnail.page, texture);
            }
        }
    }

    /// The best image of `page`: its full render, else its thumbnail.
    fn page_image(&self, page: usize) -> Option<&PageImage> {
        self.textures.get(&page).or_else(|| {
            self.thumbnails
                .as_ref()
                .and_then(|thumbnails| thumbnails.textures.get(&page))
        })
    }

    /// Shows the new versions of the document that `watcher` brings.
    pub fn with_watcher(mut self, watcher: Watcher) -> Self {
        self.watcher = Some(watcher);
        self
    }

    /// Comes forward when `activation` is set.
    pub fn with_activation(mut self, activation: Activation) -> Self {
        self.activation = Some(activation);
        self
    }

    /// Brings the window forward if a later launch asked. On Wayland,
    /// where an app can't raise itself, the request for attention is what
    /// shows (on GNOME, a "noido is ready" notice).
    fn come_forward(&self, ctx: &egui::Context) {
        if self.activation.as_ref().is_some_and(Activation::take) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

    /// Takes in the newest version of the document the watcher brought, if
    /// any. Called after the view has caught up with egui's scrolling, so
    /// the place it keeps is the current one.
    fn receive_reload(&mut self) {
        let Some(watcher) = &self.watcher else {
            return;
        };
        if let Some(reload) = std::iter::from_fn(|| watcher.try_recv()).last() {
            self.reload(reload);
        }
    }

    /// Shows `reload`, a new version of the document, at the same place.
    fn reload(&mut self, reload: Reload) {
        let current = self.view.state();
        let state = match (self.clamped, current) {
            (Some((wanted, left)), Some(current)) if same_place(&current, &left) => Some(wanted),
            _ => current,
        };
        let (width, height) = self.view.viewport_size();
        let mut view = View::new(reload.info.page_sizes);
        view.set_viewport_size(width, height);
        if let Some(state) = &state {
            // Clamps to the last page when pages were lost.
            view.restore(state);
        }
        let page_count = view.layout().page_count();
        self.clamped = match (state, view.state()) {
            (Some(wanted), Some(left)) if left.centre.page != wanted.centre.page => {
                Some((wanted, left))
            }
            _ => None,
        };
        self.view = view;
        self.tracker = RenderTracker::new(page_count);
        // Dropping the old renderer drops renders still on their way.
        self.renderer = reload.renderer;
        // The old images stay in the new layout until new renders replace
        // them, so the page doesn't flash white.
        self.textures.retain(|&page, _| page < page_count);
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.thumbnailer = reload.thumbnailer;
            thumbnails.sent = None;
            thumbnails.textures.retain(|&page, _| page < page_count);
        }
        self.focus = None;
        if let Some(persistence) = &mut self.persistence {
            persistence.store.rekey(&mut persistence.slot, reload.key);
        }
    }

    /// Restores `restored` and saves the document's state from now on.
    pub fn with_persistence(
        mut self,
        store: Store,
        slot: Slot,
        restored: Option<DocState>,
    ) -> Self {
        if let Some(state) = &restored {
            self.view.restore(&state.view);
            self.sidebar = state.sidebar;
        }
        self.persistence = Some(Persistence {
            store,
            slot,
            saved: restored,
            changed: None,
            warned: false,
        });
        self
    }

    /// What is remembered about the document now; `None` without pages.
    fn doc_state(&self) -> Option<DocState> {
        Some(DocState {
            view: self.view.state()?,
            sidebar: self.sidebar,
        })
    }

    /// Saves the document's state once it has settled.
    fn persist(&mut self, ctx: &egui::Context) {
        let (Some(state), Some(persistence)) = (self.doc_state(), &mut self.persistence) else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if let Some(left) = persistence.update(state, now) {
            ctx.request_repaint_after(Duration::from_secs_f64(left));
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
        let editing = ctx.text_edit_focused();
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
            if let Some(action) = self.keymap.action(press, editing) {
                self.apply(ctx, action);
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

    /// Draws the title bar and acts on what the reader did in it.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let status = match self.view.centre_page() {
            Some(page) => {
                let count = self.view.layout().page_count();
                titlebar::status_text(page, count, self.view.scale())
            }
            None => String::new(),
        };
        let bar = titlebar::Bar {
            name: &self.name,
            status,
            sidebar: self.sidebar,
            continuous: self.view.continuous(),
            dual: self.view.dual(),
            keymap: &self.keymap,
        };
        let focus = std::mem::take(&mut self.search_focus_requested);
        let output = egui::Panel::top("titlebar")
            .exact_size(TITLE_BAR_HEIGHT)
            .show(ui, |ui| titlebar::show(ui, &bar, &mut self.search, focus))
            .inner;
        self.bar = output.rects;
        if output.submitted && !self.sidebar.open {
            self.sidebar.open = true;
            self.sidebar_opened_by_search = true;
        }
        if output.cancelled && std::mem::take(&mut self.sidebar_opened_by_search) {
            self.sidebar.open = false;
        }
        if output.left {
            // The reader moved on; the sidebar is theirs now.
            self.sidebar_opened_by_search = false;
        }
        let ctx = ui.ctx().clone();
        for action in output.actions {
            self.apply(&ctx, action);
        }
    }

    /// Carries out `action`, from a key or the menu.
    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::ToggleSidebar => {
                self.sidebar.open = !self.sidebar.open;
                self.sidebar_opened_by_search = false;
            }
            Action::FocusSearch => self.search_focus_requested = true,
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => self.view.apply(action),
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
                    let name = format!("page-{}", rendered.page);
                    let size = self.view.layout().page_size(rendered.page);
                    let texture = PageImage::load(ctx, name, &image, size, rendered.scale);
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

        let visible = layout.visible_pages(viewport.min.y, viewport.max.y);
        let spread_count = layout.spread_count();
        // Pages of the spreads `first..=last`, so a spread is never half rendered.
        let spread_pages = |first: usize, last: usize| {
            layout.spread_pages(first).start..layout.spread_pages(last).end
        };
        let visible = if visible.is_empty() {
            visible
        } else {
            spread_pages(
                layout.spread_of(visible.start),
                layout.spread_of(visible.end - 1),
            )
        };

        // Visible pages first, then the window around the current spread,
        // nearest first and mostly ahead.
        let focus = layout.spread_of(self.focus.unwrap_or(visible.start));
        let mut order: Vec<usize> = visible.clone().collect();
        for spread in window_order(spread_count, focus, self.direction, PREFETCH_BUDGET) {
            order.extend(
                layout
                    .spread_pages(spread)
                    .filter(|page| !visible.contains(page)),
            );
        }
        let window = window_keep(spread_count, focus, KEEP);
        let keep = if window.is_empty() {
            visible.clone()
        } else {
            let window = spread_pages(window.start, window.end - 1);
            window.start.min(visible.start)..window.end.max(visible.end)
        };

        // Set before requesting: the render thread skips pages outside this
        // range, and the tracker forgets them below, so none stays pending.
        self.renderer.set_keep(keep.clone());
        // Pages requested earlier, while scrolling past, wait behind these.
        self.renderer.set_priority(order.clone());

        let display_scale = self.view.scale() * ui.ctx().pixels_per_point();
        if display_scale > 0.0 {
            for page in order {
                let wanted = self.tracker.wanted(page..page + 1, |page| {
                    let size = layout.page_size(page).expect("page in range");
                    render_scale(size, display_scale, MAX_RENDER_SIDE)
                });
                for (page, scale) in wanted {
                    self.renderer.request(RenderRequest { page, scale });
                }
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
            match self.page_image(page) {
                Some(image) => {
                    painter.image(image.texture.id(), rect, image.uv, Color32::WHITE);
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

    fn on_exit(&mut self) {
        if let (Some(state), Some(persistence)) = (self.doc_state(), &mut self.persistence)
            && persistence.saved != Some(state)
        {
            persistence.save(state);
        }
    }

    /// The window file holds only the window; egui's own memory isn't wanted.
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

impl NoidoApp {
    /// One frame of the viewer.
    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.come_forward(&ctx);
        self.receive_renders(&ctx);
        self.sync_from_egui();
        self.receive_reload();
        self.handle_input(&ctx);
        self.update_focus();
        self.update_thumbnails(&ctx);

        if !cfg!(target_os = "macos") {
            // Before the page view, so a press on an edge resizes rather than
            // starting a drag of the page.
            frame::resize_edges(&ctx);
        }
        self.title_bar(ui);
        if self.sidebar.open {
            sidebar::show(ui, &mut self.sidebar);
        }

        // The page area changes size this frame when the sidebar opens or
        // closes; lay the pages out for it now, not a frame late.
        let frame = egui::Frame::central_panel(ui.style());
        let area = ui.available_rect_before_wrap() - frame.total_margin();
        if self.last_scroll.is_some() {
            self.view.set_viewport_size(area.width(), area.height());
        }

        let (x, y) = self.view.offset();
        let wanted = vec2(x, y);
        let move_scroll = self
            .last_scroll
            .is_some_and(|last| (last.offset - wanted).abs().max_elem() > OFFSET_EPSILON);

        frame.show(ui, |ui| {
            // egui fades the edges of scroll areas; on pages it reads as a shadow.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            // Left drag pans the document (until selection tools need the button).
            let source = ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            };
            // No overscroll at the ends; it makes zoomed-in pages harder to
            // position. (It also keeps offsets noido sets from springing back.)
            ui.spacing_mut().scroll.kinetic.rubber_band = false;
            let mut scroll_area = ScrollArea::both().auto_shrink(false).scroll_source(source);
            if move_scroll {
                scroll_area = scroll_area.scroll_offset(wanted);
            }
            let output = scroll_area.show_viewport(ui, |ui, viewport| self.pages(ui, viewport));
            self.last_scroll = Some(ScrollState {
                inner: output.inner_rect,
                offset: output.state.clamped_offset(),
            });
        });
        if !cfg!(target_os = "macos") {
            frame::border(&ctx);
        }
        self.persist(&ctx);
    }
}

/// Whether the reader is still where `b` left them: same page and modes.
/// The spot on the page may differ by egui's rounding.
fn same_place(a: &ViewState, b: &ViewState) -> bool {
    a.centre.page == b.centre.page
        && a.zoom == b.zoom
        && a.scale == b.scale
        && a.continuous == b.continuous
        && a.dual == b.dual
}

/// A rendered page on the GPU, full resolution or thumbnail.
#[derive(Clone)]
struct PageImage {
    texture: TextureHandle,
    /// The part of the texture the page covers (see [`page_uv`]).
    uv: Rect,
}

impl PageImage {
    /// Uploads `image`, a render of a page of `size` at `scale`.
    fn load(
        ctx: &egui::Context,
        name: String,
        image: &Rgba8Image,
        size: Option<Size>,
        scale: f32,
    ) -> Self {
        let pixels = [image.width() as usize, image.height() as usize];
        let color_image = ColorImage::from_rgba_premultiplied(pixels, image.pixels());
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        Self {
            texture: ctx.load_texture(name, color_image, TextureOptions::LINEAR),
            uv: size.map_or(full, |size| page_uv(size, scale, image)),
        }
    }
}

/// The part of `image` that the page covers. Renders are rounded up to whole
/// pixels, so the last row and column are partly padding; stretching them
/// onto the page would squeeze the content towards the top-left, by up to a
/// pixel of the image (several screen pixels for a thumbnail).
fn page_uv(size: Size, scale: f32, image: &Rgba8Image) -> Rect {
    let covered = |length: f32, pixels: u32| (length * scale / pixels as f32).min(1.0);
    Rect::from_min_max(
        pos2(0.0, 0.0),
        pos2(
            covered(size.width, image.width()),
            covered(size.height, image.height()),
        ),
    )
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
        egui::Key::Q => Some(Key::Q),
        egui::Key::F9 => Some(Key::F9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::{Event, Modifiers, RawInput, pos2};
    use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

    use noido_core::{SidebarState, ViewState};
    use noido_render::Thumbnailer;
    use noido_state::{DocKey, DocState, Store};

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

    /// Like [`BlankEngine`], with 30 pages.
    struct BlankEngine30;
    struct BlankDocument30;

    impl PdfEngine for BlankEngine30 {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(BlankDocument30))
        }
    }

    impl Document for BlankDocument30 {
        fn page_count(&self) -> usize {
            30
        }
        fn page_size(&self, page: usize) -> Result<Size, EngineError> {
            BlankDocument.page_size(page)
        }
        fn render_tile(
            &self,
            page: usize,
            scale: f32,
            tile: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            BlankDocument.render_tile(page, scale, tile)
        }
    }

    /// Like [`BlankEngine`], but every render waits until the gate's sender
    /// is dropped.
    struct GatedEngine(Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>);
    struct GatedDocument(Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>);

    impl PdfEngine for GatedEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(GatedDocument(self.0.clone())))
        }
    }

    impl Document for GatedDocument {
        fn page_count(&self) -> usize {
            BlankDocument.page_count()
        }
        fn page_size(&self, page: usize) -> Result<Size, EngineError> {
            BlankDocument.page_size(page)
        }
        fn render_tile(
            &self,
            page: usize,
            scale: f32,
            tile: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            let _ = self.0.lock().unwrap().recv();
            BlankDocument.render_tile(page, scale, tile)
        }
    }

    /// Runs the app headlessly in egui, one frame per call.
    struct Harness {
        ctx: egui::Context,
        app: NoidoApp,
        size: Vec2,
        time: f64,
        /// What the last frame painted.
        shapes: Vec<egui::epaint::ClippedShape>,
        /// Everything the app asked of the window so far.
        commands: Vec<egui::ViewportCommand>,
        /// Whether the window is reported as maximised.
        maximized: bool,
        /// Hands the app new versions of the document, like the watcher.
        reloads: std::sync::mpsc::Sender<crate::watch::Reload>,
    }

    impl Harness {
        fn new() -> Self {
            Self::build(|app| app)
        }

        /// A harness whose app is first passed through `setup`.
        fn build(setup: impl FnOnce(NoidoApp) -> NoidoApp) -> Self {
            Self::build_with(Arc::new(BlankEngine), setup)
        }

        /// A harness whose full renders come from `engine`.
        fn build_with(
            engine: Arc<dyn PdfEngine>,
            setup: impl FnOnce(NoidoApp) -> NoidoApp,
        ) -> Self {
            let (renderer, info) =
                Renderer::start(engine, Arc::new(Vec::new()), Box::new(|| {})).unwrap();
            let ctx = egui::Context::default();
            // As in main: quitting goes through the keymap.
            ctx.options_mut(|o| o.quit_shortcuts.clear());
            let (reloads, watcher) = crate::watch::Watcher::fake();
            let mut harness = Self {
                ctx,
                app: setup(NoidoApp::new(renderer, info)).with_watcher(watcher),
                size: vec2(900.0, 1000.0),
                time: 0.0,
                shapes: vec![],
                commands: vec![],
                maximized: false,
                reloads,
            };
            harness.frames(3, vec![]);
            harness
        }

        fn frames(&mut self, count: usize, mut events: Vec<Event>) {
            for _ in 0..count {
                let mut viewports = egui::ViewportIdMap::default();
                viewports.insert(
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        maximized: Some(self.maximized),
                        ..Default::default()
                    },
                );
                let input = RawInput {
                    viewports,
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
                    time: Some(self.time),
                    predicted_dt: 1.0 / 60.0,
                    events: std::mem::take(&mut events),
                    ..Default::default()
                };
                self.time += 1.0 / 60.0;
                let app = &mut self.app;
                let output = self.ctx.run_ui(input, |ui| app.show(ui));
                self.shapes = output.shapes.clone();
                if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                    self.commands.extend(viewport.commands.iter().cloned());
                }
                output.drop_without_applying_deltas();
            }
        }

        /// Hands the app a new version of the document, opened with `engine`
        /// and identified by `key`, and runs a frame.
        fn reload(&mut self, engine: Arc<dyn PdfEngine>, key: DocKey) {
            let bytes = Arc::new(Vec::new());
            let (renderer, info) =
                Renderer::start(engine.clone(), bytes.clone(), Box::new(|| {})).unwrap();
            let thumbnailer = Thumbnailer::start(engine, bytes, Box::new(|| {}));
            let reload = crate::watch::Reload {
                renderer,
                thumbnailer,
                info,
                key,
            };
            self.reloads.send(reload).unwrap();
            self.frames(1, vec![]);
        }

        /// Runs frames until `done`, for at most a second.
        fn until(&mut self, done: impl Fn(&Harness) -> bool) {
            for _ in 0..200 {
                if done(self) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
                self.frames(1, vec![]);
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

        /// Presses the primary button at `at`, moves it by `by`, releases.
        fn drag(&mut self, at: egui::Pos2, by: Vec2) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frames(1, vec![Event::PointerMoved(at)]);
            self.frames(1, vec![button(at, true)]);
            for step in 1..=4 {
                let pos = at + by * (step as f32 / 4.0);
                self.frames(1, vec![Event::PointerMoved(pos)]);
            }
            self.frames(1, vec![button(at + by, false)]);
        }

        fn click(&mut self, at: egui::Pos2) {
            self.drag(at, Vec2::ZERO);
        }

        fn started_move(&self) -> bool {
            self.commands.contains(&egui::ViewportCommand::StartDrag)
        }

        /// Types `text` into the focused text field.
        fn typed(&mut self, text: &str) {
            self.frames(1, vec![Event::Text(text.into())]);
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

    fn test_key() -> DocKey {
        DocKey::new(b"test document", std::path::Path::new("/test.pdf"))
    }

    /// The test document's slot in a store at `dir`.
    fn test_slot(dir: &std::path::Path) -> noido_state::Slot {
        Store::at(dir).load(test_key()).0
    }

    #[test]
    fn restored_state_shows_saved_page() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = ViewState {
            zoom: noido_core::ZoomMode::FitWidth,
            scale: 1.0,
            continuous: true,
            dual: false,
            centre: noido_core::DocPoint {
                page: 6,
                x: 306.0,
                y: 400.0,
            },
        };
        let restored = DocState {
            view: state,
            sidebar: SidebarState::default(),
        };
        let h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        assert_eq!(h.centre().page, 6);
    }

    #[test]
    fn saves_after_idle_delay() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), None)
        });
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(60, vec![]);
        assert_eq!(store.load(test_key()).1, None, "saved before the delay");

        h.frames(90, vec![]);
        assert_eq!(
            store.load(test_key()).1.map(|state| state.view),
            h.app.view.state()
        );

        let file = dir
            .path()
            .join("docs")
            .join(format!("{}.toml", test_key().hex()));
        std::fs::remove_file(&file).unwrap();
        h.frames(180, vec![]);
        assert!(!file.exists(), "unchanged state was saved again");
    }

    #[test]
    fn sidebar_change_is_saved() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), None)
        });
        h.frames(150, vec![]);
        h.app.sidebar.open = true;
        h.frames(150, vec![]);
        assert!(store.load(test_key()).1.unwrap().sidebar.open);
    }

    #[test]
    fn f9_opens_sidebar_and_page_view_narrows() {
        let mut h = Harness::new();
        let (width, _) = h.app.view.viewport_size();
        let scale = h.app.view.scale();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        assert!(h.app.sidebar.open);
        let (narrower, _) = h.app.view.viewport_size();
        let sidebar = h.app.sidebar.width;
        assert!(
            (width - narrower - sidebar).abs() < 20.0,
            "{width} - {narrower} vs sidebar {sidebar}"
        );
        assert!(h.app.view.scale() < scale);

        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        assert!(!h.app.sidebar.open);
        assert_eq!(h.app.view.viewport_size().0, width);
        assert_eq!(h.app.view.scale(), scale);
    }

    /// Whether the last frame painted a shape covering `rect` (within 1 point).
    fn painted(h: &Harness, rect: Rect) -> bool {
        h.shapes.iter().any(|clipped| {
            let shape = clipped.shape.visual_bounding_rect();
            (shape.min - rect.min).length() < 1.0 && (shape.max - rect.max).length() < 1.0
        })
    }

    #[test]
    fn toggling_sidebar_draws_pages_in_place_at_once() {
        sidebar_toggle_draws_in_place(Harness::new());
    }

    /// Zoomed out, the page keeps its size but is centred in a narrower area.
    #[test]
    fn toggling_sidebar_recentres_unchanged_pages_at_once() {
        let mut h = Harness::new();
        for _ in 0..3 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        sidebar_toggle_draws_in_place(h);
    }

    fn sidebar_toggle_draws_in_place(mut h: Harness) {
        // Where the first page settles with the sidebar open, then closed.
        let page_rect = |h: &mut Harness| {
            h.frames(5, vec![]);
            h.shapes
                .iter()
                .map(|clipped| clipped.shape.visual_bounding_rect())
                .filter(|rect| rect.width() > 300.0 && rect.height() > 300.0)
                .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
                .unwrap()
        };
        h.press(egui::Key::F9, Modifiers::NONE);
        let open = page_rect(&mut h);
        // The measured page area is what the scroll area then reports, so
        // the two never disagree from frame to frame.
        let inner = h.app.last_scroll.unwrap().inner;
        assert_eq!(h.app.view.viewport_size(), (inner.width(), inner.height()));
        h.press(egui::Key::F9, Modifiers::NONE);
        let closed = page_rect(&mut h);
        assert_ne!(open, closed);

        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(painted(&h, open), "opening: page not at {open:?}");
        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(painted(&h, closed), "closing: page not at {closed:?}");
    }

    #[test]
    fn restored_sidebar_width_is_clamped() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState {
                open: true,
                width: 2000.0,
            },
        };
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(state))
        });
        h.frames(3, vec![]);
        assert!(h.app.sidebar.width <= 450.0, "{}", h.app.sidebar.width);
        assert!(h.app.view.viewport_size().0 >= 400.0);
    }

    #[test]
    fn mod_q_sends_close() {
        let mut h = Harness::new();
        h.press(egui::Key::Q, Modifiers::COMMAND);
        assert!(h.commands.contains(&egui::ViewportCommand::Close));
    }

    /// Puts the cursor in the search field.
    fn focus_search(h: &mut Harness) {
        h.press(egui::Key::F, Modifiers::COMMAND);
        h.frames(2, vec![]);
        assert!(h.ctx.text_edit_focused(), "search field not focused");
    }

    #[test]
    fn typing_w_in_search_keeps_zoom() {
        let mut h = Harness::new();
        let mode = h.app.view.mode();
        focus_search(&mut h);
        h.frames(
            1,
            vec![
                Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
                Event::Text("w".into()),
            ],
        );
        assert_eq!(h.app.view.mode(), mode);
        assert_eq!(h.app.search, "w");

        let scale = h.app.view.scale();
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        h.frames(2, vec![]);
        assert!(h.app.view.scale() > scale);
        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn enter_opens_sidebar_and_esc_closes_it() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.typed("pond");
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.sidebar.open);
        assert!(!h.ctx.text_edit_focused());
    }

    #[test]
    fn esc_keeps_sidebar_opened_by_toggle() {
        let mut h = Harness::new();
        h.press(egui::Key::F9, Modifiers::NONE);
        focus_search(&mut h);
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn search_field_keeps_min_width() {
        let mut h = Harness::new();
        h.size = vec2(480.0, 600.0);
        h.frames(3, vec![]);
        assert!(h.app.bar.search.width() >= 160.0, "{:?}", h.app.bar.search);
        h.app.sidebar = SidebarState {
            open: true,
            width: 160.0,
        };
        h.frames(3, vec![]);
        assert!(h.app.bar.search.width() >= 160.0, "{:?}", h.app.bar.search);
    }

    #[test]
    fn long_name_keeps_menu_on_screen() {
        let name = "a very long file name ".repeat(10);
        let mut h = Harness::build(|app| app.with_name(name));
        h.size = vec2(480.0, 600.0);
        h.frames(3, vec![]);
        let menu = h.app.bar.menu;
        assert!(menu.min.x >= 0.0 && menu.max.x <= 480.0, "{menu:?}");
    }

    #[test]
    fn menu_dual_matches_key() {
        let mut h = Harness::new();
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Dual page")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, entry.action);
        assert!(h.app.view.dual());
    }

    #[test]
    fn status_text_counts_from_one() {
        assert_eq!(crate::titlebar::status_text(2, 10, 1.06), "3 / 10 · 106 %");
    }

    /// A point on the title bar with no control under it.
    fn empty_bar_point(h: &Harness) -> egui::Pos2 {
        pos2(h.app.bar.search.right() + 20.0, h.app.bar.search.center().y)
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn close_button_closes() {
        let mut h = Harness::new();
        h.click(h.app.bar.close.center());
        assert!(h.commands.contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn double_click_empty_bar_maximises() {
        let mut h = Harness::new();
        let at = empty_bar_point(&h);
        h.click(at);
        h.click(at);
        assert!(
            h.commands.contains(&egui::ViewportCommand::Maximized(true)),
            "{:?}",
            h.commands
        );
    }

    #[test]
    fn drag_on_empty_bar_starts_move() {
        let mut h = Harness::new();
        h.drag(empty_bar_point(&h), vec2(30.0, 10.0));
        assert!(h.started_move());
    }

    #[test]
    fn drag_on_bar_buttons_does_not_move_window() {
        let mut h = Harness::new();
        let toggle = pos2(h.app.bar.search.left() - 20.0, h.app.bar.search.center().y);
        let mut buttons = vec![toggle, h.app.bar.menu.center()];
        if !cfg!(target_os = "macos") {
            buttons.push(h.app.bar.close.center());
        }
        for at in buttons {
            h.drag(at, vec2(30.0, 10.0));
            assert!(!h.started_move(), "dragging from {at:?} moved the window");
        }
    }

    #[test]
    fn esc_keeps_sidebar_reopened_by_toggle() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn esc_keeps_sidebar_after_leaving_search() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.click(pos2(600.0, 500.0));
        assert!(!h.ctx.text_edit_focused());
        focus_search(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn drag_on_search_field_does_not_move_window() {
        let mut h = Harness::new();
        h.drag(h.app.bar.search.center(), vec2(30.0, 10.0));
        assert!(!h.started_move());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn edge_press_begins_resize() {
        let mut h = Harness::new();
        h.click(pos2(1.0, 500.0));
        assert!(
            h.commands.contains(&egui::ViewportCommand::BeginResize(
                egui::ResizeDirection::West
            )),
            "{:?}",
            h.commands
        );
    }

    #[test]
    fn no_resize_band_when_maximised() {
        let mut h = Harness::new();
        h.maximized = true;
        h.click(pos2(1.0, 500.0));
        assert!(
            !h.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::BeginResize(_))),
            "{:?}",
            h.commands
        );
    }

    fn blank_texture(ctx: &egui::Context, name: &str) -> PageImage {
        let image = Rgba8Image::from_premultiplied(1, 1, vec![255; 4]).unwrap();
        PageImage::load(ctx, name.into(), &image, None, 1.0)
    }

    #[test]
    fn page_image_prefers_full_render() {
        let mut h = Harness::build(|app| {
            let thumbnailer =
                Thumbnailer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        let full = blank_texture(&h.ctx, "full");
        let thumbnail = blank_texture(&h.ctx, "thumbnail");
        // No frames run below, so nothing new arrives after clearing.
        h.app.textures.clear();
        let thumbnails = &mut h.app.thumbnails.as_mut().unwrap().textures;
        thumbnails.clear();
        thumbnails.insert(3, thumbnail.clone());
        assert_eq!(
            h.app.page_image(3).map(|i| i.texture.id()),
            Some(thumbnail.texture.id())
        );
        h.app.textures.insert(3, full.clone());
        assert_eq!(
            h.app.page_image(3).map(|i| i.texture.id()),
            Some(full.texture.id())
        );
        assert!(h.app.page_image(4).is_none());
    }

    #[test]
    fn jump_shows_thumbnail_before_full_render() {
        let (gate, gate_rx) = std::sync::mpsc::channel::<()>();
        let engine = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(gate_rx))));
        let mut h = Harness::build_with(engine, |app| {
            let thumbnailer =
                Thumbnailer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        for _ in 0..10 {
            h.press(egui::Key::PageDown, Modifiers::NONE);
        }
        let page = h.app.view.centre_page().unwrap();
        assert!(page > 3, "page {page}");
        for _ in 0..200 {
            if h.app
                .thumbnails
                .as_ref()
                .unwrap()
                .textures
                .contains_key(&page)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.frames(1, vec![]);
        }
        let thumbnails = &h.app.thumbnails.as_ref().unwrap().textures;
        let thumbnail = thumbnails
            .get(&page)
            .expect("thumbnail arrived")
            .texture
            .id();
        assert!(!h.app.textures.contains_key(&page));
        assert_eq!(
            h.app.page_image(page).map(|i| i.texture.id()),
            Some(thumbnail)
        );
        drop(gate);
    }

    #[test]
    fn page_uv_covers_only_the_page() {
        // Letter 120 px wide is 155.3 px tall, rendered into 156 rows; the
        // last 0.7 row is padding and must not be stretched onto the page.
        let letter = Size {
            width: 612.0,
            height: 792.0,
        };
        let scale = 120.0 / 612.0;
        let image = Rgba8Image::from_premultiplied(120, 156, vec![255; 120 * 156 * 4]).unwrap();
        let uv = page_uv(letter, scale, &image);
        assert_eq!(uv.min, pos2(0.0, 0.0));
        assert_eq!(uv.max.x, 1.0);
        assert!((uv.max.y - 792.0 * scale / 156.0).abs() < 1e-6, "{uv:?}");
    }

    #[test]
    fn renders_the_window_ahead_and_keeps_within_reach() {
        // 30 pages, so the window doesn't reach both ends.
        let mut h = Harness::build_with(Arc::new(BlankEngine30), |app| app);
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::F, Modifiers::NONE);
        h.frames(2, vec![]);
        for _ in 0..12 {
            h.press(egui::Key::PageDown, Modifiers::NONE);
        }
        assert_eq!(h.app.view.current_spread(), Some(12));
        let window = window_order(30, 12, Direction::Forward, PREFETCH_BUDGET);
        let ready = |h: &Harness| window.iter().all(|page| h.app.textures.contains_key(page));
        for _ in 0..400 {
            if ready(&h) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.frames(1, vec![]);
        }
        let mut pages: Vec<_> = h.app.textures.keys().copied().collect();
        pages.sort();
        assert!(ready(&h), "window {window:?}, textures {pages:?}");
        let keep = window_keep(30, 12, KEEP);
        assert!(
            pages.iter().all(|page| keep.contains(page)),
            "kept beyond {keep:?}: {pages:?}"
        );
        assert!(
            !pages.contains(&(12 + 7)),
            "rendered beyond the window: {pages:?}"
        );
    }

    #[test]
    fn pulling_past_the_top_does_not_overscroll() {
        let mut h = Harness::new();
        h.frames(5, vec![]);
        // The top of the first page as painted: the highest page-wide shape.
        let page_width = h.app.view.layout().page_rect(0).unwrap().width;
        let first_page_top = |h: &Harness| {
            h.shapes
                .iter()
                .map(|clipped| clipped.shape.visual_bounding_rect())
                .filter(|rect| (rect.width() - page_width).abs() < 1.0)
                .map(|rect| rect.min.y)
                .fold(f32::INFINITY, f32::min)
        };
        let resting = first_page_top(&h);
        assert!(resting.is_finite());

        let at = |y: f32| Event::PointerMoved(pos2(450.0, y));
        let button = |y: f32, pressed: bool| Event::PointerButton {
            pos: pos2(450.0, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![at(300.0)]);
        h.frames(1, vec![button(300.0, true)]);
        for step in 1..=10 {
            h.frames(1, vec![at(300.0 + 20.0 * step as f32)]);
        }
        // Still holding the button: rubber-banding would show here.
        let pulled = first_page_top(&h);
        h.frames(1, vec![button(500.0, false)]);
        assert!(
            (pulled - resting).abs() < 0.5,
            "page top {resting} -> {pulled}"
        );
    }

    /// `self.0` pages of `self.1`, rendering as a white pixel.
    #[derive(Clone, Copy)]
    struct SizedEngine(usize, Size);

    impl SizedEngine {
        /// `pages` Letter pages.
        fn letter(pages: usize) -> Self {
            Self(
                pages,
                Size {
                    width: 612.0,
                    height: 792.0,
                },
            )
        }
    }

    impl PdfEngine for SizedEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(*self))
        }
    }

    impl Document for SizedEngine {
        fn page_count(&self) -> usize {
            self.0
        }
        fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
            Ok(self.1)
        }
        fn render_tile(
            &self,
            page: usize,
            scale: f32,
            tile: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            BlankDocument.render_tile(page, scale, tile)
        }
    }

    fn recompiled_key() -> DocKey {
        DocKey::new(
            b"test document, recompiled",
            std::path::Path::new("/test.pdf"),
        )
    }

    /// Where the last frame drew page images (not white placeholders).
    fn images(h: &Harness) -> Vec<Rect> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some(clipped.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .collect()
    }

    /// Presses Page Down until the centre is on `page` or later.
    fn go_to(h: &mut Harness, page: usize) {
        for _ in 0..100 {
            if h.centre().page >= page {
                return;
            }
            h.press(egui::Key::PageDown, Modifiers::NONE);
        }
        panic!("never reached page {page}");
    }

    #[test]
    fn reload_keeps_page_and_spot() {
        let mut h = Harness::new();
        go_to(&mut h, 6);
        let centre = h.centre();
        // A4 now: the spot is in page points, so it stays on the page.
        let a4 = Size {
            width: 595.0,
            height: 842.0,
        };
        h.reload(Arc::new(SizedEngine(12, a4)), recompiled_key());
        h.frames(3, vec![]);
        assert_eq!(h.app.view.layout().page_count(), 12);
        assert_eq!(h.app.view.layout().page_size(0), Some(a4));
        // Fit width centres the narrower page, so only the height on the
        // page is kept exactly.
        assert_eq!(h.centre().page, centre.page);
        assert!(
            (h.centre().y - centre.y).abs() < 0.5,
            "moved from {centre:?} to {:?}",
            h.centre()
        );
    }

    #[test]
    fn reload_clamps_to_last_page() {
        let mut h = Harness::new();
        go_to(&mut h, 8);
        h.reload(Arc::new(SizedEngine::letter(4)), recompiled_key());
        h.frames(3, vec![]);
        assert_eq!(h.centre().page, 3);
    }

    #[test]
    fn reload_keeps_modes_and_sidebar() {
        let mut h = Harness::new();
        h.press(egui::Key::D, Modifiers::NONE);
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        let before = h.app.view.state().unwrap();
        h.reload(Arc::new(SizedEngine::letter(12)), recompiled_key());
        h.frames(3, vec![]);
        assert_eq!(h.app.view.layout().page_count(), 12);
        let after = h.app.view.state().unwrap();
        assert_eq!(after.zoom, before.zoom);
        assert_eq!(after.scale, before.scale);
        assert_eq!(after.continuous, before.continuous);
        assert_eq!(after.dual, before.dual);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn reload_keeps_old_images_until_new_ones_arrive() {
        let mut h = Harness::new();
        h.until(|h| !images(h).is_empty());
        let before = images(&h);
        assert!(!before.is_empty(), "no page rendered");
        // Renders of the new version wait until the gate goes.
        let (gate, gated) = std::sync::mpsc::channel::<()>();
        let engine = GatedEngine(Arc::new(std::sync::Mutex::new(gated)));
        h.reload(Arc::new(engine), recompiled_key());
        assert_eq!(images(&h), before);
        drop(gate);
    }

    #[test]
    fn reload_drops_images_past_new_count() {
        let mut h = Harness::new();
        go_to(&mut h, 8);
        h.until(|h| h.app.textures.keys().any(|&page| page >= 4));
        assert!(h.app.textures.keys().any(|&page| page >= 4));
        h.reload(Arc::new(SizedEngine::letter(4)), recompiled_key());
        assert!(
            h.app.textures.keys().all(|&page| page < 4),
            "textures: {:?}",
            h.app.textures.keys()
        );
    }

    #[test]
    fn reload_saves_under_new_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), None)
        });
        h.frames(150, vec![]);
        assert!(store.load(test_key()).1.is_some(), "not saved before");

        h.reload(Arc::new(SizedEngine::letter(12)), recompiled_key());
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(150, vec![]);
        let state_file = |key: DocKey| dir.path().join("docs").join(format!("{}.toml", key.hex()));
        assert!(
            state_file(recompiled_key()).exists(),
            "not saved under the new key"
        );
        assert!(!state_file(test_key()).exists(), "old state left behind");
        let (slot, saved) = store.load(recompiled_key());
        assert_eq!(saved.map(|state| state.view), h.app.view.state());
        assert!(
            store
                .window_file(&slot)
                .ends_with(format!("{}.window.ron", test_key().hex())),
            "window file moved"
        );
    }

    #[test]
    fn activation_raises_window() {
        let activation = crate::instance::Activation::default();
        let mut h = Harness::build(|app| app.with_activation(activation.clone()));
        h.frames(1, vec![]);
        assert!(!h.commands.contains(&egui::ViewportCommand::Focus));
        activation.request();
        h.frames(1, vec![]);
        assert!(
            h.commands
                .contains(&egui::ViewportCommand::Minimized(false))
        );
        assert!(h.commands.contains(&egui::ViewportCommand::Focus));
        assert!(
            h.commands
                .contains(&egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational
                ))
        );
        assert!(!activation.take(), "left set");
    }

    /// A version cut short mid-write must not cost the reader their place.
    #[test]
    fn reload_after_short_version_returns_to_place() {
        let mut h = Harness::new();
        go_to(&mut h, 8);
        let centre = h.centre();
        h.reload(Arc::new(SizedEngine::letter(4)), recompiled_key());
        h.frames(3, vec![]);
        assert_eq!(h.centre().page, 3);
        h.reload(Arc::new(SizedEngine::letter(10)), test_key());
        h.frames(3, vec![]);
        assert_eq!(h.centre().page, centre.page);
        assert!((h.centre().y - centre.y).abs() < 0.5, "{:?}", h.centre());
    }

    #[test]
    fn moving_after_short_version_keeps_new_place() {
        let mut h = Harness::new();
        go_to(&mut h, 8);
        h.reload(Arc::new(SizedEngine::letter(4)), recompiled_key());
        h.press(egui::Key::PageUp, Modifiers::NONE);
        h.press(egui::Key::PageUp, Modifiers::NONE);
        h.frames(3, vec![]);
        let moved = h.centre().page;
        assert!(moved < 3);
        h.reload(Arc::new(SizedEngine::letter(10)), test_key());
        h.frames(3, vec![]);
        assert_eq!(h.centre().page, moved);
    }
}
