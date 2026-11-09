use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::scroll_area::{DragScroll, ScrollSource};
use eframe::egui::{self, Color32, Rect, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};
use noido_core::{
    Action, Defaults, Direction, DragKind, Highlight, HighlightColour, Highlights, History, Keep,
    Key, KeyPress, Keymap, MAX_RENDER_SIDE, PageRect, PageTextState, Pins, RenderTracker,
    SidebarState, THUMBNAIL_BUDGET, Target, View, ViewState, drag_kind, fit_page, page_point,
    render_scale, window_keep, window_order,
};
use noido_render::{DocumentInfo, RenderRequest, Renderer, TextSearcher, Thumbnailer};
use noido_settings::{Settings, SettingsFile};
use noido_state::{DocState, Pagelet, Slot, Store};

use crate::frame;
use crate::image::{Fades, PageImage, paint_page};
use crate::info;
use crate::instance::Activation;
use crate::links::{FLASH_SECONDS, Links, Named};
use crate::open;
use crate::search::{self, Search};
use crate::select;
use crate::settings;
use crate::shrink::{self, Shrink};
use crate::sidebar::{self, SidebarView};
use crate::texts::Texts;
use crate::titlebar::{self, TITLE_BAR_HEIGHT};
use crate::watch::{Reload, Watcher};

/// Spreads (pages in single mode) rendered at full resolution around the
/// current one, after the visible ones: the current spread, then six ahead
/// and three behind in the direction of travel (see [`window_order`]).
const PREFETCH_BUDGET: usize = 10;
/// Spreads on each side of the current one whose full renders are kept; more
/// than the window reaches ahead, so moving back and forth doesn't re-render.
const KEEP: usize = 8;
/// How many places on each side of the history are kept rendered: the next
/// Back and the next Forward target.
const HISTORY_KEEP: usize = 1;
/// Trackpad scroll distance, in UI points, that flips one page in paged mode
/// (about one mouse wheel step).
const TRACKPAD_FLIP_DISTANCE: f32 = 125.0;
/// Offset difference, in UI points, below which egui's scroll position is left alone.
const OFFSET_EPSILON: f32 = 0.5;

/// Seconds the view must stay unchanged before its state is saved.
const SAVE_DELAY: f64 = 2.0;
/// The outline around the link under the pointer, in points.
const LINK_OUTLINE_WIDTH: f32 = 1.5;
/// The outline flashed at a link's target, in points.
const FLASH_OUTLINE_WIDTH: f32 = 2.5;
/// How strongly selected text is tinted with the selection colour.
const SELECTION_OPACITY: f32 = 0.35;
/// The gap between the selection's end and its bar, in points.
const SELECTION_BAR_GAP: f32 = 8.0;
/// How tall the selection's bar is laid out to be, with its frame's margins;
/// it measures a little more.
const SELECTION_BAR_HEIGHT: f32 = 48.0;
/// The bar's size before it has been drawn and measured.
const SELECTION_BAR_ESTIMATE: Vec2 = vec2(355.0, 50.0);
/// Space inside the pagelet's frame, in points.
const PAGELET_PADDING: f32 = 6.0;
/// The pagelet's width and height as fractions of the page view when it
/// first opens.
const PAGELET_WIDTH_SHARE: f32 = 0.8;
const PAGELET_HEIGHT_SHARE: f32 = 0.6;
/// The smallest the pagelet gets.
const PAGELET_MIN: Vec2 = vec2(160.0, 200.0);
/// Side of the pagelet's close chip, in points.
const PAGELET_CLOSE: f32 = 20.0;
/// Side of the pagelet's resize grip, in points.
const PAGELET_GRIP: f32 = 16.0;
/// How long the pagelet takes to fade to half, in seconds.
const PAGELET_FADE: f32 = 0.12;

/// The pagelet: a pinned page floating over the pages.
struct PinPreview {
    page: usize,
    /// The panel's rect, in screen points.
    rect: Rect,
    /// The page's rect inside the panel, as last drawn.
    page_rect: Rect,
    /// The page's size: the panel keeps the page's aspect.
    page_size: Vec2,
    /// How opaque it is drawn: 1 on the pointer, 0.5 off it.
    opacity: f32,
    /// Whether a resize handle is being dragged.
    resizing: bool,
}

/// What egui's scroll area reported at the end of the last frame.
#[derive(Clone, Copy)]
struct ScrollState {
    /// The scroll area's id.
    id: egui::Id,
    /// The visible area, in screen coordinates.
    inner: Rect,
    offset: Vec2,
}

/// Documents that arrive while the app runs, from outside its windows (on
/// macOS, Finder's opens, including those that started the app), for the
/// shell to open. Tests use fakes.
pub struct Incoming {
    /// Drains what has arrived: links or paths, in arrival order.
    pub take: Box<dyn FnMut() -> Vec<std::ffi::OsString> + Send>,
}

pub struct NoidoApp {
    renderer: Renderer,
    view: View,
    tracker: RenderTracker,
    textures: HashMap<usize, PageImage>,
    /// Full renders fading in over their thumbnails.
    fades: Fades,
    keymap: Keymap,
    last_scroll: Option<ScrollState>,
    /// When egui last moved the pages itself (wheel, flick, drag, bar),
    /// in egui's time, rather than noido setting where they are.
    pages_scrolled_at: Option<f64>,
    /// Trackpad distance scrolled towards the next flip, positive downwards.
    flip_distance: f32,
    persistence: Option<Persistence>,
    /// Where Back and Forward go.
    history: History,
    /// The document's highlights.
    highlights: Highlights,
    /// The pages the reader pinned.
    pins: Pins,
    /// The pagelet, while it is open.
    pin_preview: Option<PinPreview>,
    /// Where the preview pagelet was, kept while it is closed.
    pagelet: Option<Pagelet>,
    /// A pagelet to open once the view has a rect, restored with the
    /// document.
    pending_pagelet: Option<usize>,
    /// A link clicked in the pagelet, followed at the start of the next
    /// frame so the view's scroll takes the jump.
    clicked_target: Option<noido_engine::LinkTarget>,
    /// A pin click waiting out egui's double-click window.
    pending_pin: Option<(usize, f64)>,
    /// A hit was jumped to since the reader last moved: later hits, of this
    /// search or a new one, add no history entry.
    searched_away: bool,
    thumbnails: Option<Thumbnails>,
    /// The page at the window centre.
    focus: Option<usize>,
    /// The direction the focus last moved in.
    direction: Direction,
    sidebar: SidebarState,
    /// The sidebar's thumbnails.
    sidebar_view: SidebarView,
    /// A page clicked in the sidebar: it stays the current page until the
    /// reader moves the page view, since the page at the centre may be
    /// another one (short pages, the last pages).
    chosen: Option<usize>,
    /// The outline entry clicked, while `chosen` stands.
    clicked_entry: Option<usize>,
    /// The page at the middle of the sidebar after the reader scrolled it;
    /// the low-res thumbnails follow it until the current page changes.
    browse: Option<usize>,
    /// The scroll offset noido gave egui last frame, and the page view's
    /// size the frame before: a different offset at the same size means
    /// the reader scrolled.
    told: Option<(Vec2, Vec2)>,
    /// The document's file name, shown in the title bar.
    name: String,
    /// The search field's text.
    search: String,
    /// Mod+F was pressed: the search field takes focus next frame.
    search_focus_requested: bool,
    /// Enter in the search field opened the sidebar, so Esc closes it again.
    sidebar_opened_by_search: bool,
    /// The text thread, and the current query's hits.
    search_state: Search,
    /// Where the title bar's parts were drawn last frame.
    bar: titlebar::Rects,
    /// Brings new versions of the document when its file changes.
    watcher: Option<Watcher>,
    /// Re-reads the OS theme while the app runs.
    theme_live: Option<crate::theme::Live>,
    /// Set when a later launch on the same file asks the window forward.
    activation: Option<Activation>,
    /// The document's canonical path, for links to it.
    link_path: Option<std::path::PathBuf>,
    /// Where a link asked to go, and whether to leave the place shown for
    /// Back first; taken at the start of the next frame.
    arrival: Option<(Target, bool)>,
    /// Whether to leave the place shown for Back when the named place
    /// asked for is found.
    named_leaves: bool,
    /// Where the reader was before a reload to fewer pages moved them, and
    /// where that left them: a version cut short mid-write mustn't cost
    /// them their place, so the next reload returns there unless they moved.
    clamped: Option<(ViewState, ViewState)>,
    settings: Settings,
    /// Where settings changes are saved; `None` without a config directory.
    settings_file: Option<SettingsFile>,
    /// The settings page, while it is open.
    settings_page: Option<settings::Page>,
    /// The document's file, for the info page; `None` in tests without one.
    source: Option<info::Source>,
    /// The document info page, while it is open.
    info_page: Option<info::Page>,
    /// Actions asked of the window from outside it (the native menu),
    /// carried out in its next frame.
    queued: Vec<Action>,
    /// The pages' links.
    links: Links,
    /// The pages' text, for selecting.
    texts: Texts,
    /// The text selection and the select tool.
    select: select::State,
    /// The link under the pointer this frame.
    hovered_link: Option<noido_engine::Link>,
    /// The link clicked last frame, followed at the start of the next:
    /// the view takes egui's offset first.
    clicked_link: Option<noido_engine::Link>,
    /// Picks a PDF when asked, in the desktop's own dialog, handing it to
    /// [`NoidoApp::open`] with the window as the dialog's parent and the
    /// open document's directory as the dialog's starting point: the real
    /// dialog never blocks, so the window keeps painting while it is up.
    /// Fakes in tests.
    pick: open::Pick,
    /// Opens a picked document in a new window; `Arc` so the picker's
    /// thread can call it. Fakes in tests.
    open: open::OnPicked,
    /// The old view's edge after PageUp/PageDown and when its flash
    /// started: `None` while the key is still held.
    screen_edge: Option<(noido_core::DocPoint, Option<f64>)>,
    /// Whether PageUp or PageDown repeated while held: no flash then.
    page_key_repeated: bool,
    /// Where to ask the shell to open and quit, when there is one.
    requests: Option<crate::shell::Requests>,
    /// The recently opened files, newest first, as the menu offers them.
    recent: Vec<PathBuf>,
    /// A window size asked for by fitting the window to the page, and the
    /// corrections left, until the window has it.
    shrink: Option<Shrink>,
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
        if self.saved.as_ref() == Some(&state) {
            self.changed = None;
            return None;
        }
        let since = match &self.changed {
            Some((changed, since)) if *changed == state => *since,
            _ => {
                self.changed = Some((state.clone(), now));
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
            fades: Fades::default(),
            keymap: Keymap::native(),
            last_scroll: None,
            pages_scrolled_at: None,
            flip_distance: 0.0,
            persistence: None,
            history: History::default(),
            highlights: Highlights::default(),
            pins: Pins::default(),
            pagelet: None,
            pending_pagelet: None,
            clicked_target: None,
            pin_preview: None,
            pending_pin: None,
            searched_away: false,
            thumbnails: None,
            focus: None,
            direction: Direction::Forward,
            sidebar: SidebarState::default(),
            sidebar_view: SidebarView::new(page_count),
            chosen: None,
            clicked_entry: None,
            browse: None,
            told: None,
            search_focus_requested: false,
            name: String::new(),
            search: String::new(),
            sidebar_opened_by_search: false,
            search_state: Search::new(),
            bar: titlebar::Rects::default(),
            watcher: None,
            theme_live: None,
            activation: None,
            link_path: None,
            arrival: None,
            named_leaves: false,
            clamped: None,
            settings: Settings::default(),
            settings_file: None,
            settings_page: None,
            source: None,
            info_page: None,
            queued: Vec::new(),
            links: Links::new(page_count),
            texts: Texts::new(page_count),
            select: select::State::default(),
            hovered_link: None,
            clicked_link: None,
            pick: Box::new(open::pick_pdf),
            requests: None,
            open: Arc::new(open::open),
            screen_edge: None,
            page_key_repeated: false,
            recent: Vec::new(),
            shrink: None,
        }
    }

    /// Shows `name` in the title bar.
    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    /// The document's file name, as the title bar shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Arrives at `target` once the window shows: a link's page or place.
    /// Back returns to the remembered place, if there is one: call after
    /// [`NoidoApp::with_persistence`].
    pub fn opening_at(mut self, target: Option<Target>) -> Self {
        let remembered = self.persistence.as_ref().is_some_and(|p| p.saved.is_some());
        self.arrival = target.map(|target| (target, remembered));
        self
    }

    /// Links copied from the window point at `canonical`.
    pub fn with_link_path(mut self, canonical: std::path::PathBuf) -> Self {
        self.link_path = Some(canonical);
        self
    }

    /// Reads the document's info from `source` when asked.
    pub fn with_source(mut self, source: info::Source) -> Self {
        self.source = Some(source);
        self
    }

    /// Renders the sidebar's visible thumbnails sharp with `renderer`.
    pub fn with_sidebar_renderer(mut self, renderer: Renderer) -> Self {
        self.sidebar_view = self.sidebar_view.with_renderer(renderer);
        self
    }

    /// Searches the document's text with `searcher`.
    pub fn with_searcher(mut self, searcher: TextSearcher) -> Self {
        self.search_state.set_searcher(searcher);
        self
    }

    /// Reads the pages' links with `linker`.
    pub fn with_linker(mut self, linker: noido_render::Linker) -> Self {
        let page_count = self.view.layout().page_count();
        self.links.set_linker(linker, page_count);
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
        let Some(page) = self.current_page() else {
            return;
        };
        if let Some(old) = self.focus
            && old != page
        {
            self.browse = None;
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
        if let Some(page) = self.browse.or(self.focus)
            && thumbnails.sent != Some(page)
        {
            thumbnails.sent = Some(page);
            thumbnails.thumbnailer.set_focus(page, self.direction);
            let keep = window_keep(page_count, page, THUMBNAIL_BUDGET);
            thumbnails.textures.retain(|page, _| keep.contains(page));
        }
        let keep = window_keep(page_count, thumbnails.sent.unwrap_or(0), THUMBNAIL_BUDGET);
        while let Some(thumbnail) = thumbnails.thumbnailer.try_recv() {
            match thumbnail.result {
                Ok(image) if keep.contains(&thumbnail.page) => {
                    let name = format!("thumbnail-{}", thumbnail.page);
                    let image = crate::image::soften(&image);
                    let texture = PageImage::load(ctx, name, &image);
                    thumbnails.textures.insert(thumbnail.page, texture);
                }
                Ok(_) => {}
                // A failed thumbnail leaves the page white, not showing an
                // earlier version; its full render reports errors.
                Err(_) => {
                    thumbnails.textures.remove(&thumbnail.page);
                }
            }
        }
    }

    /// Makes `page` the current page, as after a jump: it stands until the
    /// reader moves the page view. Any outline entry clicked is forgotten.
    fn choose_page(&mut self, page: usize) {
        self.chosen = Some(page);
        self.clicked_entry = None;
    }

    /// The outline entry the reader is in: the one just clicked while its
    /// page stands, else the one at the reader's position.
    fn current_entry(&self) -> Option<usize> {
        let tree = self.links.outline.as_ref()?;
        if self.chosen.is_some()
            && let Some(entry) = self.clicked_entry
        {
            return Some(entry);
        }
        tree.current(self.view.reading_point(self.chosen)?)
    }

    /// The page the reader is at: the one clicked in the sidebar, else the
    /// page at the centre of the page view.
    fn current_page(&self) -> Option<usize> {
        self.chosen.or_else(|| self.view.centre_page())
    }

    /// The pages shown as current in the sidebar: the current page, or in
    /// dual mode its spread.
    fn highlighted(&self) -> std::ops::Range<usize> {
        let Some(page) = self.current_page() else {
            return 0..0;
        };
        let layout = self.view.layout();
        if self.view.dual() {
            layout.spread_pages(layout.spread_of(page))
        } else {
            page..page + 1
        }
    }

    /// The reader moved the page view: its centre is the current page again.
    fn reader_moved(&mut self) {
        // A named place found later would pull the reader away.
        self.links.cancel_named();
        self.chosen = None;
        self.clicked_entry = None;
        self.search_state.reader_moved();
        self.searched_away = false;
    }

    /// Puts a link to the page the title bar shows on the clipboard: bare,
    /// or as a Markdown link labelled with the document's name.
    fn copy_link(&self, ctx: &egui::Context, markdown: bool) {
        let (Some(path), Some(page)) = (&self.link_path, self.view.centre_page()) else {
            return;
        };
        let link = if markdown {
            noido_core::markdown_page_link(path, &self.name, page)
        } else {
            noido_core::page_link(path, page)
        };
        if let Some(link) = link {
            ctx.copy_text(link);
        }
    }

    /// Remembers the current place before a jump, for Back.
    fn leave_here(&mut self) {
        self.links.cancel_named();
        if let Some(here) = self.view.place() {
            self.history.leave(here);
        }
    }

    fn go_back(&mut self) {
        if let Some(here) = self.view.place()
            && let Some(to) = self.history.back(here)
        {
            self.view.restore_place(&to);
            self.reader_moved();
        }
    }

    fn go_forward(&mut self) {
        if let Some(here) = self.view.place()
            && let Some(to) = self.history.forward(here)
        {
            self.view.restore_place(&to);
            self.reader_moved();
        }
    }

    /// The nearest places Back and Forward would go to, as many each way as
    /// [`HISTORY_KEEP`] keeps.
    fn nearest_history_targets(&self) -> impl Iterator<Item = &noido_core::DocPoint> + '_ {
        self.history
            .back_entries()
            .iter()
            .rev()
            .take(HISTORY_KEEP)
            .chain(
                self.history
                    .forward_entries()
                    .iter()
                    .rev()
                    .take(HISTORY_KEEP),
            )
    }

    /// Searches the field's text once it has rested, and takes in hits.
    fn update_search(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let was_empty = self.search_state.text().is_empty();
        if let Some(left) = self.search_state.edited(&self.search, now) {
            if left <= 0.0 {
                let from = self.current_page().unwrap_or(0);
                self.search_state.start(from, search::First::Jump);
                if self.search_state.searched().is_some() {
                    self.sidebar.search_started();
                } else {
                    self.sidebar.search_cleared();
                }
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(left));
            }
        }
        // The first letter typed opens the sidebar; later ones leave it be.
        if was_empty && !self.search.is_empty() && !self.sidebar.open {
            self.sidebar.open = true;
            self.sidebar_opened_by_search = true;
        }
        if self.search_state.receive() == Some(search::Arrived::Jump)
            && let Some(current) = self.search_state.hits.current()
        {
            self.go_to_hit(current);
        }
    }

    /// Goes to the next hit (`forward`) or the previous one.
    fn step_hit(&mut self, forward: bool) {
        if let Some(index) = self.hit_target(forward) {
            self.go_to_hit(index);
        }
    }

    /// The hit next (`forward`) or previous would go to: from the current
    /// hit if the view is still where it put it, else from the view.
    fn hit_target(&self, forward: bool) -> Option<usize> {
        let hits = &self.search_state.hits;
        if self.search_state.at_hit && hits.current().is_some() {
            if forward {
                hits.next()
            } else {
                hits.previous()
            }
        } else {
            // Each hit's top in content points; in paged mode, pages before
            // the shown spread lie above the view and pages after it below.
            let layout = self.view.layout();
            let shown = self
                .view
                .current_spread()
                .map(|spread| layout.spread_pages(spread));
            let top_of = |hit: &noido_core::Hit| match layout.page_rect(hit.page) {
                Some(page) => {
                    let y = hit.text.rects.first().map_or(0.0, |r| r.y);
                    page.y + y * layout.scale()
                }
                None if shown.as_ref().is_some_and(|s| hit.page < s.start) => f32::NEG_INFINITY,
                None => f32::INFINITY,
            };
            let (_, y) = self.view.offset();
            let (_, height) = self.view.viewport_size();
            if forward {
                hits.first_where(|hit| top_of(hit) >= y)
            } else {
                hits.last_where(|hit| top_of(hit) < y + height)
            }
        }
    }
    /// Makes hit `index` current and brings it into view.
    fn go_to_hit(&mut self, index: usize) {
        self.search_state.hits.set_current(Some(index));
        let Some(hit) = self.search_state.hits.get(index) else {
            return;
        };
        // A search is one jump: stepping from hit to hit, or searching
        // again from a hit, adds nothing.
        if !std::mem::replace(&mut self.searched_away, true)
            && let Some(here) = self.view.place()
        {
            self.history.leave(here);
        }
        self.view.reveal(hit.page, &hit.text.rects);
        self.chosen = None;
        self.search_state.at_hit = true;
    }

    /// The best image of `page`: its full render, else its thumbnail.
    #[cfg(test)]
    fn page_image(&self, page: usize) -> Option<&PageImage> {
        self.textures.get(&page).or_else(|| self.thumbnail(page))
    }

    /// The thumbnail shown in place of `page` until it renders.
    fn thumbnail(&self, page: usize) -> Option<&PageImage> {
        self.thumbnails
            .as_ref()
            .and_then(|thumbnails| thumbnails.textures.get(&page))
    }

    /// Follows the OS theme while the app runs.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn with_theme(mut self, live: crate::theme::Live) -> Self {
        self.theme_live = Some(live);
        self
    }

    /// Shows the new versions of the document that `watcher` brings.
    pub fn with_watcher(mut self, watcher: Watcher) -> Self {
        self.watcher = Some(watcher);
        self
    }

    /// Asks the shell, not the window, to quit and to open documents, and
    /// tells it about saved settings.
    pub fn with_requests(mut self, requests: crate::shell::Requests) -> Self {
        let to_open = requests.clone();
        self.open = Arc::new(move |path| {
            to_open.send(crate::shell::Request::Open {
                path: path.to_path_buf(),
                target: None,
                reply: None,
            });
        });
        self.requests = Some(requests);
        self
    }

    /// Takes settings saved in another window.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// The recently opened files the menu offers, newest first.
    pub fn set_recent(&mut self, paths: Vec<PathBuf>) {
        self.recent = paths;
    }

    /// Puts `path` at the front of the menu's recent files, in the store
    /// when the window has one. A window with a shell leaves the list to
    /// the shell, which notes the document as its window comes up.
    fn note_recent(&mut self, path: &Path) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        if let Err(e) = persistence.store.note_recent(path) {
            eprintln!("noido: cannot save the recent files: {e}");
        }
        self.recent = persistence.store.load_recent();
    }

    /// The store and slot the document's state is kept in, if it is kept.
    pub fn persistence(&self) -> Option<(Store, Slot)> {
        let p = self.persistence.as_ref()?;
        Some((p.store.clone(), p.slot.clone()))
    }

    /// Saves the document's state now, unsettled or not.
    pub fn save_now(&mut self) {
        if let (Some(state), Some(persistence)) = (self.doc_state(), &mut self.persistence)
            && persistence.saved.as_ref() != Some(&state)
        {
            persistence.save(state);
        }
    }

    /// Comes forward when `activation` is set.
    pub fn with_activation(mut self, activation: Activation) -> Self {
        self.activation = Some(activation);
        self
    }

    /// Brings the window forward if a later launch asked. On Wayland,
    /// where an app can't raise itself, the request for attention is what
    /// shows (on GNOME, a "noido is ready" notice).
    fn come_forward(&mut self, ctx: &egui::Context) {
        let Some(request) = self.activation.as_ref().and_then(Activation::take) else {
            return;
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        if let Some(target) = request.target {
            self.arrival = Some((target, true));
        }
    }

    /// Goes where a link asked: to a page at once, like a thumbnail click;
    /// to a named place once the links thread has found it.
    fn arrive(&mut self) {
        // Until the first frame has measured the window, the layout is a
        // placeholder that a jump would be lost in.
        let (width, height) = self.view.viewport_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let Some((target, leave)) = self.arrival.take() else {
            return;
        };
        self.links.cancel_named();
        match target {
            Target::Page(page) => {
                let Some(last) = self.view.layout().page_count().checked_sub(1) else {
                    return;
                };
                if leave {
                    self.leave_here();
                }
                let page = page.min(last);
                self.view.go_to_page(page);
                self.choose_page(page);
                self.search_state.reader_moved();
            }
            Target::Named(name) => {
                self.named_leaves = leave;
                self.links.resolve(name);
            }
        }
    }

    /// Goes to the named place asked for, now that it is known.
    fn arrive_at_named(&mut self, named: Named) {
        match named {
            Named::Found(target) => {
                if self.named_leaves {
                    self.leave_here();
                }
                self.view.go_to_target(&target);
                let last = self.view.layout().page_count().saturating_sub(1);
                self.choose_page(target.page.min(last));
                self.search_state.reader_moved();
            }
            Named::Unknown(name) => {
                eprintln!("noido: no place named {name:?} in the document");
            }
        }
    }

    /// Takes in the newest version of the document the watcher brought, if
    /// any. Called after the view has caught up with egui's scrolling, so
    /// the place it keeps is the current one.
    fn receive_reload(&mut self, ctx: &egui::Context) {
        let Some(watcher) = &self.watcher else {
            return;
        };
        if let Some(reload) = watcher.try_recv() {
            self.reload(reload, ctx);
        }
    }

    /// Shows `reload`, a new version of the document, at the same place.
    fn reload(&mut self, reload: Reload, ctx: &egui::Context) {
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
        self.pins.retain_below(page_count);
        if self
            .pin_preview
            .as_ref()
            .is_some_and(|preview| preview.page >= page_count)
        {
            self.close_pagelet();
        }
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
        self.sidebar_view.replace(reload.sidebar, page_count);
        self.search_state.set_searcher(reload.searcher);
        self.links.set_linker(reload.linker, page_count);
        self.texts.reset(page_count);
        self.select.clear();
        let from = self.view.centre_page().unwrap_or(0);
        self.search_state.rerun(from);
        self.focus = None;
        self.chosen = None;
        self.clicked_entry = None;
        self.browse = None;
        if let Some(persistence) = &mut self.persistence {
            persistence.store.rekey(&mut persistence.slot, reload.key);
        }
        if let Some(source) = &mut self.source {
            source.bytes = reload.bytes;
            // An open info page reads the new version.
            if let Some(page) = &mut self.info_page {
                *page = info::Page::open(source, ctx);
            }
        }
    }

    /// Opens the document as `settings` say a new one opens; a state restored
    /// afterwards by [`Self::with_persistence`] takes over.
    pub fn with_settings(mut self, file: Option<SettingsFile>, settings: Settings) -> Self {
        let layout = self.view.layout();
        let sizes = (0..layout.page_count())
            .filter_map(|page| layout.page_size(page))
            .collect();
        let (width, height) = self.view.viewport_size();
        self.view = View::with_defaults(sizes, &settings.new_documents);
        self.view.set_viewport_size(width, height);
        self.sidebar.open = settings.new_documents.sidebar;
        self.settings = settings;
        self.settings_file = file;
        self
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
            self.history = state.history.clone();
            self.highlights = Highlights::from_items(state.highlights.clone());
            self.pins = state.pins.clone();
            self.pins.retain_below(self.view.layout().page_count());
            self.pagelet = state.pagelet.map(|pagelet| Pagelet {
                page: None,
                ..pagelet
            });
            self.pending_pagelet = state.pagelet.and_then(|pagelet| pagelet.page);
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
            history: self.history.clone(),
            highlights: self.highlights.items().to_vec(),
            pins: self.pins.clone(),
            pagelet: self.pagelet.map(|pagelet| Pagelet {
                page: self.pin_preview.as_ref().map(|preview| preview.page),
                ..pagelet
            }),
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
            if let Some((told, size)) = self.told
                && size == last.inner.size()
                && (last.offset - told).abs().max_elem() > OFFSET_EPSILON
            {
                self.reader_moved();
            }
            // egui's offset belongs to the old size: record it before resizing,
            // so the resize can re-anchor from it.
            self.view.scrolled_to(last.offset.x, last.offset.y);
            self.view
                .set_viewport_size(last.inner.width(), last.inner.height());
        }
    }

    /// Asks the window for the size fitting the page to it asked for: the page
    /// area wanted plus the chrome the window measures. Runs after the page
    /// area is laid out, so the window's size and the room it gives come from
    /// the same frame; a bar coming or going costs one more frame.
    fn settle_shrink(&mut self, ctx: &egui::Context) {
        let Some(shrink) = self.shrink else {
            return;
        };
        let area = self.view.viewport_size();
        if area.0 <= 0.0 || area.1 <= 0.0 {
            // No layout yet: the next frame has one.
            return;
        }
        let (maximized, fullscreen, screen) = ctx.input(|i| {
            let viewport = i.viewport();
            (
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
                viewport.monitor_size,
            )
        });
        if maximized || fullscreen {
            if shrink.restored() {
                // The window manager kept it maximized: asking again would
                // keep the window from ever being maximized.
                self.shrink = None;
                return;
            }
            // A maximized window takes no size; it is sized once it has a
            // frame of its own again.
            if maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
            if fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
            if let Some(shrink) = &mut self.shrink {
                shrink.asked_restore();
            }
            return;
        }
        let size = ctx.content_rect().size();
        let target = shrink::target(
            shrink,
            &shrink::Room {
                size,
                area,
                sidebar: self.sidebar.open.then_some(self.sidebar.width),
                min: Vec2::from(crate::MIN_WINDOW_SIZE),
                screen,
            },
        );
        if (target - size).abs().max_elem() <= 0.5 {
            self.shrink = None;
            return;
        }
        self.shrink = shrink.spend();
        if self.shrink.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
        }
    }

    /// Whether the settings or document info page is open: it covers the
    /// document, so the document's highlights must not change behind it.
    /// Presses and the selection's bar wait for it to close, and Undo, Redo
    /// and the delete keys are held back; view actions still reach the
    /// document, from the menu and the title bar.
    fn page_open(&self) -> bool {
        self.settings_page.is_some() || self.info_page.is_some()
    }

    fn handle_input(&mut self, ctx: &egui::Context, parent: Option<&eframe::Frame>) {
        // The settings and info pages are modal: the document doesn't move
        // behind them.
        if self.page_open() {
            return;
        }
        let editing = ctx.text_edit_focused();
        let (presses, wheel, zoom, pointer, repeated) = ctx.input(|i| {
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
                        shift: modifiers.shift,
                        alt: modifiers.alt,
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
            let repeated = i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::PageUp | egui::Key::PageDown,
                        pressed: true,
                        repeat: true,
                        ..
                    }
                )
            });
            (
                presses,
                wheel,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                repeated,
            )
        });
        self.page_key_repeated |= repeated;

        // egui moves focus with the arrows, and menus take them too.
        let egui_arrows = ctx.memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ctx);
        for press in presses {
            if egui_arrows && press.key.is_arrow() {
                continue;
            }
            if let Some(action) = self.keymap.action(press, editing) {
                self.apply(ctx, parent, action);
            }
        }
        // Esc on the pages clears the selection; in a field it's the
        // field's. egui drops focus on Esc before this, so it's the last
        // frame's focus that tells. Backspace and Delete remove the
        // highlight being edited.
        if !editing && !self.select.widget_focused && !egui::Popup::is_any_open(ctx) {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.select.clear();
            }
            let delete = ctx
                .input(|i| i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete));
            if delete && let Some(index) = self.select.editing {
                self.highlights.remove(index);
                self.select.clear();
            }
        }
        // Mod+C reaches egui as a copy event, not a key; a field copies
        // its own text.
        if !editing
            && !self.select.widget_focused
            && ctx.input(|i| i.events.contains(&egui::Event::Copy))
        {
            self.copy_selection(ctx);
        }
        // The mouse's back and forward buttons, also while typing.
        let buttons: Vec<Action> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Extra1,
                        pressed: true,
                        ..
                    } => Some(Action::Back),
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Extra2,
                        pressed: true,
                        ..
                    } => Some(Action::Forward),
                    _ => None,
                })
                .collect()
        });
        for action in buttons {
            self.apply(ctx, parent, action);
        }
        // Not elsewhere, such as over the sidebar: there the wheel scrolls
        // what is under it. Nor over a popup: the menu and the dialogs
        // scroll their own content, and the pages stay put behind them.
        let elsewhere = self
            .last_scroll
            .is_some_and(|last| pointer.is_some_and(|p| !last.inner.contains(p)));
        let over_popup = pointer.is_some_and(|p| {
            ctx.layer_id_at(p).is_some_and(|layer| {
                matches!(layer.order, egui::Order::Foreground | egui::Order::Tooltip)
            })
        });
        let over_pages = !elsewhere && !over_popup;
        if over_pages {
            let spread = self.view.current_spread();
            for step in wheel {
                self.wheel_flip(step);
            }
            if self.view.current_spread() != spread {
                self.reader_moved();
            }
        }
        if zoom != 1.0 {
            self.reader_moved();
            let anchor = self.last_scroll.and_then(|last| {
                pointer
                    .filter(|p| last.inner.contains(*p))
                    .map(|p| (p.x - last.inner.min.x, p.y - last.inner.min.y))
            });
            self.view.zoom_by(zoom, anchor);
        }
    }

    /// Draws the title bar and acts on what the reader did in it.
    fn title_bar(&mut self, ui: &mut egui::Ui, parent: Option<&eframe::Frame>) {
        let status = match self.view.centre_page() {
            Some(page) => {
                let count = self.view.layout().page_count();
                titlebar::status_text(&self.links.numbers, page, count, self.view.scale())
            }
            None => String::new(),
        };
        let bar = titlebar::Bar {
            name: &self.name,
            status,
            sidebar: self.sidebar,
            continuous: self.view.continuous(),
            dual: self.view.dual(),
            odd_left: self.view.odd_left(),
            gapless: self.view.gapless(),
            select_tool: self.select.tool,
            keymap: &self.keymap,
            counter: self.search_state.counter(),
            recent: &self.recent,
        };
        let focus = std::mem::take(&mut self.search_focus_requested);
        let frame = crate::theme::Theme::current(ui.ctx()).frame(
            ui.ctx().theme(),
            crate::theme::Surface::TitleBar,
            ui.style(),
        );
        let output = egui::Panel::top("titlebar")
            .exact_size(TITLE_BAR_HEIGHT)
            .frame(frame)
            .show(ui, |ui| titlebar::show(ui, &bar, &mut self.search, focus))
            .inner;
        self.bar = output.rects;
        if let Some(forward) = output.step {
            self.step_hit(forward);
        }
        if output.cancelled {
            if self.search.is_empty() {
                if std::mem::take(&mut self.sidebar_opened_by_search) {
                    self.sidebar.open = false;
                }
            } else {
                // The first Esc clears the search; the field keeps focus.
                self.search.clear();
                self.search_state.clear();
                self.sidebar.search_cleared();
                self.search_focus_requested = true;
            }
        }
        if output.focused {
            self.search_state.extract();
        }
        if output.left {
            // The reader moved on; the sidebar is theirs now.
            self.sidebar_opened_by_search = false;
        }
        if let Some(path) = output.recent {
            self.note_recent(&path);
            (self.open)(&path);
        }
        let ctx = ui.ctx().clone();
        for action in output.actions {
            self.apply(&ctx, parent, action);
        }
    }

    /// Carries out `action`, from a key or the menu. `parent` is the
    /// window, for dialogs that ask for it.
    fn apply(&mut self, ctx: &egui::Context, parent: Option<&eframe::Frame>, action: Action) {
        match action {
            Action::ToggleSidebar => {
                self.sidebar.open = !self.sidebar.open;
                self.sidebar_opened_by_search = false;
            }
            // The window, not the view: the zoom is held at the scale it has,
            // so the resize doesn't refit the pages. Held only when there is a
            // spread to fit to; with no pages the view has nothing to keep.
            Action::FitWindowWidth | Action::FitWindowPage => {
                let (area_width, area_height) = self.view.viewport_size();
                // Before the first layout there is no page area to fit to.
                if area_width > 0.0
                    && area_height > 0.0
                    && let Some((width, height)) = self.view.spread_viewport_size()
                {
                    self.view.hold_zoom();
                    self.shrink = Some(match action {
                        Action::FitWindowPage => Shrink::new(width, height),
                        _ => Shrink::new(width, area_height),
                    });
                }
            }
            Action::FocusSearch => self.search_focus_requested = true,
            Action::ToggleSelectTool => self.select.tool = !self.select.tool,
            // Behind the settings or info page, where it can't be seen.
            Action::Undo | Action::Redo if self.page_open() => {}
            Action::Undo => {
                if self.highlights.undo() {
                    self.select.clear();
                }
            }
            Action::Redo => {
                if self.highlights.redo() {
                    self.select.clear();
                }
            }
            Action::Copy => self.copy_selection(ctx),
            // Not view moves: going to a hit isn't the reader moving away.
            Action::NextHit => self.step_hit(true),
            Action::PreviousHit => self.step_hit(false),
            Action::Back => self.go_back(),
            Action::Forward => self.go_forward(),
            Action::CopyLink => self.copy_link(ctx, false),
            Action::CopyMarkdownLink => self.copy_link(ctx, true),
            Action::Quit if self.requests.is_some() => {
                if let Some(requests) = &self.requests {
                    requests.send(crate::shell::Request::Quit);
                }
            }
            Action::CloseWindow | Action::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close)
            }
            Action::ClearRecent => {
                if let Some(requests) = &self.requests {
                    requests.send(crate::shell::Request::ClearRecent);
                } else {
                    // A window of its own, with no shell: the store is
                    // the window's.
                    if let Some(persistence) = &self.persistence
                        && let Err(e) = persistence.store.clear_recent()
                    {
                        eprintln!("noido: cannot clear the recent files: {e}");
                    }
                    self.recent.clear();
                }
            }
            Action::OpenDocument => {
                // The dialog opens where the open document lives, if its
                // path is known; otherwise the desktop's default.
                let directory = self
                    .link_path
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_path_buf);
                (self.pick)(parent, directory, self.open.clone());
            }
            Action::OpenSettings => {
                // Another noido may have saved changes since: start from
                // them, so saving here doesn't undo them.
                if let Some(Ok(settings)) = self.settings_file.as_ref().map(SettingsFile::load) {
                    self.settings = settings;
                }
                // So Esc closes the page, not the search behind it.
                ctx.memory_mut(|m| m.stop_text_input());
                self.settings_page = Some(settings::Page::default());
            }
            // Jumps, like a thumbnail click: Back returns to where the reader
            // was; a press that moves nothing leaves no entry.
            Action::FirstPage | Action::LastPage => {
                let before = self.view.place();
                self.view.apply(action);
                if self.view.place() != before {
                    if let Some(here) = before {
                        self.history.leave(here);
                    }
                    self.reader_moved();
                }
            }
            Action::ShowDocumentInfo => {
                if let Some(source) = &self.source {
                    // So Esc closes the page, not the search behind it.
                    ctx.memory_mut(|m| m.stop_text_input());
                    self.info_page = Some(info::Page::open(source, ctx));
                }
            }
            _ => {
                let place = |view: &View| (view.offset(), view.current_spread(), view.scale());
                let before = place(&self.view);
                self.view.apply(action);
                if matches!(action, Action::ScrollScreenUp | Action::ScrollScreenDown) {
                    self.screen_edge = self.view.take_screen_edge().map(|edge| (edge, None));
                }
                // An arrow at an edge moves nothing; the place stays.
                if place(&self.view) != before {
                    self.reader_moved();
                }
            }
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
        let now = ctx.input(|i| i.time);
        self.fades.forget_finished(now);
        while let Some(rendered) = self.renderer.try_recv() {
            match rendered.result {
                Ok(image) => {
                    let name = format!("page-{}", rendered.page);
                    let texture = PageImage::load(ctx, name, &image);
                    // The first render over the thumbnail fades in; one
                    // over an older render (a zoom), or with no thumbnail
                    // to fade from, shows at once.
                    let stand_in = self.thumbnail(rendered.page).is_some();
                    if self.textures.insert(rendered.page, texture).is_none() && stand_in {
                        self.fades.start(rendered.page, now);
                    }
                    self.tracker.mark_ready(rendered.page, rendered.scale);
                }
                Err(e) => {
                    eprintln!("noido: page {}: {e}", rendered.page + 1);
                    // Not an image of an earlier version in its place.
                    self.textures.remove(&rendered.page);
                    self.tracker.mark_failed(rendered.page, rendered.scale);
                }
            }
        }
    }

    fn pages(&mut self, ui: &mut egui::Ui, viewport: Rect) {
        // The pages next and previous would go to, rendered ahead so a far
        // hit is sharp at once, and so is the one left behind.
        let hit_pages: Vec<usize> = [true, false]
            .into_iter()
            .filter_map(|forward| self.hit_target(forward))
            .filter_map(|index| self.search_state.hits.get(index).map(|hit| hit.page))
            .collect();
        self.hovered_link = None;
        let layout = self.view.layout();
        // Clicks follow links; drags still pan, as they don't click. Not
        // focusable (`Sense::click()` is): a focused page area would take
        // the arrow keys from scrolling.
        let (content, response) = ui.allocate_exact_size(
            vec2(layout.content_width(), layout.content_height()),
            Sense::CLICK,
        );
        // Not over a scroll bar, the sidebar or a popup.
        self.select.pages_hovered = response.hovered();

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

        let focus = layout.spread_of(self.focus.unwrap_or(visible.start));
        let window = window_keep(spread_count, focus, KEEP);
        let window = if window.is_empty() {
            visible.clone()
        } else {
            let window = spread_pages(window.start, window.end - 1);
            window.start.min(visible.start)..window.end.max(visible.end)
        };
        // The neighbouring hits' spreads: rendered wherever they are (the
        // window renders only a few spreads ahead and behind), and kept where
        // outside the window.
        let mut ahead: Vec<usize> = Vec::new();
        for page in hit_pages {
            for page in layout.spread_pages(layout.spread_of(page)) {
                if !visible.contains(&page) && !ahead.contains(&page) {
                    ahead.push(page);
                }
            }
        }
        // The places Back and Forward would go to: kept sharp so a jump there
        // is never a thumbnail. The stacks change only on a jump, so
        // recomputing them each frame refreshes the targets then and leaves
        // them be while reading on.
        for point in self.nearest_history_targets() {
            // A saved history entry may be past the end of a shorter version.
            // Layout clamping already keeps out-of-range pages out; this also
            // skips a stale entry whose spread would otherwise keep an
            // in-range neighbour in dual mode.
            if point.page >= layout.page_count() {
                continue;
            }
            for page in layout.spread_pages(layout.spread_of(point.page)) {
                if !visible.contains(&page) && !ahead.contains(&page) {
                    ahead.push(page);
                }
            }
        }
        // The pagelet's page is kept sharp even when it is far from the
        // reader, at the scale the panel draws it.
        let preview = self.pin_preview.as_ref().and_then(|preview| {
            let size = layout.page_size(preview.page)?;
            Some((
                preview.page,
                preview.page_rect.width() / size.width * ui.ctx().pixels_per_point(),
            ))
        });
        // A pagelet whose page is gone (a shorter version arrived) is
        // closed by `pagelet`, later in the same frame.
        if let Some((page, _)) = preview
            && !visible.contains(&page)
            && !ahead.contains(&page)
        {
            ahead.push(page);
        }
        let mut extra: Vec<usize> = ahead
            .iter()
            .copied()
            .filter(|page| !window.contains(page))
            .collect();
        // The pages on screen at the nearest Back and Forward targets: kept so
        // a return is sharp, but not requested, so a jump renders nothing it
        // does not show.
        for point in self.nearest_history_targets() {
            for page in self.view.band_around(point) {
                if !window.contains(&page) && !extra.contains(&page) {
                    extra.push(page);
                }
            }
        }

        // Visible pages first, then the pagelet's page, then the
        // neighbouring hits' (stepping to one is likely while searching),
        // then the window around the current spread, nearest first and
        // mostly ahead.
        let mut order: Vec<usize> = visible.clone().collect();
        if let Some((page, _)) = preview
            && !visible.contains(&page)
        {
            order.push(page);
        }
        for page in &ahead {
            if !order.contains(page) {
                order.push(*page);
            }
        }
        for spread in window_order(spread_count, focus, self.direction, PREFETCH_BUDGET) {
            order.extend(
                layout
                    .spread_pages(spread)
                    .filter(|page| !visible.contains(page)),
            );
        }
        let keep = Keep { window, extra };

        // Set before requesting: the render thread skips pages outside this
        // set, and the tracker forgets them below, so none stays pending.
        self.renderer.set_keep(keep.clone());
        // Pages requested earlier, while scrolling past, wait behind these.
        self.renderer.set_priority(order.clone());

        let display_scale = self.view.scale() * ui.ctx().pixels_per_point();
        if display_scale > 0.0 {
            for page in order {
                // A page both on screen and in the pagelet is rendered once,
                // at whichever scale needs more pixels.
                let scale = match preview {
                    Some((previewed, panel_scale)) if previewed == page => {
                        panel_scale.max(if visible.contains(&page) {
                            display_scale
                        } else {
                            0.0
                        })
                    }
                    _ => display_scale,
                };
                let wanted = self.tracker.wanted(page..page + 1, |page| {
                    let size = layout.page_size(page).expect("page in range");
                    render_scale(size, scale, MAX_RENDER_SIDE)
                });
                for (page, scale) in wanted {
                    self.renderer.request(RenderRequest { page, scale });
                }
            }
        }
        self.tracker.forget_outside(keep.clone());
        // Also images kept from an earlier version, which the tracker never knew.
        self.textures.retain(|&page, _| keep.contains(page));

        for page in visible.clone() {
            self.links.want(page);
            let search = &self.search_state;
            self.texts.want(page, |page| search.request_text(page));
        }
        // The pages of the highlight being edited: its ends, and a copy
        // across them, wait for their text.
        if let Some(highlight) = self
            .select
            .editing
            .and_then(|index| self.highlights.get(index))
        {
            let search = &self.search_state;
            for page in highlight.pages() {
                self.texts.want(page, |page| search.request_text(page));
            }
        }
        // An end of the selection waiting for its page's text: a drag let go
        // past the pages' edge can leave it on a page out of view.
        if let Some(selection) = self.select.selection {
            let search = &self.search_state;
            for spot in [selection.anchor, selection.focus] {
                if let noido_core::Spot::Waiting(p) = spot {
                    self.texts.want(p.page, |page| search.request_text(page));
                }
            }
        }
        // A press that took a handle is not a click on a link under it.
        let on_handle = self
            .select
            .drag
            .is_some_and(|drag| drag.kind == DragKind::Handle);
        // A press on a link is a click on the link, not on a highlight under
        // it; Shift asks for the highlight.
        let on_link = self.select.drag.is_some_and(|drag| {
            layout
                .doc_point(drag.origin.x - content.min.x, drag.origin.y - content.min.y)
                .is_some_and(|p| self.links.at(p.page, p.x, p.y).is_some())
        });
        let shown = select::Pages {
            texts: &self.texts,
            layout,
            content,
            viewport: viewport.translate(content.min.to_vec2()),
        };
        self.select
            .update(&shown, ui, &mut self.highlights, on_link);
        self.select.place(&shown);
        let visible_pages = visible.clone();
        // Not over a scroll bar, the sidebar or a popup: the response
        // isn't hovered there.
        let hovered = response.hover_pos().and_then(|pos| {
            let p = layout.doc_point(pos.x - content.min.x, pos.y - content.min.y)?;
            visible.contains(&p.page).then_some(())?;
            Some((p.page, self.links.at(p.page, p.x, p.y)?.clone()))
        });

        let painter = ui.painter();
        let hits = &self.search_state.hits;
        let paper = self.paper_tint(ui.ctx());
        let dark = ui.visuals().dark_mode;
        let now = ui.input(|i| i.time);
        let mut fading = false;
        for page in visible {
            let Some(r) = layout.page_rect(page) else {
                continue;
            };
            let rect = Rect::from_min_size(content.min + vec2(r.x, r.y), vec2(r.width, r.height));
            fading |= paint_page(
                painter,
                rect,
                self.textures.get(&page),
                self.thumbnail(page),
                self.fades.alpha(page, now),
                paper,
            );
            // The document's highlights, under the selection and the
            // matches. The one being edited follows the selection, so a
            // handle resizes it on screen.
            if let PageTextState::Known(text) = self.texts.get(page) {
                for (index, highlight) in self.highlights.items().iter().enumerate() {
                    let range = if self.select.editing == Some(index) {
                        self.select
                            .selection
                            .and_then(|s| s.on_page(page, text.char_count()))
                    } else {
                        highlight.on_page(page, text.char_count())
                    };
                    let Some(range) = range else {
                        continue;
                    };
                    let fill = select::highlight_fill(highlight.colour, dark);
                    for area in text.rects(range) {
                        let min = rect.min + vec2(area.x, area.y) * layout.scale();
                        let size = vec2(area.width, area.height) * layout.scale();
                        painter.rect_filled(Rect::from_min_size(min, size), 0.0, fill);
                    }
                }
            }
            // The selection, under the matches; a highlight being edited
            // already shows it in its own colour.
            if self.select.editing.is_none()
                && let Some(selection) = &self.select.selection
                && let PageTextState::Known(text) = self.texts.get(page)
                && let Some(range) = selection.on_page(page, text.char_count())
            {
                let fill = ui
                    .visuals()
                    .selection
                    .bg_fill
                    .gamma_multiply(SELECTION_OPACITY);
                for area in text.rects(range) {
                    let min = rect.min + vec2(area.x, area.y) * layout.scale();
                    let size = vec2(area.width, area.height) * layout.scale();
                    painter.rect_filled(Rect::from_min_size(min, size), 0.0, fill);
                }
            }
            // Matches over the page, the current one stronger.
            let first = hits.first_on(page).unwrap_or(0);
            let warning = ui.visuals().warn_fg_color;
            for (i, hit) in hits.on_page(page).iter().enumerate() {
                let fill = if hits.current() == Some(first + i) {
                    search::current_fill(warning)
                } else {
                    search::highlight_fill(warning)
                };
                for area in &hit.text.rects {
                    let min = rect.min + vec2(area.x, area.y) * layout.scale();
                    let size = vec2(area.width, area.height) * layout.scale();
                    painter.rect_filled(Rect::from_min_size(min, size), 0.0, fill);
                }
            }
        }

        if fading {
            ui.ctx().request_repaint();
        }

        // The selection's handles: a stem along each end's caret and a
        // knob below it.
        let colour = ui.visuals().selection.bg_fill;
        for (line, knob) in self.select.caret_lines.iter().zip(self.select.handles) {
            if let (Some((top, bottom)), Some(knob)) = (line, knob) {
                painter.line_segment([*top, *bottom], egui::Stroke::new(2.0, colour));
                painter.circle_filled(knob, select::HANDLE_RADIUS, colour);
            }
        }

        if let Some((page, link)) = hovered
            && let Some(r) = layout.page_rect(page)
        {
            let origin = content.min + vec2(r.x, r.y);
            let stroke = egui::Stroke::new(LINK_OUTLINE_WIDTH, ui.visuals().hyperlink_color);
            for area in &link.areas {
                let min = origin + vec2(area.x, area.y) * layout.scale();
                let size = vec2(area.width, area.height) * layout.scale();
                painter.rect_stroke(
                    Rect::from_min_size(min, size),
                    2.0,
                    stroke,
                    egui::StrokeKind::Outside,
                );
            }
            // Shift+click with a selection extends it instead.
            let extends = ui.input(|i| i.modifiers.shift) && self.select.selection.is_some();
            if response.clicked() && !extends && !on_handle {
                self.clicked_link = Some(link.clone());
            }
            if let noido_engine::LinkTarget::Web(uri) = &link.target {
                crate::theme::hover_text_at_pointer(response, uri);
            }
            self.hovered_link = Some(link);
        }

        if let Some((page, area, opacity)) = self.links.flash(ui.input(|i| i.time))
            && visible_pages.contains(&page)
            && let Some(r) = layout.page_rect(page)
        {
            let origin = content.min + vec2(r.x, r.y);
            let colour = ui.visuals().hyperlink_color.gamma_multiply(opacity);
            let min = origin + vec2(area.x, area.y) * layout.scale();
            let size = vec2(area.width, area.height) * layout.scale();
            painter.rect_stroke(
                Rect::from_min_size(min, size),
                2.0,
                egui::Stroke::new(FLASH_OUTLINE_WIDTH, colour),
                egui::StrokeKind::Outside,
            );
            ui.ctx().request_repaint();
        }

        if let Some((edge, Some(start))) = self.screen_edge
            && let Some(r) = layout.page_rect(edge.page)
        {
            let elapsed = ui.input(|i| i.time) - start;
            if elapsed < FLASH_SECONDS {
                let opacity = (1.0 - elapsed / FLASH_SECONDS) as f32;
                let colour = ui.visuals().hyperlink_color.gamma_multiply(opacity);
                let y = content.min.y + r.y + edge.y * layout.scale();
                let row = layout.spread_pages(layout.spread_of(edge.page));
                for page in row.filter_map(|page| layout.page_rect(page)) {
                    let left = content.min.x + page.x;
                    painter.line_segment(
                        [pos2(left, y), pos2(left + page.width, y)],
                        egui::Stroke::new(FLASH_OUTLINE_WIDTH, colour),
                    );
                }
                ui.ctx().request_repaint();
            } else {
                self.screen_edge = None;
            }
        }
    }

    /// Takes a press on the pages: what its drag will do is decided now,
    /// before the pages' scroll area decides whether to pan.
    fn start_drag(&mut self, ctx: &egui::Context) {
        if self.page_open() {
            return;
        }
        let (pressed, origin, shift) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.press_origin(),
                i.modifiers.shift,
            )
        });
        let Some(origin) = origin.filter(|_| pressed) else {
            return;
        };
        // Hovered as of last frame: the scroll bars and the sidebar's edge
        // lie over the pages' rect.
        let on_pages = self.select.pages_hovered
            && self
                .last_scroll
                .is_some_and(|last| last.inner.contains(origin))
            && !egui::Popup::is_any_open(ctx)
            && !self.select.bar_rect.is_some_and(|bar| bar.contains(origin));
        if on_pages {
            let handle = self.select.handle_at(origin);
            self.select.drag = Some(select::Drag {
                kind: drag_kind(self.select.tool, shift, handle.is_some()),
                origin,
                moved: false,
                started: false,
                handle,
                shift,
            });
        }
    }

    /// While a selection drag is near or past the pages' edge, scrolls
    /// toward it: faster the nearer the edge, and faster the longer held.
    fn edge_scroll(&mut self, ctx: &egui::Context) {
        let speed = self.edge_speed(ctx);
        if speed == (0.0, 0.0) {
            self.select.edge_since = None;
            return;
        }
        let (vx, vy) = speed;
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let (x, y) = self.view.offset();
        self.view.scrolled_to(x + vx * dt, y + vy * dt);
        ctx.request_repaint();
    }

    /// How fast a selection drag scrolls the pages now, per axis; starts
    /// timing the hold when the pointer reaches the band.
    fn edge_speed(&mut self, ctx: &egui::Context) -> (f32, f32) {
        let Some(drag) = self.select.drag else {
            return (0.0, 0.0);
        };
        if drag.kind == DragKind::Pan || !drag.moved {
            return (0.0, 0.0);
        }
        let (Some(last), Some(pos)) = (self.last_scroll, ctx.input(|i| i.pointer.latest_pos()))
        else {
            return (0.0, 0.0);
        };
        let r = last.inner;
        let viewport = (r.min.x, r.min.y, r.max.x, r.max.y);
        if noido_core::edge_scroll((pos.x, pos.y), viewport, 0.0) == (0.0, 0.0) {
            return (0.0, 0.0);
        }
        let now = ctx.input(|i| i.time);
        let since = *self.select.edge_since.get_or_insert(now);
        noido_core::edge_scroll((pos.x, pos.y), viewport, (now - since) as f32)
    }

    /// Puts the selected text on the clipboard; if a page's text in it is
    /// still to come, asks for it and copies once it's there.
    fn copy_selection(&mut self, ctx: &egui::Context) {
        let Some(selection) = self.select.selection else {
            self.select.copy_pending = None;
            return;
        };
        let texts = &self.texts;
        match selection.text(|page| texts.get(page)) {
            Some(text) => {
                self.select.copy_pending = None;
                if !text.is_empty() {
                    ctx.copy_text(text);
                }
            }
            None => {
                self.select.copy_pending = Some(selection);
                if let Some(pages) = selection.pages() {
                    let search = &self.search_state;
                    for page in pages {
                        self.texts.want(page, |page| search.request_text(page));
                    }
                }
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }

    /// The small bar by where the mouse let go, once the drag is over.
    fn selection_bar(&mut self, ctx: &egui::Context, parent: Option<&eframe::Frame>) {
        self.select.bar_rect = None;
        self.select.swatches = [None; select::SWATCH_COUNT];
        self.select.delete_rect = None;
        if self.select.drag.is_some() || self.page_open() {
            return;
        }
        let Some(last) = self.last_scroll else {
            return;
        };
        // The bar hangs from where the mouse let go, or failing that from
        // the selection's end.
        let (top, bottom) = match self.select.bar_at {
            Some(at) => {
                if !last.inner.contains(at) {
                    return;
                }
                (at, at)
            }
            None => {
                let Some((top, bottom)) = self.select.caret_lines[1] else {
                    return;
                };
                if !last.inner.contains(top.lerp(bottom, 0.5)) {
                    return;
                }
                (top, bottom)
            }
        };
        let (pos, pivot) = select::bar_place(
            (top, bottom),
            self.select.caret_lines,
            last.inner,
            ctx.content_rect(),
            self.select.bar_size.unwrap_or(SELECTION_BAR_ESTIMATE),
            SELECTION_BAR_GAP,
            self.select.bar_end(),
        );
        let mut copy = false;
        let mut clicked = None;
        let mut delete = false;
        // The colour of the highlight being edited, ringed in the bar.
        let selected = self
            .select
            .editing
            .and_then(|index| self.highlights.get(index));
        let shown = egui::Area::new(egui::Id::unique("selection-bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(pivot)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    // The bar stands about SELECTION_BAR_HEIGHT tall, its
                    // frame's margins and all: the colour buttons set the row's
                    // height and the text buttons centre in it.
                    let margins = ui.spacing().menu_margin;
                    let row =
                        (SELECTION_BAR_HEIGHT - f32::from(margins.top) - f32::from(margins.bottom))
                            .max(select::SWATCH_SIZE);
                    ui.horizontal(|ui| {
                        ui.set_min_height(row);
                        copy = ui.button("Copy").clicked();
                        for (i, colour) in HighlightColour::ALL.into_iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::splat(select::SWATCH_SIZE),
                                Sense::click(),
                            );
                            // Above the bar, not just above the swatch: the
                            // swatch sits centred in the bar's taller row, so
                            // the tooltip lifts by the frame's padding and the
                            // row's slack, and by the bar's own gap.
                            let name = select::colour_name(colour);
                            let slack = (row - select::SWATCH_SIZE) / 2.0;
                            let gap = f32::from(margins.top) + slack + SELECTION_BAR_GAP;
                            let mut tip = egui::Tooltip::for_enabled(&response);
                            tip.popup = tip.popup.align(egui::RectAlign::TOP).gap(gap);
                            tip.show(|ui| ui.label(name));
                            let centre = rect.center();
                            let stroke = ui.visuals().widgets.hovered.fg_stroke;
                            ui.painter().circle_filled(
                                centre,
                                select::SWATCH_RADIUS,
                                select::swatch_colour(colour),
                            );
                            if response.hovered() {
                                ui.painter()
                                    .circle_stroke(centre, select::SWATCH_RADIUS, stroke);
                            }
                            if selected.is_some_and(|h| h.colour == colour) {
                                let ring = ui.visuals().selection.stroke;
                                ui.painter().circle_stroke(
                                    centre,
                                    select::SWATCH_RADIUS + 2.0,
                                    ring,
                                );
                            }
                            if response.clicked() {
                                clicked = Some(colour);
                            }
                            self.select.swatches[i] = Some(rect);
                        }
                        // Always there, so the bar keeps its width as
                        // a highlight is made or selected; it acts only
                        // on a highlight being edited.
                        let response =
                            ui.add_enabled(selected.is_some(), egui::Button::new("Delete"));
                        delete = response.clicked();
                        self.select.delete_rect = Some(response.rect);
                    });
                });
            });
        self.select.bar_rect = Some(shown.response.rect);
        // Placed with a size that was out, so place it again.
        let size = shown.response.rect.size();
        if self.select.bar_size != Some(size) {
            self.select.bar_size = Some(size);
            ctx.request_repaint();
        }
        if copy {
            self.apply(ctx, parent, Action::Copy);
        }
        if let Some(colour) = clicked {
            // A swatch recolours the highlight being edited, or makes one
            // of the selection and keeps it selected, so the bar stays
            // open on it.
            if let Some(index) = self.select.editing {
                self.highlights.set_colour(index, colour);
            } else if let Some((start, end)) = self.select.selection.and_then(|s| s.carets())
                && start < end
            {
                self.select.editing = self.highlights.add(Highlight { start, end, colour });
            }
        }
        if delete && let Some(index) = self.select.editing {
            self.highlights.remove(index);
            self.select.clear();
        }
    }

    /// Starts the screen edge's flash once PageUp and PageDown are let go,
    /// unless one was held down long enough to repeat.
    fn start_screen_edge(&mut self, ctx: &egui::Context) {
        let (held, now) = ctx.input(|i| {
            let held = i.key_down(egui::Key::PageUp) || i.key_down(egui::Key::PageDown);
            (held, i.time)
        });
        if held {
            return;
        }
        if std::mem::take(&mut self.page_key_repeated) {
            self.screen_edge = None;
        } else if let Some((_, start @ None)) = &mut self.screen_edge {
            *start = Some(now);
        }
    }

    /// Follows `link`: jumps to an internal target and flashes it, or
    /// opens a web or mail address.
    fn follow(&mut self, ctx: &egui::Context, link: noido_engine::Link) {
        self.follow_target(ctx, link.target);
    }

    /// Goes to `target` and flashes it if it is in the document, or opens
    /// it if it is a web or mail address: for a link and an outline entry.
    fn follow_target(&mut self, ctx: &egui::Context, target: noido_engine::LinkTarget) {
        match target {
            noido_engine::LinkTarget::Internal(target) => {
                self.leave_here();
                self.view.go_to_target(&target);
                let last = self.view.layout().page_count().saturating_sub(1);
                self.choose_page(target.page.min(last));
                self.search_state.reader_moved();
                self.links.start_flash(target);
            }
            noido_engine::LinkTarget::Web(uri) => {
                if crate::links::opens(&uri) {
                    ctx.open_url(egui::OpenUrl::new_tab(uri));
                }
            }
        }
    }

    /// The size a pagelet first takes on `page`: the page fitted into a
    /// share of the document view.
    /// The page's width a pagelet first gets: a share of the document view.
    fn default_page_width(&self, page: usize, view: Rect) -> f32 {
        let Some(size) = self.view.layout().page_size(page) else {
            return PAGELET_MIN.x - 2.0 * PAGELET_PADDING;
        };
        let room = vec2(
            (view.width() * PAGELET_WIDTH_SHARE - 2.0 * PAGELET_PADDING).max(1.0),
            (view.height() * PAGELET_HEIGHT_SHARE - 2.0 * PAGELET_PADDING).max(1.0),
        );
        fit_page(
            size,
            PageRect {
                x: 0.0,
                y: 0.0,
                width: room.x,
                height: room.y,
            },
        )
        .width
    }

    /// The page's size at `page`: `width` wide, the page's aspect, within
    /// what the panel may take.
    fn page_size_at(&self, page: usize, view: Rect, width: f32) -> Vec2 {
        let Some(size) = self.view.layout().page_size(page) else {
            return PAGELET_MIN - Vec2::splat(2.0 * PAGELET_PADDING);
        };
        let aspect = size.width / size.height;
        let min = (PAGELET_MIN.x - 2.0 * PAGELET_PADDING)
            .max((PAGELET_MIN.y - 2.0 * PAGELET_PADDING) * aspect);
        let max = (view.width() - 2.0 * PAGELET_PADDING).max(min);
        let width = width.clamp(min, max);
        vec2(width, width / aspect)
    }

    /// Opens the pagelet on `page`, or swaps its page, keeping the place
    /// and size it last had.
    fn open_pagelet(&mut self, page: usize, view: Rect, bounds: Rect, pointer: Option<egui::Pos2>) {
        if self.view.layout().page_size(page).is_none() {
            return;
        }
        // One pagelet: it keeps its place while its page changes.
        let width = self
            .pagelet
            .map_or_else(|| self.default_page_width(page, view), |p| p.width);
        let page_size = self.page_size_at(page, view, width);
        // Its first place is the document view's upper-left corner, flush
        // against the sidebar.
        // Its first place is the document view's corner; once placed, the
        // reader's place stands, even over the sidebar.
        let pos = self.pagelet.map_or(view.min, |p| pos2(p.x, p.y));
        let rect = clamp_into(
            Rect::from_min_size(pos, page_size + Vec2::splat(2.0 * PAGELET_PADDING)),
            bounds,
        );
        let opacity = if pointer.is_some_and(|at| rect.contains(at)) {
            1.0
        } else {
            0.5
        };
        self.pin_preview = Some(PinPreview {
            page,
            rect,
            page_rect: Rect::from_min_size(rect.min + Vec2::splat(PAGELET_PADDING), page_size),
            page_size,
            opacity,
            resizing: false,
        });
    }

    /// Closes the pagelet, remembering where it was for the next one.
    fn close_pagelet(&mut self) {
        if let Some(preview) = self.pin_preview.take() {
            self.pagelet = Some(Pagelet {
                x: preview.rect.min.x,
                y: preview.rect.min.y,
                width: preview.page_size.x,
                page: None,
            });
        }
    }

    /// Draws the pagelet over the pages while it is open.
    fn pagelet(&mut self, ctx: &egui::Context) {
        let Some(preview) = &self.pin_preview else {
            return;
        };
        let page = preview.page;
        let Some(size) = self.view.layout().page_size(page) else {
            self.close_pagelet();
            return;
        };
        let rect = preview.rect;
        // The pagelet's links are live: they take the main view.
        self.links.want(page);
        let links = &self.links;
        let paper = self.paper_tint(ctx);
        let now = ctx.input(|i| i.time);
        // Opaque under the pointer and while the pointer is in the sidebar,
        // where the pins are read; half over the rest of the pages.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let in_sidebar = self.sidebar.open
            && pointer.is_some_and(|at| {
                let view = self
                    .last_scroll
                    .map_or_else(|| ctx.content_rect(), |last| last.inner);
                at.x < view.min.x && at.y >= view.min.y
            });
        let target =
            if preview.resizing || pointer.is_some_and(|at| rect.contains(at)) || in_sidebar {
                1.0
            } else {
                0.5
            };
        let reduce = crate::theme::Theme::current(ctx).reduce_motion();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let opacity = if reduce {
            target
        } else {
            ease_towards(preview.opacity, target, dt / PAGELET_FADE)
        };
        if (opacity - target).abs() > 0.001 {
            ctx.request_repaint();
        }
        let sharp = self.textures.get(&page);
        let stand_in = self.thumbnail(page);
        let fade = self.fades.alpha(page, now);
        let theme = crate::theme::Theme::current(ctx);
        let mut frame = theme
            .frame(
                ctx.theme(),
                crate::theme::Surface::Dialog,
                &ctx.global_style(),
            )
            .inner_margin(PAGELET_PADDING);
        // The surface fades with its contents, so the page behind the panel
        // shows through.
        frame.fill = frame.fill.gamma_multiply(opacity);
        let page_size = preview.page_size;
        let mut close = false;
        let mut fitted = preview.page_rect;
        let mut follow = None;
        let mut resized = None;
        let mut resizing = false;
        // The page's size is the panel's: a drag keeps its aspect.
        let aspect = size.width / size.height;
        let min = (PAGELET_MIN.x - 2.0 * PAGELET_PADDING)
            .max((PAGELET_MIN.y - 2.0 * PAGELET_PADDING) * aspect);
        let bounds = ctx.content_rect().shrink2(Vec2::splat(PAGELET_PADDING));
        let window = egui::Window::new("pagelet")
            .title_bar(false)
            .resizable(false)
            .movable(true)
            .current_pos(rect.min)
            .default_size(rect.size())
            .constrain(true)
            .frame(frame)
            .show(ctx, |ui| {
                // The content is the page's size; the area takes that plus
                // its own margins.
                let page_rect = Rect::from_min_size(ui.max_rect().min, page_size);
                ui.set_min_size(page_size);
                let fit = fit_page(
                    size,
                    PageRect {
                        x: page_rect.min.x,
                        y: page_rect.min.y,
                        width: page_size.x,
                        height: page_size.y,
                    },
                );
                let page_rect =
                    Rect::from_min_size(pos2(fit.x, fit.y), vec2(fit.width, fit.height));
                fitted = page_rect;
                let fading = paint_page(
                    ui.painter(),
                    page_rect,
                    sharp,
                    stand_in,
                    fade,
                    paper.gamma_multiply(opacity),
                );
                if fading {
                    // The fade finishes only if something asks for the
                    // frames: the pointer may not move at all.
                    ui.ctx().request_repaint();
                }
                // A resize handle on every side and corner; the page's
                // aspect stands, with the opposite side or corner fixed.
                let inset = (rect.size() - page_rect.size()) / 2.0;
                for (index, (direction, hit, cursor)) in
                    resize_handles(page_rect).into_iter().enumerate()
                {
                    let response = ui.interact(
                        hit,
                        ui.scope_id().with(("pagelet-resize", index)),
                        Sense::drag(),
                    );
                    if response.hovered() || response.dragged() {
                        ui.set_cursor_icon(cursor);
                    }
                    resizing |= response.dragged();
                    if response.dragged()
                        && response.drag_delta() != Vec2::ZERO
                        && let Some(pointer) = ui.ctx().input(|i| i.pointer.interact_pos())
                    {
                        let page = resized_page(page_rect, aspect, direction, pointer, bounds, min);
                        let outer =
                            Rect::from_min_size(page.min - inset, page.size() + inset * 2.0);
                        resized = Some((outer, page.size()));
                    }
                }
                // The close chip, after the handles so a click on it wins.
                let chip = Rect::from_min_size(
                    pos2(page_rect.max.x - PAGELET_CLOSE, page_rect.min.y),
                    Vec2::splat(PAGELET_CLOSE),
                );
                let response =
                    ui.interact(chip, ui.scope_id().with("pagelet-close"), Sense::click());
                close = response.clicked();
                if response.hovered() {
                    ui.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                paint_close(ui, chip, response.hovered(), opacity);
                // The grip marks the bottom-right corner.
                paint_grip(
                    ui,
                    Rect::from_min_size(
                        page_rect.max - Vec2::splat(PAGELET_GRIP),
                        Vec2::splat(PAGELET_GRIP),
                    ),
                    opacity,
                );
                // A link under the pointer: the hand, the link lit up, and
                // a click follows it in the main view.
                let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
                let link = pointer.and_then(|at| {
                    let (x, y) = page_point(
                        size,
                        PageRect {
                            x: fit.x,
                            y: fit.y,
                            width: fit.width,
                            height: fit.height,
                        },
                        at.x,
                        at.y,
                    )?;
                    links.at(page, x, y).cloned()
                });
                if let Some(link) = link {
                    ui.set_cursor_icon(egui::CursorIcon::PointingHand);
                    // The link lights up, as one on a page does: every
                    // area, in the theme's link colour, at the panel's
                    // scale.
                    let scale = page_rect.width() / size.width;
                    let stroke = Stroke::new(LINK_OUTLINE_WIDTH, ui.visuals().hyperlink_color);
                    for area in &link.areas {
                        let min = page_rect.min + vec2(area.x, area.y) * scale;
                        let area_size = vec2(area.width, area.height) * scale;
                        ui.painter().rect_stroke(
                            Rect::from_min_size(min, area_size),
                            2.0,
                            stroke,
                            egui::StrokeKind::Outside,
                        );
                    }
                    if !close && ui.input(|i| i.pointer.primary_clicked()) {
                        follow = Some(link.target);
                    }
                }
            });
        let drawn = window.map_or(rect, |shown| shown.response.rect);
        if close {
            self.close_pagelet();
        } else if let Some(preview) = &mut self.pin_preview {
            if let Some((outer, page_size)) = resized {
                preview.rect = outer;
                preview.page_size = page_size;
                fitted =
                    Rect::from_min_size(outer.min + (outer.size() - page_size) / 2.0, page_size);
            } else {
                preview.rect = drawn;
            }
            preview.page_rect = fitted;
            preview.opacity = opacity;
            preview.resizing = resizing;
            self.pagelet = Some(Pagelet {
                x: preview.rect.min.x,
                y: preview.rect.min.y,
                width: preview.page_size.x,
                page: None,
            });
        }
        // Followed next frame, so the view's scroll applies the jump.
        if let Some(target) = follow {
            self.clicked_target = Some(target);
        }
    }

    /// The pagelet's page rect, for the tests.
    #[cfg(test)]
    fn pagelet_page_rect(&self) -> Option<Rect> {
        self.pin_preview.as_ref().map(|preview| preview.page_rect)
    }
}

/// The pagelet's eight resize handles: each side and corner, with its
/// direction (`-1`, `0` or `1` per axis), hit area and cursor.
fn resize_handles(page: Rect) -> Vec<((i8, i8), Rect, egui::CursorIcon)> {
    const EDGE: f32 = 6.0;
    const CORNER: f32 = 14.0;
    use egui::CursorIcon::{ResizeHorizontal, ResizeNeSw, ResizeNwSe, ResizeVertical};
    let (p, e, c) = (page, EDGE, CORNER);
    vec![
        (
            (1, 0),
            Rect::from_min_max(pos2(p.max.x - e, p.min.y + c), pos2(p.max.x, p.max.y - c)),
            ResizeHorizontal,
        ),
        (
            (-1, 0),
            Rect::from_min_max(pos2(p.min.x, p.min.y + c), pos2(p.min.x + e, p.max.y - c)),
            ResizeHorizontal,
        ),
        (
            (0, 1),
            Rect::from_min_max(pos2(p.min.x + c, p.max.y - e), pos2(p.max.x - c, p.max.y)),
            ResizeVertical,
        ),
        (
            (0, -1),
            Rect::from_min_max(pos2(p.min.x + c, p.min.y), pos2(p.max.x - c, p.min.y + e)),
            ResizeVertical,
        ),
        (
            (1, 1),
            Rect::from_min_max(pos2(p.max.x - c, p.max.y - c), p.max),
            ResizeNwSe,
        ),
        (
            (1, -1),
            Rect::from_min_max(pos2(p.max.x - c, p.min.y), pos2(p.max.x, p.min.y + c)),
            ResizeNeSw,
        ),
        (
            (-1, 1),
            Rect::from_min_max(pos2(p.min.x, p.max.y - c), pos2(p.min.x + c, p.max.y)),
            ResizeNeSw,
        ),
        (
            (-1, -1),
            Rect::from_min_max(p.min, pos2(p.min.x + c, p.min.y + c)),
            ResizeNwSe,
        ),
    ]
}

/// `page` after dragging `direction`'s handle to `pointer`: the opposite
/// side or corner stays, its `aspect` and `width` follow the pointer, and
/// it stays within `min` and `bounds`.
fn resized_page(
    page: Rect,
    aspect: f32,
    (dx, dy): (i8, i8),
    pointer: egui::Pos2,
    bounds: Rect,
    min: f32,
) -> Rect {
    let anchor = pos2(
        if dx > 0 {
            page.min.x
        } else if dx < 0 {
            page.max.x
        } else {
            page.min.x
        },
        if dy > 0 {
            page.min.y
        } else if dy < 0 {
            page.max.y
        } else {
            page.min.y
        },
    );
    let max = bounds.width().min(bounds.height() * aspect).max(min);
    let width = if dx != 0 && dy != 0 {
        (pointer.x - anchor.x)
            .abs()
            .max((pointer.y - anchor.y).abs() * aspect)
    } else if dx != 0 {
        (pointer.x - anchor.x).abs()
    } else {
        (pointer.y - anchor.y).abs() * aspect
    };
    let width = width.clamp(min, max);
    let height = width / aspect;
    let size = vec2(width, height);
    let min_pos = pos2(
        if dx < 0 { anchor.x - width } else { anchor.x },
        if dy < 0 { anchor.y - height } else { anchor.y },
    );
    clamp_into(Rect::from_min_size(min_pos, size), bounds)
}

/// `rect` moved inside `area`, shrunk to it if it is bigger.
fn clamp_into(rect: Rect, area: Rect) -> Rect {
    let width = rect.width().min(area.width());
    let height = rect.height().min(area.height());
    let min = pos2(
        rect.min
            .x
            .clamp(area.min.x, (area.max.x - width).max(area.min.x)),
        rect.min
            .y
            .clamp(area.min.y, (area.max.y - height).max(area.min.y)),
    );
    Rect::from_min_size(min, vec2(width, height))
}

/// `from` moved `step` towards `to`, not past it.
fn ease_towards(from: f32, to: f32, step: f32) -> f32 {
    if (to - from).abs() <= step {
        to
    } else {
        from + step * (to - from).signum()
    }
}

/// The pagelet's resize grip: the usual diagonal, on a chip.
fn paint_grip(ui: &egui::Ui, rect: Rect, opacity: f32) {
    let visuals = ui.visuals();
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let fill = visuals
        .panel_fill
        .gamma_multiply(0.85)
        .gamma_multiply(opacity);
    painter.rect_filled(rect, PAGELET_GRIP / 4.0, fill);
    let ink = visuals.weak_text_color().gamma_multiply(opacity);
    let stroke = Stroke::new(1.2, ink);
    for step in 0..3 {
        let d = 3.0 + 3.0 * step as f32;
        painter.line_segment([rect.max - vec2(d, 3.0), rect.max - vec2(3.0, d)], stroke);
    }
}

/// The pagelet's close chip: a cross on a chip, as a pin's chip is drawn.
fn paint_close(ui: &egui::Ui, rect: Rect, hovered: bool, opacity: f32) {
    let visuals = ui.visuals();
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let fill = visuals
        .panel_fill
        .gamma_multiply(if hovered { 0.95 } else { 0.85 })
        .gamma_multiply(opacity);
    painter.rect_filled(rect, PAGELET_CLOSE / 4.0, fill);
    let centre = rect.center();
    let arm = 4.0;
    let stroke = Stroke::new(1.5, visuals.weak_text_color().gamma_multiply(opacity));
    painter.line_segment([centre + vec2(-arm, -arm), centre + vec2(arm, arm)], stroke);
    painter.line_segment([centre + vec2(-arm, arm), centre + vec2(arm, -arm)], stroke);
}

impl eframe::App for NoidoApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.logic_pass(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.show(ui, Some(&*frame));
    }

    fn on_exit(&mut self) {
        self.save_now();
    }

    /// The window file holds only the window; egui's own memory isn't wanted.
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

impl NoidoApp {
    /// Carries out `action` in the window's next frame, as if from its
    /// keys: for the native menu, which acts on the window from outside.
    pub(crate) fn queue(&mut self, action: Action) {
        self.queued.push(action);
    }

    /// The actions waiting for the next frame.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> &[Action] {
        &self.queued
    }

    /// What runs every pass, also while the window is hidden or minimized
    /// and `ui` is skipped.
    pub(crate) fn logic_pass(&mut self, ctx: &egui::Context) {
        // Here, not in `show`: a minimized window must still un-minimize.
        self.come_forward(ctx);
    }

    /// One frame of the viewer.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, parent: Option<&eframe::Frame>) {
        let ctx = ui.ctx().clone();
        if let Some(live) = &mut self.theme_live
            && live.poll(&ctx)
        {
            ctx.request_repaint();
        }
        self.receive_renders(&ctx);
        let lowres = self.thumbnails.as_ref().map(|t| &t.textures);
        self.sidebar_view.receive(&ctx, lowres);
        self.sync_from_egui();
        self.receive_reload(&ctx);
        // After a reload, so an answer about the old version is dropped; and
        // after egui's scrolling is taken in, which would undo a jump.
        if let Some(named) = self.links.receive(ctx.input(|i| i.time)) {
            self.arrive_at_named(named);
        }
        // A different outline: an entry clicked in the old one means nothing.
        if self.links.take_outline_changed() {
            self.clicked_entry = None;
            self.sidebar_view.outline_changed();
        }
        self.arrive();
        self.handle_input(&ctx, parent);
        for action in std::mem::take(&mut self.queued) {
            self.apply(&ctx, parent, action);
        }
        self.start_screen_edge(&ctx);
        if let Some(link) = self.clicked_link.take() {
            self.follow(&ctx, link);
        }
        // A link from the pagelet waits a frame too, so the jump meets
        // egui's scroll offset rather than being undone by it.
        if let Some(target) = self.clicked_target.take() {
            self.follow_target(&ctx, target);
        }
        self.update_focus();
        self.update_thumbnails(&ctx);

        if !cfg!(target_os = "macos") {
            // Before the page view, so a press on an edge resizes rather than
            // starting a drag of the page.
            frame::resize_edges(&ctx);
        }
        self.title_bar(ui, parent);
        self.update_search(&ctx);
        for (page, text) in self.search_state.take_texts() {
            self.texts.insert(page, text);
        }
        if let Some(mut pending) = self.select.copy_pending {
            // Placed like the selection, so an end placed when its page's
            // text came doesn't count as a change.
            let texts = &self.texts;
            pending.resolve(|page| texts.get(page));
            if let Some(selection) = &mut self.select.selection {
                selection.resolve(|page| texts.get(page));
            }
            if self.select.selection == Some(pending) {
                self.copy_selection(&ctx);
            } else {
                self.select.copy_pending = None;
            }
        }
        self.start_drag(&ctx);
        self.edge_scroll(&ctx);
        if self.sidebar.open {
            let current = self.current_page();
            let current_entry = self.current_entry();
            let highlighted = self.highlighted();
            let paper = self.paper_tint(&ctx);
            let mut input = sidebar::Input {
                layout: self.view.layout(),
                lowres: self.thumbnails.as_ref().map(|t| &t.textures),
                current,
                highlighted,
                paper,
                results: self.search_state.searched().map(|_| sidebar::Results {
                    hits: &self.search_state.hits,
                    generation: self.search_state.generation(),
                    status: self.search_state.status(self.view.layout().page_count()),
                    numbers: &self.links.numbers,
                }),
                // Within a quarter second: wheel steps and a flick's last
                // frames come further apart than every frame.
                scrolling: self
                    .pages_scrolled_at
                    .is_some_and(|at| ctx.input(|i| i.time) - at < 0.25),
                highlights: self.highlights.items(),
                texts: &mut self.texts,
                request_text: &|page| self.search_state.request_text(page),
                numbers: &self.links.numbers,
                outline: self.links.outline.as_mut(),
                current_entry,
                pins: &self.pins,
            };
            let output = self.sidebar_view.show(ui, &mut self.sidebar, &mut input);
            if let Some(page) = output.clicked {
                self.leave_here();
                self.view.go_to_page(page);
                self.choose_page(page);
                self.search_state.reader_moved();
            }
            if let Some(entry) = output.outline_entry
                && let Some(target) = self
                    .links
                    .outline
                    .as_ref()
                    .and_then(|tree| tree.target(entry).cloned())
            {
                let internal = matches!(target, noido_engine::LinkTarget::Internal(_));
                self.follow_target(&ctx, target);
                if internal {
                    self.clicked_entry = Some(entry);
                }
            }
            if output.browsing.is_some() {
                self.browse = output.browsing;
            }
            if let Some(page) = output.toggle_pin {
                let pinned = self.pins.toggle(page);
                if !pinned
                    && self
                        .pin_preview
                        .as_ref()
                        .is_some_and(|preview| preview.page == page)
                {
                    self.close_pagelet();
                }
            }
            if let Some(page) = output.pin_clicked {
                let now = ctx.input(|i| i.time);
                let delay = ctx.options(|o| o.input_options.max_double_click_delay);
                let double = self
                    .pending_pin
                    .is_some_and(|(pending, at)| pending == page && now - at < delay);
                if double {
                    self.pending_pin = None;
                    self.leave_here();
                    self.view.go_to_page(page);
                    self.choose_page(page);
                    self.search_state.reader_moved();
                } else {
                    self.pending_pin = Some((page, now));
                }
            }
            if let Some(index) = output.hit {
                self.go_to_hit(index);
            }
        } else {
            self.sidebar_view.hidden();
        }

        // A pagelet restored open waits for the view's rect, then shows.
        if let Some(page) = self.pending_pagelet.take()
            && page < self.view.layout().page_count()
            && self.pins.contains(page)
        {
            let view = self
                .last_scroll
                .map_or_else(|| ctx.content_rect(), |last| last.inner);
            let bounds = Rect::from_min_max(pos2(ctx.content_rect().min.x, view.min.y), view.max);
            let pointer = ctx.input(|i| i.pointer.hover_pos());
            self.open_pagelet(page, view, bounds, pointer);
        }

        // A pin click waits out egui's double-click window before it
        // previews, so the first click of a double click opens nothing.
        if let Some((page, at)) = self.pending_pin {
            let delay = ctx.options(|o| o.input_options.max_double_click_delay);
            let now = ctx.input(|i| i.time);
            if page >= self.view.layout().page_count() {
                self.pending_pin = None;
            } else if now - at >= delay {
                self.pending_pin = None;
                let view = self
                    .last_scroll
                    .map_or_else(|| ctx.content_rect(), |last| last.inner);
                let bounds =
                    Rect::from_min_max(pos2(ctx.content_rect().min.x, view.min.y), view.max);
                let pointer = ctx.input(|i| i.pointer.hover_pos());
                self.open_pagelet(page, view, bounds, pointer);
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(delay - (now - at)));
            }
        }

        // The page area changes size this frame when the sidebar opens or
        // closes; lay the pages out for it now, not a frame late.
        let mut frame = crate::theme::Theme::current(&ctx).frame(
            ctx.theme(),
            crate::theme::Surface::Central,
            ui.style(),
        );
        let bar = crate::theme::solid_bar(ui);
        if cfg!(target_os = "macos") && bar.is_some() {
            // Solid bars meet the window's edge, as AppKit's do. Not
            // elsewhere: noido's resize edges there would take presses on
            // a bar's outer side.
            frame = frame.inner_margin(0);
        }
        let area = ui.available_rect_before_wrap() - frame.total_margin();
        // Solid bars take room from the pages: lay them out beside the bars
        // they need there.
        let solid = bar.map(|bar| {
            let outer = (area.width(), area.height());
            let bars = noido_core::scroll_bars(outer, bar, |(width, height)| {
                self.view.content_size_at(width, height)
            });
            (bars, bars.inner(outer, bar))
        });
        if self.last_scroll.is_some() {
            let (width, height) = solid.map_or((area.width(), area.height()), |(_, room)| room);
            self.view.set_viewport_size(width, height);
            self.settle_shrink(&ctx);
        }

        let (x, y) = self.view.offset();
        let wanted = vec2(x, y);
        let move_scroll = self
            .last_scroll
            .is_some_and(|last| (last.offset - wanted).abs().max_elem() > OFFSET_EPSILON);

        frame.show(ui, |ui| {
            // egui fades the edges of scroll areas; on pages it reads as a shadow.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            // Left drag pans the document, unless it selects.
            let panning = self.select.drag.is_none_or(|d| d.kind == DragKind::Pan);
            let source = ScrollSource {
                drag: if panning {
                    DragScroll::Always
                } else {
                    DragScroll::Never
                },
                ..Default::default()
            };
            // No overscroll at the ends; it makes zoomed-in pages harder to
            // position. (It also keeps offsets noido sets from springing back.)
            ui.spacing_mut().scroll.kinetic.rubber_band = false;
            let mut scroll_area = ScrollArea::both().auto_shrink(false).scroll_source(source);
            if let Some((bars, _)) = solid {
                let last = self.last_scroll.map(|last| last.id);
                scroll_area = crate::theme::with_solid_bars(ui.ctx(), scroll_area, bars, last);
            }
            if move_scroll {
                scroll_area = scroll_area.scroll_offset(wanted);
            }
            // A hand where a drag pans (not over the scroll bars), unless a
            // resize edge or the sidebar's handle set a cursor already.
            let cursor_free = ui.ctx().output(|o| o.cursor_icon) == egui::CursorIcon::Default;
            if cursor_free {
                scroll_area = scroll_area
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_drag_cursor(egui::CursorIcon::Grabbing);
            }
            let output = crate::theme::scope(ui, crate::theme::Role::Scrollbar, |ui| {
                if let Some((bars, _)) = solid {
                    crate::theme::paint_grooves(ui, ui.available_rect_before_wrap(), bars);
                }
                let shown = scroll_area.show_viewport(ui, |ui, viewport| self.pages(ui, viewport));
                if let Some((bars, _)) = solid {
                    crate::theme::paint_thumbs(ui, &shown, bars);
                }
                shown
            });
            // The pointing hand over a link, unless a drag is under way.
            if cursor_free
                && self.hovered_link.is_some()
                && !ui.ctx().input(|i| i.pointer.is_decidedly_dragging())
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            // The I-beam where a drag selects: over text, or while selecting.
            let shift = ui.ctx().input(|i| i.modifiers.shift);
            let selecting = match self.select.drag {
                Some(drag) => drag.kind == DragKind::Select,
                None => {
                    self.select.over_text
                        && self.hovered_link.is_none()
                        && drag_kind(self.select.tool, shift, false) == DragKind::Select
                }
            };
            if cursor_free && selecting {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            // The arrow over a handle, where a press takes it.
            let on_handle = self.select.drag.is_none()
                && self.select.pages_hovered
                && !egui::Popup::is_any_open(ui.ctx())
                && ui
                    .ctx()
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|pos| self.select.handle_at(pos).is_some());
            if cursor_free && on_handle {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }

            let moved = self.last_scroll.is_some_and(|last| {
                (last.offset - output.state.clamped_offset())
                    .abs()
                    .max_elem()
                    > OFFSET_EPSILON
            });
            if moved && !move_scroll {
                self.pages_scrolled_at = Some(ui.ctx().input(|i| i.time));
            }
            let size = self.last_scroll.map(|last| last.inner.size());
            self.told = size.map(|size| (wanted, size));
            self.last_scroll = Some(ScrollState {
                id: output.id,
                inner: output.inner_rect,
                offset: output.state.clamped_offset(),
            });
        });
        self.pagelet(&ctx);
        self.selection_bar(&ctx, parent);
        if !cfg!(target_os = "macos") {
            frame::border(&ctx);
        }
        self.settings_page(&ctx);
        self.info_page(&ctx);
        self.persist(&ctx);
        self.select.widget_focused = ctx.memory(|m| m.focused().is_some());
    }

    /// Shows the document info page while it is open.
    fn info_page(&mut self, ctx: &egui::Context) {
        let (Some(page), Some(source)) = (&mut self.info_page, &self.source) else {
            return;
        };
        let layout = self.view.layout();
        let sizes: Vec<noido_engine::Size> = (0..layout.page_count())
            .filter_map(|page| layout.page_size(page))
            .collect();
        if info::show(page, ctx, source, &sizes) {
            self.info_page = None;
        }
    }

    /// The paper the document is shown on: the chosen one, or the one
    /// whose square on the settings page is hovered.
    fn paper(&self) -> noido_core::Paper {
        self.settings_page
            .as_ref()
            .and_then(settings::Page::preview)
            .unwrap_or(self.settings.paper)
    }

    /// The tint pages are drawn with: the colour of [`Self::paper`], faded
    /// to from the last one.
    fn paper_tint(&self, ctx: &egui::Context) -> Color32 {
        let id = egui::Id::unique("paper tint");
        let channel = |i: usize, value: u8| {
            ctx.animate_value_with_time(id.with(i), value.into(), settings::FADE)
                .round() as u8
        };
        let [r, g, b] = self.paper().rgb();
        Color32::from_rgb(channel(0, r), channel(1, g), channel(2, b))
    }

    /// Shows the settings page while it is open, saving each change.
    fn settings_page(&mut self, ctx: &egui::Context) {
        let Some(page) = &mut self.settings_page else {
            return;
        };
        let current = self
            .view
            .state()
            .map(|state| Defaults::from_view(&state, self.sidebar.open));
        let output = settings::show(page, ctx, &mut self.settings, current);
        if output.changed
            && let Some(file) = &self.settings_file
        {
            let saved = file.save(&self.settings);
            if saved.is_ok()
                && let Some(requests) = &self.requests
            {
                requests.send(crate::shell::Request::SettingsSaved(self.settings));
            }
            page.error = saved.err().map(|e| format!("Couldn't save settings: {e}"));
        }
        if output.close {
            self.settings_page = None;
        }
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
        && a.odd_left == b.odd_left
        && a.gapless == b.gapless
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
        egui::Key::Home => Some(Key::Home),
        egui::Key::End => Some(Key::End),
        egui::Key::Plus => Some(Key::Plus),
        egui::Key::Equals => Some(Key::Equals),
        egui::Key::Minus => Some(Key::Minus),
        egui::Key::W => Some(Key::W),
        egui::Key::F => Some(Key::F),
        egui::Key::C => Some(Key::C),
        egui::Key::D => Some(Key::D),
        egui::Key::O => Some(Key::O),
        egui::Key::Q => Some(Key::Q),
        egui::Key::G => Some(Key::G),
        egui::Key::S => Some(Key::S),
        egui::Key::Z => Some(Key::Z),
        egui::Key::Y => Some(Key::Y),
        egui::Key::I => Some(Key::I),
        egui::Key::L => Some(Key::L),
        egui::Key::Comma => Some(Key::Comma),
        egui::Key::F3 => Some(Key::F3),
        egui::Key::F9 => Some(Key::F9),
        egui::Key::ArrowUp => Some(Key::ArrowUp),
        egui::Key::ArrowDown => Some(Key::ArrowDown),
        egui::Key::ArrowLeft => Some(Key::ArrowLeft),
        egui::Key::ArrowRight => Some(Key::ArrowRight),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use egui::{Event, Modifiers, RawInput, pos2};
    use noido_engine::{Document, EngineError, PdfBytes, PdfEngine, PixelRect, Rgba8Image, Size};

    use noido_core::{Caret, Highlight, HighlightColour, SidebarState, ViewState};
    use noido_render::{TextSearcher, Thumbnailer};
    use noido_settings::{Settings, SettingsFile};
    use noido_state::{DocKey, DocState, Store};

    use super::*;
    use crate::image::paper_colour;
    use crate::search;

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

    /// Like [`BlankEngine`], with no pages at all.
    struct EmptyEngine;
    struct EmptyDocument;

    impl PdfEngine for EmptyEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(EmptyDocument))
        }
    }

    impl Document for EmptyDocument {
        fn page_count(&self) -> usize {
            0
        }
        fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 0.0,
                height: 0.0,
            })
        }
        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Ok(Rgba8Image::from_premultiplied(1, 1, vec![255; 4]).unwrap())
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

    /// An app on [`BlankEngine`], for tests outside this module.
    pub(crate) fn blank_app() -> NoidoApp {
        let (renderer, info) =
            Renderer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {})).unwrap();
        NoidoApp::new(renderer, info)
    }

    /// The settings `app` shows, for tests outside this module.
    pub(crate) fn settings_of(app: &NoidoApp) -> Settings {
        app.settings
    }

    /// The recent files `app`'s menu offers, for tests outside this
    /// module.
    pub(crate) fn recent_of(app: &NoidoApp) -> &[PathBuf] {
        &app.recent
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
        /// The pointer's cursor after the last frame.
        cursor: egui::CursorIcon,
        /// The URLs the app asked to open.
        opened: Vec<String>,
        /// The text the app put on the clipboard.
        copied: Vec<String>,
        /// Whether the window is reported as maximised.
        maximized: bool,
        /// Whether the window is reported as full screen.
        fullscreen: bool,
        /// The size of the screen the window is on, as egui reports it.
        monitor: Option<Vec2>,
        /// Hands the app new versions of the document, like the watcher.
        reloads: crate::watch::Newest,
        /// When the last frame asked for the next one.
        repaint_delay: Duration,
        /// The modifiers held; a change is sent with the next frame.
        modifiers: Modifiers,
        /// The modifiers the app was last told of.
        sent_modifiers: Modifiers,
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
            Self::build_themed(engine, "adwaita", setup)
        }

        /// A harness with the theme `preset` from its first frame.
        fn build_themed(
            engine: Arc<dyn PdfEngine>,
            preset: &str,
            setup: impl FnOnce(NoidoApp) -> NoidoApp,
        ) -> Self {
            let (renderer, info) =
                Renderer::start(engine, Arc::new(Vec::new()), Box::new(|| {})).unwrap();
            let ctx = egui::Context::default();
            crate::theme::install_preset(&ctx, preset);
            ctx.set_theme(egui::ThemePreference::Dark);
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
                cursor: egui::CursorIcon::Default,
                opened: vec![],
                copied: vec![],
                maximized: false,
                fullscreen: false,
                monitor: None,
                reloads,
                repaint_delay: Duration::MAX,
                modifiers: Modifiers::NONE,
                sent_modifiers: Modifiers::NONE,
            };
            harness.frames(3, vec![]);
            harness
        }

        /// The harness with a theme whose scroll bars are solid
        /// (windows-11: a 17-point groove), as macOS with "Show scroll
        /// bars: Always".
        fn solid_bars(mut self) -> Self {
            crate::theme::install_preset(&self.ctx, "windows-11");
            self.ctx.set_theme(egui::ThemePreference::Dark);
            self.frames(3, vec![]);
            self
        }

        /// The harness with the OS asking for reduced motion.
        fn reduced_motion(mut self) -> Self {
            let theme = crate::theme::Theme::current(&self.ctx);
            (*theme).clone().with_reduce_motion(true).install(&self.ctx);
            self.frames(3, vec![]);
            self
        }

        fn frames(&mut self, count: usize, mut events: Vec<Event>) {
            for _ in 0..count {
                if self.modifiers != self.sent_modifiers {
                    self.sent_modifiers = self.modifiers;
                    events.insert(0, Event::ModifiersChanged(self.modifiers));
                }
                let mut viewports = egui::ViewportIdMap::default();
                viewports.insert(
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        maximized: Some(self.maximized),
                        fullscreen: Some(self.fullscreen),
                        monitor_size: self.monitor,
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
                let output = self.ctx.run_ui(input, |ui| {
                    app.logic_pass(ui.ctx());
                    app.show(ui, None)
                });
                self.shapes = output.shapes.clone();
                if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                    self.commands.extend(viewport.commands.iter().cloned());
                    self.repaint_delay = viewport.repaint_delay;
                    // What a window does with them, so a resize is measured
                    // by the next frame.
                    for command in &viewport.commands {
                        match command {
                            egui::ViewportCommand::InnerSize(size) => self.size = *size,
                            egui::ViewportCommand::Maximized(maximized) => {
                                self.maximized = *maximized;
                            }
                            egui::ViewportCommand::Fullscreen(fullscreen) => {
                                self.fullscreen = *fullscreen;
                            }
                            _ => {}
                        }
                    }
                }
                self.cursor = output.platform_output.cursor_icon;
                for command in &output.platform_output.commands {
                    match command {
                        egui::OutputCommand::OpenUrl(open) => self.opened.push(open.url.clone()),
                        egui::OutputCommand::CopyText(text) => self.copied.push(text.clone()),
                        _ => {}
                    }
                }
                output.drop_without_applying_deltas();
            }
        }

        /// Hands the app a new version of the document, opened with `engine`
        /// and identified by `key`, and runs a frame.
        fn reload(&mut self, engine: Arc<dyn PdfEngine>, key: DocKey) {
            let bytes = Arc::new(b"new version".to_vec());
            let (renderer, info) =
                Renderer::start(engine.clone(), bytes.clone(), Box::new(|| {})).unwrap();
            let thumbnailer = Thumbnailer::start(engine.clone(), bytes.clone(), Box::new(|| {}));
            let sidebar =
                Renderer::start_background(engine.clone(), bytes.clone(), Box::new(|| {}));
            let linker =
                noido_render::Linker::start(engine.clone(), bytes.clone(), Box::new(|| {}));
            let searcher = TextSearcher::start(engine, bytes.clone(), Box::new(|| {}));
            let reload = crate::watch::Reload {
                renderer,
                thumbnailer,
                sidebar,
                searcher,
                linker,
                info,
                key,
                bytes,
            };
            self.reloads.put(reload);
            self.frames(1, vec![]);
        }

        /// Like [`Harness::reload`], with the sidebar's renders from `sidebar`.
        fn reload_with_sidebar(
            &mut self,
            engine: Arc<dyn PdfEngine>,
            sidebar: RecordingEngine,
            key: DocKey,
        ) {
            let bytes = Arc::new(Vec::new());
            let (renderer, info) =
                Renderer::start(engine.clone(), bytes.clone(), Box::new(|| {})).unwrap();
            let thumbnailer = Thumbnailer::start(engine.clone(), bytes.clone(), Box::new(|| {}));
            let sidebar =
                Renderer::start_background(Arc::new(sidebar), bytes.clone(), Box::new(|| {}));
            let linker =
                noido_render::Linker::start(engine.clone(), bytes.clone(), Box::new(|| {}));
            let searcher = TextSearcher::start(engine, bytes.clone(), Box::new(|| {}));
            self.reloads.put(crate::watch::Reload {
                renderer,
                thumbnailer,
                sidebar,
                searcher,
                linker,
                info,
                key,
                bytes,
            });
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
            let modifiers = self.modifiers;
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
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

        /// Every text drawn last frame, with where it was drawn.
        fn latest_texts(&self) -> Vec<(String, Rect)> {
            fn collect(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Text(t) => out.push((
                        t.galley.text().to_string(),
                        Rect::from_min_size(t.pos, t.galley.size()),
                    )),
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            collect(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut texts = Vec::new();
            for shape in &self.shapes {
                collect(&shape.shape, &mut texts);
            }
            texts
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

        /// The theme installed in the harness.
        fn theme(&self) -> Arc<crate::theme::Theme> {
            crate::theme::Theme::current(&self.ctx)
        }

        /// The colour of the last frame's text exactly equal to `text`.
        fn text_color(&self, text: &str) -> Option<Color32> {
            fn find(shape: &egui::Shape, text: &str) -> Option<Color32> {
                match shape {
                    egui::Shape::Text(t) if t.galley.text() == text => {
                        let span = t
                            .galley
                            .job
                            .sections
                            .first()
                            .map(|section| section.format.color)
                            .filter(|colour| *colour != Color32::PLACEHOLDER);
                        Some(span.or(t.override_text_color).unwrap_or(t.fallback_color))
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                    _ => None,
                }
            }
            self.shapes.iter().find_map(|c| find(&c.shape, text))
        }

        /// Every piece of text the last frame painted.
        fn texts(&self) -> Vec<String> {
            fn collect(shape: &egui::Shape, into: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => into.push(t.galley.text().to_owned()),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, into)),
                    _ => {}
                }
            }
            let mut texts = Vec::new();
            self.shapes
                .iter()
                .for_each(|c| collect(&c.shape, &mut texts));
            texts
        }

        /// Clicks the middle of `text`.
        fn click_text(&mut self, text: &str) {
            let rect = self
                .text_rect(text)
                .unwrap_or_else(|| panic!("{text:?} not drawn"));
            self.click(rect.center());
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
    fn shift_f_fits_the_window_to_the_page() {
        let mut h = Harness::new();
        // Zoomed out, so the page is smaller than the window.
        for _ in 0..2 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.frames(2, vec![]);
        let scale = h.app.view.scale();
        let (width, height) = h.app.view.spread_viewport_size().unwrap();
        assert!(width < h.size.x);

        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(8, vec![]);

        let (area_width, area_height) = h.app.view.viewport_size();
        eprintln!(
            "after: window {:?} area {:?} page {width}",
            h.size,
            h.app.view.viewport_size()
        );
        assert!((area_width - width).abs() < 1.0, "{area_width} for {width}");
        assert!(
            (area_height - height).abs() < 1.0,
            "{area_height} for {height}"
        );
        assert!(h.size.x < 900.0, "the window is {} wide", h.size.x);
        assert!(h.size.y < 1000.0, "the window is {} high", h.size.y);
        // The zoom is held, not refitted.
        assert_eq!(h.app.view.scale(), scale);
        assert_eq!(h.app.view.mode(), noido_core::ZoomMode::Custom);
    }

    #[test]
    fn shift_w_fits_the_width_and_leaves_the_height() {
        let mut h = Harness::new();
        for _ in 0..2 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.frames(2, vec![]);
        let (width, _) = h.app.view.spread_viewport_size().unwrap();
        let height = h.app.view.viewport_size().1;

        h.press(egui::Key::W, Modifiers::SHIFT);
        h.frames(8, vec![]);

        let (area_width, area_height) = h.app.view.viewport_size();
        assert!((area_width - width).abs() < 1.0, "{area_width} for {width}");
        assert!(
            (area_height - height).abs() < 1.0,
            "{area_height} for {height}"
        );
        assert!(h.size.x < 900.0, "the window is {} wide", h.size.x);
        assert!((h.size.y - 1000.0).abs() < 1.0, "window {}", h.size.y);
    }

    #[test]
    fn fitting_the_window_keeps_the_sidebar_its_width() {
        let mut h = Harness::new();
        h.press(egui::Key::F9, Modifiers::NONE);
        for _ in 0..6 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.frames(3, vec![]);
        let width = h.app.sidebar.width;

        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(10, vec![]);

        assert_eq!(h.app.sidebar.width, width);
    }

    #[test]
    fn fitting_the_window_restores_it_first() {
        let mut h = Harness::new();
        h.maximized = true;
        h.frames(1, vec![]);
        h.press(egui::Key::F, Modifiers::SHIFT);
        assert!(
            h.commands
                .contains(&egui::ViewportCommand::Maximized(false))
        );
        let sized = |h: &Harness| {
            h.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_)))
        };
        assert!(!sized(&h), "a maximized window takes no size");
        h.frames(8, vec![]);
        assert!(sized(&h));
    }

    #[test]
    fn fitting_the_window_leaves_fit_width_working() {
        let mut h = Harness::new();
        for _ in 0..2 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(8, vec![]);
        h.press(egui::Key::W, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.view.mode(), noido_core::ZoomMode::FitWidth);
        assert!(h.app.view.layout().content_width() <= h.app.view.viewport_size().0 + 0.5);
    }

    #[test]
    fn fitting_the_window_settles_and_stays() {
        let mut h = Harness::new().solid_bars();
        h.press(egui::Key::C, Modifiers::NONE);
        h.frames(2, vec![]);
        let (width, height) = h.app.view.spread_viewport_size().unwrap();

        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(8, vec![]);

        let (area_width, area_height) = h.app.view.viewport_size();
        assert!((area_width - width).abs() < 1.0, "{area_width} for {width}");
        assert!(
            (area_height - height).abs() < 1.0,
            "{area_height} for {height}"
        );
        // The bar a solid theme draws comes along and isn't chased.
        let asked = h.commands.len();
        h.frames(4, vec![]);
        assert_eq!(h.commands.len(), asked, "the window settled");
        // Asking again for the size it has asks for nothing.
        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(4, vec![]);
        assert_eq!(h.commands.len(), asked, "nothing to resize");
    }

    #[test]
    fn fitting_the_window_before_it_is_laid_out_asks_nothing() {
        let mut h = Harness::new();
        // As on the first frame, when the window has given the pages no room.
        h.app.view = View::new(vec![noido_engine::Size {
            width: 612.0,
            height: 792.0,
        }]);
        let ctx = h.ctx.clone();
        for action in [Action::FitWindowWidth, Action::FitWindowPage] {
            h.app.apply(&ctx, None, action);
            assert_eq!(h.app.shrink, None, "{action:?}");
        }
    }

    #[test]
    fn fitting_the_window_asks_once_for_a_restore() {
        let mut h = Harness::new();
        h.press(egui::Key::F, Modifiers::SHIFT);
        // A window manager that keeps the window maximized.
        for _ in 0..6 {
            h.maximized = true;
            h.frames(1, vec![]);
        }
        let asked = h
            .commands
            .iter()
            .filter(|command| **command == egui::ViewportCommand::Maximized(false))
            .count();
        assert_eq!(asked, 1, "asked {asked} times");
        assert_eq!(h.app.shrink, None, "the request is dropped");
    }

    #[test]
    fn fitting_the_window_stops_at_the_screen() {
        let mut h = Harness::new();
        let screen = vec2(700.0, 600.0);
        h.monitor = Some(screen);
        h.size = vec2(500.0, 400.0);
        h.frames(2, vec![]);
        // Zoomed in until the page is bigger than the screen both ways.
        for _ in 0..6 {
            h.press(egui::Key::Equals, Modifiers::COMMAND);
        }
        h.frames(2, vec![]);
        let (page_width, _) = h.app.view.spread_viewport_size().unwrap();
        assert!(page_width > screen.x, "the page is {page_width} wide");

        h.press(egui::Key::F, Modifiers::SHIFT);
        h.frames(8, vec![]);

        assert!(
            (h.size.x - screen.x).abs() < 1.0,
            "the window is {} wide",
            h.size.x
        );
        assert!(
            (h.size.y - screen.y).abs() < 1.0,
            "the window is {} high",
            h.size.y
        );
        let asked = h.commands.len();
        h.frames(4, vec![]);
        assert_eq!(h.commands.len(), asked, "the window settled");
    }

    #[test]
    fn fitting_the_window_restores_a_fullscreen_window_first() {
        let mut h = Harness::new();
        h.fullscreen = true;
        h.frames(1, vec![]);
        h.press(egui::Key::F, Modifiers::SHIFT);
        let restores = |h: &Harness| {
            h.commands
                .iter()
                .filter(|command| **command == egui::ViewportCommand::Fullscreen(false))
                .count()
        };
        let sized = |h: &Harness| {
            h.commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::InnerSize(_)))
        };
        assert_eq!(restores(&h), 1);
        assert!(!sized(&h), "a fullscreen window takes no size");
        h.frames(8, vec![]);
        assert!(sized(&h));
    }

    #[test]
    fn fitting_the_window_asks_once_for_a_fullscreen_restore() {
        let mut h = Harness::new();
        h.press(egui::Key::F, Modifiers::SHIFT);
        // A window manager that keeps the window full screen.
        for _ in 0..6 {
            h.fullscreen = true;
            h.frames(1, vec![]);
        }
        let asked = h
            .commands
            .iter()
            .filter(|command| **command == egui::ViewportCommand::Fullscreen(false))
            .count();
        assert_eq!(asked, 1, "asked {asked} times");
        assert_eq!(h.app.shrink, None, "the request is dropped");
    }

    #[test]
    fn fitting_the_window_twice_before_it_resizes_asks_once() {
        let mut h = Harness::new();
        for _ in 0..2 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.frames(2, vec![]);
        let (width, _) = h.app.view.spread_viewport_size().unwrap();
        let height = h.app.view.viewport_size().1;

        // Both presses in the frame that resizes: the second replaces the
        // request the first has not settled yet.
        let shift = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        };
        h.frames(1, vec![shift(egui::Key::F), shift(egui::Key::W)]);
        h.frames(8, vec![]);

        let (area_width, area_height) = h.app.view.viewport_size();
        assert!((area_width - width).abs() < 1.0, "{area_width} for {width}");
        // The width press keeps the height the page area had: the last press
        // is the one that counts.
        assert!(
            (area_height - height).abs() < 1.0,
            "{area_height} for {height}"
        );
        // One ask for the replacement, then it settles.
        let sized = |h: &Harness| {
            h.commands
                .iter()
                .filter(|command| matches!(command, egui::ViewportCommand::InnerSize(_)))
                .count()
        };
        assert_eq!(sized(&h), 1, "asked {} times", sized(&h));
        let asked = h.commands.len();
        h.frames(4, vec![]);
        assert_eq!(h.commands.len(), asked, "the window settled");
    }

    #[test]
    fn fitting_the_window_with_no_pages_does_nothing() {
        let mut h = Harness::build_with(Arc::new(EmptyEngine), |app| app);
        let size = h.size;
        let mode = h.app.view.mode();
        let scale = h.app.view.scale();
        let ctx = h.ctx.clone();
        for action in [Action::FitWindowWidth, Action::FitWindowPage] {
            h.app.apply(&ctx, None, action);
            assert_eq!(h.app.shrink, None, "{action:?}");
        }
        h.frames(6, vec![]);

        assert_eq!(h.size, size, "the window moved");
        assert_eq!(
            h.commands
                .iter()
                .filter(|command| matches!(command, egui::ViewportCommand::InnerSize(_)))
                .count(),
            0,
            "asked for a size"
        );
        assert_eq!(h.app.view.mode(), mode);
        assert_eq!(h.app.view.scale(), scale);
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

    /// The screen edge lines drawn last frame: (left, right, y) each.
    fn edge_lines(h: &Harness) -> Vec<(f32, f32, f32)> {
        fn find(shape: &egui::Shape, out: &mut Vec<(f32, f32, f32)>) {
            match shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.width == FLASH_OUTLINE_WIDTH =>
                {
                    out.push((points[0].x, points[1].x, points[0].y));
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.shapes {
            find(&clipped.shape, &mut out);
        }
        out
    }

    fn key_event(key: egui::Key, pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn page_down_flashes_old_bottom_edge_once_released() {
        let mut h = Harness::new();
        let (_, top) = h.app.view.offset();
        let height = h.app.view.viewport_size().1;
        let edge = h.app.view.layout().doc_point(0.0, top + height).unwrap();
        h.frames(1, vec![key_event(egui::Key::PageDown, true, false)]);
        h.frames(2, vec![]);
        assert!(edge_lines(&h).is_empty(), "not while held");

        h.frames(1, vec![key_event(egui::Key::PageDown, false, false)]);
        let lines = edge_lines(&h);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let page = h.app.view.layout().page_rect(edge.page).unwrap();
        let at = screen_pos(&h, edge.page, 0.0, edge.y);
        let left = screen_pos(&h, edge.page, 0.0, 0.0).x;
        assert!((lines[0].2 - at.y).abs() < 1.0, "{lines:?} vs {at:?}");
        assert!((lines[0].0 - left).abs() < 1.0);
        assert!((lines[0].1 - (left + page.width)).abs() < 1.0);

        h.frames(70, vec![]);
        assert!(edge_lines(&h).is_empty());
    }

    #[test]
    fn held_page_down_flashes_nothing() {
        let mut h = Harness::new();
        h.frames(1, vec![key_event(egui::Key::PageDown, true, false)]);
        h.frames(1, vec![key_event(egui::Key::PageDown, true, true)]);
        h.frames(1, vec![key_event(egui::Key::PageDown, false, false)]);
        h.frames(2, vec![]);
        assert!(edge_lines(&h).is_empty());

        // A tap after it flashes again.
        h.frames(1, vec![key_event(egui::Key::PageDown, true, false)]);
        h.frames(1, vec![key_event(egui::Key::PageDown, false, false)]);
        assert_eq!(edge_lines(&h).len(), 1);
    }

    #[test]
    fn paged_flip_flashes_no_edge() {
        let mut h = Harness::new();
        h.press(egui::Key::C, Modifiers::NONE);
        for _ in 0..5 {
            // Each scroll's flash fades before the next press.
            h.frames(70, vec![]);
            h.frames(1, vec![key_event(egui::Key::PageDown, true, false)]);
            h.frames(1, vec![key_event(egui::Key::PageDown, false, false)]);
            if h.app.view.current_spread() == Some(1) {
                break;
            }
        }
        assert_eq!(h.app.view.current_spread(), Some(1));
        assert!(edge_lines(&h).is_empty());
    }

    #[test]
    fn arrow_down_scrolls_a_step() {
        let mut h = Harness::new();
        h.frames(2, vec![]);
        h.press(egui::Key::ArrowDown, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.view.offset().1, noido_core::SCROLL_STEP);
    }

    #[test]
    fn arrows_move_the_cursor_while_searching() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.press(egui::Key::ArrowDown, Modifiers::NONE);
        h.press(egui::Key::ArrowRight, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.view.offset(), (0.0, 0.0));
    }

    #[test]
    fn arrows_leave_the_view_while_menu_is_open() {
        let mut h = Harness::new();
        h.click(h.app.bar.menu.center());
        h.frames(2, vec![]);
        assert!(egui::Popup::is_any_open(&h.ctx), "menu not open");
        h.press(egui::Key::ArrowDown, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.view.offset(), (0.0, 0.0));
    }

    #[test]
    fn arrows_leave_the_view_while_a_widget_has_focus() {
        let mut h = Harness::new();
        h.press(egui::Key::Tab, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.ctx.memory(|m| m.focused().is_some()), "nothing focused");
        h.press(egui::Key::ArrowDown, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.view.offset(), (0.0, 0.0));
    }

    #[test]
    fn arrow_that_moves_nothing_keeps_place_at_hit() {
        // Continuous fit width: left and right have nowhere to go.
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.press(egui::Key::G, Modifiers::COMMAND);
        // Out of the search field.
        h.click(h.app.last_scroll.unwrap().inner.center());
        h.frames(2, vec![]);
        assert!(!h.ctx.text_edit_focused());
        assert!(h.app.search_state.at_hit);
        h.press(egui::Key::ArrowRight, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.search_state.at_hit);
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

    #[test]
    fn o_puts_page_one_alone_on_the_right() {
        let mut h = Harness::new();
        h.press(egui::Key::D, Modifiers::NONE);
        h.press(egui::Key::O, Modifiers::NONE);
        assert!(!h.app.view.odd_left());
        let layout = h.app.view.layout();
        assert_eq!(layout.spread_pages(0), 0..1);
        assert_eq!(
            layout.page_rect(0).unwrap().x,
            layout.page_rect(2).unwrap().x
        );
        assert!(!h.app.view.state().unwrap().odd_left);
        h.press(egui::Key::O, Modifiers::NONE);
        assert_eq!(h.app.view.layout().spread_pages(0), 0..2);
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
            odd_left: true,
            gapless: false,
            centre: noido_core::DocPoint {
                page: 6,
                x: 306.0,
                y: 400.0,
            },
        };
        let restored = DocState {
            view: state,
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::default(),
            pagelet: None,
        };
        let h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        assert_eq!(h.centre().page, 6);
    }

    fn with_new_documents(defaults: noido_core::Defaults) -> noido_settings::Settings {
        noido_settings::Settings {
            new_documents: defaults,
            ..noido_settings::Settings::default()
        }
    }

    #[test]
    fn new_document_opens_with_settings() {
        let settings = with_new_documents(noido_core::Defaults {
            continuous: false,
            dual: true,
            ..Default::default()
        });
        let h = Harness::build(|app| app.with_settings(None, settings));
        assert!(!h.app.view.continuous());
        assert!(h.app.view.dual());
        assert_eq!(h.app.view.current_spread(), Some(0));
    }

    #[test]
    fn new_document_opens_with_sidebar_setting() {
        let settings = with_new_documents(noido_core::Defaults {
            sidebar: true,
            ..Default::default()
        });
        let h = Harness::build(|app| app.with_settings(None, settings));
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn saved_sidebar_beats_settings() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = with_new_documents(noido_core::Defaults {
            sidebar: true,
            ..Default::default()
        });
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::default(),
            pagelet: None,
        };
        let h = Harness::build(|app| {
            app.with_settings(None, settings).with_persistence(
                Store::at(dir.path()),
                test_slot(dir.path()),
                Some(restored),
            )
        });
        assert!(!h.app.sidebar.open);
    }

    #[test]
    fn fixed_zoom_setting_opens_at_that_scale() {
        let settings = with_new_documents(noido_core::Defaults {
            zoom: noido_core::StartZoom::Fixed,
            zoom_percent: 150,
            ..Default::default()
        });
        let h = Harness::build(|app| app.with_settings(None, settings));
        assert_eq!(h.app.view.scale(), 1.5);
        assert_eq!(h.app.view.offset().1, 0.0);
    }

    #[test]
    fn saved_state_beats_settings() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = with_new_documents(noido_core::Defaults {
            zoom: noido_core::StartZoom::Fixed,
            zoom_percent: 150,
            continuous: false,
            dual: true,
            odd_left: false,
            gapless: false,
            sidebar: true,
        });
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 2,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::default(),
            pagelet: None,
        };
        let h = Harness::build(|app| {
            app.with_settings(None, settings).with_persistence(
                Store::at(dir.path()),
                test_slot(dir.path()),
                Some(restored),
            )
        });
        let view = &h.app.view;
        assert_eq!(view.mode(), noido_core::ZoomMode::FitWidth);
        assert!(view.continuous() && !view.dual() && view.odd_left());
        assert_eq!(h.centre().page, 2);
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

    #[test]
    fn toggling_sidebar_draws_pages_in_place_with_solid_bars() {
        sidebar_toggle_draws_in_place(Harness::new().solid_bars());
    }

    /// A Mac starts with solid bars, not after a theme change: from the
    /// first frames the pages are laid out beside the bar, the thumbnails
    /// too once the sidebar opens.
    #[test]
    fn solid_bars_from_the_first_frame() {
        let mut h = Harness::build_themed(Arc::new(BlankEngine), "windows-11", |app| app);
        let inner = h.app.last_scroll.unwrap().inner;
        assert_eq!(h.app.view.viewport_size(), (inner.width(), inner.height()));
        assert!(h.app.view.layout().content_width() <= inner.width() + 0.5);

        h.press(egui::Key::F9, Modifiers::NONE);
        let bar_left = h.app.sidebar.width - 1.0 - 17.0;
        let drawn = h.app.sidebar_view.drawn();
        assert!(!drawn.is_empty());
        for (page, rect) in drawn {
            assert!(
                rect.max.x + noido_core::COLUMN_MARGIN <= bar_left + 0.5,
                "thumbnail {page} ends at {}",
                rect.max.x
            );
        }
    }

    /// The pages are laid out in the room the scroll area leaves beside a
    /// solid bar: at fit width they fit it, so no horizontal bar.
    #[test]
    fn solid_bars_leave_fitted_pages_no_horizontal_bar() {
        let mut h = Harness::new().solid_bars();
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        assert_eq!(h.app.view.viewport_size(), (inner.width(), inner.height()));
        let layout = h.app.view.layout();
        assert!(
            layout.content_width() <= inner.width() + 0.5,
            "content {} wider than {}",
            layout.content_width(),
            inner.width()
        );
        assert!(layout.content_height() > inner.height());
    }

    /// Zoomed in past the width, the pages scroll sideways too, and are laid
    /// out above the horizontal bar.
    #[test]
    fn solid_bars_scroll_zoomed_in_pages_both_ways() {
        let mut h = Harness::new().solid_bars();
        for _ in 0..3 {
            h.press(egui::Key::Equals, Modifiers::COMMAND);
        }
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        assert_eq!(h.app.view.viewport_size(), (inner.width(), inner.height()));
        assert!(h.app.view.layout().content_width() > inner.width());
    }

    /// With both bars, the corner where their grooves meet has no hairline
    /// through it: the vertical one ends where the horizontal groove begins.
    #[test]
    fn groove_hairlines_stop_at_the_corner() {
        let mut h = Harness::new().solid_bars();
        for _ in 0..3 {
            h.press(egui::Key::Equals, Modifiers::COMMAND);
        }
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        let vertical: Vec<[egui::Pos2; 2]> = h
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { points, .. }
                    if (points[0].x - inner.max.x).abs() < 0.5
                        && (points[1].x - inner.max.x).abs() < 0.5 =>
                {
                    Some(*points)
                }
                _ => None,
            })
            .collect();
        assert!(!vertical.is_empty(), "no hairline beside the pages");
        for points in vertical {
            assert!(
                points[0].y.max(points[1].y) <= inner.max.y + 0.5,
                "hairline runs to {} through the corner below {}",
                points[0].y.max(points[1].y),
                inner.max.y
            );
        }
    }

    /// Filled rects the last frame painted with this x range (within half a
    /// point), as (top, bottom).
    fn strips(h: &Harness, left: f32, right: f32) -> Vec<(f32, f32)> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => Some(rect.rect),
                _ => None,
            })
            .filter(|rect| (rect.min.x - left).abs() < 0.5 && (rect.max.x - right).abs() < 0.5)
            .map(|rect| (rect.min.y, rect.max.y))
            .collect()
    }

    /// A solid bar runs in a groove the bar's width, down the whole page
    /// area and sidebar; overlay bars have none.
    #[test]
    fn solid_bars_run_in_a_groove() {
        let mut h = Harness::new().solid_bars();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        let pages = strips(&h, inner.max.x, inner.max.x + 17.0);
        assert!(
            pages.contains(&(inner.min.y, inner.max.y)),
            "no groove beside the pages: {pages:?}"
        );
        // Inside the panel's 1-point border on its right.
        let right = h.app.sidebar.width - 1.0;
        let sidebar = strips(&h, right - 17.0, right);
        assert!(
            sidebar.iter().any(|(top, bottom)| bottom - top > 500.0),
            "no groove in the sidebar: {sidebar:?}"
        );

        let mut h = Harness::new();
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        assert!(strips(&h, inner.max.x - 17.0, inner.max.x).is_empty());
    }

    /// A press anywhere across the groove, below the thumb, scrolls down.
    #[test]
    fn the_whole_groove_takes_clicks() {
        for dx in [0.5, 2.0, 8.5, 15.0, 16.5] {
            let mut h = Harness::new().solid_bars();
            h.frames(3, vec![]);
            let inner = h.app.last_scroll.unwrap().inner;
            let before = h.app.view.offset().1;
            h.click(pos2(inner.max.x + dx, inner.max.y - 20.0));
            h.frames(3, vec![]);
            assert!(
                h.app.view.offset().1 > before,
                "a press {dx} into the groove did not scroll"
            );
        }
    }

    /// The thumb is a capsule the theme's thumb width (windows-11: 6),
    /// centred in the 17-point groove.
    #[test]
    fn solid_bars_have_a_thin_thumb() {
        let mut h = Harness::new().solid_bars();
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        let thumbs = strips(&h, inner.max.x + 5.5, inner.max.x + 11.5);
        assert_eq!(thumbs.len(), 1, "{thumbs:?}");
        let (top, bottom) = thumbs[0];
        assert_eq!(top, inner.min.y);
        assert!(bottom < inner.max.y);
    }

    /// On macOS solid bars meet the window's edge; elsewhere the resize
    /// edges keep them off it, and overlay bars keep the margin everywhere.
    #[test]
    fn solid_bars_meet_the_edge_on_macos() {
        let mut h = Harness::new().solid_bars();
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        let margin = if cfg!(target_os = "macos") { 0.0 } else { 8.0 };
        assert_eq!(inner.max.x + 17.0, h.size.x - margin);

        // Overlay bars keep the margin everywhere.
        let mut h = Harness::new();
        h.frames(3, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        assert_eq!(inner.max.x, h.size.x - 8.0);
    }

    /// The sidebar's thumbnails are laid out beside its solid bar, not
    /// under it.
    #[test]
    fn solid_bars_leave_the_thumbnails_clear() {
        let mut h = Harness::new().solid_bars();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        // Inside the panel's 1-point border on its right.
        let bar_left = h.app.sidebar.width - 1.0 - 17.0;
        let drawn = h.app.sidebar_view.drawn();
        assert!(!drawn.is_empty());
        for (page, rect) in drawn {
            assert!(
                rect.max.x + noido_core::COLUMN_MARGIN <= bar_left + 0.5,
                "thumbnail {page} ends at {}, the bar starts at {bar_left}",
                rect.max.x
            );
        }
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
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState {
                open: true,
                width: 2000.0,
                ..Default::default()
            },
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::default(),
            pagelet: None,
        };
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(state))
        });
        h.frames(3, vec![]);
        assert!(h.app.sidebar.width <= 450.0, "{}", h.app.sidebar.width);
        assert!(h.app.view.viewport_size().0 >= 400.0);
    }

    #[test]
    fn the_app_is_send() {
        fn send<T: Send>() {}
        send::<NoidoApp>();
    }

    /// A shell's end of a harness: what the windows asked, and how often
    /// the shell was woken.
    fn shell_end() -> (
        crate::shell::Requests,
        std::sync::mpsc::Receiver<crate::shell::Request>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        (crate::shell::Requests::new(tx, wake), rx, woken)
    }

    /// A harness whose menu offers `paths` as the recently opened files.
    fn recent_harness(paths: &[&str]) -> Harness {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        Harness::build(move |mut app| {
            app.set_recent(paths);
            app
        })
    }

    /// Opens the menu and runs the frames that draw it, with the pointer
    /// away from the entries so none opens by itself.
    fn open_menu(h: &mut Harness) {
        h.click(h.app.bar.menu.center());
        h.frames(2, vec![]);
        h.frames(1, vec![Event::PointerMoved(pos2(2.0, 2.0))]);
    }

    /// Opens the menu's "Open Recent" submenu and runs the frames that
    /// draw it.
    fn open_recent(h: &mut Harness) {
        let button = h.text_rect("Open Recent").expect("no Open Recent");
        h.click(button.center());
        h.frames(2, vec![]);
    }

    #[test]
    fn open_recent_sits_under_open_and_lines_up_with_it() {
        let mut h = recent_harness(&["/papers/a.pdf"]);
        open_menu(&mut h);
        let open = h.text_rect("Open…").expect("no Open…");
        let recent = h.text_rect("Open Recent").expect("no Open Recent");
        assert!(recent.top() > open.top(), "Open Recent is above Open…");
        // Past the checkmark column, like every other label.
        assert!(
            (recent.left() - open.left()).abs() < 0.5,
            "{open:?} and {recent:?} do not line up"
        );
    }

    #[test]
    fn recent_files_are_offered_behind_open_recent() {
        let mut h = recent_harness(&["/papers/a.pdf", "/scans/b.pdf"]);
        open_menu(&mut h);
        assert!(h.text_rect("Open Recent").is_some(), "no Open Recent");
        open_recent(&mut h);
        assert!(h.text_rect("a.pdf").is_some(), "a.pdf not offered");
        assert!(h.text_rect("b.pdf").is_some(), "b.pdf not offered");
        assert!(h.text_rect("Clear recent").is_some(), "no clear entry");
    }

    #[test]
    fn a_name_that_fits_beside_the_menu_is_shown_whole() {
        let mut h = recent_harness(&["/papers/network-analysis.pdf"]);
        open_menu(&mut h);
        open_recent(&mut h);
        assert!(h.text_rect("network-analysis.pdf").is_some(), "not whole");
    }

    #[test]
    fn a_name_too_wide_beside_the_menu_is_cut_and_leaves_the_menu_in_view() {
        let name = format!(
            "{}.pdf",
            "a-very-long-paper-title-about-network-analysis-".repeat(3)
        );
        let mut h = recent_harness(&[&format!("/papers/{name}")]);
        open_menu(&mut h);
        open_recent(&mut h);
        let menu_left = h.text_rect("Fit width").expect("no Fit width").left();
        let cut = h
            .latest_texts()
            .into_iter()
            .find(|(text, _)| text.contains('…') && text.ends_with(".pdf"))
            .unwrap_or_else(|| panic!("no cut label: {:?}", h.latest_texts()));
        assert_ne!(cut.0, name, "the name was not cut");
        // Beside the menu, not across it.
        assert!(cut.1.right() <= menu_left, "{:?} covers the menu", cut);
    }

    #[test]
    fn a_narrow_window_still_keeps_the_submenu_beside_the_menu() {
        let name = format!(
            "{}.pdf",
            "a-very-long-paper-title-about-network-analysis-".repeat(3)
        );
        let mut h = recent_harness(&[&format!("/papers/{name}")]);
        h.size = vec2(520.0, 800.0);
        h.frames(3, vec![]);
        open_menu(&mut h);
        open_recent(&mut h);
        let menu_left = h.text_rect("Fit width").expect("no Fit width").left();
        // The cut itself is the core's to make, and this window is too
        // narrow to say how much of the name survives; that it is cut and
        // stays beside the menu is this test's business.
        let cut = h
            .latest_texts()
            .into_iter()
            .find(|(text, _)| text.contains('…') && text.contains("pdf"))
            .unwrap_or_else(|| panic!("no cut label: {:?}", h.latest_texts()));
        assert!(cut.1.right() <= menu_left, "{:?} covers the menu", cut);
    }

    #[test]
    fn no_recent_files_leaves_open_recent_with_nothing_to_offer() {
        let mut h = Harness::new();
        open_menu(&mut h);
        // The menu is up: "Open…" is drawn.
        assert!(h.text_rect("Open…").is_some(), "the menu is not open");
        assert!(h.text_rect("Open Recent").is_some(), "no Open Recent");
        // And it opens on nothing.
        open_recent(&mut h);
        assert!(h.text_rect("Clear recent").is_none());
    }

    #[test]
    fn clicking_clear_recent_asks_the_shell() {
        let (requests, rx, _) = shell_end();
        let mut h = Harness::build(move |mut app| {
            app = app.with_requests(requests);
            app.set_recent(vec![PathBuf::from("/papers/a.pdf")]);
            app
        });
        open_menu(&mut h);
        open_recent(&mut h);
        let clear = h.text_rect("Clear recent").expect("no clear entry");
        h.click(clear.center());
        h.frames(2, vec![]);
        assert!(matches!(
            rx.try_recv(),
            Ok(crate::shell::Request::ClearRecent)
        ));
    }

    #[test]
    fn hovering_a_recent_file_shows_where_it_is() {
        let mut h = recent_harness(&["/papers/network-analysis.pdf"]);
        open_menu(&mut h);
        open_recent(&mut h);
        let entry = h.text_rect("network-analysis.pdf").expect("not offered");
        h.frames(1, vec![Event::PointerMoved(entry.center())]);
        h.frames(60, vec![]);
        let shown = [
            "/papers/network-analysis.pdf",
            "papers/network-analysis.pdf",
        ]
        .iter()
        .any(|text| h.text_rect(text).is_some());
        assert!(shown, "no path on hover");
    }

    #[test]
    fn clicking_a_recent_file_opens_it() {
        let mut h = recent_harness(&["/papers/a.pdf"]);
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let into = opened.clone();
        h.app.open = Arc::new(move |path| into.lock().unwrap().push(path.to_path_buf()));
        open_menu(&mut h);
        open_recent(&mut h);
        let entry = h.text_rect("a.pdf").expect("a.pdf not offered");
        h.click(entry.center());
        h.frames(2, vec![]);
        assert_eq!(
            opened.lock().unwrap().as_slice(),
            &[PathBuf::from("/papers/a.pdf")]
        );
    }

    /// A harness whose window keeps its state in `store`, and whose menu
    /// offers the store's recent files.
    fn persisting_harness(store: &Store, document: &std::path::Path) -> Harness {
        let store = store.clone();
        let document = document.to_path_buf();
        Harness::build(move |mut app| {
            let slot = store.load(DocKey::new(b"x", &document)).0;
            app = app.with_persistence(store.clone(), slot, None);
            app.set_recent(store.load_recent());
            app
        })
    }

    #[test]
    fn clicking_a_recent_file_moves_it_to_the_front() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let a = dir.path().join("a.pdf");
        let b = dir.path().join("b.pdf");
        for path in [&a, &b] {
            fs::write(path, b"pdf").unwrap();
        }
        store.note_recent(&a).unwrap();
        store.note_recent(&b).unwrap();
        let mut h = persisting_harness(&store, &b);
        h.app.open = Arc::new(|_| {});
        open_menu(&mut h);
        open_recent(&mut h);
        let entry = h.text_rect("a.pdf").expect("a.pdf not offered");
        h.click(entry.center());
        h.frames(2, vec![]);
        assert_eq!(
            store.load_recent(),
            vec![fs::canonicalize(&a).unwrap(), fs::canonicalize(&b).unwrap()]
        );
    }

    #[test]
    fn without_a_shell_clearing_the_recent_files_clears_the_store() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let a = dir.path().join("a.pdf");
        fs::write(&a, b"pdf").unwrap();
        store.note_recent(&a).unwrap();
        let mut h = persisting_harness(&store, &a);
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::ClearRecent);
        assert!(store.load_recent().is_empty());
        assert!(h.app.recent.is_empty());
    }

    #[test]
    fn clearing_recent_files_asks_the_shell() {
        let (requests, rx, _) = shell_end();
        let mut h = Harness::build(|app| app.with_requests(requests));
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::ClearRecent);
        assert!(matches!(
            rx.try_recv(),
            Ok(crate::shell::Request::ClearRecent)
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn with_requests_quit_asks_the_shell() {
        let (requests, rx, woken) = shell_end();
        let mut h = Harness::build(|app| app.with_requests(requests));
        h.press(egui::Key::Q, Modifiers::COMMAND);
        assert!(matches!(rx.try_recv(), Ok(crate::shell::Request::Quit)));
        assert!(rx.try_recv().is_err());
        assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            !h.commands.contains(&egui::ViewportCommand::Close),
            "{:?}",
            h.commands
        );
    }

    #[test]
    fn close_window_with_requests_still_closes_its_window() {
        let (requests, rx, _) = shell_end();
        let mut h = Harness::build(|app| app.with_requests(requests));
        h.press(egui::Key::W, Modifiers::COMMAND);
        assert!(h.commands.contains(&egui::ViewportCommand::Close));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn with_requests_a_picked_file_is_a_request_to_open() {
        let (requests, rx, _) = shell_end();
        let mut h = Harness::build(|app| app.with_requests(requests));
        h.app.pick = Box::new(|_, _, on_picked| on_picked(Path::new("/tmp/picked.pdf")));
        h.press(egui::Key::O, Modifiers::COMMAND);
        match rx.try_recv() {
            Ok(crate::shell::Request::Open {
                path,
                target,
                reply,
            }) => {
                assert_eq!(path, PathBuf::from("/tmp/picked.pdf"));
                assert!(target.is_none());
                assert!(reply.is_none());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn with_requests_a_saved_settings_page_is_announced() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = SettingsFile::at(dir.path());
        let (requests, rx, _) = shell_end();
        let mut h = Harness::build(|app| {
            app.with_requests(requests)
                .with_settings(Some(file), Settings::default())
        });
        open_settings(&mut h);
        assert!(rx.try_recv().is_err(), "sent before any change");
        h.click_text("Gapless mode (default: off)");
        h.frames(2, vec![]);
        match rx.try_recv() {
            Ok(crate::shell::Request::SettingsSaved(settings)) => {
                assert!(settings.new_documents.gapless);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn save_now_saves_a_fresh_change() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), None)
        });
        h.press(egui::Key::PageDown, Modifiers::NONE);
        assert_eq!(store.load(test_key()).1, None, "saved before the delay");
        h.app.save_now();
        assert_eq!(
            store.load(test_key()).1.map(|state| state.view),
            h.app.view.state()
        );
    }

    #[test]
    fn a_queued_action_is_carried_out_in_the_next_frame() {
        let mut h = Harness::new();
        h.app.queue(Action::OpenSettings);
        assert!(h.app.settings_page.is_none());
        h.frames(1, vec![]);
        assert!(h.app.settings_page.is_some());
        assert!(h.app.queued.is_empty());
    }

    #[test]
    fn set_settings_replaces_the_window_settings() {
        let mut h = Harness::new();
        let mut s = Settings::default();
        s.new_documents.sidebar = !s.new_documents.sidebar;
        assert_ne!(h.app.settings, s);
        h.app.set_settings(s);
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::OpenSettings);
        assert_eq!(h.app.settings, s);
    }

    #[test]
    fn mod_q_and_mod_w_send_close() {
        for key in [egui::Key::Q, egui::Key::W] {
            let mut h = Harness::new();
            h.press(key, Modifiers::COMMAND);
            assert!(
                h.commands.contains(&egui::ViewportCommand::Close),
                "{key:?} did not close"
            );
        }
    }

    #[test]
    fn mod_o_opens_the_picked_document() {
        let mut h = Harness::new();
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let picked = opened.clone();
        let open: open::OnPicked =
            Arc::new(move |path| picked.lock().unwrap().push(path.to_path_buf()));
        h.app.open = open;
        // The window comes along as the dialog's parent; `None` only in tests.
        h.app.pick = Box::new(|_, _, on_picked| on_picked(Path::new("/tmp/picked.pdf")));
        h.press(egui::Key::O, Modifiers::COMMAND);
        assert_eq!(
            opened.lock().unwrap().as_slice(),
            &[PathBuf::from("/tmp/picked.pdf")]
        );
    }

    #[test]
    fn menu_entry_opens_the_picked_document() {
        let mut h = Harness::new();
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let picked = opened.clone();
        let open: open::OnPicked =
            Arc::new(move |path| picked.lock().unwrap().push(path.to_path_buf()));
        h.app.open = open;
        h.app.pick = Box::new(|_, _, on_picked| on_picked(Path::new("/tmp/from-menu.pdf")));
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Open…")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, entry.action);
        assert_eq!(
            opened.lock().unwrap().as_slice(),
            &[PathBuf::from("/tmp/from-menu.pdf")]
        );
    }

    #[test]
    fn a_cancelled_pick_opens_nothing() {
        let mut h = Harness::new();
        let opened: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let picked = opened.clone();
        let open: open::OnPicked =
            Arc::new(move |path| picked.lock().unwrap().push(path.to_path_buf()));
        h.app.open = open;
        // A cancelled pick hands nothing over.
        h.app.pick = Box::new(|_, _, _| {});
        h.press(egui::Key::O, Modifiers::COMMAND);
        assert!(opened.lock().unwrap().is_empty());
    }

    #[test]
    fn open_dialog_starts_in_the_documents_directory() {
        let mut h = Harness::build(|app| app.with_link_path("/docs/paper.pdf".into()));
        let dirs: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = dirs.clone();
        h.app.pick = Box::new(move |_, directory, _| seen.lock().unwrap().push(directory));
        h.press(egui::Key::O, Modifiers::COMMAND);
        assert_eq!(
            dirs.lock().unwrap().as_slice(),
            &[Some(PathBuf::from("/docs"))]
        );
    }

    #[test]
    fn open_dialog_without_a_document_starts_in_the_default_directory() {
        let mut h = Harness::new();
        let dirs: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = dirs.clone();
        h.app.pick = Box::new(move |_, directory, _| seen.lock().unwrap().push(directory));
        h.press(egui::Key::O, Modifiers::COMMAND);
        assert_eq!(dirs.lock().unwrap().as_slice(), &[None]);
    }

    /// Puts the cursor in the search field.
    fn open_settings(h: &mut Harness) {
        h.press(egui::Key::Comma, Modifiers::COMMAND);
        h.frames(2, vec![]);
        assert!(h.app.settings_page.is_some(), "settings not open");
    }

    /// A harness whose settings are saved in `dir`.
    fn settings_in(dir: &std::path::Path) -> Harness {
        let file = SettingsFile::at(dir);
        Harness::build(|app| app.with_settings(Some(file), Settings::default()))
    }

    #[test]
    fn ctrl_comma_opens_settings() {
        let mut h = Harness::new();
        open_settings(&mut h);
    }

    #[test]
    fn menu_entry_opens_settings() {
        let mut h = Harness::new();
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Settings…")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, entry.action);
        assert!(h.app.settings_page.is_some());
    }

    #[test]
    fn rows_show_defaults() {
        let mut h = Harness::new();
        open_settings(&mut h);
        for text in [
            "Zoom (default: fit width)",
            "Fit width",
            "Fit page",
            "Fixed",
            "Continuous (default: on)",
            "Dual page (default: off)",
            "Odd pages left (default: on)",
            "Gapless mode (default: off)",
            "Show sidebar (default: off)",
            "Set from current view",
        ] {
            assert!(h.text_rect(text).is_some(), "{text:?} not drawn");
        }
    }

    #[test]
    fn escape_closes_settings_only() {
        let mut h = Harness::new();
        h.press(egui::Key::F9, Modifiers::NONE);
        open_settings(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.settings_page.is_none());
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn escape_in_settings_keeps_search() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.typed("pond");
        h.frames(2, vec![]);
        open_settings(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.settings_page.is_none());
        assert_eq!(h.app.search, "pond");
    }

    #[test]
    fn view_keys_do_nothing_while_open() {
        let mut h = Harness::new();
        open_settings(&mut h);
        let before = (h.app.view.offset(), h.app.view.scale(), h.app.view.mode());
        h.press(egui::Key::D, Modifiers::NONE);
        h.press(egui::Key::F, Modifiers::NONE);
        h.frames(
            2,
            vec![wheel(
                egui::MouseWheelUnit::Line,
                -3.0,
                egui::MouseWheelSource::Wheel,
            )],
        );
        assert!(!h.app.view.dual());
        assert_eq!(
            (h.app.view.offset(), h.app.view.scale(), h.app.view.mode()),
            before
        );
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        h.press(egui::Key::D, Modifiers::NONE);
        assert!(h.app.view.dual());
    }

    #[test]
    fn changing_a_setting_saves_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        open_settings(&mut h);
        h.click_text("Dual page (default: off)");
        h.frames(2, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap();
        assert!(saved.new_documents.dual);
        assert!(!h.app.view.dual(), "the open document changed");
    }

    #[test]
    fn choosing_fixed_zoom_saves_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        open_settings(&mut h);
        h.click_text("Fixed");
        h.frames(2, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap();
        assert_eq!(saved.new_documents.zoom, noido_core::StartZoom::Fixed);
    }

    #[test]
    fn set_from_current_view_copies_view() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::G, Modifiers::NONE);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let percent = (h.app.view.scale() * 100.0).round() as u32;
        open_settings(&mut h);
        h.click_text("Set from current view");
        h.frames(2, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap().new_documents;
        assert_eq!(saved.zoom, noido_core::StartZoom::Fixed);
        assert_eq!(saved.zoom_percent, percent);
        assert!(!saved.continuous);
        assert!(saved.gapless);
        assert!(saved.sidebar);
    }

    #[test]
    fn gapless_row_saves() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        open_settings(&mut h);
        h.click_text("Gapless mode (default: off)");
        h.frames(2, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap();
        assert!(saved.new_documents.gapless);
        assert!(!h.app.view.gapless(), "the open document changed");
    }

    #[test]
    fn new_document_opens_gapless() {
        let settings = with_new_documents(noido_core::Defaults {
            dual: true,
            gapless: true,
            ..Default::default()
        });
        let h = Harness::build(|app| app.with_settings(None, settings));
        assert!(h.app.view.gapless());
        assert_eq!(
            h.app.view.layout().page_rect(1).unwrap().x,
            h.app.view.layout().page_rect(0).unwrap().x + 612.0 * h.app.view.scale()
        );
    }

    #[test]
    fn sidebar_row_saves() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        open_settings(&mut h);
        h.click_text("Show sidebar (default: off)");
        h.frames(2, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap();
        assert!(saved.new_documents.sidebar);
        assert!(!h.app.sidebar.open, "the open document changed");
    }

    #[test]
    fn opening_settings_rereads_the_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        // Another noido saved a change since this one started.
        let file = SettingsFile::at(dir.path());
        let mut other = Settings::default();
        other.new_documents.continuous = false;
        file.save(&other).unwrap();
        open_settings(&mut h);
        h.click_text("Dual page (default: off)");
        h.frames(2, vec![]);
        let saved = file.load().unwrap().new_documents;
        assert!(saved.dual);
        assert!(!saved.continuous, "the other window's change was lost");
    }

    #[test]
    fn failed_save_shows_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "").unwrap();
        let mut h = settings_in(&blocker.join("noido"));
        open_settings(&mut h);
        h.click_text("Dual page (default: off)");
        h.frames(2, vec![]);
        let error = h.app.settings_page.as_ref().unwrap().error.clone();
        assert!(error.is_some_and(|e| e.starts_with("Couldn't save settings")));
        assert!(h.app.settings.new_documents.dual, "kept for this run");
    }

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
        // Typing opened the sidebar; F9 works while typing.
        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(!h.app.sidebar.open);
    }

    #[test]
    fn typing_opens_sidebar_and_two_escs_close_it() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.typed("pond");
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open, "the first Esc only clears");
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.sidebar.open);
        assert!(!h.ctx.text_edit_focused());
    }

    #[test]
    fn enter_does_not_open_sidebar() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.sidebar.open);
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
            ..Default::default()
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
    fn g_toggles_gapless() {
        let mut h = Harness::new();
        assert!(!h.app.view.gapless());
        h.press(egui::Key::G, Modifiers::NONE);
        assert!(h.app.view.gapless());
        h.frames(2, vec![]);
        h.press(egui::Key::G, Modifiers::NONE);
        assert!(!h.app.view.gapless());
    }

    #[test]
    fn menu_gapless_matches_key() {
        let mut h = Harness::new();
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Gapless mode")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, entry.action);
        assert!(h.app.view.gapless());
    }

    #[test]
    fn reload_keeps_gapless() {
        let mut h = Harness::new();
        h.press(egui::Key::G, Modifiers::NONE);
        h.frames(3, vec![]);
        h.reload(Arc::new(SizedEngine::letter(12)), recompiled_key());
        h.frames(3, vec![]);
        assert!(h.app.view.gapless());
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
        h.app.apply(&ctx, None, entry.action);
        assert!(h.app.view.dual());
    }

    #[test]
    fn status_text_counts_from_one() {
        assert_eq!(
            crate::titlebar::status_text(&noido_core::PageNumbers::default(), 2, 10, 1.06),
            "3 / 10 · 106 %"
        );
    }

    /// A point on the title bar with no control under it.
    fn empty_bar_point(h: &Harness) -> egui::Pos2 {
        pos2(h.app.bar.search.right() + 20.0, h.app.bar.search.center().y)
    }

    /// A point in the middle of the page area.
    fn page_point(h: &Harness) -> egui::Pos2 {
        h.app.last_scroll.unwrap().inner.center()
    }

    #[test]
    fn open_hand_over_pages() {
        let mut h = Harness::new();
        let at = page_point(&h);
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
    }

    #[test]
    fn closed_hand_while_dragging() {
        let mut h = Harness::new();
        let at = page_point(&h);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(at)]);
        h.frames(1, vec![button(true)]);
        h.frames(1, vec![Event::PointerMoved(at + vec2(0.0, -30.0))]);
        assert_eq!(h.cursor, egui::CursorIcon::Grabbing);
        h.frames(1, vec![button(false)]);
        h.frames(1, vec![]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
    }

    /// A point on the vertical scroll bar, at the page area's right edge.
    fn scroll_bar_point(h: &Harness) -> egui::Pos2 {
        let inner = h.app.last_scroll.unwrap().inner;
        pos2(inner.right() - 3.0, inner.top() + 40.0)
    }

    #[test]
    fn no_hand_over_scroll_bar() {
        let mut h = Harness::new();
        let at = scroll_bar_point(&h);
        h.frames(3, vec![Event::PointerMoved(at)]);
        h.frames(2, vec![]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
    }

    #[test]
    fn no_closed_hand_dragging_scroll_bar() {
        let mut h = Harness::new();
        let at = scroll_bar_point(&h);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(3, vec![Event::PointerMoved(at)]);
        h.frames(1, vec![button(true)]);
        h.frames(1, vec![Event::PointerMoved(at + vec2(0.0, 30.0))]);
        assert_ne!(h.cursor, egui::CursorIcon::Grabbing);
    }

    #[test]
    fn no_hand_over_title_bar() {
        let mut h = Harness::new();
        let at = empty_bar_point(&h);
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
    }

    #[test]
    fn no_hand_under_settings_page() {
        let mut h = Harness::new();
        open_settings(&mut h);
        // A corner of the page area, outside the dialog.
        let at = h.app.last_scroll.unwrap().inner.left_bottom() + vec2(20.0, -20.0);
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_ne!(h.cursor, egui::CursorIcon::Grab);
    }

    /// Documents whose title is their bytes, as text.
    struct PropsEngine;
    struct PropsDocument(String);

    impl PdfEngine for PropsEngine {
        fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(PropsDocument(
                String::from_utf8_lossy(&bytes).into_owned(),
            )))
        }
    }

    impl Document for PropsDocument {
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
            rect: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            BlankDocument.render_tile(page, scale, rect)
        }
        fn properties(&self) -> noido_engine::Properties {
            let font = |name: &str, embedded| noido_engine::Font {
                name: name.into(),
                embedded,
            };
            noido_engine::Properties {
                title: Some(self.0.clone()),
                author: Some("Åsa Olson".into()),
                created: Some(noido_engine::Date {
                    year: 2024,
                    month: 3,
                    day: 5,
                    hour: 14,
                    minute: 22,
                    second: 1,
                    utc_offset: Some(60),
                }),
                version: Some("1.7".into()),
                tagged: true,
                fonts: vec![
                    font("ABCDEF+MinionPro-It", true),
                    font("ABCDEF+MinionPro-Regular", true),
                    font("Helvetica", false),
                ],
                ..Default::default()
            }
        }
    }

    /// A harness whose document is /docs/paper.pdf, made of `bytes`.
    fn with_info_source(bytes: &[u8]) -> Harness {
        let source = crate::info::Source {
            engine: Arc::new(PropsEngine),
            bytes: Arc::new(bytes.to_vec()),
            path: "/docs/paper.pdf".into(),
        };
        Harness::build(|app| app.with_source(source))
    }

    fn show_info(h: &mut Harness) {
        h.press(egui::Key::I, Modifiers::COMMAND);
        // The page takes its new size as the properties arrive; wait for its
        // last font row rather than a fixed number of frames.
        h.until(|h| h.text_rect("Regular, Italic").is_some());
        assert!(h.app.info_page.is_some(), "document info not shown");
    }

    #[test]
    fn ctrl_i_shows_document_info() {
        let mut h = with_info_source(b"Info Title");
        show_info(&mut h);
        for text in [
            "paper.pdf",
            "/docs",
            "10 bytes",
            "Info Title",
            "Åsa Olson",
            "2024-03-05 14:22 +01:00",
            "10",
            "216 × 279 mm (612 × 792 pt)",
            "1.7",
            "Minion Pro",
            "Regular, Italic",
            "Helvetica",
            "Regular (not embedded)",
        ] {
            assert!(h.text_rect(text).is_some(), "{text:?} not drawn");
        }
    }

    /// Like [`PropsEngine`], each open waiting for a go-ahead on `0`.
    struct GatedProps(std::sync::Mutex<std::sync::mpsc::Receiver<()>>);

    impl PdfEngine for GatedProps {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            let _ = self.0.lock().unwrap().recv();
            Ok(Box::new(BookDocument))
        }
    }

    /// Properties like a typeset book's: long names, many styles.
    struct BookDocument;

    impl Document for BookDocument {
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
            rect: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            BlankDocument.render_tile(page, scale, rect)
        }
        fn properties(&self) -> noido_engine::Properties {
            let fonts = [
                "Intervogue-BlackOblq",
                "Intervogue-MedOblq",
                "Intervogue-BoldOblq",
                "Intervogue-Reg",
                "Intervogue-Black",
                "Intervogue-Med",
                "Intervogue-Bold",
                "MinionPro-SemiboldSubh",
                "MinionPro-Semibold",
                "MinionPro-SemiboldIt",
                "DeutscheZierschrift",
                "AlbertusMedium-Regular",
                "MrsEavesOT-BoldItalic",
            ];
            noido_engine::Properties {
                creator: Some("Adobe InDesign 17.1 (Windows)".into()),
                producer: Some("Adobe PDF Library 16.0.5".into()),
                version: Some("1.7".into()),
                fonts: fonts
                    .iter()
                    .map(|name| noido_engine::Font {
                        name: format!("ABCDEF+{name}"),
                        embedded: true,
                    })
                    .collect(),
                ..Default::default()
            }
        }
    }

    #[test]
    fn info_stays_still_when_the_properties_arrive() {
        let (go, gate) = std::sync::mpsc::channel();
        let title = "A title long enough that it has to wrap onto a second line in the dialog";
        let source = crate::info::Source {
            engine: Arc::new(GatedProps(std::sync::Mutex::new(gate))),
            bytes: Arc::new(title.as_bytes().to_vec()),
            path: "/docs/paper.pdf".into(),
        };
        let mut h = Harness::build(|app| app.with_source(source));
        for _ in 0..2 {
            h.press(egui::Key::I, Modifiers::COMMAND);
            h.frames(3, vec![]);
            go.send(()).unwrap();
            let mut seen = vec![];
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                h.frames(1, vec![]);
                seen.push(h.text_rect("Document info").unwrap().min.x);
            }
            assert!(h.text_rect("Intervogue").is_some(), "properties not shown");
            assert!(seen.iter().all(|&x| x == seen[0]), "dialog moved: {seen:?}");
            h.press(egui::Key::Escape, Modifiers::NONE);
            h.frames(2, vec![]);
        }
    }

    #[test]
    fn info_scroll_bar_runs_along_the_right_edge() {
        let (go, gate) = std::sync::mpsc::channel();
        go.send(()).unwrap();
        let source = crate::info::Source {
            engine: Arc::new(GatedProps(std::sync::Mutex::new(gate))),
            bytes: Arc::new(vec![]),
            path: "/docs/paper.pdf".into(),
        };
        let mut h = Harness::build(|app| app.with_source(source));
        h.size = vec2(900.0, 500.0);
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.until(|h| h.text_rect("Intervogue").is_some());
        h.frames(3, vec![]);
        let modal = h
            .ctx
            .memory(|m| m.area_rect(egui::Id::unique("document-info")))
            .unwrap();
        h.frames(
            10,
            vec![Event::PointerMoved(pos2(
                modal.right() - 4.0,
                modal.center().y,
            ))],
        );
        fn rects(shape: &egui::Shape, out: &mut Vec<Rect>) {
            match shape {
                egui::Shape::Rect(r) => out.push(r.rect),
                egui::Shape::Vec(v) => v.iter().for_each(|s| rects(s, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        h.shapes.iter().for_each(|c| rects(&c.shape, &mut out));
        // The scroll bar: narrow and tall, inside the page.
        out.retain(|r| {
            r.width() < 20.0 && r.height() > 30.0 && modal.x_range().contains(r.center().x)
        });
        assert!(!out.is_empty(), "no scroll bar");
        for bar in out {
            assert!(
                modal.right() - bar.right() <= 4.0,
                "bar ends at {}, the page at {}",
                bar.right(),
                modal.right()
            );
        }
    }

    #[test]
    fn rows_without_values_are_left_out() {
        let mut h = with_info_source(b"Info Title");
        show_info(&mut h);
        for text in ["Subject", "Keywords", "Modified", "Outline"] {
            assert!(h.text_rect(text).is_none(), "{text:?} drawn");
        }
    }

    #[test]
    fn menu_entry_shows_document_info() {
        let mut h = with_info_source(b"Info Title");
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Document info…")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, entry.action);
        h.until(|h| h.text_rect("Info Title").is_some());
        assert!(h.text_rect("Info Title").is_some());
    }

    #[test]
    fn escape_closes_document_info_only() {
        let mut h = with_info_source(b"Info Title");
        h.press(egui::Key::F9, Modifiers::NONE);
        show_info(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.info_page.is_none());
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn view_keys_do_nothing_while_info_is_shown() {
        let mut h = with_info_source(b"Info Title");
        show_info(&mut h);
        let before = (h.app.view.offset(), h.app.view.scale(), h.app.view.mode());
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(
            (h.app.view.offset(), h.app.view.scale(), h.app.view.mode()),
            before
        );
    }

    #[test]
    fn info_shown_during_a_reload_reads_the_new_version() {
        let mut h = with_info_source(b"Info Title");
        show_info(&mut h);
        h.reload(Arc::new(BlankEngine), recompiled_key());
        h.until(|h| h.text_rect("new version").is_some());
        assert!(h.text_rect("new version").is_some());
        assert!(h.text_rect("Info Title").is_none());
    }

    #[test]
    fn ctrl_i_without_a_source_does_nothing() {
        let mut h = Harness::new();
        h.press(egui::Key::I, Modifiers::COMMAND);
        h.frames(2, vec![]);
        assert!(h.app.info_page.is_none());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn resize_edge_keeps_its_cursor() {
        let mut h = Harness::new();
        let at = pos2(h.size.x - 2.0, page_point(&h).y);
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::ResizeEast);
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
        h.typed("x");
        h.press(egui::Key::F9, Modifiers::NONE);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
    }

    #[test]
    fn esc_keeps_sidebar_after_leaving_search() {
        let mut h = Harness::new();
        focus_search(&mut h);
        h.typed("x");
        h.click(pos2(600.0, 500.0));
        assert!(!h.ctx.text_edit_focused());
        focus_search(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
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
        PageImage::load(ctx, name.into(), &image)
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

    /// The textures the last frame drew, in order, within `area` if given.
    fn drawn_textures(h: &Harness, area: Option<Rect>) -> Vec<egui::TextureId> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh)
                    if mesh.texture_id != egui::TextureId::default()
                        && area.is_none_or(|area| {
                            area.contains(clipped.shape.visual_bounding_rect().center())
                        }) =>
                {
                    Some(mesh.texture_id)
                }
                _ => None,
            })
            .collect()
    }

    /// Long enough for any fade to finish.
    const FADE_FRAMES: usize = (crate::image::FADE_SECONDS * 60.0) as usize + 2;

    #[test]
    fn full_render_fades_in_over_its_thumbnail() {
        let (gate, gate_rx) = std::sync::mpsc::channel::<()>();
        let engine = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(gate_rx))));
        let mut h = Harness::build_with(engine, |app| {
            let thumbnailer =
                Thumbnailer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        let has_thumbnail =
            |h: &Harness| h.app.thumbnails.as_ref().unwrap().textures.contains_key(&0);
        h.until(has_thumbnail);
        assert!(has_thumbnail(&h), "no thumbnail");
        let thumbnail = h.app.thumbnails.as_ref().unwrap().textures[&0].texture.id();
        drop(gate);
        h.until(|h| h.app.textures.contains_key(&0));
        let full = h.app.textures[&0].texture.id();
        h.frames(1, vec![]);
        let drawn = drawn_textures(&h, None);
        let under = drawn.iter().position(|id| *id == thumbnail);
        let over = drawn.iter().position(|id| *id == full);
        assert!(
            matches!((under, over), (Some(u), Some(o)) if u < o),
            "thumbnail under the render: {drawn:?}"
        );
        assert!(
            h.repaint_delay < Duration::from_millis(20),
            "no repaint while fading"
        );
        h.frames(FADE_FRAMES, vec![]);
        let drawn = drawn_textures(&h, None);
        assert!(
            drawn.contains(&full) && !drawn.contains(&thumbnail),
            "{drawn:?}"
        );
    }

    #[test]
    fn render_before_its_thumbnail_does_not_fade() {
        let (gate, gate_rx) = std::sync::mpsc::channel::<()>();
        let gated = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(gate_rx))));
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(gated, Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        h.until(|h| h.app.textures.contains_key(&0));
        let full = h.app.textures[&0].texture.id();
        drop(gate);
        let has_thumbnail =
            |h: &Harness| h.app.thumbnails.as_ref().unwrap().textures.contains_key(&0);
        h.until(has_thumbnail);
        assert!(has_thumbnail(&h), "no thumbnail");
        h.frames(1, vec![]);
        assert_eq!(drawn_textures(&h, None), [full]);
    }

    #[test]
    fn rerender_does_not_fade() {
        let mut h = Harness::build(|app| {
            let thumbnailer =
                Thumbnailer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        h.until(|h| {
            h.app.textures.contains_key(&0)
                && h.app.thumbnails.as_ref().unwrap().textures.contains_key(&0)
        });
        h.frames(FADE_FRAMES, vec![]);
        let thumbnail = h.app.thumbnails.as_ref().unwrap().textures[&0].texture.id();
        let before = h.app.textures[&0].texture.id();
        h.press(egui::Key::Plus, Modifiers::COMMAND);
        h.until(|h| h.app.textures.get(&0).map(|t| t.texture.id()) != Some(before));
        let after = h.app.textures[&0].texture.id();
        assert_ne!(after, before, "no re-render");
        for _ in 0..3 {
            let drawn = drawn_textures(&h, None);
            assert!(!drawn.contains(&thumbnail), "{drawn:?}");
            h.frames(1, vec![]);
        }
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

    /// 200 wide, short pages, so several fill the viewport at fit width;
    /// records every page rendered.
    #[derive(Clone, Default)]
    struct WideEngine(Arc<std::sync::Mutex<Vec<usize>>>);

    impl WideEngine {
        fn rendered(&self) -> Vec<usize> {
            self.0.lock().unwrap().clone()
        }
    }

    impl PdfEngine for WideEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(self.clone()))
        }
    }

    impl Document for WideEngine {
        fn page_count(&self) -> usize {
            200
        }
        fn page_size(&self, _page: usize) -> Result<Size, EngineError> {
            Ok(Size {
                width: 1000.0,
                height: 300.0,
            })
        }
        fn render_tile(
            &self,
            page: usize,
            scale: f32,
            tile: PixelRect,
        ) -> Result<Rgba8Image, EngineError> {
            self.0.lock().unwrap().push(page);
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
    fn a_minimized_window_comes_forward_in_the_logic_pass_without_a_ui_pass() {
        let activation = crate::instance::Activation::default();
        let mut h = Harness::build(|app| app.with_activation(activation.clone()));
        h.frames(1, vec![]);
        activation.request(None);
        // While minimized, eframe runs only the logic pass.
        let ctx = h.ctx.clone();
        let output = ctx.run_logic(&RawInput::default(), |ctx| h.app.logic_pass(ctx));
        let commands = output
            .viewport_commands
            .get(&egui::ViewportId::ROOT)
            .cloned()
            .unwrap_or_default();
        assert!(commands.contains(&egui::ViewportCommand::Minimized(false)));
        assert!(commands.contains(&egui::ViewportCommand::Focus));
        assert!(activation.take().is_none(), "left set");
    }

    #[test]
    fn activation_raises_window() {
        let activation = crate::instance::Activation::default();
        let mut h = Harness::build(|app| app.with_activation(activation.clone()));
        h.frames(1, vec![]);
        assert!(!h.commands.contains(&egui::ViewportCommand::Focus));
        activation.request(None);
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
        assert!(activation.take().is_none(), "left set");
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

    /// Pages whose renders always fail.
    struct FailingEngine(usize);

    impl PdfEngine for FailingEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(FailingEngine(self.0)))
        }
    }

    impl Document for FailingEngine {
        fn page_count(&self) -> usize {
            self.0
        }
        fn page_size(&self, page: usize) -> Result<Size, EngineError> {
            BlankDocument.page_size(page)
        }
        fn render_tile(&self, _: usize, _: f32, _: PixelRect) -> Result<Rgba8Image, EngineError> {
            Err(EngineError::Render("broken".into()))
        }
    }

    #[test]
    fn failed_render_drops_old_image() {
        let mut h = Harness::new();
        h.until(|h| h.app.textures.contains_key(&0));
        h.reload(Arc::new(FailingEngine(10)), recompiled_key());
        h.until(|h| !h.app.textures.contains_key(&0));
        assert!(!h.app.textures.contains_key(&0), "old image of page 1 kept");
    }

    #[test]
    fn old_images_outside_window_are_dropped() {
        let mut h = Harness::build_with(Arc::new(BlankEngine30), |app| app);
        h.until(|h| h.app.textures.contains_key(&0));
        // The new version's renders fail, so nothing replaces the old images.
        h.reload(Arc::new(FailingEngine(30)), recompiled_key());
        go_to(&mut h, 25);
        h.frames(3, vec![]);
        assert!(
            !h.app.textures.contains_key(&0),
            "textures: {:?}",
            h.app.textures.keys()
        );
    }

    #[test]
    fn failed_thumbnail_drops_old_one() {
        let mut h = Harness::build(|app| {
            let thumbnailer =
                Thumbnailer::start(Arc::new(BlankEngine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
        });
        let has_thumbnail =
            |h: &Harness| h.app.thumbnails.as_ref().unwrap().textures.contains_key(&0);
        h.until(has_thumbnail);
        assert!(has_thumbnail(&h));
        h.reload(Arc::new(FailingEngine(10)), recompiled_key());
        h.until(|h| !has_thumbnail(h));
        assert!(!has_thumbnail(&h), "old thumbnail of page 1 kept");
    }

    /// Ten Letter pages; records the page and scale of every render.
    #[derive(Clone, Default)]
    struct RecordingEngine(Arc<std::sync::Mutex<Vec<(usize, f32)>>>);

    impl RecordingEngine {
        fn recorded(&self) -> Vec<(usize, f32)> {
            self.0.lock().unwrap().clone()
        }

        fn pages(&self) -> std::collections::BTreeSet<usize> {
            self.recorded().into_iter().map(|(page, _)| page).collect()
        }
    }

    impl PdfEngine for RecordingEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(self.clone()))
        }
    }

    impl Document for RecordingEngine {
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
            self.0.lock().unwrap().push((page, scale));
            BlankDocument.render_tile(page, scale, tile)
        }
    }

    /// A harness with the sidebar open, its sharp thumbnails from `engine`.
    fn with_sidebar(engine: RecordingEngine) -> Harness {
        let mut h = Harness::build(|app| {
            let renderer =
                Renderer::start_background(Arc::new(engine), Arc::new(Vec::new()), Box::new(|| {}));
            app.with_sidebar_renderer(renderer)
        });
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        h
    }

    /// The pages the sidebar drew last frame.
    fn drawn_pages(h: &Harness) -> Vec<usize> {
        h.app
            .sidebar_view
            .drawn()
            .iter()
            .map(|(page, _)| *page)
            .collect()
    }

    /// Scrolls the sidebar by `dy` points with the wheel.
    fn wheel_sidebar(h: &mut Harness, dy: f32) {
        h.frames(1, vec![Event::PointerMoved(pos2(100.0, 500.0))]);
        let step = wheel(
            egui::MouseWheelUnit::Point,
            dy,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![step]);
        h.frames(30, vec![]);
    }

    #[test]
    fn sidebar_shows_a_thumbnail_per_visible_page() {
        let h = with_sidebar(RecordingEngine::default());
        let drawn = h.app.sidebar_view.drawn();
        assert!(drawn.len() >= 2, "{drawn:?}");
        let pages = drawn_pages(&h);
        assert_eq!(pages, (0..pages.len()).collect::<Vec<_>>());
        let width = h.app.sidebar_view.thumb_width();
        assert!(
            (width - (h.app.sidebar.width - 2.0 * noido_core::COLUMN_MARGIN)).abs() < 2.0,
            "{width} in a sidebar {} wide",
            h.app.sidebar.width
        );
        for (_, rect) in drawn {
            assert!((rect.width() - width).abs() < 0.5, "{rect:?}");
            assert!((rect.height() / rect.width() - 792.0 / 612.0).abs() < 0.01);
            assert!(rect.max.x <= h.app.sidebar.width);
        }
    }

    #[test]
    fn sidebar_shows_pages_not_spreads_in_dual_mode() {
        let mut h = with_sidebar(RecordingEngine::default());
        let sizes = |h: &Harness| -> Vec<Vec2> {
            h.app
                .sidebar_view
                .drawn()
                .iter()
                .map(|(_, r)| r.size())
                .collect()
        };
        let before = sizes(&h);
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.view.dual());
        // The sidebar may have scrolled to the new centre page.
        let pages = drawn_pages(&h);
        assert!(pages.windows(2).all(|w| w[1] == w[0] + 1), "{pages:?}");
        assert!(
            sizes(&h)
                .iter()
                .all(|size| (*size - before[0]).length() < 0.01),
            "{:?} vs {before:?}",
            sizes(&h)
        );
    }

    /// The pages drawn last frame and the sidebar's margin around them, of
    /// the 10 pages.
    fn drawn_with_margin(h: &Harness) -> std::collections::BTreeSet<usize> {
        let drawn = drawn_pages(h);
        let (first, last) = (drawn[0], *drawn.last().unwrap());
        (first.saturating_sub(2)..(last + 3).min(10)).collect()
    }

    #[test]
    fn sidebar_requests_visible_pages_and_a_margin_at_its_width() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        let wanted = drawn_with_margin(&h);
        assert!(wanted.len() > drawn_pages(&h).len(), "{wanted:?}");
        h.until(|_| engine.pages() == wanted);
        assert_eq!(engine.pages(), wanted);
        let scale = h.app.sidebar_view.thumb_width() / 612.0;
        for (page, recorded) in engine.recorded() {
            assert!(
                (recorded / scale - 1.0).abs() < 0.01,
                "page {page}: {recorded} vs {scale}"
            );
        }
    }

    #[test]
    fn widening_sidebar_requests_larger_scale() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        h.until(|_| !engine.recorded().is_empty());
        let (width, scale) = (h.app.sidebar.width, engine.recorded()[0].1);
        h.drag(pos2(width - 1.0, 500.0), vec2(100.0, 0.0));
        h.frames(2, vec![]);
        assert!(
            h.app.sidebar.width > width + 50.0,
            "{}",
            h.app.sidebar.width
        );
        h.until(|_| engine.recorded().iter().any(|&(_, s)| s > scale * 1.2));
        assert!(engine.recorded().iter().any(|&(_, s)| s > scale * 1.2));
    }

    #[test]
    fn sidebar_rerequests_on_pixels_per_point_change() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        h.until(|_| !engine.recorded().is_empty());
        h.ctx.set_pixels_per_point(2.0);
        h.frames(2, vec![]);
        // The window is narrower in points now, so the sidebar may be too.
        let scale = h.app.sidebar_view.thumb_width() * 2.0 / 612.0;
        let sharp = |s: f32| (s / scale - 1.0).abs() < 0.01;
        h.until(|_| engine.recorded().iter().any(|&(_, s)| sharp(s)));
        assert!(
            engine.recorded().iter().any(|&(_, s)| sharp(s)),
            "{:?} vs {scale}",
            engine.recorded()
        );
    }

    #[test]
    fn sidebar_drops_sharp_images_scrolled_out() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&0));
        wheel_sidebar(&mut h, -10_000.0);
        let drawn = drawn_pages(&h);
        assert!(drawn.contains(&9), "{drawn:?}");
        let kept = drawn_with_margin(&h);
        let sharp = h.app.sidebar_view.sharp_pages();
        assert!(!sharp.contains(&0), "{sharp:?}");
        assert!(sharp.iter().all(|page| kept.contains(page)), "{sharp:?}");
    }

    /// Presses `key` `times` times, each once the sidebar's renders have
    /// come in; the current page's thumbnail is drawn at once, and sharp.
    fn page_with_sidebar(h: &mut Harness, key: egui::Key, times: usize) {
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        let settled = |h: &Harness| {
            let sharp = h.app.sidebar_view.sharp_pages();
            drawn_with_margin(h).iter().all(|page| sharp.contains(page))
        };
        for _ in 0..times {
            h.until(settled);
            assert!(
                settled(h),
                "drawn {:?}, sharp {:?}",
                drawn_pages(h),
                h.app.sidebar_view.sharp_pages()
            );
            h.press(key, Modifiers::NONE);
            let current = h.app.current_page().unwrap();
            let sharp = h.app.sidebar_view.sharp_pages();
            assert!(
                drawn_pages(h).contains(&current) && sharp.contains(&current),
                "page {current} drawn {:?}, sharp {sharp:?}",
                drawn_pages(h)
            );
        }
    }

    #[test]
    fn sidebar_thumbnails_are_sharp_when_paging_into_view() {
        let mut h = with_sidebar(RecordingEngine::default());
        page_with_sidebar(&mut h, egui::Key::PageDown, 9);
        assert_eq!(h.app.current_page(), Some(9));
        page_with_sidebar(&mut h, egui::Key::PageUp, 9);
        assert_eq!(h.app.current_page(), Some(0));
    }

    #[test]
    fn sidebar_thumbnails_are_sharp_when_paging_into_view_in_dual_mode() {
        let mut h = with_sidebar(RecordingEngine::default());
        // Paged: a PageDown is a spread. In this small window a continuous
        // one scrolls past several, beyond any margin.
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(h.app.view.dual());
        page_with_sidebar(&mut h, egui::Key::PageDown, 5);
        assert_eq!(
            h.app.view.layout().spread_of(9),
            h.app.view.current_spread().unwrap()
        );
        page_with_sidebar(&mut h, egui::Key::PageUp, 5);
        assert_eq!(h.app.view.current_spread(), Some(0));
    }

    /// A margin page's render arrives out of view: when it comes into view
    /// it is already there, with nothing to fade from.
    #[test]
    fn sidebar_margin_render_does_not_fade() {
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            let sidebar = Renderer::start_background(
                Arc::new(BlankEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            app.with_thumbnails(thumbnailer)
                .with_sidebar_renderer(sidebar)
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        let textures = &mut h.app.thumbnails.as_mut().unwrap().textures;
        for page in 0..10 {
            textures.insert(page, lowres.clone());
        }
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let margin = *drawn_pages(&h).last().unwrap() + 1;
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&margin));
        assert!(h.app.sidebar_view.sharp_pages().contains(&margin));
        assert!(!h.app.sidebar_view.fading(margin));
    }

    #[test]
    fn closing_sidebar_drops_sharp_images() {
        let mut h = with_sidebar(RecordingEngine::default());
        h.until(|h| !h.app.sidebar_view.sharp_pages().is_empty());
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(5, vec![]);
        assert!(h.app.sidebar_view.sharp_pages().is_empty());
        assert!(h.app.sidebar_view.drawn().is_empty());
    }

    /// Waits until every page of the sidebar's kept range is sharp, and the
    /// renders have gone quiet; returns the sharp pages.
    fn sharp_settled(h: &mut Harness) -> Vec<usize> {
        let done = |h: &Harness| {
            let kept = h.app.sidebar_view.kept_pages();
            !kept.is_empty() && h.app.sidebar_view.sharp_pages() == kept.collect::<Vec<_>>()
        };
        h.until(done);
        assert!(done(h), "{:?}", h.app.sidebar_view.sharp_pages());
        h.frames(3, vec![]);
        h.app.sidebar_view.sharp_pages()
    }

    #[test]
    fn sharp_thumbnails_survive_other_tabs() {
        for away in [SidebarTab::Outline, SidebarTab::Results] {
            let engine = RecordingEngine::default();
            let mut h = with_sidebar(engine.clone());
            let sharp = sharp_settled(&mut h);
            let renders = engine.recorded().len();
            click_tab(&mut h, away);
            h.frames(5, vec![]);
            assert_eq!(h.app.sidebar_view.sharp_pages(), sharp, "{away:?}");
            click_tab(&mut h, SidebarTab::Thumbnails);
            // At once: sharp, and not asked for again.
            assert_eq!(h.app.sidebar_view.sharp_pages(), sharp, "{away:?}");
            assert!(!h.app.sidebar_view.drawn().is_empty());
            h.frames(5, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(50));
            h.frames(2, vec![]);
            assert_eq!(engine.recorded().len(), renders, "{away:?}: rendered again");
        }
    }

    #[test]
    fn no_new_sharp_renders_off_the_thumbnails_tab() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        let sharp = sharp_settled(&mut h);
        let renders = engine.recorded().len();
        click_tab(&mut h, SidebarTab::Outline);
        go_to(&mut h, 9);
        h.frames(10, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.frames(2, vec![]);
        assert_eq!(engine.recorded().len(), renders, "asked for new renders");
        assert_eq!(h.app.sidebar_view.sharp_pages(), sharp);
    }

    /// The thumbnails follow the page again on coming back, shedding the
    /// images out of range.
    #[test]
    fn thumbnails_follow_and_shed_on_coming_back_from_far() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        let sharp = sharp_settled(&mut h);
        assert!(sharp.contains(&0), "{sharp:?}");
        click_tab(&mut h, SidebarTab::Outline);
        go_to(&mut h, 9);
        h.frames(3, vec![]);
        // Still there off the tab, so shedding them is something.
        assert!(h.app.sidebar_view.sharp_pages().contains(&0));
        click_tab(&mut h, SidebarTab::Thumbnails);
        let drawn = drawn_pages(&h);
        assert!(drawn.contains(&9), "{drawn:?}");
        let kept = drawn_with_margin(&h);
        let now = h.app.sidebar_view.sharp_pages();
        assert!(!now.contains(&0), "{now:?}");
        assert!(now.iter().all(|page| kept.contains(page)), "{now:?}");
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&9));
        assert!(h.app.sidebar_view.sharp_pages().contains(&9));
    }

    /// Closing the sidebar from another tab frees the sharp images and the
    /// renderer's keep range, though the thumbnails count as not shown.
    #[test]
    fn closing_from_other_tabs_frees_sharp_images() {
        for away in [SidebarTab::Outline, SidebarTab::Results] {
            let mut h = with_sidebar(RecordingEngine::default());
            sharp_settled(&mut h);
            click_tab(&mut h, away);
            h.frames(3, vec![]);
            assert!(!h.app.sidebar_view.sharp_pages().is_empty(), "{away:?}");
            h.press(egui::Key::F9, Modifiers::NONE);
            h.frames(5, vec![]);
            assert!(!h.app.sidebar.open);
            assert!(h.app.sidebar_view.sharp_pages().is_empty(), "{away:?}");
            assert_eq!(h.app.sidebar_view.kept_pages(), 0..0, "{away:?}");
            assert_eq!(h.app.sidebar_view.renderer_keep(), 0..0, "{away:?}");
            // Opened again, and back on the thumbnails, they are asked
            // for afresh.
            h.press(egui::Key::F9, Modifiers::NONE);
            click_tab(&mut h, SidebarTab::Thumbnails);
            h.until(|h| !h.app.sidebar_view.sharp_pages().is_empty());
            assert!(!h.app.sidebar_view.sharp_pages().is_empty(), "{away:?}");
        }
    }

    /// A sidebar over low-res thumbnails whose sharp renders wait for the
    /// returned gate to drop.
    fn gated_over_lowres() -> (std::sync::mpsc::Sender<()>, Harness) {
        let (gate, waiting) = std::sync::mpsc::channel::<()>();
        let engine = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(waiting))));
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            let sidebar = Renderer::start_background(engine, Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
                .with_sidebar_renderer(sidebar)
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        let textures = &mut h.app.thumbnails.as_mut().unwrap().textures;
        for page in 0..10 {
            textures.insert(page, lowres.clone());
        }
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        (gate, h)
    }

    /// Renders landing while another tab shows are simply there on coming
    /// back: no fade from the low-res thumbnail, which is not drawn.
    #[test]
    fn renders_landing_off_the_tab_show_without_a_fade() {
        let (gate, mut h) = gated_over_lowres();
        let kept: Vec<usize> = h.app.sidebar_view.kept_pages().collect();
        assert!(!kept.is_empty());
        assert!(h.app.sidebar_view.sharp_pages().is_empty());
        click_tab(&mut h, SidebarTab::Outline);
        drop(gate);
        // A fade is short: looked for on every frame they land in.
        let faded = std::cell::Cell::new(false);
        h.until(|h| {
            let fading = kept.iter().any(|&page| h.app.sidebar_view.fading(page));
            faded.set(faded.get() || fading);
            h.app.sidebar_view.sharp_pages() == kept
        });
        assert_eq!(h.app.sidebar_view.sharp_pages(), kept);
        assert!(!faded.get(), "faded in off the tab");
        click_tab(&mut h, SidebarTab::Thumbnails);
        assert!(kept.iter().all(|&page| !h.app.sidebar_view.fading(page)));
        let (_, first) = h.app.sidebar_view.drawn()[0];
        let drawn = drawn_textures(&h, Some(first));
        assert_eq!(drawn.len(), 1, "{drawn:?}");
    }

    /// Closed from another tab before any render landed, the sidebar still
    /// lets go of the pages asked for: none lands while it is closed.
    #[test]
    fn closing_with_renders_in_flight_lets_them_go() {
        let (gate, mut h) = gated_over_lowres();
        click_tab(&mut h, SidebarTab::Results);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(3, vec![]);
        assert_eq!(h.app.sidebar_view.kept_pages(), 0..0);
        assert_eq!(h.app.sidebar_view.renderer_keep(), 0..0);
        drop(gate);
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.frames(3, vec![]);
        assert!(h.app.sidebar_view.sharp_pages().is_empty());
    }

    /// Scrolled away by the reader, the thumbnails follow the current page
    /// again on coming back from another tab.
    #[test]
    fn thumbnails_follow_on_coming_back_after_scrolling_away() {
        let mut h = with_sidebar(RecordingEngine::default());
        sharp_settled(&mut h);
        wheel_sidebar(&mut h, -10_000.0);
        assert!(!drawn_pages(&h).contains(&0));
        click_tab(&mut h, SidebarTab::Outline);
        click_tab(&mut h, SidebarTab::Thumbnails);
        let drawn = drawn_pages(&h);
        assert!(drawn.contains(&0), "{drawn:?}");
    }

    /// The same on closing and reopening the sidebar.
    #[test]
    fn thumbnails_follow_on_reopening_after_scrolling_away() {
        let mut h = with_sidebar(RecordingEngine::default());
        sharp_settled(&mut h);
        wheel_sidebar(&mut h, -10_000.0);
        assert!(!drawn_pages(&h).contains(&0));
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let drawn = drawn_pages(&h);
        assert!(drawn.contains(&0), "{drawn:?}");
    }

    fn sepia() -> Color32 {
        let [r, g, b] = noido_core::Paper::Sepia.rgb();
        Color32::from_rgb(r, g, b)
    }

    fn on_sepia() -> Settings {
        Settings {
            paper: noido_core::Paper::Sepia,
            ..Settings::default()
        }
    }

    /// The rectangles filled with `colour` last frame.
    fn filled_with(h: &Harness, colour: Color32) -> Vec<Rect> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(r) if r.fill == colour => Some(r.rect),
                _ => None,
            })
            .collect()
    }

    /// The centre and radius of each circle stroked in `colour` last frame.
    fn stroked_circles(h: &Harness, colour: Color32) -> Vec<(egui::Pos2, f32)> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(c) if c.stroke.width > 0.0 && c.stroke.color == colour => {
                    Some((c.center, c.radius))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_harness_uses_the_fixed_theme() {
        let h = Harness::new();
        assert_eq!(
            h.ctx.global_style().visuals.panel_fill,
            Color32::from_rgb(0x22, 0x22, 0x26)
        );
        assert_eq!(
            h.theme().palette(egui::Theme::Dark).selection,
            h.ctx.global_style().visuals.selection.bg_fill
        );
    }

    #[test]
    fn title_bar_uses_theme_colours() {
        let mut h = Harness::new();
        h.app.name = "test.pdf".into();
        h.frames(1, vec![]);
        let palette = h.theme().palette(egui::Theme::Dark);
        let status = titlebar::status_text(
            &h.app.links.numbers,
            h.app.current_page().unwrap_or(0),
            h.app.view.layout().page_count(),
            h.app.view.scale(),
        );
        assert_eq!(h.text_color("test.pdf"), Some(palette.title_text));
        assert_eq!(h.text_color(&status), Some(palette.muted));
        assert!(!filled_with(&h, palette.title_bar).is_empty());
    }

    #[test]
    fn the_menu_scrolls_when_the_window_is_short() {
        let mut h = Harness::new();
        h.size = vec2(900.0, 600.0);
        h.frames(2, vec![]);
        h.click(h.app.bar.menu.center());
        h.frames(30, vec![]);
        // The menu stops at the window's edge instead of running past it...
        let first = h.text_rect("Open…").expect("the menu opens");
        let last = h.text_rect("Dual page").expect("the menu fills the window");
        assert!(last.bottom() <= 600.0, "{last:?} is inside the window");
        assert!(
            h.text_rect("Quit").is_none(),
            "Quit is past the room the window has"
        );
        // ...and the rest is a wheel away.
        let over = egui::pos2(first.center().x, first.center().y + 80.0);
        h.frames(1, vec![Event::PointerMoved(over)]);
        for _ in 0..12 {
            h.frames(
                2,
                vec![wheel(
                    egui::MouseWheelUnit::Line,
                    -3.0,
                    egui::MouseWheelSource::Wheel,
                )],
            );
        }
        let quit = h.text_rect("Quit").expect("the menu scrolls to Quit");
        assert!(quit.bottom() <= 600.0, "{quit:?} is inside the window");
    }

    #[test]
    fn the_menu_may_use_the_whole_window() {
        let mut h = Harness::new();
        h.size = vec2(900.0, 1200.0);
        h.frames(2, vec![]);
        h.click(h.app.bar.menu.center());
        h.frames(30, vec![]);
        let quit = h
            .text_rect("Quit")
            .expect("a tall window shows the whole menu");
        assert!(quit.bottom() <= 1200.0, "{quit:?} is inside the window");
    }

    #[test]
    fn wheeling_the_menu_does_not_turn_the_page() {
        let mut h = fitted_paged();
        open_menu(&mut h);
        let first = h.text_rect("Open…").expect("the menu opens");
        let over = egui::pos2(first.center().x, first.center().y + 80.0);
        h.frames(1, vec![Event::PointerMoved(over)]);
        let page = h.app.current_page();
        for _ in 0..12 {
            h.frames(
                2,
                vec![wheel(
                    egui::MouseWheelUnit::Line,
                    -3.0,
                    egui::MouseWheelSource::Wheel,
                )],
            );
        }
        assert_eq!(h.app.current_page(), page, "the wheel turned the page");
        assert!(
            h.text_rect("Quit").is_some(),
            "the wheel did not reach the menu"
        );
    }

    #[test]
    fn menu_uses_the_menu_fill_and_hover() {
        let mut h = Harness::new();
        let palette = h.theme().palette(egui::Theme::Dark);
        h.click(h.app.bar.menu.center());
        h.frames(30, vec![]);
        assert!(!filled_deep(&h, palette.menu_fill).is_empty());
        // An entry near the top: a long menu is taller than the window and
        // the rest is a wheel away.
        let entry = h.text_rect("Fit width").expect("the menu's first entries");
        h.frames(30, vec![Event::PointerMoved(entry.center())]);
        assert!(!filled_deep(&h, palette.menu_hover).is_empty());
    }

    #[test]
    fn settings_uses_the_dialog_fill() {
        let mut h = Harness::new();
        let palette = h.theme().palette(egui::Theme::Dark);
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::OpenSettings);
        h.frames(30, vec![]);
        let dialog = egui::CornerRadius::same(18);
        assert!(
            filled_rects_deep(&h, palette.dialog_fill)
                .iter()
                .any(|r| r.corner_radius == dialog),
            "settings frame not at the dialog radius"
        );
    }

    #[test]
    fn info_uses_the_dialog_fill() {
        let mut h = with_info_source(b"Info Title");
        show_info(&mut h);
        h.frames(30, vec![]);
        let palette = h.theme().palette(egui::Theme::Dark);
        let dialog = egui::CornerRadius::same(18);
        assert!(
            filled_rects_deep(&h, palette.dialog_fill)
                .iter()
                .any(|r| r.corner_radius == dialog),
            "info frame not at the dialog radius"
        );
    }

    #[test]
    fn menu_labels_use_the_menu_text_colour() {
        let mut h = Harness::new();
        let palette = h.theme().palette(egui::Theme::Dark);
        h.click(h.app.bar.menu.center());
        h.frames(1, vec![]);
        assert_eq!(h.text_color("Open…"), Some(palette.menu_text));
    }

    #[test]
    fn search_field_uses_the_input_fill() {
        let h = Harness::new();
        let palette = h.theme().palette(egui::Theme::Dark);
        assert!(!filled_with(&h, palette.input).is_empty());
    }

    // macOS draws the traffic lights itself, so there is no close button.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn close_hover_uses_the_danger_colour() {
        let mut h = Harness::new();
        let palette = h.theme().palette(egui::Theme::Dark);
        h.frames(1, vec![Event::PointerMoved(h.app.bar.close.center())]);
        assert!(!filled_with(&h, palette.danger).is_empty());
    }

    #[test]
    fn a_queued_theme_requests_a_repaint() {
        let (live, tx) =
            crate::theme::Live::with_loader(|| Ok(crate::theme::Theme::preset("adwaita").unwrap()));
        let mut h = Harness::build(move |app| app.with_theme(live));
        tx.send(()).unwrap();
        h.frames(1, vec![]);
        assert_eq!(h.repaint_delay, Duration::ZERO);
    }

    #[test]
    fn changing_the_theme_repaints_without_restarting() {
        let mut h = Harness::new();
        let dark = h.theme().palette(egui::Theme::Dark).title_text;
        h.ctx.set_theme(egui::ThemePreference::Light);
        h.frames(1, vec![]);
        let light = h.theme().palette(egui::Theme::Light).title_text;
        assert_ne!(dark, light);
        h.app.name = "test.pdf".into();
        h.frames(1, vec![]);
        assert_eq!(h.text_color("test.pdf"), Some(light));
    }

    #[test]
    fn a_theme_change_reaches_an_open_dialog() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::OpenSettings);
        h.frames(30, vec![]);
        h.ctx.set_theme(egui::ThemePreference::Light);
        h.frames(30, vec![]);
        let light = h.theme().palette(egui::Theme::Light).dialog_fill;
        let dialog = egui::CornerRadius::same(18);
        assert!(
            filled_rects_deep(&h, light)
                .iter()
                .any(|r| r.corner_radius == dialog),
            "open settings did not repaint with the light dialog"
        );
    }

    #[test]
    fn a_light_theme_sits_the_pages_on_the_desk() {
        let mut h = Harness::new();
        h.ctx.set_theme(egui::ThemePreference::Light);
        h.frames(1, vec![]);
        let desk = Color32::from_rgb(0xd2, 0xd2, 0xd3);
        assert_ne!(desk, h.ctx.global_style().visuals.panel_fill);
        let area = filled_with(&h, desk);
        assert!(
            area.iter()
                .any(|r| r.min.y >= TITLE_BAR_HEIGHT && r.width() > 200.0 && r.height() > 200.0),
            "no desk below the title bar: {area:?}"
        );
    }

    #[test]
    fn a_dark_theme_keeps_its_background_behind_the_pages() {
        let h = Harness::new();
        let area = filled_with(&h, Color32::from_rgb(0x22, 0x22, 0x26));
        assert!(
            area.iter().any(|r| r.width() > 200.0 && r.height() > 200.0),
            "no page area on the background: {area:?}"
        );
    }

    #[test]
    fn changing_the_theme_changes_the_global_style() {
        let mut h = Harness::new();
        h.ctx.set_theme(egui::ThemePreference::Light);
        h.frames(1, vec![]);
        assert_eq!(
            h.ctx.global_style().visuals.panel_fill,
            Color32::from_rgb(0xfa, 0xfa, 0xfb)
        );
    }

    /// The rectangles filled with `colour`, including nested shapes and
    /// their corner radii.
    fn filled_rects_deep(h: &Harness, colour: Color32) -> Vec<egui::epaint::RectShape> {
        fn find(shape: &egui::Shape, colour: Color32, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(r) if r.fill == colour => out.push(r.clone()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, colour, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.shapes {
            find(&clipped.shape, colour, &mut out);
        }
        out
    }

    /// The rectangles filled with `colour`, including shapes nested in a
    /// `Shape::Vec` such as a popup's frame.
    fn filled_deep(h: &Harness, colour: Color32) -> Vec<Rect> {
        filled_rects_deep(h, colour)
            .into_iter()
            .map(|r| r.rect)
            .collect()
    }

    /// The tints of the textured meshes drawn last frame: pages and
    /// thumbnails.
    fn image_tints(h: &Harness) -> Vec<Color32> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some(mesh.vertices.iter().map(|v| v.color))
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn placeholders_are_paper_coloured() {
        let mut h = Harness::build_with(Arc::new(SlowEngine), |app| {
            app.with_settings(None, on_sepia())
        });
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let sepia = filled_with(&h, sepia());
        let thumbnails: Vec<Rect> = h.app.sidebar_view.drawn().iter().map(|d| d.1).collect();
        assert!(!thumbnails.is_empty());
        for rect in &thumbnails {
            assert!(sepia.contains(rect), "thumbnail {rect:?} not sepia");
        }
        assert!(sepia.len() > thumbnails.len(), "no sepia page");
    }

    #[test]
    fn pages_and_thumbnails_are_tinted_with_paper() {
        let mut h = Harness::build(|app| {
            // Fails to open: no low-res thumbnails arrive to replace ours.
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            app.with_thumbnails(thumbnailer)
                .with_settings(None, on_sepia())
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        h.app
            .thumbnails
            .as_mut()
            .unwrap()
            .textures
            .insert(0, lowres.clone());
        h.press(egui::Key::F9, Modifiers::NONE);
        h.until(|h| images(h).len() > 1);
        // Pages fading in over the thumbnail are tinted lighter until done.
        h.frames(FADE_FRAMES, vec![]);
        let thumbnail = h.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == lowres.texture.id())
        });
        assert!(thumbnail, "thumbnail not drawn");
        let tints = image_tints(&h);
        assert!(tints.iter().all(|&tint| tint == sepia()), "{tints:?}");
    }

    #[test]
    fn choosing_paper_saves_and_shows_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        h.until(|h| !images(h).is_empty());
        open_settings(&mut h);
        // Until the page has faded in: its colours are dimmed before.
        h.frames(30, vec![]);
        let swatch = filled_with(&h, sepia());
        assert_eq!(swatch.len(), 1, "one sepia square: {swatch:?}");
        h.click(swatch[0].center());
        // Until the fade to it has ended.
        h.frames(30, vec![]);
        let saved = SettingsFile::at(dir.path()).load().unwrap();
        assert_eq!(saved.paper, noido_core::Paper::Sepia);
        let tints = image_tints(&h);
        assert!(!tints.is_empty());
        assert!(tints.iter().all(|&tint| tint == sepia()), "{tints:?}");
    }

    #[test]
    fn hovering_paper_previews_it_without_saving() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = settings_in(dir.path());
        h.until(|h| !images(h).is_empty());
        open_settings(&mut h);
        h.frames(30, vec![]);
        let swatch = filled_with(&h, sepia())[0];
        // Until the fade to it has ended.
        h.frames(30, vec![Event::PointerMoved(swatch.center())]);
        let tints = image_tints(&h);
        assert!(!tints.is_empty());
        assert!(tints.iter().all(|&tint| tint == sepia()), "{tints:?}");
        let away = h.text_rect("Close").unwrap();
        h.frames(30, vec![Event::PointerMoved(away.center())]);
        let tints = image_tints(&h);
        assert!(
            tints.iter().all(|&tint| tint == Color32::WHITE),
            "{tints:?}"
        );
        assert_eq!(h.app.settings.paper, noido_core::Paper::White);
        assert!(!SettingsFile::at(dir.path()).path().exists(), "saved");
    }

    #[test]
    fn hover_change_on_paper_row_asks_for_a_frame() {
        let mut h = Harness::new();
        open_settings(&mut h);
        h.frames(30, vec![]);
        let sepia = filled_with(&h, sepia())[0];
        let grey = filled_with(&h, paper_colour(noido_core::Paper::Grey))[0];
        let close = h.text_rect("Close").unwrap();
        // The page is drawn before the settings page sees a move, so the
        // preview and the scrim change a frame later. egui asks for that
        // frame after any pointer input; were it not to, they would stick.
        for event in [
            Event::PointerMoved(grey.center()),
            Event::PointerMoved(close.center()),
            Event::PointerMoved(sepia.center()),
            Event::PointerGone,
        ] {
            h.frames(1, vec![event.clone()]);
            assert_eq!(h.repaint_delay, Duration::ZERO, "after {event:?}");
            h.frames(30, vec![]);
        }
        assert_eq!(h.app.paper(), noido_core::Paper::White);
    }

    #[test]
    fn paper_squares_show_every_paper_in_listed_order() {
        let mut h = Harness::new();
        open_settings(&mut h);
        h.frames(30, vec![]);
        // Every coloured square in the dialog, left to right as painted.
        let mut row: Vec<_> = noido_core::Paper::ALL
            .into_iter()
            .flat_map(|paper| {
                filled_with(&h, paper_colour(paper))
                    .into_iter()
                    .filter(|rect| rect.width() == 20.0 && rect.height() == 20.0)
                    .map(move |rect| (paper, rect.center()))
            })
            .collect();
        row.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        // Written out, not read from `Paper::ALL`: the row is what the user
        // asked for, so a reordering should fail here rather than agree.
        let expected = [
            noido_core::Paper::White,
            noido_core::Paper::Sepia,
            noido_core::Paper::Bisque,
            noido_core::Paper::Sand,
            noido_core::Paper::Grey,
            noido_core::Paper::DarkGrey,
            noido_core::Paper::DarkerGrey,
        ];
        let order: Vec<_> = row.iter().map(|&(paper, _)| paper).collect();
        assert_eq!(order, expected, "squares in the wrong order");
        assert!(
            row.windows(2).all(|pair| pair[0].1.y == pair[1].1.y),
            "not one row: {row:?}"
        );
    }

    #[test]
    fn between_paper_squares_previews_one_of_them() {
        let mut h = Harness::new();
        open_settings(&mut h);
        h.frames(30, vec![]);
        let sepia = filled_with(&h, sepia())[0];
        let bisque = filled_with(&h, paper_colour(noido_core::Paper::Bisque))[0];
        assert!(sepia.max.x < bisque.min.x, "squares touch");
        let between = pos2((sepia.max.x + bisque.min.x) / 2.0, sepia.center().y);
        h.frames(2, vec![Event::PointerMoved(between)]);
        let preview = h.app.paper();
        assert!(
            [noido_core::Paper::Sepia, noido_core::Paper::Bisque].contains(&preview),
            "{preview:?}"
        );
    }

    #[test]
    fn paper_fades_to_the_hovered_one() {
        let mut h = Harness::new();
        h.until(|h| !images(h).is_empty());
        open_settings(&mut h);
        h.frames(30, vec![]);
        let swatch = filled_with(&h, sepia())[0];
        // A frame to see the hover, one to start the fade.
        h.frames(3, vec![Event::PointerMoved(swatch.center())]);
        let tint = image_tints(&h)[0];
        assert!(tint != Color32::WHITE && tint != sepia(), "{tint:?}");
        h.frames(30, vec![]);
        assert_eq!(image_tints(&h)[0], sepia());
    }

    /// The dimming drawn over the whole window, if any.
    fn scrim(h: &Harness) -> Option<Color32> {
        let window = Rect::from_min_size(pos2(0.0, 0.0), h.size);
        h.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) if r.rect == window && r.fill.a() > 0 => Some(r.fill),
            _ => None,
        })
    }

    #[test]
    fn hovering_paper_row_clears_scrim() {
        let mut h = Harness::new();
        open_settings(&mut h);
        h.frames(30, vec![]);
        assert!(scrim(&h).is_some(), "no scrim to begin with");
        let label = h.text_rect("Paper").unwrap();
        h.frames(30, vec![Event::PointerMoved(label.center())]);
        assert_eq!(scrim(&h), None);
        let away = h.text_rect("Close").unwrap();
        h.frames(30, vec![Event::PointerMoved(away.center())]);
        assert!(scrim(&h).is_some(), "scrim not back");
    }

    #[test]
    fn sidebar_draws_lowres_without_sharp() {
        let mut h = Harness::build(|app| {
            // Fails to open: no low-res thumbnails arrive to replace ours.
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            app.with_thumbnails(thumbnailer)
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        h.app
            .thumbnails
            .as_mut()
            .unwrap()
            .textures
            .insert(0, lowres.clone());
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let (_, first) = h.app.sidebar_view.drawn()[0];
        let shown = h.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) => {
                mesh.texture_id == lowres.texture.id()
                    && first.contains(clipped.shape.visual_bounding_rect().center())
            }
            _ => false,
        });
        assert!(shown);
    }

    #[test]
    fn sidebar_render_fades_in_over_lowres() {
        let (gate, gate_rx) = std::sync::mpsc::channel::<()>();
        let gated = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(gate_rx))));
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            let sidebar = Renderer::start_background(gated, Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
                .with_sidebar_renderer(sidebar)
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        h.app
            .thumbnails
            .as_mut()
            .unwrap()
            .textures
            .insert(0, lowres.clone());
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let (_, first) = h.app.sidebar_view.drawn()[0];
        assert_eq!(drawn_textures(&h, Some(first)), [lowres.texture.id()]);
        drop(gate);
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&0));
        h.frames(1, vec![]);
        let drawn = drawn_textures(&h, Some(first));
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert_eq!(drawn[0], lowres.texture.id(), "lowres under the render");
        assert!(
            h.repaint_delay < Duration::from_millis(20),
            "no repaint while fading"
        );
        h.frames(FADE_FRAMES, vec![]);
        let drawn = drawn_textures(&h, Some(first));
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert_ne!(drawn[0], lowres.texture.id());
    }

    #[test]
    fn sidebar_render_before_lowres_does_not_fade() {
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            let sidebar = Renderer::start_background(
                Arc::new(BlankEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            app.with_thumbnails(thumbnailer)
                .with_sidebar_renderer(sidebar)
        });
        h.press(egui::Key::F9, Modifiers::NONE);
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&0));
        let lowres = blank_texture(&h.ctx, "lowres");
        h.app
            .thumbnails
            .as_mut()
            .unwrap()
            .textures
            .insert(0, lowres.clone());
        h.frames(1, vec![]);
        let (_, first) = h.app.sidebar_view.drawn()[0];
        let drawn = drawn_textures(&h, Some(first));
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert_ne!(drawn[0], lowres.texture.id());
    }

    /// An engine that opens nothing.
    struct UnopenableEngine;

    impl PdfEngine for UnopenableEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Err(EngineError::Open("stub".into()))
        }
    }

    /// A harness on `engine`'s pages with the sidebar open, no sharp renders.
    fn sidebar_on(engine: Arc<dyn PdfEngine>) -> Harness {
        let mut h = Harness::build_with(engine, |app| app);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        h
    }

    /// Clicks the thumbnail of `page`, which must be in view.
    #[test]
    fn sidebar_uses_the_sidebar_fill_and_selection() {
        let mut h = Harness::new();
        h.app.sidebar.open = true;
        h.frames(2, vec![]);
        let palette = h.theme().palette(egui::Theme::Dark);
        assert!(!filled_deep(&h, palette.sidebar).is_empty());
        assert!(!filled_deep(&h, palette.sidebar_selection).is_empty());
    }

    fn click_thumbnail(h: &mut Harness, page: usize) {
        let (_, rect) = *h
            .app
            .sidebar_view
            .drawn()
            .iter()
            .find(|(p, _)| *p == page)
            .unwrap_or_else(|| panic!("page {page} not in view: {:?}", drawn_pages(h)));
        h.click(rect.center());
        h.frames(2, vec![]);
    }

    /// Page `page`'s thumbnail rect, which must be in view.
    fn thumb_rect(h: &Harness, page: usize) -> Rect {
        h.app
            .sidebar_view
            .drawn()
            .iter()
            .find(|(p, _)| *p == page)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("page {page} not drawn: {:?}", drawn_pages(h)))
    }

    /// Page `page`'s pin chip rect, which must be in view.
    fn pin_rect(h: &Harness, page: usize) -> Rect {
        h.app
            .sidebar_view
            .pin_icons()
            .iter()
            .find(|(p, _, _)| *p == page)
            .map(|(_, rect, _)| *rect)
            .unwrap_or_else(|| panic!("page {page}'s pin not drawn"))
    }

    #[test]
    fn every_visible_thumbnail_has_a_pin_in_its_corner() {
        let h = sidebar_on(Arc::new(BlankEngine));
        let drawn = drawn_pages(&h);
        assert!(!drawn.is_empty());
        for page in drawn {
            let (thumb, pin) = (thumb_rect(&h, page), pin_rect(&h, page));
            assert!(thumb.contains_rect(pin), "{page}: {pin:?} in {thumb:?}");
            assert!(
                pin.center().x > thumb.center().x,
                "{page}: not at the right"
            );
            assert!(pin.center().y < thumb.center().y, "{page}: not at the top");
        }
    }

    #[test]
    fn clicking_a_pin_pins_without_navigating() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        h.click(pin_rect(&h, 2).center());
        h.frames(2, vec![]);
        assert!(h.app.pins.contains(2));
        assert_eq!(h.app.chosen, None, "it navigated");
        assert_eq!(h.app.current_page(), Some(0));
        // Again unpins.
        h.click(pin_rect(&h, 2).center());
        h.frames(2, vec![]);
        assert!(!h.app.pins.contains(2));
    }

    #[test]
    fn a_pinned_pins_ink_is_the_accent() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        let ink = |h: &Harness, page: usize| {
            h.app
                .sidebar_view
                .pin_inks()
                .iter()
                .find(|(p, _)| *p == page)
                .map(|(_, colour)| *colour)
                .unwrap_or_else(|| panic!("page {page}'s ink not recorded"))
        };
        let accent = h.ctx.global_style().visuals.selection.stroke.color;
        h.click(pin_rect(&h, 2).center());
        h.frames(2, vec![]);
        assert_eq!(ink(&h, 2), accent, "a pinned page's pin");
        assert_ne!(ink(&h, 1), accent, "an unpinned page's pin");
    }

    #[test]
    fn hovering_a_pin_brightens_its_chip() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        let pin = pin_rect(&h, 2);
        let chip_fill = |h: &Harness| {
            h.shapes.iter().find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(r) if r.rect == pin => Some(r.fill),
                _ => None,
            })
        };
        let resting = chip_fill(&h).expect("a chip");
        h.frames(1, vec![Event::PointerMoved(pin.center())]);
        h.frames(1, vec![]);
        let hovered = chip_fill(&h).expect("a chip");
        assert_ne!(hovered, resting, "the chip did not brighten");
        assert_eq!(h.cursor, egui::CursorIcon::PointingHand);
    }

    #[test]
    fn a_card_click_pins_nothing() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        // The middle of the card, clear of the pin.
        h.click(thumb_rect(&h, 2).center());
        h.frames(2, vec![]);
        assert!(h.app.pins.pages().is_empty(), "the card click pinned");
        assert_eq!(h.app.current_page(), Some(2));
    }

    #[test]
    fn pins_survive_a_saved_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 6,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::from_sorted([2, 7]),
            pagelet: None,
        };
        let h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        assert_eq!(h.app.pins.pages(), [2, 7]);
    }

    /// A harness with pins set, the sidebar open, on the Pins tab.
    fn pinned_on(pins: &[usize]) -> Harness {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        h.app.pins = Pins::from_sorted(pins.iter().copied());
        click_tab(&mut h, SidebarTab::Pins);
        h
    }

    #[test]
    fn four_tabs_fit_their_shares_at_the_narrowest_sidebar() {
        // The width must be set before the panel's first show: afterwards
        // its own persisted width wins.
        let mut h = Harness::build_with(Arc::new(BlankEngine), |mut app| {
            app.sidebar.open = true;
            app.sidebar.width = noido_core::SIDEBAR_MIN_WIDTH;
            app
        });
        h.frames(3, vec![]);
        assert_eq!(
            h.app.sidebar.width,
            noido_core::SIDEBAR_MIN_WIDTH,
            "the panel took another width"
        );
        let tabs = h.app.sidebar_view.tab_rects().to_vec();
        assert_eq!(tabs.len(), 4);
        for (tab, rect) in tabs {
            let label = h
                .latest_texts()
                .into_iter()
                .find(|(t, r)| rect.contains_rect(*r) && !t.is_empty())
                .unwrap_or_else(|| panic!("no label in {tab:?}"));
            assert!(
                rect.contains_rect(label.1),
                "{tab:?}: {:?} outside {rect:?}",
                label.0
            );
        }
    }

    #[test]
    fn the_pins_tab_shows_only_the_pinned_pages() {
        let h = pinned_on(&[2, 7]);
        assert_eq!(drawn_pages(&h), [2, 7]);
        // Each card carries its pin too, so it can be unpinned here.
        let icons = h.app.sidebar_view.pin_icons();
        assert_eq!(icons.len(), 2);
        assert!(icons.iter().all(|(_, _, pinned)| *pinned));
    }

    #[test]
    fn clicking_a_pins_pin_unpins_it_without_previewing() {
        let mut h = pinned_on(&[2]);
        h.click(pin_rect(&h, 2).center());
        // Longer than the double-click window.
        h.frames(40, vec![]);
        assert!(!h.app.pins.contains(2));
        assert!(h.app.pin_preview.is_none(), "it previewed");
        assert!(drawn_pages(&h).is_empty(), "the card stayed");
    }

    #[test]
    fn an_empty_pins_tab_says_how_to_pin() {
        let h = pinned_on(&[]);
        assert!(
            h.texts()
                .iter()
                .any(|t| t == "No pins yet. Pin pages from the Pages tab.")
        );
        assert!(drawn_pages(&h).is_empty());
    }

    #[test]
    fn the_pins_tab_marks_the_current_page() {
        let mut h = pinned_on(&[2, 7]);
        click_tab(&mut h, SidebarTab::Thumbnails);
        click_thumbnail(&mut h, 2);
        click_tab(&mut h, SidebarTab::Pins);
        let fill = h.ctx.global_style().visuals.selection.bg_fill;
        let card = thumb_rect(&h, 2);
        assert!(filled_with(&h, fill).iter().any(|r| r.intersects(card)));
    }

    #[test]
    fn the_pins_tab_keeps_the_pages_renders() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        let sharp = sharp_settled(&mut h);
        h.app.pins = Pins::from_sorted([8, 9]);
        click_tab(&mut h, SidebarTab::Pins);
        h.until(|h| h.app.sidebar_view.pinned_kept().len() == 2);
        assert_eq!(h.app.sidebar_view.pinned_kept(), &[8, 9]);
        // The Pages tab's sharp thumbnails are still there.
        for page in sharp {
            assert!(h.app.sidebar_view.sharp_pages().contains(&page), "{page}");
        }
        // Once the pinned pages' renders have settled, going back to Pages
        // asks for nothing again.
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&9));
        h.frames(3, vec![]);
        let renders = engine.recorded().len();
        click_tab(&mut h, SidebarTab::Thumbnails);
        h.frames(5, vec![]);
        assert_eq!(engine.recorded().len(), renders, "asked for Pages again");
    }

    #[test]
    fn the_pins_tab_fades_a_sharp_render_in() {
        let (gate, waiting) = std::sync::mpsc::channel::<()>();
        let engine = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(waiting))));
        let mut h = Harness::build(|app| {
            let thumbnailer = Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            let sidebar = Renderer::start_background(engine, Arc::new(Vec::new()), Box::new(|| {}));
            app.with_thumbnails(thumbnailer)
                .with_sidebar_renderer(sidebar)
        });
        let lowres = blank_texture(&h.ctx, "lowres");
        let textures = &mut h.app.thumbnails.as_mut().unwrap().textures;
        for page in 0..10 {
            textures.insert(page, lowres.clone());
        }
        h.app.pins = Pins::from_sorted([0]);
        h.app.sidebar.open = true;
        h.frames(2, vec![]);
        click_tab(&mut h, SidebarTab::Pins);
        h.frames(2, vec![]);
        assert!(h.app.sidebar_view.sharp_pages().is_empty());
        drop(gate);
        let faded = std::cell::Cell::new(false);
        h.until(|h| {
            faded.set(faded.get() || h.app.sidebar_view.fading(0));
            h.app.sidebar_view.sharp_pages().contains(&0)
        });
        assert!(h.app.sidebar_view.sharp_pages().contains(&0));
        assert!(faded.get(), "the pinned page popped in without a fade");
    }

    #[test]
    fn the_pagelet_asks_for_its_page_at_the_panels_scale() {
        let engine = RecordingEngine::default();
        let mut h = Harness::build_with(Arc::new(engine.clone()), |app| app);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.app.pins = Pins::from_sorted([5]);
        click_tab(&mut h, SidebarTab::Pins);
        h.click(thumb_rect(&h, 5).center());
        wait_for_pagelet(&mut h, 5);
        let ppp = h.ctx.pixels_per_point();
        // The panel's page rect settles after its first frame; wait for a
        // render at the scale the settled rect calls for.
        h.until(|h| {
            let Some(rect) = h.app.pagelet_page_rect() else {
                return false;
            };
            let page = h.app.view.layout().page_size(5).unwrap();
            let wanted = rect.width() / page.width * h.ctx.pixels_per_point();
            engine
                .recorded()
                .iter()
                .any(|(page, scale)| *page == 5 && (*scale - wanted).abs() < 0.05 * wanted)
        });
        let page = h.app.view.layout().page_size(5).unwrap();
        let rect = h.app.pagelet_page_rect().unwrap();
        let wanted = rect.width() / page.width * ppp;
        let recorded = engine.recorded();
        let scales: Vec<f32> = recorded
            .iter()
            .filter(|(p, _)| *p == 5)
            .map(|(_, s)| *s)
            .collect();
        assert!(!scales.is_empty(), "page 5 never rendered");
        assert!(
            scales.iter().any(|s| (s - wanted).abs() < 0.05 * wanted),
            "{scales:?} vs {wanted}"
        );
    }

    #[test]
    fn the_pagelet_keeps_its_page_far_from_the_view() {
        let engine = RecordingEngine::default();
        let mut h = Harness::build_with(Arc::new(engine.clone()), |app| app);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.app.pins = Pins::from_sorted([9]);
        click_tab(&mut h, SidebarTab::Pins);
        h.click(thumb_rect(&h, 9).center());
        wait_for_pagelet(&mut h, 9);
        h.until(|h| h.app.textures.contains_key(&9));
        // The main view stays on page 0: the pagelet's page stays sharp.
        h.frames(10, vec![]);
        assert!(
            h.app.textures.contains_key(&9),
            "the pagelet's page was dropped"
        );
        // Closed, it is let go like any page off the keep.
        let rect = h.app.pin_preview.as_ref().unwrap().rect;
        let chip = egui::pos2(
            rect.max.x - PAGELET_PADDING - PAGELET_CLOSE / 2.0,
            rect.min.y + PAGELET_PADDING + PAGELET_CLOSE / 2.0,
        );
        h.click(chip);
        h.frames(5, vec![]);
        h.until(|h| !h.app.textures.contains_key(&9));
        assert!(!h.app.textures.contains_key(&9));
    }

    #[test]
    fn a_page_both_visible_and_previewed_settles_on_one_scale() {
        let engine = RecordingEngine::default();
        let mut h = Harness::build_with(Arc::new(engine.clone()), |app| app);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.app.pins = Pins::from_sorted([0]);
        click_tab(&mut h, SidebarTab::Pins);
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        zoomed_out(&mut h);
        h.frames(30, vec![]);
        // Settled: no further renders of it.
        let settled = engine.recorded().len();
        h.frames(30, vec![]);
        assert_eq!(engine.recorded().len(), settled, "still re-rendering");
        let last = engine
            .recorded()
            .into_iter()
            .rev()
            .find_map(|(page, scale)| (page == 0).then_some(scale))
            .expect("page 0 rendered");
        let panel = h.app.pagelet_page_rect().unwrap().width() / 612.0 * h.ctx.pixels_per_point();
        let display = h.app.view.scale() * h.ctx.pixels_per_point();
        let wanted = panel.max(display);
        assert!((last - wanted).abs() < 0.1 * wanted, "{last} vs {wanted}");
    }

    /// The alpha of the image mesh drawn over `rect` in the last frame.
    fn image_alpha(h: &Harness, rect: Rect) -> Option<u8> {
        fn find(shape: &egui::Shape, rect: Rect) -> Option<u8> {
            match shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    let points: Vec<egui::Pos2> =
                        mesh.vertices.iter().map(|vertex| vertex.pos).collect();
                    let drawn = egui::Rect::from_points(&points);
                    let same = (drawn.min - rect.min).length() < 1.5
                        && (drawn.max - rect.max).length() < 1.5;
                    same.then(|| mesh.vertices[0].color.a())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, rect)),
                _ => None,
            }
        }
        h.shapes.iter().find_map(|c| find(&c.shape, rect))
    }

    /// The alpha of the pagelet's surface (its frame fill) last frame.
    fn surface_alpha(h: &Harness, rect: Rect) -> Option<u8> {
        fn find(shape: &egui::Shape, rect: Rect) -> Option<u8> {
            match shape {
                egui::Shape::Rect(r)
                    if (r.rect.min - rect.min).length() < 1.5
                        && (r.rect.max - rect.max).length() < 1.5
                        && r.fill.a() > 0 =>
                {
                    Some(r.fill.a())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, rect)),
                _ => None,
            }
        }
        h.shapes.iter().find_map(|c| find(&c.shape, rect))
    }

    #[test]
    fn the_pagelet_fades_to_half_off_the_pointer() {
        let mut h = pinned_on(&[0]);
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        h.until(|h| h.app.textures.contains_key(&0));
        let page = h.app.pagelet_page_rect().unwrap();
        // Away from the panel and the sidebar.
        let away = h.app.last_scroll.unwrap().inner.max - vec2(30.0, 30.0);
        h.frames(1, vec![Event::PointerMoved(away)]);
        h.frames(20, vec![]);
        assert_eq!(image_alpha(&h, page), Some(128), "off the pointer");
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        assert_eq!(
            surface_alpha(&h, panel),
            Some(128),
            "the surface stayed opaque"
        );
        // On the panel: opaque.
        let at = panel.center();
        h.frames(1, vec![Event::PointerMoved(at)]);
        h.frames(20, vec![]);
        assert_eq!(image_alpha(&h, page), Some(255), "on the pointer");
        assert_eq!(
            surface_alpha(&h, panel),
            Some(255),
            "the surface stayed faded"
        );
    }

    #[test]
    fn the_pagelet_stays_opaque_over_the_sidebar() {
        let mut h = pinned_on(&[0]);
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        h.until(|h| h.app.textures.contains_key(&0));
        let page = h.app.pagelet_page_rect().unwrap();
        // The pointer rests on the card it clicked: in the sidebar.
        h.frames(20, vec![]);
        assert_eq!(image_alpha(&h, page), Some(255), "faded over the sidebar");
        let view = h.app.last_scroll.unwrap().inner;
        let sidebar = egui::pos2(view.min.x - 30.0, view.min.y + 40.0);
        h.frames(1, vec![Event::PointerMoved(sidebar)]);
        h.frames(20, vec![]);
        assert_eq!(image_alpha(&h, page), Some(255), "faded over the sidebar");
        // Away from both, it fades.
        let away = view.max - vec2(30.0, 30.0);
        h.frames(1, vec![Event::PointerMoved(away)]);
        h.frames(20, vec![]);
        assert_eq!(image_alpha(&h, page), Some(128), "stayed opaque off it");
    }

    #[test]
    fn a_double_click_leaves_an_open_pagelet() {
        let mut h = pinned_on(&[2, 5]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        let at = thumb_rect(&h, 5).center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(at)]);
        for _ in 0..2 {
            h.frames(1, vec![button(true)]);
            h.frames(1, vec![button(false)]);
        }
        h.frames(40, vec![]);
        assert_eq!(h.app.current_page(), Some(5), "it did not navigate");
        assert!(
            pagelet_on(&h, 2),
            "the pagelet closed or changed page: {:?}",
            h.app.pin_preview.as_ref().map(|p| p.page)
        );
    }

    #[test]
    fn the_pagelets_page_fades_in_without_a_pointer_move() {
        // A render that arrives after the window's own opening animation:
        // only the page's fade can ask for the frames that finish it.
        let (gate, waiting) = std::sync::mpsc::channel::<()>();
        let engine = Arc::new(GatedEngine(Arc::new(std::sync::Mutex::new(waiting))));
        let mut h = Harness::build_with(engine, |app| {
            app.with_thumbnails(Thumbnailer::start(
                Arc::new(UnopenableEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        });
        let lowres = blank_texture(&h.ctx, "lowres 9");
        h.app
            .thumbnails
            .as_mut()
            .unwrap()
            .textures
            .insert(9, lowres);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.app.pins = Pins::from_sorted([9]);
        click_tab(&mut h, SidebarTab::Pins);
        h.frames(2, vec![]);
        h.click(thumb_rect(&h, 9).center());
        wait_for_pagelet(&mut h, 9);
        // Past the window's own fade-in, the page still waiting.
        h.frames(20, vec![]);
        assert!(!h.app.textures.contains_key(&9));
        drop(gate);
        h.until(|h| h.app.textures.contains_key(&9));
        h.frames(3, vec![]);
        assert!(h.app.fades.started(9), "no fade to watch");
        assert_eq!(
            h.repaint_delay,
            Duration::ZERO,
            "the pagelet's fade asked for no repaint"
        );
        // And the page ends up sharp: the pointer is in the sidebar, so the
        // panel is opaque.
        h.frames(15, vec![]);
        let page = h.app.pagelet_page_rect().unwrap();
        assert_eq!(image_alpha(&h, page), Some(255));
    }

    #[test]
    fn reduced_motion_swaps_the_fade_at_once() {
        let mut h = pinned_on(&[0]).reduced_motion();
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        h.until(|h| h.app.textures.contains_key(&0));
        // Past the page's own fade-in, then away from the panel and the
        // sidebar: with reduced motion the panel is half at once.
        h.frames(20, vec![]);
        let away = h.app.last_scroll.unwrap().inner.max - vec2(30.0, 30.0);
        h.frames(1, vec![Event::PointerMoved(away)]);
        h.frames(1, vec![]);
        let page = h.app.pagelet_page_rect().unwrap();
        assert_eq!(image_alpha(&h, page), Some(128));
    }

    /// A harness with a link engine, the sidebar open on the Pins tab with
    /// `pins`, and the pagelet on `page` once its links are in.
    fn pagelet_linking(pins: &[usize], page: usize) -> Harness {
        let engine = link_engine();
        let linker = engine.clone();
        let mut h = Harness::build_with(Arc::new(engine), move |mut app| {
            app.sidebar.open = true;
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        });
        h.app.pins = Pins::from_sorted(pins.iter().copied());
        click_tab(&mut h, SidebarTab::Pins);
        h.click(thumb_rect(&h, page).center());
        wait_for_pagelet(&mut h, page);
        h.until(|h| h.app.links.at(page, 100.0, 96.0).is_some());
        h
    }

    /// Where page point (`x`, `y`) of the pagelet's page is on screen.
    fn pagelet_pos(h: &Harness, x: f32, y: f32) -> egui::Pos2 {
        let page = h
            .app
            .view
            .layout()
            .page_size(h.app.pin_preview.as_ref().unwrap().page)
            .unwrap();
        let fitted = h.app.pagelet_page_rect().unwrap();
        let scale = fitted.width() / page.width;
        fitted.min + vec2(x * scale, y * scale)
    }

    #[test]
    fn a_link_in_the_pagelet_moves_the_main_view() {
        let mut h = pagelet_linking(&[0], 0);
        let at = pagelet_pos(&h, 100.0, 96.0);
        hover_link(&mut h, at);
        h.click(at);
        h.frames(3, vec![]);
        assert_eq!(h.app.chosen, Some(5));
        assert_eq!(h.app.history.back_entries().len(), 1);
        assert!(h.app.pin_preview.is_some(), "the pagelet closed");
        assert_eq!(h.app.pin_preview.as_ref().unwrap().page, 0);
        // The main view is scrolled onto the target, not just marked.
        assert_eq!(h.centre().page, 5);
    }

    #[test]
    fn a_link_in_the_pagelet_lights_up_on_hover() {
        let mut h = pagelet_linking(&[0], 0);
        let at = pagelet_pos(&h, 100.0, 96.0);
        hover_link(&mut h, at);
        // Past the window's own fade-in, which dims everything it paints.
        h.frames(10, vec![]);
        let outlines = link_outlines(&h);
        assert!(!outlines.is_empty(), "no outline under the pointer");
        // The pagelet's link, at the pagelet's scale.
        let page = h.app.pagelet_page_rect().unwrap();
        let scale = page.width() / 612.0;
        let wanted = Rect::from_min_size(
            page.min + vec2(72.0, 90.0) * scale,
            vec2(60.0, 12.0) * scale,
        );
        assert!(
            outlines
                .iter()
                .any(|r| (r.center() - wanted.center()).length() < 1.5),
            "{outlines:?} vs {wanted:?}"
        );
    }

    #[test]
    fn a_web_link_in_the_pagelet_opens_it() {
        let mut h = pagelet_linking(&[0], 0);
        let at = pagelet_pos(&h, 100.0, 206.0);
        h.click(at);
        h.frames(2, vec![]);
        assert_eq!(h.opened, vec!["https://example.org/".to_owned()]);
    }

    #[test]
    fn a_click_in_the_pagelet_outside_its_links_does_nothing() {
        let mut h = pagelet_linking(&[0], 0);
        // A point on the pagelet's page that is not one of its links: in a
        // bare part of the page, over the page drawn beneath the panel.
        let at = pagelet_pos(&h, 300.0, 700.0);
        h.frames(1, vec![Event::PointerMoved(at)]);
        h.frames(1, vec![]);
        // The pages beneath take no hover from the pointer over the panel.
        assert!(h.app.hovered_link.is_none(), "the view's link was hovered");
        assert!(!h.app.select.pages_hovered, "the pages were hovered");
        h.click(at);
        h.frames(2, vec![]);
        assert_eq!(h.app.chosen, None, "reached the view beneath");
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn the_pins_keep_does_not_outlive_a_shorter_reload() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine);
        h.app.pins = Pins::from_sorted([9]);
        click_tab(&mut h, SidebarTab::Pins);
        h.frames(3, vec![]);
        h.reload(Arc::new(SizedEngine::letter(3)), recompiled_key());
        h.frames(3, vec![]);
        let keep = h.app.sidebar_view.renderer_keep();
        assert!(keep.end <= 3, "{keep:?}");
    }

    #[test]
    fn pins_off_the_thumbnails_window_are_rendered_and_kept() {
        let engine = RecordingEngine::default();
        let mut h = with_sidebar(engine.clone());
        h.app.pins = Pins::from_sorted([9]);
        click_tab(&mut h, SidebarTab::Pins);
        h.until(|h| h.app.sidebar_view.sharp_pages().contains(&9));
        assert!(h.app.sidebar_view.pinned_kept().contains(&9));
        assert!(engine.pages().contains(&9));
    }

    /// The pagelet has opened on `page`.
    fn pagelet_on(h: &Harness, page: usize) -> bool {
        h.app.pin_preview.as_ref().is_some_and(|p| p.page == page)
    }

    /// Runs frames until the pagelet opens, for at most a second.
    fn wait_for_pagelet(h: &mut Harness, page: usize) {
        h.until(|h| pagelet_on(h, page));
        assert!(pagelet_on(h, page), "pagelet on {page} never opened");
        // The first drawn frame settles the panel's rect.
        h.frames(2, vec![]);
    }

    #[test]
    fn a_single_click_opens_the_pagelet_after_the_double_click_window() {
        let mut h = pinned_on(&[5]);
        let card = thumb_rect(&h, 5);
        h.click(card.center());
        // Not at once: the click may still become a double click.
        h.frames(2, vec![]);
        assert!(
            h.app.pin_preview.is_none(),
            "opened before the window passed"
        );
        wait_for_pagelet(&mut h, 5);
        assert_eq!(h.app.chosen, None, "it navigated");
        assert_eq!(h.app.current_page(), Some(0));
        // The page is fitted with its aspect kept.
        let fitted = h.app.pagelet_page_rect().unwrap();
        assert!(
            (fitted.width() / fitted.height() - 612.0 / 792.0).abs() < 0.01,
            "{fitted:?}"
        );
    }

    #[test]
    fn a_double_click_goes_there_and_never_opens_the_pagelet() {
        let mut h = pinned_on(&[5]);
        let at = thumb_rect(&h, 5).center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(at)]);
        for _ in 0..2 {
            h.frames(1, vec![button(true)]);
            h.frames(1, vec![button(false)]);
        }
        for _ in 0..40 {
            h.frames(1, vec![]);
            assert!(h.app.pin_preview.is_none(), "the pagelet opened");
        }
        assert_eq!(h.app.current_page(), Some(5));
        assert_eq!(h.app.chosen, Some(5));
    }

    #[test]
    fn the_close_chip_closes_the_pagelet() {
        let mut h = pinned_on(&[5]);
        h.click(thumb_rect(&h, 5).center());
        wait_for_pagelet(&mut h, 5);
        let rect = h.app.pin_preview.as_ref().unwrap().rect;
        let chip = egui::pos2(
            rect.max.x - PAGELET_PADDING - PAGELET_CLOSE / 2.0,
            rect.min.y + PAGELET_PADDING + PAGELET_CLOSE / 2.0,
        );
        h.click(chip);
        h.frames(2, vec![]);
        assert!(h.app.pin_preview.is_none(), "the chip did not close it");
    }

    #[test]
    fn a_pin_click_opens_no_pagelet() {
        let mut h = pinned_on(&[]);
        click_tab(&mut h, SidebarTab::Thumbnails);
        h.click(pin_rect(&h, 2).center());
        h.frames(40, vec![]);
        assert!(h.app.pins.contains(2), "the pin did not pin");
        assert!(h.app.pin_preview.is_none(), "the pin opened the pagelet");
    }

    #[test]
    fn there_is_one_pagelet_and_it_keeps_its_place() {
        let mut h = pinned_on(&[2, 5]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        let rect = h.app.pin_preview.as_ref().unwrap().rect;
        h.drag(rect.min + vec2(40.0, 40.0), vec2(60.0, 40.0));
        h.frames(2, vec![]);
        let mine = h.app.pin_preview.as_ref().unwrap().rect;
        assert_ne!(mine, rect, "the panel did not move");
        // Another pin swaps the page; the one pagelet keeps its place.
        h.click(thumb_rect(&h, 5).center());
        wait_for_pagelet(&mut h, 5);
        assert_eq!(h.app.pin_preview.as_ref().unwrap().rect, mine);
    }

    #[test]
    fn a_first_pagelet_sits_in_the_documents_corner() {
        let mut h = pinned_on(&[2]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        h.frames(1, vec![]);
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        let view = h.app.last_scroll.unwrap().inner;
        assert!(
            (panel.min.x - view.min.x).abs() < 1.0,
            "{panel:?} vs {view:?}"
        );
        assert!(
            (panel.min.y - view.min.y).abs() < 1.0,
            "{panel:?} vs {view:?}"
        );
    }

    #[test]
    fn the_open_pagelet_is_saved_with_the_document() {
        let mut h = pinned_on(&[2]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        assert_eq!(
            h.app.doc_state().unwrap().pagelet.unwrap().page,
            Some(2),
            "an open pagelet is saved with its page"
        );
        let rect = h.app.pin_preview.as_ref().unwrap().rect;
        let chip = egui::pos2(
            rect.max.x - PAGELET_PADDING - PAGELET_CLOSE / 2.0,
            rect.min.y + PAGELET_PADDING + PAGELET_CLOSE / 2.0,
        );
        h.click(chip);
        h.frames(2, vec![]);
        assert_eq!(
            h.app.doc_state().unwrap().pagelet.unwrap().page,
            None,
            "a closed pagelet is saved shut"
        );
    }

    #[test]
    fn an_open_pagelet_is_restored_with_the_document() {
        let dir = tempfile::TempDir::new().unwrap();
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::from_sorted([2]),
            pagelet: Some(noido_state::Pagelet {
                x: 300.0,
                y: 120.0,
                width: 360.0,
                page: Some(2),
            }),
        };
        let h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        assert!(pagelet_on(&h, 2), "the pagelet did not reopen");
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        assert!((panel.min.x - 300.0).abs() < 1.0, "{panel:?}");
        assert!((panel.min.y - 120.0).abs() < 1.0, "{panel:?}");
        let page = h.app.pagelet_page_rect().unwrap();
        assert!((page.width() - 360.0).abs() < 1.0, "{page:?}");
    }

    #[test]
    fn a_restored_pagelet_whose_page_is_not_pinned_stays_shut() {
        let dir = tempfile::TempDir::new().unwrap();
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::from_sorted([2]),
            pagelet: Some(noido_state::Pagelet {
                x: 300.0,
                y: 120.0,
                width: 360.0,
                page: Some(9),
            }),
        };
        let h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        assert!(h.app.pin_preview.is_none(), "it opened an unpinned page");
    }

    #[test]
    fn the_pagelets_place_is_restored_with_the_document() {
        let dir = tempfile::TempDir::new().unwrap();
        let restored = DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::FitWidth,
                scale: 1.0,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page: 0,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::from_sorted([2]),
            pagelet: Some(noido_state::Pagelet {
                x: 300.0,
                y: 120.0,
                width: 360.0,
                page: None,
            }),
        };
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), Some(restored))
        });
        h.app.sidebar.open = true;
        h.frames(2, vec![]);
        click_tab(&mut h, SidebarTab::Pins);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        assert!((panel.min.x - 300.0).abs() < 1.0, "{panel:?}");
        assert!((panel.min.y - 120.0).abs() < 1.0, "{panel:?}");
        let page = h.app.pagelet_page_rect().unwrap();
        assert!((page.width() - 360.0).abs() < 1.0, "{page:?}");
    }

    /// The pagelet's resize grip's centre.
    fn grip_centre(h: &Harness) -> egui::Pos2 {
        h.app.pagelet_page_rect().unwrap().max - Vec2::splat(PAGELET_GRIP / 2.0)
    }

    #[test]
    fn resizing_the_pagelet_keeps_the_pages_aspect() {
        let mut h = pinned_on(&[0]);
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        let before = h.app.pagelet_page_rect().unwrap();
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        h.drag(grip_centre(&h), vec2(80.0, 20.0));
        h.frames(3, vec![]);
        let after = h.app.pagelet_page_rect().unwrap();
        eprintln!(
            "before={before:?} after={after:?} panel={:?}",
            h.app.pin_preview.as_ref().unwrap().rect
        );
        assert!(
            after.width() > before.width() + 10.0,
            "{before:?} -> {after:?}"
        );
        assert!(
            (after.width() / after.height() - 612.0 / 792.0).abs() < 0.01,
            "{after:?}"
        );
        let moved = h.app.pin_preview.as_ref().unwrap().rect;
        assert!(
            (moved.min - panel.min).length() < 1.0,
            "it moved: {panel:?} -> {moved:?}"
        );
    }

    /// Opens the pagelet on page 0 and returns its page rect.
    fn pagelet_page(h: &mut Harness) -> Rect {
        h.click(thumb_rect(h, 0).center());
        wait_for_pagelet(h, 0);
        h.app.pagelet_page_rect().unwrap()
    }

    #[test]
    fn resizing_from_the_left_edge_keeps_the_right_edge() {
        let mut h = pinned_on(&[0]);
        let before = pagelet_page(&mut h);
        let at = egui::pos2(before.min.x + 3.0, before.center().y);
        h.drag(at, vec2(-40.0, 0.0));
        h.frames(3, vec![]);
        let after = h.app.pagelet_page_rect().unwrap();
        assert!(
            (after.max.x - before.max.x).abs() < 1.0,
            "{before:?} -> {after:?}"
        );
        assert!(after.width() > before.width() + 20.0, "{after:?}");
        assert!((after.width() / after.height() - 612.0 / 792.0).abs() < 0.01);
    }

    #[test]
    fn resizing_from_the_right_edge_keeps_the_left_edge() {
        let mut h = pinned_on(&[0]);
        let before = pagelet_page(&mut h);
        let at = egui::pos2(before.max.x - 3.0, before.center().y);
        h.drag(at, vec2(40.0, 0.0));
        h.frames(3, vec![]);
        let after = h.app.pagelet_page_rect().unwrap();
        assert!(
            (after.min.x - before.min.x).abs() < 1.0,
            "{before:?} -> {after:?}"
        );
        assert!(after.width() > before.width() + 20.0, "{after:?}");
    }

    #[test]
    fn resizing_from_the_top_edge_keeps_the_bottom() {
        let mut h = pinned_on(&[0]);
        let before = pagelet_page(&mut h);
        let at = egui::pos2(before.center().x, before.min.y + 3.0);
        h.drag(at, vec2(0.0, -20.0));
        h.frames(3, vec![]);
        let after = h.app.pagelet_page_rect().unwrap();
        assert!(
            (after.max.y - before.max.y).abs() < 1.0,
            "{before:?} -> {after:?}"
        );
        assert!(after.height() > before.height() + 15.0, "{after:?}");
    }

    #[test]
    fn resizing_from_the_bottom_edge_keeps_the_top() {
        let mut h = pinned_on(&[0]);
        let before = pagelet_page(&mut h);
        let at = egui::pos2(before.center().x, before.max.y - 3.0);
        h.drag(at, vec2(0.0, 30.0));
        h.frames(3, vec![]);
        let after = h.app.pagelet_page_rect().unwrap();
        assert!(
            (after.min.y - before.min.y).abs() < 1.0,
            "{before:?} -> {after:?}"
        );
        assert!(after.height() > before.height() + 20.0, "{after:?}");
    }

    #[test]
    fn a_pagelet_over_the_sidebar_keeps_its_x() {
        let mut h = pinned_on(&[2]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        let rect = h.app.pin_preview.as_ref().unwrap().rect;
        // Move it left, over the sidebar.
        h.drag(rect.center(), vec2(-200.0, 0.0));
        h.frames(2, vec![]);
        let moved = h.app.pin_preview.as_ref().unwrap().rect;
        let view = h.app.last_scroll.unwrap().inner;
        assert!(
            moved.min.x < view.min.x,
            "not over the sidebar: {moved:?} vs {view:?}"
        );
        // Close it, then open it again as a click would.
        h.app.close_pagelet();
        h.frames(1, vec![]);
        let bounds = Rect::from_min_max(pos2(h.ctx.content_rect().min.x, view.min.y), view.max);
        h.app.open_pagelet(2, view, bounds, None);
        h.frames(1, vec![]);
        assert!(
            (h.app.pin_preview.as_ref().unwrap().rect.min.x - moved.min.x).abs() < 1.0,
            "moved when reopened"
        );
    }

    #[test]
    fn the_pagelet_stays_opaque_while_a_handle_is_dragged() {
        let mut h = pinned_on(&[0]);
        let _ = pagelet_page(&mut h);
        h.until(|h| h.app.textures.contains_key(&0));
        h.frames(20, vec![]);
        let page = h.app.pagelet_page_rect().unwrap();
        let corner = page.max - Vec2::splat(2.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(corner)]);
        h.frames(1, vec![button(corner, true)]);
        // Far past the window: the panel stops at its edge, the pointer
        // does not.
        let out = egui::pos2(1200.0, 1200.0);
        h.frames(1, vec![Event::PointerMoved(out)]);
        h.frames(20, vec![]);
        let panel = h.app.pin_preview.as_ref().unwrap().rect;
        assert!(!panel.contains(out), "{out:?} in {panel:?}");
        assert_eq!(
            image_alpha(&h, h.app.pagelet_page_rect().unwrap()),
            Some(255),
            "translucent while the border is dragged"
        );
        h.frames(1, vec![button(out, false)]);
        h.frames(20, vec![]);
        assert_eq!(
            image_alpha(&h, h.app.pagelet_page_rect().unwrap()),
            Some(128),
            "still opaque after the drag"
        );
    }

    #[test]
    fn resizing_stops_at_the_shortest_pagelet() {
        let mut h = pinned_on(&[0]);
        h.click(thumb_rect(&h, 0).center());
        wait_for_pagelet(&mut h, 0);
        h.drag(grip_centre(&h), vec2(-2000.0, -2000.0));
        h.frames(3, vec![]);
        let once = h.app.pagelet_page_rect().unwrap();
        h.drag(grip_centre(&h), vec2(-2000.0, -2000.0));
        h.frames(3, vec![]);
        let twice = h.app.pagelet_page_rect().unwrap();
        assert!(
            (twice.width() - once.width()).abs() < 0.5,
            "{once:?} -> {twice:?}"
        );
        assert!((twice.width() / twice.height() - 612.0 / 792.0).abs() < 0.01);
    }

    #[test]
    fn closing_and_reopening_keeps_the_pagelets_rect() {
        let mut h = pinned_on(&[2, 5]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        let first = h.app.pin_preview.as_ref().unwrap().rect;
        let chip = egui::pos2(
            first.max.x - PAGELET_PADDING - PAGELET_CLOSE / 2.0,
            first.min.y + PAGELET_PADDING + PAGELET_CLOSE / 2.0,
        );
        h.click(chip);
        h.frames(2, vec![]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        assert_eq!(h.app.pin_preview.as_ref().unwrap().rect, first);
    }

    #[test]
    fn unpinning_the_previewed_page_closes_the_pagelet() {
        let mut h = pinned_on(&[2]);
        h.click(thumb_rect(&h, 2).center());
        wait_for_pagelet(&mut h, 2);
        click_tab(&mut h, SidebarTab::Thumbnails);
        h.click(pin_rect(&h, 2).center());
        h.frames(2, vec![]);
        assert!(!h.app.pins.contains(2));
        assert!(h.app.pin_preview.is_none());
    }

    #[test]
    fn a_reload_to_fewer_pages_closes_a_lost_pagelet_and_prunes_pins() {
        let mut h = pinned_on(&[5, 9]);
        h.click(thumb_rect(&h, 9).center());
        wait_for_pagelet(&mut h, 9);
        h.reload(Arc::new(SizedEngine::letter(3)), recompiled_key());
        h.frames(2, vec![]);
        assert!(h.app.pins.pages().is_empty(), "{:?}", h.app.pins.pages());
        assert!(h.app.pin_preview.is_none());
    }

    /// Zooms out until pages are much shorter than the window.
    fn zoomed_out(h: &mut Harness) {
        for _ in 0..4 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
        }
        h.frames(2, vec![]);
    }

    #[test]
    fn click_on_thumbnail_goes_to_page() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_thumbnail(&mut h, 2);
        let rect = h.app.view.layout().page_rect(2).unwrap();
        assert!((h.app.view.offset().1 - (rect.y - noido_core::MARGIN)).abs() < 1.0);
        assert_eq!(h.app.view.mode(), noido_core::ZoomMode::FitWidth);
        assert_eq!(h.app.current_page(), Some(2));
    }

    #[test]
    fn click_keeps_paged_and_dual() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        click_thumbnail(&mut h, 2);
        let view = &h.app.view;
        assert!(!view.continuous() && view.dual());
        assert_eq!(view.current_spread(), Some(view.layout().spread_of(2)));
    }

    #[test]
    fn click_on_short_page_highlights_it() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        zoomed_out(&mut h);
        click_thumbnail(&mut h, 2);
        assert_ne!(h.app.view.centre_page(), Some(2));
        assert_eq!(h.app.current_page(), Some(2));
        assert_eq!(h.app.sidebar_view.highlighted(), 2..3);
    }

    #[test]
    fn click_on_last_page_highlights_it() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        wheel_sidebar(&mut h, -10_000.0);
        click_thumbnail(&mut h, 9);
        assert_eq!(h.app.current_page(), Some(9));
        assert_eq!(h.app.sidebar_view.highlighted(), 9..10);
    }

    #[test]
    fn scrolling_page_view_clears_chosen_page() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        zoomed_out(&mut h);
        click_thumbnail(&mut h, 2);
        assert_eq!(h.app.chosen, Some(2));
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        let step = wheel(
            egui::MouseWheelUnit::Point,
            -50.0,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![step]);
        h.frames(10, vec![]);
        assert_eq!(h.app.chosen, None);
        assert_eq!(h.app.current_page(), h.app.view.centre_page());
    }

    #[test]
    fn page_key_clears_chosen_page() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        zoomed_out(&mut h);
        click_thumbnail(&mut h, 2);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        assert_eq!(h.app.chosen, None);
    }

    #[test]
    fn resize_keeps_chosen_page() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        zoomed_out(&mut h);
        click_thumbnail(&mut h, 2);
        h.size += vec2(100.0, 100.0);
        h.frames(5, vec![]);
        h.size -= vec2(200.0, 200.0);
        h.frames(5, vec![]);
        assert_eq!(h.app.chosen, Some(2));
    }

    #[test]
    fn dual_mode_highlights_both_pages_of_spread() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        click_thumbnail(&mut h, 2);
        let layout = h.app.view.layout();
        let spread = layout.spread_pages(layout.spread_of(2));
        assert_eq!(spread.len(), 2);
        assert_eq!(h.app.sidebar_view.highlighted(), spread);
    }

    #[test]
    fn sidebar_follows_current_page() {
        let mut h = sidebar_on(Arc::new(BlankEngine30));
        go_to(&mut h, 20);
        h.frames(10, vec![]);
        let page = h.app.current_page().unwrap();
        let (_, rect) = *h
            .app
            .sidebar_view
            .drawn()
            .iter()
            .find(|(p, _)| *p == page)
            .expect("current page in view");
        assert!(rect.min.y >= TITLE_BAR_HEIGHT, "{rect:?}");
        assert!(
            rect.max.y + noido_core::LABEL_HEIGHT <= h.size.y,
            "{rect:?}"
        );
    }

    #[test]
    fn hand_scrolled_sidebar_stays_until_page_changes() {
        let mut h = sidebar_on(Arc::new(BlankEngine30));
        wheel_sidebar(&mut h, -100_000.0);
        assert!(drawn_pages(&h).contains(&29));
        h.frames(10, vec![]);
        assert!(drawn_pages(&h).contains(&29));
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(10, vec![]);
        let page = h.app.current_page().unwrap();
        assert!(
            drawn_pages(&h).contains(&page),
            "{page} {:?}",
            drawn_pages(&h)
        );
    }

    #[test]
    fn browsing_sidebar_moves_thumbnailer_focus() {
        let engine = Arc::new(SizedEngine::letter(200));
        let mut h = Harness::build_with(engine.clone(), |app| {
            app.with_thumbnails(Thumbnailer::start(
                engine,
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        });
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        wheel_sidebar(&mut h, -1_000_000.0);
        let sent = h.app.thumbnails.as_ref().unwrap().sent;
        assert!(sent.is_some_and(|page| page >= 100), "{sent:?}");
        // The reader's page is untouched.
        assert_eq!(h.app.current_page(), Some(0));
    }

    #[test]
    fn wheel_over_sidebar_does_not_flip() {
        let mut h = fitted_paged();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let spread = h.app.view.current_spread();
        h.frames(1, vec![Event::PointerMoved(pos2(100.0, 500.0))]);
        let step = wheel(
            egui::MouseWheelUnit::Line,
            -1.0,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![step]);
        h.frames(5, vec![]);
        assert_eq!(h.app.view.current_spread(), spread);
    }

    #[test]
    fn reload_swaps_sidebar_renderer() {
        let mut h = with_sidebar(RecordingEngine::default());
        h.until(|h| !h.app.sidebar_view.sharp_pages().is_empty());
        click_thumbnail(&mut h, 1);
        let engine = RecordingEngine::default();
        h.reload_with_sidebar(
            Arc::new(SizedEngine::letter(5)),
            engine.clone(),
            recompiled_key(),
        );
        h.until(|_| !engine.recorded().is_empty());
        assert!(!engine.recorded().is_empty(), "new sidebar renderer unused");
        assert!(
            h.app
                .sidebar_view
                .sharp_pages()
                .iter()
                .all(|&page| page < 5)
        );
        assert_eq!(h.app.chosen, None);
    }

    /// Ten Letter pages; each render takes 150 ms.
    struct SlowEngine;

    impl PdfEngine for SlowEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(SlowEngine))
        }
    }

    impl Document for SlowEngine {
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
            std::thread::sleep(std::time::Duration::from_millis(150));
            BlankDocument.render_tile(page, scale, tile)
        }
    }

    #[test]
    fn render_arriving_while_closed_is_requested_again() {
        let mut h = Harness::build(|app| {
            let renderer = Renderer::start_background(
                Arc::new(SlowEngine),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            );
            app.with_sidebar_renderer(renderer)
        });
        h.press(egui::Key::F9, Modifiers::NONE);
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Closed while the first render is on its way; it arrives meanwhile.
        h.press(egui::Key::F9, Modifiers::NONE);
        std::thread::sleep(std::time::Duration::from_millis(400));
        h.frames(2, vec![]);
        h.press(egui::Key::F9, Modifiers::NONE);
        let all_sharp = |h: &Harness| {
            let drawn = drawn_pages(h);
            !drawn.is_empty() && h.app.sidebar_view.sharp_pages() == drawn
        };
        for _ in 0..3 {
            h.until(all_sharp);
        }
        assert!(
            all_sharp(&h),
            "drawn {:?} sharp {:?}",
            drawn_pages(&h),
            h.app.sidebar_view.sharp_pages()
        );
    }

    /// Whether a gradient to transparent, egui's scroll-area edge fade, was
    /// painted left of `x`.
    fn fade_left_of(h: &Harness, x: f32) -> bool {
        h.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) => {
                let alphas: Vec<u8> = mesh.vertices.iter().map(|v| v.color.a()).collect();
                clipped.shape.visual_bounding_rect().max.x <= x
                    && alphas.contains(&0)
                    && alphas.iter().any(|&a| a > 0)
            }
            _ => false,
        })
    }

    #[test]
    fn sidebar_edges_have_no_fade() {
        let mut h = sidebar_on(Arc::new(BlankEngine30));
        wheel_sidebar(&mut h, -2_000.0);
        assert!(!fade_left_of(&h, h.app.sidebar.width + 1.0));
    }

    /// Rectangles painted in the selection colour last frame.
    fn selection_rects(h: &Harness) -> Vec<Rect> {
        let fill = h.ctx.global_style().visuals.selection.bg_fill;
        let tabs = h.app.sidebar_view.tab_rects();
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                // Not the title bar's buttons, nor the sidebar's tabs.
                egui::Shape::Rect(r)
                    if r.fill == fill
                        && r.rect.min.y >= TITLE_BAR_HEIGHT
                        && !tabs.iter().any(|(_, t)| t.contains_rect(r.rect)) =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn current_page_has_frame_and_number_pill() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_thumbnail(&mut h, 1);
        let (_, thumb) = h.app.sidebar_view.drawn()[1];
        let rects = selection_rects(&h);
        let frame = thumb.expand(sidebar::HIGHLIGHT);
        assert!(rects.contains(&frame), "{rects:?} vs {frame:?}");
        // A pill behind the number, under the frame, narrower than the page.
        let pill = rects
            .iter()
            .find(|r| {
                r.min.y >= frame.max.y && r.max.y <= thumb.max.y + noido_core::LABEL_HEIGHT + 2.0
            })
            .unwrap_or_else(|| panic!("no pill in {rects:?}"));
        assert!(
            pill.width() < thumb.width() / 2.0 && pill.height() > 12.0,
            "{pill:?}"
        );
        assert!((pill.center().x - thumb.center().x).abs() < 0.5);
        // Apart from the frame, and with room below before the next one.
        assert!(pill.min.y - frame.max.y >= 3.0, "{pill:?} under {frame:?}");
        let (_, next) = h.app.sidebar_view.drawn()[2];
        let next_frame = next.expand(sidebar::HIGHLIGHT);
        assert!(
            next_frame.min.y - pill.max.y >= 9.0,
            "{pill:?} over {next_frame:?}"
        );
        // Only the current page has them.
        assert_eq!(rects.len(), 2, "{rects:?}");
    }

    #[test]
    fn dragging_sidebar_scrolls_it_without_clicking() {
        let mut h = sidebar_on(Arc::new(BlankEngine30));
        let (_, before) = h.app.sidebar_view.drawn()[0];
        h.drag(before.center(), vec2(0.0, -300.0));
        h.frames(10, vec![]);
        let first = drawn_pages(&h)[0];
        let (_, after) = h.app.sidebar_view.drawn()[0];
        assert!(
            first > 0 || after.min.y < before.min.y - 200.0,
            "{before:?} {after:?}"
        );
        assert_eq!(h.app.chosen, None, "the drag counted as a click");
    }

    /// The top of the sidebar's thumb (solid bars), drawn last frame.
    fn sidebar_thumb_top(h: &Harness) -> f32 {
        // Inside the panel's 1-point border; the 6-point thumb centred in
        // the 17-point groove.
        let groove = h.app.sidebar.width - 1.0 - 17.0;
        let thumbs = strips(h, groove + 5.5, groove + 11.5);
        assert_eq!(thumbs.len(), 1, "{thumbs:?}");
        thumbs[0].0
    }

    /// Pages forward until the sidebar has to follow, and lets it settle.
    fn page_until_the_sidebar_follows(h: &mut Harness) {
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        for _ in 0..30 {
            let before = h.app.sidebar_view.offset();
            h.press(egui::Key::PageDown, Modifiers::NONE);
            h.frames(30, vec![]);
            if h.app.sidebar_view.offset() > before + 1.0 {
                return;
            }
        }
        panic!("the sidebar never followed");
    }

    /// After a PageDown that makes the sidebar follow: the thumb's top
    /// before, then each frame from the press's own the sidebar's offset,
    /// the thumb's top, and whether the frame asked for another at once.
    fn follow_frames(h: &mut Harness, frames: usize) -> (f32, Vec<f32>, Vec<f32>, Vec<bool>) {
        page_until_the_sidebar_follows(h);
        let start = sidebar_thumb_top(h);
        let (mut offsets, mut tops, mut repaints) = (vec![], vec![], vec![]);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        for frame in 0..frames {
            if frame > 0 {
                h.frames(1, vec![]);
            }
            offsets.push(h.app.sidebar_view.offset());
            tops.push(sidebar_thumb_top(h));
            repaints.push(h.repaint_delay == Duration::ZERO);
        }
        (start, offsets, tops, repaints)
    }

    #[test]
    fn sidebar_thumb_glides_while_the_thumbnails_jump() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        let (start, offsets, tops, repaints) = follow_frames(&mut h, 30);
        // The thumbnails are there at once.
        assert!(offsets.iter().all(|o| *o == offsets[0]), "{offsets:?}");
        let end = *tops.last().unwrap();
        assert!(end > start + 1.0, "{start} {tops:?}");
        // The thumb is on its way from the press's own frame.
        assert!(tops[0] > start && tops[0] < end - 0.5, "{start} {tops:?}");
        // Drawn again until there, without input.
        assert!(repaints[..15].iter().all(|r| *r), "{repaints:?}");
        // There within a third of a second.
        assert_eq!(tops[20], end, "{start} {tops:?}");
        for pair in tops.windows(2) {
            assert!(pair[1] >= pair[0], "{start} {tops:?}");
        }
    }

    /// Scrolling the pages by wheel or flick, the thumb follows gently:
    /// still on its way when the quick pace would have stopped, so it
    /// doesn't stand still between pages.
    #[test]
    fn sidebar_thumb_follows_a_flick_gently() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        page_until_the_sidebar_follows(&mut h);
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 500.0))]);
        let step = wheel(
            egui::MouseWheelUnit::Point,
            -1500.0,
            egui::MouseWheelSource::Wheel,
        );
        h.frames(1, vec![step]);
        let (mut offsets, mut tops) = (vec![], vec![]);
        for _ in 0..120 {
            h.frames(1, vec![]);
            offsets.push(h.app.sidebar_view.offset());
            tops.push(sidebar_thumb_top(&h));
        }
        let last_follow = offsets
            .windows(2)
            .rposition(|w| w[1] != w[0])
            .expect("the sidebar followed")
            + 1;
        let end = *tops.last().unwrap();
        assert!(tops[last_follow + 20] < end - 0.1, "{last_follow} {tops:?}");
        assert_eq!(tops[last_follow + 60], end, "{last_follow} {tops:?}");
    }

    /// Key steps in quick succession stay quick: noido moved the pages,
    /// not a wheel or flick.
    #[test]
    fn sidebar_thumb_stays_quick_for_repeated_keys() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        page_until_the_sidebar_follows(&mut h);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(4, vec![]);
        let before = h.app.sidebar_view.offset();
        h.press(egui::Key::PageDown, Modifiers::NONE);
        assert!(
            h.app.sidebar_view.offset() > before + 1.0,
            "no second follow"
        );
        let mut tops = vec![sidebar_thumb_top(&h)];
        for _ in 0..40 {
            h.frames(1, vec![]);
            tops.push(sidebar_thumb_top(&h));
        }
        assert_eq!(tops[20], tops[40], "{tops:?}");
        assert_eq!(h.app.pages_scrolled_at, None);
    }

    /// Mid-glide, the pointer coming over the groove puts the thumb where
    /// egui takes presses on it.
    #[test]
    fn sidebar_thumb_is_in_place_under_the_pointer() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        page_until_the_sidebar_follows(&mut h);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        let on_its_way = sidebar_thumb_top(&h);
        let groove = h.app.sidebar.width - 1.0 - 17.0;
        h.frames(1, vec![Event::PointerMoved(pos2(groove + 8.0, 500.0))]);
        let at = sidebar_thumb_top(&h);
        h.frames(30, vec![]);
        assert!(on_its_way < at - 0.5, "{on_its_way} {at}");
        assert_eq!(at, sidebar_thumb_top(&h));
    }

    /// Back from the list of matches, the thumb is drawn where it is, as
    /// on opening the sidebar: it was out of sight.
    #[test]
    fn sidebar_thumb_jumps_back_from_the_matches() {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 30];
        pages[25] = &["a cat"];
        let mut h = searching(&TextEngine::new(&pages)).solid_bars();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        let top = sidebar_thumb_top(&h);
        // Off to page 25 from the list of matches.
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        let (_, row) = h.app.sidebar_view.result_rows()[0];
        h.click(row.center());
        h.frames(2, vec![]);
        assert_eq!(h.app.view.centre_page(), Some(25));
        focus_search(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        assert_eq!(h.app.search_state.searched(), None);
        let first = sidebar_thumb_top(&h);
        assert!(first > top + 100.0, "{top} {first}");
        h.frames(30, vec![]);
        assert_eq!(first, sidebar_thumb_top(&h));
    }

    #[test]
    fn sidebar_thumb_jumps_with_reduced_motion() {
        let mut h = sidebar_on(Arc::new(BlankEngine30))
            .solid_bars()
            .reduced_motion();
        let (start, _, tops, _) = follow_frames(&mut h, 3);
        assert!(tops[0] > start + 1.0, "{start} {tops:?}");
        assert_eq!(tops[0], tops[2], "{start} {tops:?}");
    }

    /// Closed mid-glide and opened again, the thumb is drawn in place, as
    /// on any opening.
    #[test]
    fn sidebar_thumb_is_in_place_on_reopening() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        page_until_the_sidebar_follows(&mut h);
        let start = sidebar_thumb_top(&h);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        assert!(sidebar_thumb_top(&h) > start, "no glide");
        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(!h.app.sidebar.open);
        h.press(egui::Key::F9, Modifiers::NONE);
        let first = sidebar_thumb_top(&h);
        h.frames(30, vec![]);
        assert_eq!(first, sidebar_thumb_top(&h));
    }

    #[test]
    fn sidebar_thumb_goes_with_the_reader_scrolling_mid_glide() {
        let mut h = sidebar_on(Arc::new(BlankEngine30)).solid_bars();
        // Settled, the thumb's top is linear in the offset: two points.
        let (offset0, top0) = (h.app.sidebar_view.offset(), sidebar_thumb_top(&h));
        page_until_the_sidebar_follows(&mut h);
        let (offset1, top1) = (h.app.sidebar_view.offset(), sidebar_thumb_top(&h));
        let in_place =
            |offset: f32| top0 + (offset - offset0) * (top1 - top0) / (offset1 - offset0);
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(1, vec![Event::PointerMoved(pos2(100.0, 500.0))]);
        let step = wheel(
            egui::MouseWheelUnit::Point,
            300.0,
            egui::MouseWheelSource::Wheel,
        );
        let mut last = h.app.sidebar_view.offset();
        h.frames(1, vec![step]);
        let mut moved = 0;
        for _ in 0..30 {
            let offset = h.app.sidebar_view.offset();
            if offset != last {
                // The reader moved it: the thumb is with the thumbnails
                // that very frame, not gliding.
                moved += 1;
                let top = sidebar_thumb_top(&h);
                assert!(
                    (top - in_place(offset)).abs() < 0.5,
                    "at {offset} the thumb is at {top}, in place {}",
                    in_place(offset)
                );
            }
            last = offset;
            h.frames(1, vec![]);
        }
        assert!(moved > 0, "the wheel never moved the sidebar");
    }

    // Text highlights on the thumbnails.

    /// The thumbnail of `page` drawn last frame.
    fn sidebar_thumb(h: &Harness, page: usize) -> Rect {
        h.app
            .sidebar_view
            .drawn()
            .iter()
            .find_map(|&(drawn, rect)| (drawn == page).then_some(rect))
            .unwrap_or_else(|| panic!("page {page} is not drawn: {:?}", drawn_pages(h)))
    }

    /// The rectangles of `colour` painted inside `thumb` last frame.
    fn marks_in(h: &Harness, thumb: Rect, colour: Color32) -> Vec<Rect> {
        filled_with(h, colour)
            .into_iter()
            .filter(|mark| thumb.contains(mark.center()))
            .collect()
    }

    #[test]
    fn sidebar_paints_highlights_on_thumbnails() {
        let (mut h, _) = selecting();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.until(|h| drawn_pages(h).contains(&0));
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Yellow, dark);
        assert!(
            marks_in(&h, sidebar_thumb(&h, 0), fill).is_empty(),
            "a mark before the highlight was made"
        );

        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 0 },
            end: Caret { page: 0, char: 7 },
            colour: HighlightColour::Yellow,
        });
        h.frames(2, vec![]);
        let thumb = sidebar_thumb(&h, 0);
        let marks = marks_in(&h, thumb, fill);
        assert_eq!(marks.len(), 1, "{marks:?}");

        // Scaled from the highlight's page-point rectangle, as on the page.
        let noido_core::PageTextState::Known(text) = h.app.texts.get(0) else {
            panic!("page 0's text is not known");
        };
        let area = text.rects(0..7)[0];
        let scale = thumb.width() / 612.0;
        let expected =
            thumb.min + vec2(area.x + area.width / 2.0, area.y + area.height / 2.0) * scale;
        assert!(
            (marks[0].center() - expected).length() < 0.5,
            "{} is not at {expected:?}",
            marks[0].center()
        );
        let expected_size = vec2(area.width, area.height) * scale;
        assert!(
            (marks[0].size() - expected_size).length() < 0.5,
            "{:?} is not {expected_size:?}",
            marks[0].size()
        );
    }

    #[test]
    fn sidebar_paints_a_highlight_across_pages_on_each_thumbnail() {
        let (mut h, engine) = selecting();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.until(|h| drawn_pages(h).contains(&1));
        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 5 },
            end: Caret { page: 1, char: 3 },
            colour: HighlightColour::Blue,
        });
        h.until(|h| matches!(h.app.texts.get(1), noido_core::PageTextState::Known(_)));
        h.frames(2, vec![]);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Blue, dark);
        for page in 0..2 {
            let thumb = sidebar_thumb(&h, page);
            assert!(
                !marks_in(&h, thumb, fill).is_empty(),
                "page {page} has no mark in {thumb:?}"
            );
        }
        // Both pages' text was asked for once; further frames add nothing.
        let calls = engine.calls();
        h.frames(5, vec![]);
        assert_eq!(engine.calls(), calls, "the sidebar asked for text again");
    }

    #[test]
    fn sidebar_waits_for_a_pages_text_before_marking_its_thumbnail() {
        let engine = TextEngine::new(&[&["the cat sat"], &["a dog"]]).gated();
        let mut h = searching(&engine);
        h.press(egui::Key::F9, Modifiers::NONE);
        h.frames(2, vec![]);
        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 0 },
            end: Caret { page: 0, char: 7 },
            colour: HighlightColour::Green,
        });
        h.frames(2, vec![]);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Green, dark);
        assert!(
            marks_in(&h, sidebar_thumb(&h, 0), fill).is_empty(),
            "a mark before the page's text arrived"
        );

        engine.open_gate();
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        h.frames(2, vec![]);
        assert!(
            !marks_in(&h, sidebar_thumb(&h, 0), fill).is_empty(),
            "no mark after the text arrived"
        );
    }

    #[test]
    fn search_results_show_no_thumbnail_marks() {
        let (mut h, _) = selecting();
        h.press(egui::Key::F9, Modifiers::NONE);
        h.until(|h| drawn_pages(h).contains(&0));
        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 0 },
            end: Caret { page: 0, char: 7 },
            colour: HighlightColour::Pink,
        });
        h.frames(2, vec![]);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Pink, dark);
        let in_sidebar = |h: &Harness| {
            filled_with(h, fill)
                .into_iter()
                .any(|mark| mark.max.x < h.app.sidebar.width)
        };
        assert!(in_sidebar(&h), "no mark in the sidebar to begin with");

        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        assert!(h.app.sidebar_view.drawn().is_empty());
        assert!(!in_sidebar(&h), "a mark over the matches");
    }

    // ---- Search ----

    /// Letter pages with text: each page's lines, 12 pt glyphs 6 pt wide,
    /// lines 20 pt apart from y = 100, starting at x = 72. Counts `text`
    /// calls; with a gate, each `text` call waits until the gate opens.
    #[derive(Clone)]
    struct TextEngine {
        pages: Arc<Vec<Vec<String>>>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
        gate: Option<Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>>,
        /// The pages rendered, in order.
        rendered: Arc<std::sync::Mutex<Vec<usize>>>,
        /// Each page's links.
        links: Arc<Vec<Vec<noido_engine::Link>>>,
        /// With a link gate, each `links` and `named_destination` call waits
        /// until it opens.
        link_gate: Option<Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>>,
        /// The named places.
        named: Arc<Vec<(String, noido_engine::Destination)>>,
        /// Each page's label.
        labels: Arc<Vec<Option<String>>>,
        /// The outline.
        outline: Arc<Vec<noido_engine::OutlineEntry>>,
    }

    impl TextEngine {
        fn new(pages: &[&[&str]]) -> TextEngine {
            TextEngine {
                pages: Arc::new(
                    pages
                        .iter()
                        .map(|lines| lines.iter().map(|&l| l.to_owned()).collect())
                        .collect(),
                ),
                calls: Arc::default(),
                gate: None,
                rendered: Arc::default(),
                links: Arc::new(vec![Vec::new(); pages.len()]),
                link_gate: None,
                named: Arc::default(),
                labels: Arc::default(),
                outline: Arc::default(),
            }
        }

        fn with_outline(mut self, outline: Vec<noido_engine::OutlineEntry>) -> TextEngine {
            self.outline = Arc::new(outline);
            self
        }

        fn with_labels(mut self, labels: &[Option<&str>]) -> TextEngine {
            self.labels = Arc::new(labels.iter().map(|l| l.map(str::to_owned)).collect());
            self
        }

        fn with_named(mut self, name: &str, page: usize) -> TextEngine {
            let target = noido_engine::Destination {
                page,
                x: None,
                y: None,
            };
            Arc::make_mut(&mut self.named).push((name.to_owned(), target));
            self
        }

        fn with_links(mut self, page: usize, links: Vec<noido_engine::Link>) -> TextEngine {
            Arc::make_mut(&mut self.links)[page] = links;
            self
        }

        fn gated_links(mut self) -> TextEngine {
            self.link_gate = Some(Arc::default());
            self
        }

        fn open_link_gate(&self) {
            if let Some(gate) = &self.link_gate {
                *gate.0.lock().unwrap() = true;
                gate.1.notify_all();
            }
        }

        /// Ten pages, "cat" on pages 2 and 7.
        fn cats() -> TextEngine {
            let mut pages: Vec<&[&str]> = vec![&["a dog"]; 10];
            pages[2] = &["a dog", "the cat sat"];
            pages[7] = &["one cat"];
            TextEngine::new(&pages)
        }

        fn gated(mut self) -> TextEngine {
            self.gate = Some(Arc::default());
            self
        }

        fn open_gate(&self) {
            if let Some(gate) = &self.gate {
                *gate.0.lock().unwrap() = true;
                gate.1.notify_all();
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl PdfEngine for TextEngine {
        fn open(&self, _bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
            Ok(Box::new(self.clone()))
        }
    }

    impl Document for TextEngine {
        fn page_count(&self) -> usize {
            self.pages.len()
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
            self.rendered.lock().unwrap().push(page);
            BlankDocument.render_tile(page, scale, tile)
        }
        fn text(&self, page: usize) -> Result<Vec<noido_engine::Glyph>, EngineError> {
            use noido_engine::{Glyph, Point, Quad};
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            let mut glyphs = Vec::new();
            for (line, text) in self.pages[page].iter().enumerate() {
                let y = 100.0 + line as f32 * 20.0;
                for (i, c) in text.chars().enumerate() {
                    let x = 72.0 + i as f32 * 6.0;
                    let p = |x, y| Point { x, y };
                    glyphs.push(Glyph {
                        text: c.to_string(),
                        origin: p(x, y),
                        size: 12.0,
                        quad: Quad {
                            corners: [
                                p(x, y + 2.4),
                                p(x + 6.0, y + 2.4),
                                p(x + 6.0, y - 9.6),
                                p(x, y - 9.6),
                            ],
                        },
                        invisible: false,
                    });
                }
            }
            Ok(glyphs)
        }
        fn links(&self, page: usize) -> Result<Vec<noido_engine::Link>, EngineError> {
            if let Some(gate) = &self.link_gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            Ok(self.links[page].clone())
        }
        fn named_destination(&self, name: &str) -> Option<noido_engine::Destination> {
            if let Some(gate) = &self.link_gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            self.named.iter().find(|(n, _)| n == name).map(|(_, t)| *t)
        }
        fn page_labels(&self) -> Vec<Option<String>> {
            if let Some(gate) = &self.link_gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            self.labels.to_vec()
        }
        fn outline(&self) -> Vec<noido_engine::OutlineEntry> {
            if let Some(gate) = &self.link_gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            self.outline.to_vec()
        }
    }

    /// A harness whose document and searcher are `engine`.
    fn searching(engine: &TextEngine) -> Harness {
        let searcher = engine.clone();
        Harness::build_with(Arc::new(engine.clone()), move |app| {
            app.with_searcher(TextSearcher::start(
                Arc::new(searcher),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        })
    }

    #[test]
    fn opening_extracts_only_visible_pages() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        h.frames(20, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.frames(2, vec![]);
        let (_, top) = h.app.view.offset();
        let height = h.app.last_scroll.unwrap().inner.height();
        let visible = h.app.view.layout().visible_pages(top, top + height).len();
        assert!(visible > 0);
        assert_eq!(engine.calls(), visible);
    }

    #[test]
    fn first_focus_extracts() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        focus_search(&mut h);
        h.until(|_| engine.calls() == 10);
        assert_eq!(engine.calls(), 10);
    }

    #[test]
    fn reload_brings_new_searcher() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        let new = TextEngine::cats();
        h.reload(Arc::new(new.clone()), recompiled_key());
        focus_search(&mut h);
        h.until(|_| new.calls() == 10);
        assert_eq!(new.calls(), 10);
        // The old version only ever read the pages shown, for selecting.
        assert!(engine.calls() < 10, "{}", engine.calls());
    }

    /// Types `text` into the search field and waits for the search to finish.
    fn search_for(h: &mut Harness, text: &str) {
        focus_search(h);
        h.typed(text);
        let done = |h: &Harness| {
            h.app.search_state.searched() == Some(text) && h.app.search_state.finished()
        };
        h.until(done);
        assert!(done(h), "search never finished");
    }

    #[test]
    fn typing_searches_after_delay() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        focus_search(&mut h);
        h.typed("cat");
        h.frames(5, vec![]);
        assert_eq!(h.app.search_state.searched(), None);
        h.frames(15, vec![]);
        assert_eq!(h.app.search_state.searched(), Some("cat"));
    }

    #[test]
    fn first_hit_is_jumped_to_once() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        assert_eq!(h.app.search_state.hits.len(), 2);
        assert_eq!(h.app.search_state.hits.current(), Some(0));
        assert_eq!(h.app.view.centre_page(), Some(2));
        assert_eq!(h.app.search_state.first(), search::First::Done);
    }

    #[test]
    fn search_starts_at_current_page() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        go_to(&mut h, 5);
        search_for(&mut h, "cat");
        assert_eq!(h.app.search_state.hits.current(), Some(1));
        assert_eq!(h.app.view.centre_page(), Some(7));
    }

    #[test]
    fn reader_move_cancels_pending_jump() {
        let engine = TextEngine::cats().gated();
        let mut h = searching(&engine);
        focus_search(&mut h);
        h.typed("cat");
        h.frames(20, vec![]);
        assert_eq!(h.app.search_state.searched(), Some("cat"));
        h.press(egui::Key::PageDown, Modifiers::NONE);
        h.frames(3, vec![]);
        let offset = h.app.view.offset();
        engine.open_gate();
        h.until(|h| h.app.search_state.finished());
        h.frames(3, vec![]);
        assert_eq!(h.app.search_state.hits.len(), 2);
        assert_eq!(h.app.view.offset(), offset);
        assert_eq!(h.app.search_state.hits.current(), None);
    }

    #[test]
    fn stale_generation_is_ignored() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "a");
        h.typed("t");
        h.until(|h| h.app.search_state.searched() == Some("at") && h.app.search_state.finished());
        // "cat", "sat" on page 2, "cat" on page 7.
        let hits = &h.app.search_state.hits;
        assert_eq!(hits.len(), 3);
        for i in 0..3 {
            assert_eq!(hits.get(i).unwrap().text.range.len(), 2);
        }
    }

    #[test]
    fn typing_keeps_sidebar_hidden_after_f9() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        focus_search(&mut h);
        h.typed("c");
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
        h.press(egui::Key::F9, Modifiers::NONE);
        assert!(!h.app.sidebar.open);
        h.typed("a");
        h.frames(20, vec![]);
        assert!(!h.app.sidebar.open);
    }

    #[test]
    fn esc_clears_search_and_keeps_focus() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.search, "");
        assert!(h.app.search_state.hits.is_empty());
        assert_eq!(h.app.search_state.searched(), None);
        assert!(h.ctx.text_edit_focused());
        assert!(h.app.sidebar.open);
    }

    use noido_core::SidebarTab;

    fn click_tab(h: &mut Harness, tab: SidebarTab) {
        let (_, rect) = *h
            .app
            .sidebar_view
            .tab_rects()
            .iter()
            .find(|(t, _)| *t == tab)
            .unwrap_or_else(|| panic!("no tab {tab:?}"));
        h.click(rect.center());
        h.frames(2, vec![]);
    }

    #[test]
    fn tabs_show_labels() {
        let h = sidebar_on(Arc::new(BlankEngine));
        let texts = h.texts();
        for label in ["Pages", "Pins", "Outline", "Search"] {
            assert!(texts.iter().any(|t| t == label), "{label}: {texts:?}");
        }
        let rects = h.app.sidebar_view.tab_rects().to_vec();
        assert_eq!(rects.len(), 4);
        // The narrowest width is checked in
        // `four_tabs_fit_their_shares_at_the_narrowest_sidebar`.
        // The active tab is filled with the selection colour.
        let fill = h.ctx.global_style().visuals.selection.bg_fill;
        let (_, pages) = h.app.sidebar_view.tab_rects()[0];
        assert!(h.shapes.iter().any(|c| {
            matches!(&c.shape, egui::Shape::Rect(r) if r.fill == fill && pages.contains_rect(r.rect))
        }));
        let (_, outline) = *h
            .app
            .sidebar_view
            .tab_rects()
            .iter()
            .find(|(tab, _)| *tab == SidebarTab::Outline)
            .expect("an Outline tab");
        assert!(!h.shapes.iter().any(|c| {
            matches!(&c.shape, egui::Shape::Rect(r) if r.fill == fill && outline.contains_rect(r.rect))
        }));
    }

    #[test]
    fn clicking_a_tab_switches() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_tab(&mut h, SidebarTab::Outline);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Outline);
        assert_eq!(h.app.sidebar.last, SidebarTab::Outline);
        assert!(h.app.sidebar_view.drawn().is_empty());
        click_tab(&mut h, SidebarTab::Thumbnails);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Thumbnails);
        assert!(!h.app.sidebar_view.drawn().is_empty());
    }

    #[test]
    fn search_goes_to_results_and_clearing_returns() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::F9, Modifiers::NONE);
        click_tab(&mut h, SidebarTab::Outline);
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Results);
        assert!(!h.app.sidebar_view.result_rows().is_empty());
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Outline);
        assert!(h.app.sidebar_view.result_rows().is_empty());
    }

    #[test]
    fn new_query_from_another_tab_goes_to_results() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::F9, Modifiers::NONE);
        search_for(&mut h, "cat");
        click_tab(&mut h, SidebarTab::Thumbnails);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Thumbnails);
        h.app.search = "dog".into();
        h.until(|h| h.app.search_state.searched() == Some("dog") && h.app.search_state.finished());
        h.frames(2, vec![]);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Results);
    }

    #[test]
    fn stepping_hits_from_another_tab_stays() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::F9, Modifiers::NONE);
        search_for(&mut h, "cat");
        click_tab(&mut h, SidebarTab::Thumbnails);
        let before = current(&h);
        h.press(egui::Key::F3, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_ne!(current(&h), before);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Thumbnails);
    }

    #[test]
    fn search_stays_while_on_another_tab() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::F9, Modifiers::NONE);
        search_for(&mut h, "cat");
        let hits = h.app.search_state.hits.len();
        click_tab(&mut h, SidebarTab::Outline);
        h.frames(3, vec![]);
        assert_eq!(h.app.search_state.searched(), Some("cat"));
        assert_eq!(h.app.search_state.hits.len(), hits);
        assert!(hits > 0);
        click_tab(&mut h, SidebarTab::Results);
        assert!(!h.app.sidebar_view.result_rows().is_empty());
    }

    #[test]
    fn reload_rerunning_the_search_does_not_switch() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::F9, Modifiers::NONE);
        search_for(&mut h, "cat");
        click_tab(&mut h, SidebarTab::Thumbnails);
        h.reload(Arc::new(more_cats()), recompiled_key());
        h.until(|h| h.app.search_state.finished() && !h.app.search_state.hits.is_empty());
        h.frames(2, vec![]);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Thumbnails);
    }

    #[test]
    fn results_tab_without_a_search_says_so() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_tab(&mut h, SidebarTab::Results);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Results);
        assert!(h.texts().iter().any(|t| t == "No search"));
        assert_eq!(h.app.sidebar.last, SidebarTab::Thumbnails);
    }

    #[test]
    fn thumbnails_not_drawn_off_the_tab() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_tab(&mut h, SidebarTab::Thumbnails);
        assert!(!h.app.sidebar_view.drawn().is_empty());
        click_tab(&mut h, SidebarTab::Outline);
        h.frames(3, vec![]);
        assert!(h.app.sidebar_view.drawn().is_empty());
        click_tab(&mut h, SidebarTab::Thumbnails);
        assert!(!h.app.sidebar_view.drawn().is_empty());
    }

    #[test]
    fn search_with_closed_sidebar_then_esc_twice() {
        let mut h = searching(&more_cats());
        h.app.sidebar.choose(SidebarTab::Outline);
        assert!(!h.app.sidebar.open);
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Results);
        assert!(!h.app.sidebar_view.result_rows().is_empty());
        // Results are not what is saved.
        assert_eq!(h.app.doc_state().unwrap().sidebar.last, SidebarTab::Outline);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.sidebar.open);
        assert_eq!(h.app.sidebar.tab, SidebarTab::Outline);
        assert_eq!(h.app.doc_state().unwrap().sidebar.last, SidebarTab::Outline);
    }

    #[test]
    fn reload_reruns_search_without_moving() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        let offset = h.app.view.offset();
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 10];
        pages[4] = &["cat"];
        pages[8] = &["cat"];
        h.reload(Arc::new(TextEngine::new(&pages)), recompiled_key());
        h.until(|h| h.app.search_state.finished() && !h.app.search_state.hits.is_empty());
        h.frames(2, vec![]);
        let hits = &h.app.search_state.hits;
        let found: Vec<usize> = (0..hits.len()).map(|i| hits.get(i).unwrap().page).collect();
        assert_eq!(found, [4, 8]);
        assert_eq!(hits.current(), Some(0));
        assert_eq!(h.app.view.offset(), offset);
    }

    /// Ten pages, "cat" on pages 2 (twice), 5 and 7.
    fn more_cats() -> TextEngine {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 10];
        pages[2] = &["a cat", "the cat sat"];
        pages[5] = &["cat"];
        pages[7] = &["one cat"];
        TextEngine::new(&pages)
    }

    fn current(h: &Harness) -> Option<usize> {
        h.app.search_state.hits.current()
    }

    #[test]
    fn enter_goes_to_next_hit_and_wraps() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        assert_eq!(current(&h), Some(0));
        h.press(egui::Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h), Some(1));
        h.frames(1, vec![]);
        assert!(h.ctx.text_edit_focused(), "Enter keeps the field focused");
        h.press(egui::Key::Enter, Modifiers::NONE);
        h.press(egui::Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h), Some(3));
        assert_eq!(h.app.view.centre_page(), Some(7));
        h.press(egui::Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h), Some(0));
    }

    #[test]
    fn shift_enter_goes_back() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.press(egui::Key::Enter, Modifiers::SHIFT);
        assert_eq!(current(&h), Some(3));
        h.press(egui::Key::Enter, Modifiers::SHIFT);
        assert_eq!(current(&h), Some(2));
    }

    #[test]
    fn mod_g_and_f3_outside_field() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.click(pos2(600.0, 500.0));
        assert!(!h.ctx.text_edit_focused());
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(current(&h), Some(1));
        h.press(egui::Key::F3, Modifiers::NONE);
        assert_eq!(current(&h), Some(2));
        h.press(egui::Key::G, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(current(&h), Some(1));
        h.press(egui::Key::F3, Modifiers::SHIFT);
        assert_eq!(current(&h), Some(0));
    }

    #[test]
    fn next_after_scrolling_goes_from_view() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        go_to(&mut h, 6);
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(current(&h), Some(3));
        let top = h.app.view.offset().1;
        let bottom = top + h.app.view.viewport_size().1;
        assert!(h.app.view.layout().visible_pages(top, bottom).contains(&7));
    }

    #[test]
    fn previous_after_scrolling_goes_from_view() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        go_to(&mut h, 6);
        h.press(egui::Key::G, Modifiers::COMMAND | Modifiers::SHIFT);
        // The last hit starting above the view's bottom: page 5's, or page
        // 6's view reaches page 7's top.
        let page = h
            .app
            .search_state
            .hits
            .get(current(&h).unwrap())
            .unwrap()
            .page;
        assert!(page == 5 || page == 7, "{page}");
        assert_ne!(current(&h), Some(1));
    }

    #[test]
    fn next_without_hits_does_nothing() {
        let mut h = searching(&more_cats());
        let offset = h.app.view.offset();
        h.press(egui::Key::G, Modifiers::COMMAND);
        h.press(egui::Key::F3, Modifiers::SHIFT);
        assert_eq!(current(&h), None);
        assert_eq!(h.app.view.offset(), offset);
        search_for(&mut h, "zebra");
        h.press(egui::Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h), None);
    }

    #[test]
    fn next_in_paged_mode_flips_to_hit() {
        let mut h = searching(&more_cats());
        h.press(egui::Key::C, Modifiers::NONE);
        assert!(!h.app.view.continuous());
        search_for(&mut h, "cat");
        assert_eq!(h.app.view.current_spread(), Some(2));
        h.press(egui::Key::G, Modifiers::COMMAND);
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(h.app.view.current_spread(), Some(5));
    }

    #[test]
    fn mod_f_selects_existing_text() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.click(pos2(600.0, 500.0));
        focus_search(&mut h);
        h.typed("dog");
        assert_eq!(h.app.search, "dog");
    }

    /// The rectangles filled with `colour` below the title bar.
    fn filled(h: &Harness, colour: Color32) -> Vec<Rect> {
        h.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(r) if r.fill == colour && r.rect.min.y >= TITLE_BAR_HEIGHT => {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn highlights_drawn_for_visible_hits() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        let warning = h.theme().palette(egui::Theme::Dark).warning;
        let current = filled(&h, search::current_fill(warning));
        assert_eq!(current.len(), 1, "{current:?}");
        // "a cat": "cat" is chars 2..5 of the first line, at 72 + 2 × 6 pt.
        let scale = h.app.view.scale();
        assert!(
            (current[0].width() - 18.0 * scale).abs() < 1.0,
            "{current:?}"
        );
        assert!(
            (current[0].height() - 12.0 * scale).abs() < 1.0,
            "{current:?}"
        );
        let others = filled(&h, search::highlight_fill(warning));
        assert!(!others.is_empty());
        assert!(others.iter().all(|r| r.min.y > current[0].min.y));
    }

    #[test]
    fn sidebar_shows_results_while_text() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        assert!(h.app.sidebar.open);
        assert!(h.app.sidebar_view.drawn().is_empty(), "thumbnails drawn");
        let rows: Vec<usize> = h
            .app
            .sidebar_view
            .result_rows()
            .iter()
            .map(|&(i, _)| i)
            .collect();
        assert_eq!(rows, [0, 1, 2, 3]);
    }

    #[test]
    fn click_on_result_goes_to_hit() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        let (_, row) = h.app.sidebar_view.result_rows()[3];
        h.click(row.center());
        h.frames(2, vec![]);
        assert_eq!(current(&h), Some(3));
        assert_eq!(h.app.view.centre_page(), Some(7));
    }

    #[test]
    fn results_follow_current_hit() {
        let pages: Vec<&[&str]> = vec![&["cat"]; 60];
        let mut h = searching(&TextEngine::new(&pages));
        search_for(&mut h, "cat");
        for _ in 0..40 {
            h.press(egui::Key::G, Modifiers::COMMAND);
        }
        h.frames(3, vec![]);
        assert_eq!(current(&h), Some(40));
        let rows = h.app.sidebar_view.result_rows();
        assert!(rows.iter().any(|&(i, _)| i == 40), "{rows:?}");
    }

    /// The list of matches scrolls with the same solid bar as the
    /// thumbnails: its cards beside it, in a groove, with a thin thumb, and
    /// a press in the groove scrolls.
    #[test]
    fn results_have_the_solid_bar() {
        let pages: Vec<&[&str]> = vec![&["cat"]; 60];
        let mut h = searching(&TextEngine::new(&pages)).solid_bars();
        search_for(&mut h, "cat");
        h.frames(3, vec![]);
        // Inside the panel's 1-point border on its right.
        let right = h.app.sidebar.width - 1.0;
        let groove = right - 17.0;
        let rows = h.app.sidebar_view.result_rows().to_vec();
        assert!(!rows.is_empty());
        for (index, row) in &rows {
            assert!(
                row.max.x <= groove + 0.5,
                "card {index} ends at {}, the bar starts at {groove}",
                row.max.x
            );
        }
        let grooves = strips(&h, groove, right);
        assert!(
            grooves.iter().any(|(top, bottom)| bottom - top > 500.0),
            "no groove: {grooves:?}"
        );
        assert_eq!(strips(&h, groove + 5.5, groove + 11.5).len(), 1, "no thumb");

        // Mid-groove: its outer few points are the sidebar's resize handle.
        let first = rows[0].0;
        h.click(pos2(groove + 8.5, 900.0));
        h.frames(3, vec![]);
        assert!(
            h.app.sidebar_view.result_rows()[0].0 > first,
            "a press in the groove did not scroll the matches"
        );
    }

    /// The matches' cards end left of the scroll bar when the list
    /// overflows, solid or floating, at the narrowest sidebar too; a list
    /// that fits gets the whole width.
    #[test]
    fn result_cards_leave_the_scroll_bar_clear() {
        for (solid, bar) in [(false, 12.0), (true, 17.0)] {
            for width in [None, Some(noido_core::SIDEBAR_MIN_WIDTH)] {
                let make = |count: usize| {
                    let pages: Vec<&[&str]> = vec![&["cat"]; count];
                    let mut h = searching(&TextEngine::new(&pages));
                    if solid {
                        h = h.solid_bars();
                    }
                    if let Some(width) = width {
                        h.app.sidebar.width = width;
                    }
                    search_for(&mut h, "cat");
                    h.frames(4, vec![]);
                    h
                };
                let h = make(60);
                let bar_left = h.app.sidebar.width - 1.0 - bar;
                let rows = h.app.sidebar_view.result_rows();
                assert!(rows.len() > 5, "{}", rows.len());
                for (index, row) in rows {
                    assert!(
                        row.max.x <= bar_left + 0.5,
                        "solid {solid}, {width:?}: card {index} ends at {}, the bar starts at {bar_left}",
                        row.max.x
                    );
                }
                let short = make(2);
                let rows = short.app.sidebar_view.result_rows();
                assert_eq!(rows.len(), 2);
                for (_, row) in rows {
                    let edge = short.app.sidebar.width - 1.0;
                    assert!(
                        row.max.x >= edge - 0.5,
                        "solid {solid}, {width:?}: a list that fits ends at {}",
                        row.max.x
                    );
                }
            }
        }
    }

    /// The cards' text keeps the panel's colours under the scrollbar role:
    /// the matched word is drawn in the strong text colour, whether the bars
    /// are solid (where the role makes egui's thumb colours transparent) or
    /// overlay.
    #[test]
    fn results_keep_their_text_colours_under_the_scrollbar_role() {
        for solid in [true, false] {
            let pages: Vec<&[&str]> = vec![&["cat"]; 60];
            let mut h = searching(&TextEngine::new(&pages));
            if solid {
                h = h.solid_bars();
            }
            search_for(&mut h, "cat");
            h.frames(3, vec![]);
            let strong = h.ctx.global_style().visuals.strong_text_color();
            let sidebar = h.app.sidebar.width;
            let colours: Vec<Color32> = h
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.pos.x < sidebar => Some(text),
                    _ => None,
                })
                .flat_map(|text| text.galley.job.sections.iter().map(|s| s.format.color))
                .collect();
            assert!(
                colours.contains(&strong),
                "solid {solid}: no match in {strong:?} among {colours:?}"
            );
            assert!(
                !colours.contains(&Color32::TRANSPARENT),
                "solid {solid}: invisible text"
            );
        }
    }

    /// A few matches need no bar: the cards take the whole width.
    #[test]
    fn few_results_need_no_solid_bar() {
        let mut h = searching(&more_cats()).solid_bars();
        search_for(&mut h, "cat");
        h.frames(3, vec![]);
        let right = h.app.sidebar.width - 1.0;
        for (index, row) in h.app.sidebar_view.result_rows() {
            assert_eq!(row.max.x, right, "card {index}");
        }
        assert!(strips(&h, right - 17.0, right).is_empty());
    }

    #[test]
    fn results_are_cards_of_one_height() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        let rows = h.app.sidebar_view.result_rows();
        assert!(rows.len() > 1, "{rows:?}");
        let font = egui::TextStyle::Body.resolve(&h.ctx.global_style());
        let body = h.ctx.fonts_mut(|fonts| fonts.row_height(&font));
        for &(_, row) in rows {
            assert_eq!(row.height(), rows[0].1.height());
            // A header and three lines of text.
            assert!(row.height() > 4.0 * body, "{} vs {body}", row.height());
        }
    }

    #[test]
    fn cleared_search_shows_thumbnails_again() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(3, vec![]);
        assert!(h.app.sidebar_view.result_rows().is_empty());
        assert!(!h.app.sidebar_view.drawn().is_empty());
        let warning = h.theme().palette(egui::Theme::Dark).warning;
        assert!(filled(&h, search::highlight_fill(warning)).is_empty());
        assert!(filled(&h, search::current_fill(warning)).is_empty());
    }

    #[test]
    fn counter_and_status() {
        let mut h = searching(&more_cats());
        search_for(&mut h, "cat");
        assert_eq!(h.app.search_state.counter(), "1 of 4");
        search_for(&mut h, "zebra");
        assert_eq!(h.app.search_state.counter(), "No matches");
    }

    #[test]
    fn previous_in_dual_mode_finds_visible_right_page_hit() {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 10];
        pages[5] = &["cat"];
        pages[9] = &["cat"];
        let mut h = searching(&TextEngine::new(&pages));
        h.press(egui::Key::C, Modifiers::NONE);
        h.press(egui::Key::D, Modifiers::NONE);
        search_for(&mut h, "cat");
        let spread = h.app.view.current_spread();
        let shown = h.app.view.layout().spread_pages(spread.unwrap());
        assert!(shown.contains(&5), "{shown:?}");
        h.click(pos2(600.0, 500.0));
        // A reader move that keeps the spread: next/previous go from the view.
        h.press(egui::Key::Minus, Modifiers::COMMAND);
        assert_eq!(h.app.view.current_spread(), spread);
        h.press(egui::Key::F3, Modifiers::SHIFT);
        assert_eq!(current(&h), Some(0), "the visible hit on page 5");
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        assert_eq!(h.app.view.current_spread(), spread);
        h.press(egui::Key::F3, Modifiers::NONE);
        assert_eq!(current(&h), Some(0), "the visible hit on page 5");
    }

    #[test]
    fn thumbnails_keep_their_place_after_search() {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 30];
        pages[25] = &["cat"];
        let mut h = searching(&TextEngine::new(&pages));
        h.press(egui::Key::F9, Modifiers::NONE);
        go_to(&mut h, 25);
        h.frames(5, vec![]);
        assert!(drawn_pages(&h).contains(&25));
        search_for(&mut h, "cat");
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(5, vec![]);
        assert!(drawn_pages(&h).contains(&25), "{:?}", drawn_pages(&h));
        assert_eq!(h.app.browse, None);
    }

    #[test]
    fn blank_query_is_no_search() {
        let mut h = searching(&TextEngine::cats());
        focus_search(&mut h);
        h.typed("  ");
        h.frames(20, vec![]);
        assert_eq!(h.app.search_state.searched(), None);
        assert_eq!(h.app.search_state.counter(), "");
        assert_eq!(h.app.search_state.status(10), "");
    }

    #[test]
    fn results_scroll_to_new_querys_first_hit() {
        let pages: Vec<&[&str]> = vec![&["cat"]; 60];
        let mut h = searching(&TextEngine::new(&pages));
        search_for(&mut h, "ca");
        h.frames(3, vec![]);
        wheel_sidebar(&mut h, -2000.0);
        assert!(
            !h.app
                .sidebar_view
                .result_rows()
                .iter()
                .any(|&(i, _)| i == 0)
        );
        h.typed("t");
        h.until(|h| h.app.search_state.searched() == Some("cat") && h.app.search_state.finished());
        h.frames(3, vec![]);
        assert_eq!(current(&h), Some(0));
        let rows = h.app.sidebar_view.result_rows();
        assert!(rows.iter().any(|&(i, _)| i == 0), "{rows:?}");
    }

    /// 200 pages, "cat" on pages 0 and 150.
    fn far_cats() -> TextEngine {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 200];
        pages[0] = &["cat"];
        pages[150] = &["cat"];
        TextEngine::new(&pages)
    }

    #[test]
    fn next_hit_page_rendered_ahead() {
        let engine = far_cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        assert_eq!(h.app.view.centre_page(), Some(0));
        h.until(|_| engine.rendered.lock().unwrap().contains(&150));
        assert!(engine.rendered.lock().unwrap().contains(&150));
        h.until(|h| h.app.textures.contains_key(&150));
        assert!(h.app.textures.contains_key(&150));
    }

    #[test]
    fn stepping_there_and_back_stays_sharp() {
        let engine = far_cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        h.until(|h| h.app.textures.contains_key(&150));
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(h.app.view.centre_page(), Some(150));
        h.frames(10, vec![]);
        assert!(h.app.textures.contains_key(&0), "page 0 dropped");
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(h.app.view.centre_page(), Some(0));
        assert!(h.app.textures.contains_key(&150), "page 150 dropped");
    }

    #[test]
    fn without_search_only_window_is_rendered() {
        let engine = far_cats();
        let mut h = searching(&engine);
        h.frames(30, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.frames(5, vec![]);
        let rendered = engine.rendered.lock().unwrap().clone();
        assert!(!rendered.is_empty());
        assert!(rendered.iter().all(|&p| p < 30), "{rendered:?}");
    }

    #[test]
    fn previous_hit_behind_the_prefetch_window_is_rendered() {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 40];
        pages[20] = &["cat"];
        pages[26] = &["cat owl"];
        let engine = TextEngine::new(&pages);
        let mut h = searching(&engine);
        // A jump there, not scrolling past page 20.
        search_for(&mut h, "owl");
        assert_eq!(h.app.view.centre_page(), Some(26));
        assert!(!engine.rendered.lock().unwrap().contains(&20));
        search_for(&mut h, "cat");
        assert_eq!(h.app.view.centre_page(), Some(26));
        // Six pages behind: inside the keep window, outside the 3 behind
        // rendered ahead.
        h.until(|_| engine.rendered.lock().unwrap().contains(&20));
        assert!(engine.rendered.lock().unwrap().contains(&20));
    }

    #[test]
    fn dual_mode_renders_hit_spread_ahead() {
        let engine = far_cats();
        let mut h = searching(&engine);
        h.press(egui::Key::D, Modifiers::NONE);
        search_for(&mut h, "cat");
        let layout = h.app.view.layout();
        let spread = layout.spread_pages(layout.spread_of(150));
        h.until(|_| {
            let rendered = engine.rendered.lock().unwrap();
            spread.clone().all(|p| rendered.contains(&p))
        });
        let rendered = engine.rendered.lock().unwrap();
        assert!(spread.clone().all(|p| rendered.contains(&p)), "{spread:?}");
    }

    /// 200 pages; a link on page 0 jumps to page 150.
    fn far_link_engine() -> TextEngine {
        let mut pages: Vec<&[&str]> = vec![&["a dog"]; 200];
        pages[0] = &["first line", "second line"];
        TextEngine::new(&pages).with_links(
            0,
            vec![internal(
                vec![area(72.0, 90.0, 60.0, 12.0)],
                150,
                Some(300.0),
            )],
        )
    }

    #[test]
    fn history_target_kept_after_a_far_jump() {
        let engine = far_link_engine();
        let mut h = linking(&engine);
        h.until(|h| h.app.textures.contains_key(&0));
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.view.centre_page(), Some(150));
        go_to(&mut h, 155);
        h.frames(5, vec![]);
        assert!(
            h.app.textures.contains_key(&0),
            "page 0 dropped: {:?}",
            h.app.textures.keys().collect::<Vec<_>>()
        );
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.app.view.centre_page(), Some(0));
    }

    #[test]
    fn forward_target_kept_after_going_back() {
        let engine = far_link_engine();
        let mut h = linking(&engine);
        h.until(|h| h.app.textures.contains_key(&0));
        click_link(&mut h, 0, 100.0, 96.0);
        h.until(|h| h.app.textures.contains_key(&150));
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.app.view.centre_page(), Some(0));
        h.frames(5, vec![]);
        assert!(
            h.app.textures.contains_key(&150),
            "page 150 dropped: {:?}",
            h.app.textures.keys().collect::<Vec<_>>()
        );
        h.press(egui::Key::ArrowRight, Modifiers::ALT);
        assert_eq!(h.app.view.centre_page(), Some(150));
    }

    #[test]
    fn dual_mode_keeps_the_history_targets_spread() {
        let engine = far_link_engine();
        let mut h = linking(&engine);
        h.press(egui::Key::D, Modifiers::NONE);
        h.frames(2, vec![]);
        h.until(|h| {
            let layout = h.app.view.layout();
            let spread = layout.spread_pages(layout.spread_of(0));
            spread
                .clone()
                .all(|page| h.app.textures.contains_key(&page))
        });
        click_link(&mut h, 0, 100.0, 96.0);
        go_to(&mut h, 155);
        h.frames(5, vec![]);
        let layout = h.app.view.layout();
        let spread = layout.spread_pages(layout.spread_of(0));
        assert!(
            spread
                .clone()
                .all(|page| h.app.textures.contains_key(&page)),
            "{spread:?} dropped: {:?}",
            h.app.textures.keys().collect::<Vec<_>>()
        );
    }

    /// The pages the page view shows now.
    fn pages_shown(h: &Harness) -> std::ops::Range<usize> {
        let (_, top) = h.app.view.offset();
        let (_, height) = h.app.view.viewport_size();
        h.app.view.layout().visible_pages(top, top + height)
    }

    #[test]
    fn a_far_jump_keeps_the_pages_visible_beside_the_history_target() {
        let engine = WideEngine::default();
        let mut h = Harness::build_with(Arc::new(engine), |app| app);
        let band = pages_shown(&h);
        assert!(band.len() >= 3, "only {band:?} shown");
        h.until(|h| band.clone().all(|page| h.app.textures.contains_key(&page)));
        // A page jump, as an open or a wanted page asks for: leave, then go.
        h.app.arrival = Some((Target::Page(150), true));
        h.frames(3, vec![]);
        assert!(h.app.view.centre_page().is_some_and(|page| page >= 150));
        let kept: Vec<usize> = band
            .clone()
            .filter(|page| h.app.textures.contains_key(page))
            .collect();
        assert_eq!(
            kept,
            band.clone().collect::<Vec<_>>(),
            "dropped after the jump: {:?}",
            h.app.textures.keys().collect::<Vec<_>>()
        );
        // Back is sharp at once: nothing was dropped, so nothing re-renders.
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert!(
            band.clone().all(|page| h.app.textures.contains_key(&page)),
            "dropped on the way back: {:?}",
            h.app.textures.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_history_targets_band_is_kept_but_not_rendered_ahead() {
        let engine = WideEngine::default();
        let mut h = Harness::build_with(Arc::new(engine.clone()), |app| app);
        let band = pages_shown(&h);
        assert!(band.len() >= 3, "only {band:?} shown");
        h.until(|h| band.clone().all(|page| h.app.textures.contains_key(&page)));
        h.app.arrival = Some((Target::Page(150), true));
        h.frames(3, vec![]);
        // Zooming asks for every page the view keeps at the new scale. The
        // band stays kept, but the pages of it beyond the target's own spread
        // (which is prefetched) are never in the order asked for.
        let before = engine.rendered().len();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, Action::ZoomIn);
        h.frames(3, vec![]);
        let target = h.app.history.back_entries().last().expect("a Back target");
        let layout = h.app.view.layout();
        let requested = layout.spread_pages(layout.spread_of(target.page));
        let ahead: Vec<usize> = engine.rendered()[before..]
            .iter()
            .copied()
            .filter(|page| band.contains(page) && !requested.contains(page))
            .collect();
        assert!(ahead.is_empty(), "band rendered ahead: {ahead:?}");
    }

    #[test]
    fn stale_history_entry_survives_a_shorter_reload() {
        // An end-to-end guard on the reload-and-history path: the layout clamps
        // the stale page's spread, so the guard in `pages()` is not what makes
        // this pass. Its only extra effect is the dual/odd-left-false neighbour.
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.view.centre_page(), Some(5));
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.app.view.centre_page(), Some(0));
        // A version with four pages: the Forward target, page 5, is gone.
        h.reload(Arc::new(SizedEngine::letter(4)), recompiled_key());
        h.frames(5, vec![]);
        assert_eq!(h.app.view.centre_page(), Some(0));
    }

    #[test]
    fn search_field_has_room_above_and_below_the_text() {
        let h = Harness::new();
        // egui's 20 pt, plus 2 pt above and below.
        assert_eq!(h.app.bar.search.height(), 24.0);
    }

    // ---- Links ----

    fn area(x: f32, y: f32, width: f32, height: f32) -> noido_engine::Rect {
        noido_engine::Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn web(uri: &str, areas: Vec<noido_engine::Rect>) -> noido_engine::Link {
        noido_engine::Link {
            areas,
            target: noido_engine::LinkTarget::Web(uri.into()),
        }
    }

    fn internal(areas: Vec<noido_engine::Rect>, page: usize, y: Option<f32>) -> noido_engine::Link {
        noido_engine::Link {
            areas,
            target: noido_engine::LinkTarget::Internal(noido_engine::Destination {
                page,
                x: Some(72.0),
                y,
            }),
        }
    }

    /// Ten pages with two lines of text; on page 0 an internal link at
    /// (72, 90) to page 5, a web link at (72, 200), a link of two areas at
    /// y 300 and 320, and two overlapping web links around (350, 430).
    fn link_engine() -> TextEngine {
        TextEngine::new(&[&["first line", "second line"] as &[&str]; 10]).with_links(
            0,
            vec![
                internal(vec![area(72.0, 90.0, 60.0, 12.0)], 5, Some(300.0)),
                web("https://example.org/", vec![area(72.0, 200.0, 100.0, 12.0)]),
                web(
                    "https://two.example/",
                    vec![
                        area(72.0, 300.0, 100.0, 12.0),
                        area(72.0, 320.0, 50.0, 12.0),
                    ],
                ),
                web("https://x.example/", vec![area(300.0, 400.0, 100.0, 40.0)]),
                web("https://y.example/", vec![area(300.0, 420.0, 100.0, 40.0)]),
            ],
        )
    }

    /// A harness whose document and links thread are `engine`.
    fn linking(engine: &TextEngine) -> Harness {
        let linker = engine.clone();
        Harness::build_with(Arc::new(engine.clone()), move |app| {
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        })
    }

    /// Where page point (`x`, `y`) on `page` is on screen.
    fn screen_pos(h: &Harness, page: usize, x: f32, y: f32) -> egui::Pos2 {
        let view = &h.app.view;
        let (cx, cy) = view
            .layout()
            .content_point(noido_core::DocPoint { page, x, y })
            .unwrap();
        let (ox, oy) = view.offset();
        h.app.last_scroll.unwrap().inner.min + vec2(cx - ox, cy - oy)
    }

    /// Moves the pointer to `at` and runs frames until the pointing hand
    /// shows there.
    fn hover_link(h: &mut Harness, at: egui::Pos2) {
        h.frames(1, vec![Event::PointerMoved(at)]);
        h.until(|h| h.cursor == egui::CursorIcon::PointingHand);
        assert_eq!(h.cursor, egui::CursorIcon::PointingHand);
    }

    /// The rectangles stroked in the hyperlink colour last frame.
    fn link_outlines(h: &Harness) -> Vec<Rect> {
        let colour = h.ctx.global_style().visuals.hyperlink_color;
        fn find(shape: &egui::Shape, colour: Color32, out: &mut Vec<Rect>) {
            match shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.stroke.color == colour => {
                    out.push(r.rect);
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, colour, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.shapes {
            find(&clipped.shape, colour, &mut out);
        }
        out
    }

    #[test]
    fn hand_over_a_link() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 100.0, 96.0);
        hover_link(&mut h, at);
    }

    #[test]
    fn link_outlines_use_the_theme_link_colour() {
        let mut h = linking(&link_engine());
        let palette = h.theme().palette(egui::Theme::Dark);
        assert_eq!(h.ctx.global_style().visuals.hyperlink_color, palette.link);
        let at = screen_pos(&h, 0, 100.0, 96.0);
        hover_link(&mut h, at);
        assert!(!link_outlines(&h).is_empty());
    }

    #[test]
    fn grab_beside_a_link() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 100.0, 96.0);
        hover_link(&mut h, at);
        let beside = screen_pos(&h, 0, 400.0, 600.0);
        h.frames(2, vec![Event::PointerMoved(beside)]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
        assert!(link_outlines(&h).is_empty());
    }

    #[test]
    fn outline_drawn_around_every_area_of_hovered_link() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 100.0, 306.0);
        hover_link(&mut h, at);
        let outlines = link_outlines(&h);
        assert_eq!(outlines.len(), 2, "{outlines:?}");
        let second = screen_pos(&h, 0, 72.0, 320.0);
        assert!(
            outlines.iter().any(|r| (r.min - second).length() < 4.0),
            "{outlines:?}"
        );
    }

    #[test]
    fn no_hand_over_scroll_bar_at_a_link() {
        let engine = TextEngine::new(&[&["text"] as &[&str]; 3]).with_links(
            0,
            vec![web(
                "https://page.example/",
                vec![area(0.0, 0.0, 612.0, 792.0)],
            )],
        );
        let mut h = linking(&engine);
        for _ in 0..3 {
            h.app.view.apply(Action::ZoomIn);
        }
        h.frames(2, vec![]);
        let at = screen_pos(&h, 0, 300.0, 300.0);
        hover_link(&mut h, at);
        let bar = scroll_bar_point(&h);
        assert!(h.app.view.layout().page_rect(0).unwrap().width > h.size.x);
        h.frames(3, vec![Event::PointerMoved(bar)]);
        assert_ne!(h.cursor, egui::CursorIcon::PointingHand);
    }

    #[test]
    fn topmost_of_overlapping_links_wins() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 350.0, 430.0);
        hover_link(&mut h, at);
        h.until(|h| h.text_rect("https://y.example/").is_some());
        assert!(h.text_rect("https://y.example/").is_some());
        assert!(h.text_rect("https://x.example/").is_none());
    }

    #[test]
    fn hand_appears_when_links_arrive_under_still_pointer() {
        let engine = link_engine().gated_links();
        let mut h = linking(&engine);
        let at = screen_pos(&h, 0, 100.0, 96.0);
        h.frames(5, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
        engine.open_link_gate();
        h.until(|h| h.cursor == egui::CursorIcon::PointingHand);
        assert_eq!(h.cursor, egui::CursorIcon::PointingHand);
    }

    #[test]
    fn web_link_shows_its_address() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 100.0, 206.0);
        hover_link(&mut h, at);
        h.until(|h| h.text_rect("https://example.org/").is_some());
        assert!(h.text_rect("https://example.org/").is_some());
    }

    /// The flash's outlines drawn last frame (wider than the hover outline).
    fn flash_outlines(h: &Harness) -> Vec<Rect> {
        fn find(shape: &egui::Shape, out: &mut Vec<Rect>) {
            match shape {
                egui::Shape::Rect(r) if r.stroke.width == FLASH_OUTLINE_WIDTH => {
                    out.push(r.rect);
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &h.shapes {
            find(&clipped.shape, &mut out);
        }
        out
    }

    /// Hovers the link at page point (`x`, `y`) on `page`, then clicks it.
    fn click_link(h: &mut Harness, page: usize, x: f32, y: f32) {
        let at = screen_pos(h, page, x, y);
        hover_link(h, at);
        h.click(at);
        h.frames(1, vec![]);
    }

    #[test]
    fn click_on_internal_link_centres_target() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        let target = screen_pos(&h, 5, 72.0, 300.0);
        let centre = h.app.last_scroll.unwrap().inner.center();
        assert!(
            (target.y - centre.y).abs() < 1.0,
            "{target:?} vs {centre:?}"
        );
    }

    #[test]
    fn click_sets_target_as_current_page() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.chosen, Some(5));
    }

    #[test]
    fn drag_starting_on_link_pans_and_does_not_follow() {
        let mut h = linking(&link_engine());
        let at = screen_pos(&h, 0, 100.0, 96.0);
        hover_link(&mut h, at);
        h.drag(at, vec2(0.0, -100.0));
        h.frames(2, vec![]);
        // Panned (kinetic scrolling may carry it further), not followed.
        let y = h.app.view.offset().1;
        assert!(y >= 90.0, "{y}");
        assert_eq!(h.app.chosen, None);
    }

    #[test]
    fn click_in_paged_mode_shows_target_spread() {
        let mut h = linking(&link_engine());
        h.press(egui::Key::C, Modifiers::NONE);
        h.frames(2, vec![]);
        assert!(!h.app.view.continuous());
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.view.current_spread(), Some(5));
    }

    #[test]
    fn click_on_web_link_opens_it() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 206.0);
        assert_eq!(h.opened, vec!["https://example.org/".to_owned()]);
        assert_eq!(h.app.chosen, None);
    }

    #[test]
    fn javascript_and_file_links_do_not_open() {
        let engine = TextEngine::new(&[&["text"] as &[&str]; 3]).with_links(
            0,
            vec![
                web("javascript:alert(1)", vec![area(72.0, 100.0, 100.0, 20.0)]),
                web("file:///etc/passwd", vec![area(72.0, 200.0, 100.0, 20.0)]),
                web("no scheme at all", vec![area(72.0, 300.0, 100.0, 20.0)]),
            ],
        );
        let mut h = linking(&engine);
        for y in [110.0, 210.0, 310.0] {
            click_link(&mut h, 0, 100.0, y);
        }
        assert!(h.opened.is_empty(), "{:?}", h.opened);
    }

    #[test]
    fn uppercase_https_and_mailto_open() {
        let engine = TextEngine::new(&[&["text"] as &[&str]; 3]).with_links(
            0,
            vec![
                web("HTTPS://EXAMPLE.ORG/", vec![area(72.0, 100.0, 100.0, 20.0)]),
                web(
                    "mailto:someone@example.org",
                    vec![area(72.0, 200.0, 100.0, 20.0)],
                ),
            ],
        );
        let mut h = linking(&engine);
        click_link(&mut h, 0, 100.0, 110.0);
        click_link(&mut h, 0, 100.0, 210.0);
        assert_eq!(
            h.opened,
            vec![
                "HTTPS://EXAMPLE.ORG/".to_owned(),
                "mailto:someone@example.org".to_owned()
            ]
        );
    }

    #[test]
    fn flash_drawn_after_jump_and_gone_after_a_second() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        h.until(|h| !flash_outlines(h).is_empty());
        assert!(!flash_outlines(&h).is_empty());
        let flash = flash_outlines(&h);
        assert_eq!(flash.len(), 1);
        // No line below 300 on page 5: a bar at its height.
        let at = screen_pos(&h, 5, 72.0, 300.0);
        assert!(
            (flash[0].center().y - at.y).abs() < 4.0,
            "{flash:?} vs {at:?}"
        );
        h.frames(70, vec![]);
        assert!(flash_outlines(&h).is_empty());
    }

    #[test]
    fn new_jump_replaces_flash() {
        let engine = link_engine().with_links(
            5,
            vec![internal(
                vec![area(72.0, 280.0, 100.0, 30.0)],
                8,
                Some(85.0),
            )],
        );
        let mut h = linking(&engine);
        click_link(&mut h, 0, 100.0, 96.0);
        h.until(|h| !flash_outlines(h).is_empty());
        assert!(!flash_outlines(&h).is_empty());
        click_link(&mut h, 5, 100.0, 295.0);
        // Page 8's first line, from 90.4 down.
        let line = screen_pos(&h, 8, 72.0, 90.4);
        h.until(|h| {
            flash_outlines(h)
                .iter()
                .any(|r| (r.min - line).length() < 4.0)
        });
        let flash = flash_outlines(&h);
        assert_eq!(flash.len(), 1, "{flash:?}");
        assert!(
            (flash[0].min - line).length() < 4.0,
            "{flash:?} vs {line:?}"
        );
    }

    #[test]
    fn reload_drops_links_and_flash() {
        // The jump lands on page 5, where each version has a link of its own.
        let old = link_engine().with_links(
            5,
            vec![web(
                "https://old.example/",
                vec![area(300.0, 250.0, 100.0, 30.0)],
            )],
        );
        let mut h = linking(&old);
        click_link(&mut h, 0, 100.0, 96.0);
        h.until(|h| !flash_outlines(h).is_empty());
        assert!(!flash_outlines(&h).is_empty());
        let new = TextEngine::new(&[&["text"] as &[&str]; 10]).with_links(
            5,
            vec![web(
                "https://new.example/",
                vec![area(72.0, 330.0, 100.0, 30.0)],
            )],
        );
        h.reload(Arc::new(new), recompiled_key());
        h.frames(1, vec![]);
        assert!(flash_outlines(&h).is_empty());
        let stale = screen_pos(&h, 5, 350.0, 265.0);
        h.frames(5, vec![Event::PointerMoved(stale)]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
        let fresh = screen_pos(&h, 5, 100.0, 345.0);
        hover_link(&mut h, fresh);
    }

    #[test]
    fn tab_never_focuses_the_page_area() {
        let mut h = linking(&link_engine());
        for _ in 0..40 {
            h.press(egui::Key::Tab, Modifiers::NONE);
            let focused = h.ctx.memory(|m| m.focused());
            if let Some(response) = focused.and_then(|id| h.ctx.read_response(id)) {
                assert!(
                    response.rect.height() < 2000.0,
                    "the page area took focus: {:?}",
                    response.rect
                );
            }
        }
    }

    /// Clicks `button` (the mouse's back or forward) mid-window.
    fn mouse_button(h: &mut Harness, button: egui::PointerButton) {
        let pos = pos2(h.size.x / 2.0, h.size.y / 2.0);
        let event = |pressed| Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(pos)]);
        h.frames(1, vec![event(true)]);
        h.frames(1, vec![event(false)]);
        h.frames(1, vec![]);
    }

    /// Four pages, labelled i, ii, none and 4.
    fn labelled() -> TextEngine {
        TextEngine::new(&[&["one"], &["two"], &["three"], &["four"]]).with_labels(&[
            Some("i"),
            Some("ii"),
            None,
            Some("4"),
        ])
    }

    fn numbers_known(h: &Harness) -> bool {
        h.app.links.numbers != noido_core::PageNumbers::default()
    }

    fn shows_status(h: &Harness, start: &str) -> bool {
        h.texts().iter().any(|t| t.starts_with(start))
    }

    #[test]
    fn title_bar_shows_the_printed_number() {
        let mut h = linking(&labelled());
        h.until(numbers_known);
        h.frames(1, vec![]);
        assert!(shows_status(&h, "i (1 / 4) · "), "{:?}", h.texts());
    }

    #[test]
    fn title_bar_shows_file_numbers_before_the_reply() {
        let engine = labelled().gated_links();
        let mut h = linking(&engine);
        h.frames(3, vec![]);
        assert!(shows_status(&h, "1 / 4 · "), "{:?}", h.texts());
        engine.open_link_gate();
    }

    #[test]
    fn thumbnail_label_shows_both_numbers() {
        let mut h = linking(&labelled());
        h.app.sidebar.open = true;
        h.until(numbers_known);
        h.frames(2, vec![]);
        assert!(h.text_color("i (1)").is_some(), "{:?}", h.texts());
    }

    #[test]
    fn search_card_shows_both_numbers() {
        let engine = TextEngine::cats().with_labels(&[None, None, Some("iii")]);
        let (linker, searcher) = (engine.clone(), engine.clone());
        let mut h = Harness::build_with(Arc::new(engine), move |app| {
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
            .with_searcher(TextSearcher::start(
                Arc::new(searcher),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        });
        h.until(numbers_known);
        search_for(&mut h, "cat");
        h.frames(2, vec![]);
        assert!(h.text_color("Page iii (3)").is_some(), "{:?}", h.texts());
    }

    #[test]
    fn no_labels_look_as_before() {
        let engine = TextEngine::new(&[&["one"], &["two"]]);
        let mut h = linking(&engine);
        h.app.sidebar.open = true;
        h.frames(5, vec![]);
        assert!(shows_status(&h, "1 / 2 · "), "{:?}", h.texts());
        assert!(h.text_color("1").is_some(), "{:?}", h.texts());
    }

    #[test]
    fn reload_keeps_labels_until_the_reply() {
        let mut h = linking(&labelled());
        h.until(numbers_known);
        let new = TextEngine::new(&[&["one"]])
            .with_labels(&[Some("x")])
            .gated_links();
        h.reload(Arc::new(new.clone()), recompiled_key());
        h.frames(2, vec![]);
        assert!(shows_status(&h, "i (1 / 1) · "), "{:?}", h.texts());
        new.open_link_gate();
        h.until(|h| shows_status(h, "x (1 / 1) · "));
        assert!(shows_status(&h, "x (1 / 1) · "), "{:?}", h.texts());
    }

    #[test]
    fn long_label_keeps_the_file_number_at_narrow_width() {
        let label = "A label far too long to fit under any thumbnail at all";
        let engine = TextEngine::new(&[&["one"]]).with_labels(&[Some(label)]);
        let mut h = linking(&engine);
        h.app.sidebar.open = true;
        h.app.sidebar.width = noido_core::SIDEBAR_MIN_WIDTH;
        h.until(numbers_known);
        h.frames(2, vec![]);
        let shown = h
            .texts()
            .into_iter()
            .find(|t| t.ends_with("… (1)"))
            .unwrap_or_else(|| panic!("no elided number: {:?}", h.texts()));
        assert!(shown.starts_with("A label"), "{shown}");
        assert!(shown.len() < label.len(), "{shown}");
    }

    #[test]
    fn following_a_link_and_going_back() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.centre().page, 5);
        go_to(&mut h, 7);
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 0);
        h.press(egui::Key::ArrowRight, Modifiers::ALT);
        assert_eq!(h.centre().page, 7);
    }

    #[test]
    fn thumbnail_click_adds_an_entry() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_thumbnail(&mut h, 2);
        assert_eq!(h.app.current_page(), Some(2));
        assert_eq!(h.app.history.back_entries().len(), 1);
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.app.current_page(), Some(0));
        mouse_button(&mut h, egui::PointerButton::Extra2);
        assert_eq!(h.app.current_page(), Some(2));
    }

    #[test]
    fn home_and_end_jump_pages_and_come_back() {
        let mut h = Harness::new();
        go_to(&mut h, 5);
        let read_to = h.centre();
        h.press(egui::Key::Home, Modifiers::NONE);
        assert_eq!(h.centre().page, 0);
        assert_eq!(h.app.view.offset().1, 0.0);
        h.press(egui::Key::Home, Modifiers::NONE);
        assert_eq!(
            h.app.history.back_entries().len(),
            1,
            "a press that moves nothing adds no entry"
        );
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert!(h.distance_from_centre(read_to) < 1.0);
        h.press(egui::Key::End, Modifiers::NONE);
        assert_eq!(h.centre().page, 9);
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 5);
    }

    #[test]
    fn back_keeps_the_zoom_and_modes() {
        let mut h = sidebar_on(Arc::new(BlankEngine));
        click_thumbnail(&mut h, 2);
        h.press(egui::Key::Plus, Modifiers::COMMAND);
        h.press(egui::Key::D, Modifiers::NONE);
        let before = (h.app.view.mode(), h.app.view.scale(), h.app.view.dual());
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.app.current_page(), Some(0));
        assert_eq!(
            (h.app.view.mode(), h.app.view.scale(), h.app.view.dual()),
            before
        );
    }

    #[test]
    fn a_search_is_one_entry() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        assert_eq!(h.app.view.centre_page(), Some(2));
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(h.app.view.centre_page(), Some(7));
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.app.view.centre_page(), Some(0));
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn ctrl_g_after_reading_away_adds_an_entry() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        go_to(&mut h, 4);
        let read_to = h.app.view.centre_page();
        h.press(egui::Key::G, Modifiers::COMMAND);
        assert_eq!(h.app.view.centre_page(), Some(7));
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.app.view.centre_page(), read_to);
    }

    #[test]
    fn history_is_in_the_saved_state() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        let state = h.app.doc_state().unwrap();
        assert_eq!(state.history.back_entries().len(), 1);
        assert_eq!(state.history.back_entries()[0].page, 0);
    }

    #[test]
    fn back_after_the_file_got_shorter() {
        let mut h = sidebar_on(Arc::new(BlankEngine30));
        go_to(&mut h, 20);
        wheel_sidebar(&mut h, 100_000.0);
        click_thumbnail(&mut h, 1);
        assert_eq!(h.app.current_page(), Some(1));
        h.reload(Arc::new(BlankEngine), recompiled_key());
        assert_eq!(h.app.current_page(), Some(1));
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 9);
    }

    #[test]
    fn back_does_nothing_behind_the_settings() {
        let mut h = linking(&link_engine());
        click_link(&mut h, 0, 100.0, 96.0);
        h.press(egui::Key::Comma, Modifiers::COMMAND);
        assert!(h.app.settings_page.is_some());
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.centre().page, 5);
    }

    #[test]
    fn a_new_query_without_moving_adds_no_entry() {
        let engine = TextEngine::cats();
        let mut h = searching(&engine);
        search_for(&mut h, "cat");
        assert_eq!(h.app.view.centre_page(), Some(2));
        h.press(egui::Key::Escape, Modifiers::NONE);
        search_for(&mut h, "cat");
        mouse_button(&mut h, egui::PointerButton::Extra1);
        assert_eq!(h.app.view.centre_page(), Some(0));
        assert!(h.app.history.back_entries().is_empty());
    }

    // ---- Arriving from a link ----

    fn remembered(page: usize) -> DocState {
        DocState {
            view: ViewState {
                zoom: noido_core::ZoomMode::Custom,
                scale: 0.8,
                continuous: true,
                dual: false,
                odd_left: true,
                gapless: false,
                centre: noido_core::DocPoint {
                    page,
                    x: 306.0,
                    y: 400.0,
                },
            },
            sidebar: SidebarState::default(),
            history: History::default(),
            highlights: Vec::new(),
            pins: Pins::default(),
            pagelet: None,
        }
    }

    #[test]
    fn opens_at_the_linked_page() {
        let h = Harness::build(|app| app.opening_at(Some(Target::Page(5))));
        assert_eq!(h.centre().page, 5);
        assert_eq!(h.app.current_page(), Some(5));
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn opening_at_a_page_of_no_pages_does_nothing() {
        let h = Harness::build_with(Arc::new(SizedEngine::letter(0)), |app| {
            app.opening_at(Some(Target::Page(2)))
        });
        assert_eq!(h.app.current_page(), None);
    }

    #[test]
    fn opening_past_the_end_goes_to_the_last_page() {
        let h = Harness::build(|app| app.opening_at(Some(Target::Page(99))));
        assert_eq!(h.app.current_page(), Some(9));
    }

    #[test]
    fn opening_at_a_page_keeps_the_remembered_place_for_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut h = Harness::build(|app| {
            app.with_persistence(
                Store::at(dir.path()),
                test_slot(dir.path()),
                Some(remembered(7)),
            )
            .opening_at(Some(Target::Page(2)))
        });
        assert_eq!(h.app.current_page(), Some(2));
        assert_eq!(
            h.app.view.state().unwrap().zoom,
            noido_core::ZoomMode::Custom
        );
        assert!((h.app.view.scale() - 0.8).abs() < 1e-4);
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 7);
    }

    #[test]
    fn hand_over_jumps_and_back_returns() {
        let activation = crate::instance::Activation::default();
        let mut h = Harness::build(|app| app.with_activation(activation.clone()));
        go_to(&mut h, 4);
        activation.request(Some(Target::Page(8)));
        h.frames(1, vec![]);
        assert_eq!(h.app.current_page(), Some(8));
        assert!(h.commands.contains(&egui::ViewportCommand::Focus));
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 4);
    }

    #[test]
    fn hand_over_without_a_page_only_comes_forward() {
        let activation = crate::instance::Activation::default();
        let mut h = Harness::build(|app| app.with_activation(activation.clone()));
        go_to(&mut h, 4);
        let page = h.centre().page;
        activation.request(None);
        h.frames(1, vec![]);
        assert_eq!(h.centre().page, page);
        assert!(h.app.history.back_entries().is_empty());
    }

    /// A harness on ten pages with a place named "sec" on page 6, opening
    /// at `target`.
    fn named_places(target: Target) -> Harness {
        let engine = TextEngine::new(&[&["a line"] as &[&str]; 10]).with_named("sec", 6);
        let linker = engine.clone();
        Harness::build_with(Arc::new(engine), move |app| {
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
            .opening_at(Some(target))
        })
    }

    #[test]
    fn named_place_is_followed() {
        let mut h = named_places(Target::Named("sec".into()));
        h.until(|h| h.app.current_page() == Some(6));
        assert_eq!(h.app.current_page(), Some(6));
        assert!(h.app.links.flash(h.time).is_none(), "flashed");
    }

    #[test]
    fn unknown_named_place_stays() {
        let mut h = named_places(Target::Named("nope".into()));
        h.until(|h| h.app.current_page() != Some(0));
        assert_eq!(h.app.current_page(), Some(0));
        assert!(h.app.history.back_entries().is_empty());
    }

    /// A harness on ten pages with a place named "sec" on page 6, opening
    /// at it; the lookup waits until `engine.open_link_gate()`.
    fn gated_named_place() -> (Harness, TextEngine) {
        let engine = TextEngine::new(&[&["a line"] as &[&str]; 10])
            .with_named("sec", 6)
            .gated_links();
        let linker = engine.clone();
        let h = Harness::build_with(Arc::new(engine.clone()), move |app| {
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
            .opening_at(Some(Target::Named("sec".into())))
        });
        (h, engine)
    }

    #[test]
    fn moving_drops_a_pending_named_place() {
        let (mut h, engine) = gated_named_place();
        go_to(&mut h, 2);
        let page = h.centre().page;
        engine.open_link_gate();
        h.until(|h| h.centre().page != page);
        assert_eq!(h.centre().page, page);
    }

    #[test]
    fn a_reload_drops_the_old_versions_named_place() {
        let (mut h, engine) = gated_named_place();
        let page = h.centre().page;
        // The old version's answer waits, unread, when the new one arrives.
        engine.open_link_gate();
        std::thread::sleep(std::time::Duration::from_millis(100));
        h.reload(
            Arc::new(TextEngine::new(&[&["a line"] as &[&str]; 10])),
            recompiled_key(),
        );
        h.until(|h| h.centre().page != page);
        assert_eq!(h.centre().page, page);
    }

    #[test]
    fn newer_arrival_drops_a_pending_named_place() {
        let activation = crate::instance::Activation::default();
        let engine = TextEngine::new(&[&["a line"] as &[&str]; 10])
            .with_named("sec", 6)
            .gated_links();
        let linker = engine.clone();
        let mut h = Harness::build_with(Arc::new(engine.clone()), {
            let activation = activation.clone();
            move |app| {
                app.with_linker(noido_render::Linker::start(
                    Arc::new(linker),
                    Arc::new(Vec::new()),
                    Box::new(|| {}),
                ))
                .with_activation(activation)
            }
        });
        activation.request(Some(Target::Named("sec".into())));
        h.frames(1, vec![]);
        activation.request(Some(Target::Page(2)));
        h.frames(1, vec![]);
        engine.open_link_gate();
        h.until(|h| h.app.current_page() != Some(2));
        assert_eq!(h.app.current_page(), Some(2));
    }

    #[test]
    fn copy_link_copies_the_centre_page() {
        let mut h = Harness::build(|app| {
            app.with_name("Some Book.pdf".into())
                .with_link_path("/b/Some Book.pdf".into())
        });
        go_to(&mut h, 4);
        let page = h.centre().page + 1;
        h.press(egui::Key::L, Modifiers::COMMAND);
        h.press(egui::Key::L, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(
            h.copied,
            vec![
                format!("noido:///b/Some%20Book.pdf#page={page}"),
                format!("[Some Book, p. {page}](noido:///b/Some%20Book.pdf#page={page})"),
            ]
        );
    }

    #[test]
    fn copy_without_a_path_does_nothing() {
        let mut h = Harness::new();
        h.press(egui::Key::L, Modifiers::COMMAND);
        assert!(h.copied.is_empty());
    }

    // Text selection.

    /// "the cat sat" / "on the mat" on page 0 (glyphs 6 pt wide from x 72,
    /// baselines 100 and 120), "a dog" on page 1.
    fn selecting() -> (Harness, TextEngine) {
        let engine = TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"], &["one more"]]);
        let mut h = searching(&engine);
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        (h, engine)
    }

    /// Drags from page point (`x`, `y`) to (`to_x`, `to_y`) on `page` with
    /// `modifiers` held.
    fn drag_text(
        h: &mut Harness,
        page: usize,
        (x, y): (f32, f32),
        (to_x, to_y): (f32, f32),
        modifiers: Modifiers,
    ) {
        let from = screen_pos(h, page, x, y);
        let to = screen_pos(h, page, to_x, to_y);
        h.modifiers = modifiers;
        h.drag(from, to - from);
        h.frames(1, vec![]);
        h.modifiers = Modifiers::NONE;
    }

    fn selected_on(h: &Harness, page: usize) -> Option<std::ops::Range<usize>> {
        let noido_core::PageTextState::Known(text) = h.app.texts.get(page) else {
            return None;
        };
        h.app.select.selection?.on_page(page, text.char_count())
    }

    #[test]
    fn shift_drag_selects_and_does_not_pan() {
        let (mut h, _) = selecting();
        let offset = h.app.view.offset();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(0..5));
        assert_eq!(h.app.view.offset(), offset);
    }

    #[test]
    fn plain_drag_pans_and_selects_nothing() {
        let (mut h, _) = selecting();
        let offset = h.app.view.offset();
        drag_text(&mut h, 0, (73.0, 300.0), (100.0, 95.0), Modifiers::NONE);
        assert_eq!(h.app.select.selection, None);
        assert_ne!(h.app.view.offset(), offset);
    }

    #[test]
    fn tool_drag_selects_and_shift_drag_pans_keeping_it() {
        let (mut h, _) = selecting();
        h.press(egui::Key::S, Modifiers::NONE);
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::NONE);
        assert_eq!(selected_on(&h, 0), Some(0..5));
        let offset = h.app.view.offset();
        drag_text(&mut h, 0, (73.0, 300.0), (100.0, 115.0), Modifiers::SHIFT);
        assert_ne!(h.app.view.offset(), offset);
        assert_eq!(selected_on(&h, 0), Some(0..5));
    }

    #[test]
    fn s_menu_and_button_toggle_the_tool() {
        let mut h = Harness::new();
        h.press(egui::Key::S, Modifiers::NONE);
        assert!(h.app.select.tool);
        let entry = noido_core::menu()
            .into_iter()
            .flatten()
            .find(|entry| entry.label == "Select text")
            .unwrap();
        let ctx = h.ctx.clone();
        h.app.apply(&ctx, None, entry.action);
        assert!(!h.app.select.tool);
        h.frames(2, vec![]);
        h.click(h.app.bar.select.center());
        h.frames(1, vec![]);
        assert!(h.app.select.tool);
    }

    #[test]
    fn selection_is_drawn() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        let fill = h
            .ctx
            .global_style()
            .visuals
            .selection
            .bg_fill
            .gamma_multiply(SELECTION_OPACITY);
        let drawn = filled_with(&h, fill);
        let inside = screen_pos(&h, 0, 80.0, 95.0);
        assert!(drawn.iter().any(|r| r.contains(inside)), "{drawn:?}");
    }

    #[test]
    fn esc_clears_selection() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        h.press(egui::Key::Escape, Modifiers::NONE);
        assert_eq!(h.app.select.selection, None);
        assert_eq!(h.app.select.editing, None);

        // With a highlight selected for editing, Esc lets go of it too and
        // leaves the highlight drawn where it is stored.
        select_the_highlight(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        assert_eq!(h.app.select.selection, None);
        assert_eq!(h.app.select.editing, None);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Yellow, dark);
        let at = screen_pos(&h, 0, 80.0, 95.0);
        assert!(
            filled_with(&h, fill).iter().any(|r| r.contains(at)),
            "the highlight is not drawn at {at:?}"
        );
    }

    #[test]
    fn esc_in_search_field_keeps_selection() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        focus_search(&mut h);
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(1, vec![]);
        assert_eq!(selected_on(&h, 0), Some(0..5));
    }

    #[test]
    fn click_on_pages_clears_selection() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        let at = screen_pos(&h, 0, 300.0, 400.0);
        h.click(at);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.selection, None);
    }

    #[test]
    fn ibeam_over_text_with_shift() {
        let (mut h, _) = selecting();
        let at = screen_pos(&h, 0, 80.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::Text);
        h.modifiers = Modifiers::NONE;
        h.frames(2, vec![Event::PointerMoved(at)]);
        assert_eq!(h.cursor, egui::CursorIcon::Grab);
    }

    #[test]
    fn selection_follows_zoom() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        h.frames(3, vec![]);
        let fill = h
            .ctx
            .global_style()
            .visuals
            .selection
            .bg_fill
            .gamma_multiply(SELECTION_OPACITY);
        let inside = screen_pos(&h, 0, 80.0, 95.0);
        let outside = screen_pos(&h, 0, 120.0, 95.0);
        let drawn = filled_with(&h, fill);
        assert!(drawn.iter().any(|r| r.contains(inside)), "{drawn:?}");
        assert!(!drawn.iter().any(|r| r.contains(outside)), "{drawn:?}");
    }

    #[test]
    fn reload_clears_selection() {
        let (mut h, _) = selecting();
        go_to(&mut h, 2);
        h.until(|h| matches!(h.app.texts.get(2), noido_core::PageTextState::Known(_)));
        drag_text(&mut h, 2, (73.0, 95.0), (90.0, 95.0), Modifiers::SHIFT);
        assert!(selected_on(&h, 2).is_some());
        h.reload(Arc::new(TextEngine::new(&[&["short"]])), recompiled_key());
        h.frames(3, vec![]);
        assert_eq!(h.app.select.selection, None);
    }

    /// Presses at page point (`x`, `y`) on page 0 with `modifiers`, moves
    /// to screen point `to` and holds there for `frames` frames, then
    /// releases. Returns the offset before the release.
    fn hold_drag(
        h: &mut Harness,
        (x, y): (f32, f32),
        to: egui::Pos2,
        modifiers: Modifiers,
        frames: usize,
    ) -> (f32, f32) {
        let from = screen_pos(h, 0, x, y);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        h.modifiers = modifiers;
        h.frames(1, vec![Event::PointerMoved(from)]);
        h.frames(1, vec![button(from, true)]);
        h.frames(1, vec![Event::PointerMoved(from + (to - from) * 0.5)]);
        h.frames(1, vec![Event::PointerMoved(to)]);
        h.frames(frames, vec![]);
        let offset = h.app.view.offset();
        h.frames(1, vec![button(to, false)]);
        h.modifiers = Modifiers::NONE;
        offset
    }

    #[test]
    fn drag_past_bottom_scrolls_and_extends() {
        let (mut h, _) = selecting();
        let before = h.app.view.offset();
        let below = h.app.last_scroll.unwrap().inner.center_bottom() + vec2(0.0, 50.0);
        let after = hold_drag(&mut h, (73.0, 95.0), below, Modifiers::SHIFT, 30);
        assert!(after.1 > before.1 + 100.0, "{before:?} → {after:?}");
        let (start, end) = h.app.select.selection.unwrap().carets().unwrap();
        assert!(end.page > start.page, "{start:?}..{end:?}");
    }

    #[test]
    fn drag_past_right_edge_scrolls_sideways() {
        let (mut h, _) = selecting();
        for _ in 0..4 {
            h.press(egui::Key::Equals, Modifiers::COMMAND);
        }
        h.frames(3, vec![]);
        let before = h.app.view.offset();
        let inner = h.app.last_scroll.unwrap().inner;
        let right = egui::pos2(inner.right() + 50.0, inner.center().y);
        let at = (inner.left() + 20.0, inner.center().y);
        let p = h
            .app
            .view
            .layout()
            .doc_point(
                at.0 - inner.left() + before.0,
                at.1 - inner.top() + before.1,
            )
            .unwrap();
        let after = hold_drag(&mut h, (p.x, p.y), right, Modifiers::SHIFT, 30);
        assert!(after.0 > before.0 + 100.0, "{before:?} → {after:?}");
    }

    #[test]
    fn no_edge_scroll_when_panning() {
        let (mut h, _) = selecting();
        let below = h.app.last_scroll.unwrap().inner.center_bottom() + vec2(0.0, 50.0);
        let from = screen_pos(&h, 0, 73.0, 400.0);
        h.frames(1, vec![Event::PointerMoved(from)]);
        h.frames(
            1,
            vec![Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
        );
        h.frames(1, vec![Event::PointerMoved(below)]);
        h.frames(3, vec![]);
        let held = h.app.view.offset();
        h.frames(20, vec![]);
        assert_eq!(h.app.view.offset(), held);
    }

    /// Selects "the cat" on page 0 with Shift+drag.
    fn select_the_cat(h: &mut Harness) {
        drag_text(h, 0, (73.0, 95.0), (112.0, 95.0), Modifiers::SHIFT);
        assert_eq!(selected_on(h, 0), Some(0..7));
    }

    #[test]
    fn copy_event_copies_selection() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![Event::Copy]);
        assert_eq!(h.copied, vec!["the cat".to_owned()]);
    }

    #[test]
    fn copy_in_search_field_does_not_copy_selection() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        focus_search(&mut h);
        h.frames(1, vec![Event::Copy]);
        assert!(!h.copied.contains(&"the cat".to_owned()), "{:?}", h.copied);
    }

    #[test]
    fn copy_with_nothing_selected_does_nothing() {
        let (mut h, _) = selecting();
        h.frames(1, vec![Event::Copy]);
        assert!(h.copied.is_empty());
    }

    #[test]
    fn bar_shows_on_release_and_copies() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let copy = h.text_rect("Copy").expect("the bar shows");
        let line_top = screen_pos(&h, 0, 100.0, 90.4).y;
        assert!(copy.bottom() <= line_top, "{copy:?} above {line_top}");
        h.click(copy.center());
        h.frames(1, vec![]);
        assert_eq!(h.copied, vec!["the cat".to_owned()]);
        assert!(
            h.app.select.selection.is_some(),
            "a click on the bar keeps it"
        );
    }

    #[test]
    fn bar_hidden_while_dragging() {
        let (mut h, _) = selecting();
        let from = screen_pos(&h, 0, 73.0, 95.0);
        let to = screen_pos(&h, 0, 112.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.frames(1, vec![Event::PointerMoved(from)]);
        h.frames(
            1,
            vec![Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            }],
        );
        h.frames(2, vec![Event::PointerMoved(to)]);
        assert!(h.text_rect("Copy").is_none());
    }

    #[test]
    fn handle_drag_moves_that_end_with_tool_off() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let offset = h.app.view.offset();
        let end = h.app.select.handles[1].expect("the end's handle shows");
        let step = screen_pos(&h, 0, 126.0, 95.0).x - screen_pos(&h, 0, 114.0, 95.0).x;
        h.drag(end, vec2(step, 0.0));
        h.frames(1, vec![]);
        assert_eq!(selected_on(&h, 0), Some(0..9));
        assert_eq!(h.app.view.offset(), offset);
    }

    #[test]
    fn arrow_over_a_handle() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        let edge = end + vec2(select::HANDLE_REACH, 0.0);
        h.frames(2, vec![Event::PointerMoved(edge)]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
        h.modifiers = Modifiers::SHIFT;
        h.frames(2, vec![Event::PointerMoved(end)]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
        h.modifiers = Modifiers::NONE;
        let away = end + vec2(select::HANDLE_REACH + 2.0, 0.0);
        h.frames(2, vec![Event::PointerMoved(away)]);
        assert_ne!(h.cursor, egui::CursorIcon::Default);
    }

    #[test]
    fn arrow_while_dragging_a_handle() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        let button = |pressed| Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frames(1, vec![Event::PointerMoved(end)]);
        h.frames(1, vec![button(true)]);
        h.frames(2, vec![Event::PointerMoved(end + vec2(0.0, 40.0))]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
        h.frames(1, vec![button(false)]);
    }

    #[test]
    fn copy_waits_for_unknown_pages() {
        let engine =
            TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"], &["one more"]]).gated();
        let mut h = searching(&engine);
        let at = |page, char| noido_core::Spot::At(noido_core::Caret { page, char });
        h.app.select.selection = Some(noido_core::Selection {
            anchor: at(0, 0),
            focus: at(1, 3),
        });
        h.frames(1, vec![Event::Copy]);
        assert!(h.copied.is_empty());
        engine.open_gate();
        h.until(|h| !h.copied.is_empty());
        assert_eq!(h.copied, vec!["the cat sat on the mat a d".to_owned()]);
    }

    /// Clicks `count` times at page point (`x`, `y`) on page 0 with
    /// `modifiers` held.
    fn click_times(h: &mut Harness, (x, y): (f32, f32), count: usize, modifiers: Modifiers) {
        let at = screen_pos(h, 0, x, y);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        h.modifiers = modifiers;
        h.frames(1, vec![Event::PointerMoved(at)]);
        for _ in 0..count {
            h.frames(1, vec![button(true)]);
            h.frames(1, vec![button(false)]);
        }
        h.frames(1, vec![]);
        h.modifiers = Modifiers::NONE;
    }

    /// Runs egui's click window (0.3 s) out, so the next click is a single
    /// one rather than part of a double click.
    fn past_double_click(h: &mut Harness) {
        h.frames(25, vec![]);
    }

    #[test]
    fn double_click_selects_word_with_tool() {
        let (mut h, _) = selecting();
        h.press(egui::Key::S, Modifiers::NONE);
        click_times(&mut h, (99.0, 95.0), 2, Modifiers::NONE);
        assert_eq!(selected_on(&h, 0), Some(4..7));
    }

    #[test]
    fn triple_click_selects_line_with_shift() {
        let (mut h, _) = selecting();
        click_times(&mut h, (80.0, 115.0), 3, Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(12..22));
    }

    #[test]
    fn double_click_with_tool_off_selects_nothing() {
        let (mut h, _) = selecting();
        click_times(&mut h, (99.0, 95.0), 2, Modifiers::NONE);
        assert_eq!(h.app.select.selection, None);
    }

    #[test]
    fn shift_click_extends() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(10, vec![]);
        click_times(&mut h, (127.0, 95.0), 1, Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(0..9));
    }

    /// A harness on `link_engine()` with its links and its text.
    fn linking_text() -> Harness {
        linking_text_with(link_engine())
    }

    /// The app on `engine`, with links and text, once page 0's text is in.
    fn linking_text_with(engine: TextEngine) -> Harness {
        let (linker, searcher) = (engine.clone(), engine.clone());
        let mut h = Harness::build_with(Arc::new(engine), move |app| {
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
            .with_searcher(TextSearcher::start(
                Arc::new(searcher),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        });
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        h
    }

    #[test]
    fn shift_click_on_link_extends_and_does_not_follow() {
        let mut h = linking_text();
        // Right to left, so a shift+click to its left extends it: the
        // bar covers the line above, not this one.
        drag_text(&mut h, 0, (130.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(5..10));
        let at = screen_pos(&h, 0, 75.0, 96.0);
        hover_link(&mut h, at);
        h.frames(10, vec![]);
        click_times(&mut h, (75.0, 96.0), 1, Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(1..10));
        assert_eq!(h.app.chosen, None);
    }

    #[test]
    fn tool_click_on_link_follows() {
        let mut h = linking_text();
        h.press(egui::Key::S, Modifiers::NONE);
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.chosen, Some(5));
    }

    #[test]
    fn tool_drag_from_link_selects_and_does_not_follow() {
        let mut h = linking_text();
        h.press(egui::Key::S, Modifiers::NONE);
        let at = screen_pos(&h, 0, 73.0, 96.0);
        hover_link(&mut h, at);
        drag_text(&mut h, 0, (73.0, 96.0), (100.0, 96.0), Modifiers::NONE);
        assert_eq!(selected_on(&h, 0), Some(0..5));
        assert_eq!(h.app.chosen, None);
    }

    #[test]
    fn press_on_scroll_bar_neither_selects_nor_clears() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        let bar = scroll_bar_point(&h);
        h.click(bar);
        h.frames(1, vec![]);
        assert_eq!(
            selected_on(&h, 0),
            Some(0..7),
            "a click on the bar keeps it"
        );
        h.press(egui::Key::S, Modifiers::NONE);
        let x = h.app.view.offset().0;
        h.drag(bar, vec2(0.0, 60.0));
        h.frames(1, vec![]);
        assert_eq!(
            selected_on(&h, 0),
            Some(0..7),
            "a drag on the bar doesn't select"
        );
        assert_eq!(h.app.view.offset().0, x);
    }

    #[test]
    fn no_ibeam_over_scroll_bar() {
        let (mut h, _) = selecting();
        h.press(egui::Key::S, Modifiers::NONE);
        let bar = scroll_bar_point(&h);
        h.frames(2, vec![Event::PointerMoved(bar)]);
        assert_ne!(h.cursor, egui::CursorIcon::Text);
    }

    #[test]
    fn new_selection_drops_pending_copy() {
        let engine =
            TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"], &["one more"]]).gated();
        let mut h = searching(&engine);
        let at = |page, char| noido_core::Spot::At(noido_core::Caret { page, char });
        h.app.select.selection = Some(noido_core::Selection {
            anchor: at(0, 0),
            focus: at(1, 3),
        });
        h.frames(1, vec![Event::Copy]);
        h.app.select.selection = Some(noido_core::Selection {
            anchor: at(0, 4),
            focus: at(0, 7),
        });
        engine.open_gate();
        h.until(|h| matches!(h.app.texts.get(1), noido_core::PageTextState::Known(_)));
        h.frames(3, vec![]);
        assert!(h.copied.is_empty(), "{:?}", h.copied);
    }

    #[test]
    fn pending_copy_survives_an_end_being_placed() {
        let engine =
            TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"], &["one more"]]).gated();
        let mut h = searching(&engine);
        let at = |page, char| noido_core::Spot::At(noido_core::Caret { page, char });
        let waiting = noido_core::Spot::Waiting(noido_core::DocPoint {
            page: 0,
            x: 99.0,
            y: 95.0,
        });
        h.app.select.selection = Some(noido_core::Selection {
            anchor: waiting,
            focus: at(1, 3),
        });
        h.frames(1, vec![Event::Copy]);
        engine.open_gate();
        h.until(|h| !h.copied.is_empty());
        assert_eq!(h.copied, vec!["at sat on the mat a d".to_owned()]);
    }

    #[test]
    fn tool_click_with_a_wobble_on_link_follows_without_selecting() {
        let mut h = linking_text();
        h.press(egui::Key::S, Modifiers::NONE);
        // Just left of a glyph's middle, so the wobble crosses it.
        let at = screen_pos(&h, 0, 104.0, 96.0);
        hover_link(&mut h, at);
        h.drag(at, vec2(5.0, 0.0));
        h.frames(1, vec![]);
        assert_eq!(h.app.select.selection, None);
        assert_eq!(h.app.chosen, Some(5));
    }

    #[test]
    fn drag_without_a_button_held_ends() {
        let (mut h, _) = selecting();
        let origin = screen_pos(&h, 0, 73.0, 95.0);
        h.app.select.drag = Some(select::Drag {
            kind: DragKind::Select,
            origin,
            moved: true,
            started: false,
            handle: None,
            shift: true,
        });
        h.frames(1, vec![Event::PointerMoved(screen_pos(&h, 0, 112.0, 95.0))]);
        h.frames(1, vec![]);
        assert!(h.app.select.drag.is_none());
    }

    #[test]
    fn esc_closing_the_menu_keeps_selection() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.click(h.app.bar.menu.center());
        h.frames(2, vec![]);
        assert!(egui::Popup::is_any_open(&h.ctx), "the menu is open");
        h.press(egui::Key::Escape, Modifiers::NONE);
        h.frames(1, vec![]);
        assert_eq!(selected_on(&h, 0), Some(0..7));
    }

    #[test]
    fn click_on_a_handle_over_a_link_does_not_follow() {
        let engine = TextEngine::new(&[&["first line", "second line"] as &[&str]; 3]).with_links(
            0,
            vec![internal(
                vec![area(60.0, 80.0, 300.0, 60.0)],
                2,
                Some(300.0),
            )],
        );
        let mut h = linking_text_with(engine);
        drag_text(&mut h, 0, (73.0, 95.0), (100.0, 95.0), Modifiers::SHIFT);
        let selected = selected_on(&h, 0);
        h.frames(1, vec![]);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        h.frames(3, vec![Event::PointerMoved(end)]);
        assert_eq!(h.cursor, egui::CursorIcon::Default);
        h.click(end);
        h.frames(2, vec![]);
        assert_eq!(h.app.chosen, None);
        assert_eq!(selected_on(&h, 0), selected);
    }

    #[test]
    fn no_arrow_over_a_handle_with_the_menu_open() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        h.click(h.app.bar.menu.center());
        h.frames(2, vec![]);
        assert!(egui::Popup::is_any_open(&h.ctx), "the menu is open");
        h.frames(2, vec![Event::PointerMoved(end)]);
        assert_ne!(h.cursor, egui::CursorIcon::Default);
    }

    /// Presses on page 0's text and holds the pointer at `at`; the offsets
    /// after each of `seconds` seconds.
    fn hold_selecting_at(h: &mut Harness, at: egui::Pos2, seconds: usize) -> Vec<(f32, f32)> {
        let from = screen_pos(h, 0, 73.0, 95.0);
        let button = |pressed| Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::SHIFT,
        };
        h.modifiers = Modifiers::SHIFT;
        h.frames(1, vec![Event::PointerMoved(from)]);
        h.frames(1, vec![button(true)]);
        h.frames(1, vec![Event::PointerMoved(at)]);
        let mut offsets = vec![h.app.view.offset()];
        for _ in 0..seconds {
            h.frames(60, vec![]);
            offsets.push(h.app.view.offset());
        }
        h.frames(1, vec![button(false)]);
        h.modifiers = Modifiers::NONE;
        offsets
    }

    fn long_selecting() -> Harness {
        let engine = TextEngine::new(&[&["the cat sat", "on the mat"] as &[&str]; 30]);
        let mut h = searching(&engine);
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        h
    }

    #[test]
    fn edge_scroll_speeds_up_while_held() {
        let mut h = long_selecting();
        let edge = h.app.last_scroll.unwrap().inner.center_bottom() - vec2(0.0, 1.0);
        let o = hold_selecting_at(&mut h, edge, 2);
        let (first, second) = (o[1].1 - o[0].1, o[2].1 - o[1].1);
        assert!(first > 0.0, "{o:?}");
        assert!(second > first * 1.5, "{o:?}");
    }

    #[test]
    fn edge_scroll_speed_up_starts_again_after_leaving_the_band() {
        let mut h = long_selecting();
        let inner = h.app.last_scroll.unwrap().inner;
        let edge = inner.center_bottom() - vec2(0.0, 1.0);
        let from = screen_pos(&h, 0, 73.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.frames(1, vec![Event::PointerMoved(from)]);
        h.frames(
            1,
            vec![Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            }],
        );
        let second_at = |h: &mut Harness| {
            let start = h.app.view.offset().1;
            h.frames(60, vec![]);
            h.app.view.offset().1 - start
        };
        h.frames(1, vec![Event::PointerMoved(edge)]);
        let first = second_at(&mut h);
        h.frames(1, vec![Event::PointerMoved(inner.center())]);
        h.frames(10, vec![]);
        h.frames(1, vec![Event::PointerMoved(edge)]);
        let again = second_at(&mut h);
        assert!(first > 0.0);
        assert!(again < first * 1.25, "first {first}, again {again}");
    }

    /// Selects "the cat" on page 0 and makes it a yellow highlight.
    fn highlight_the_cat(h: &mut Harness) {
        highlight_the_cat_in(h, HighlightColour::Yellow);
    }

    /// Selects "the cat" on page 0 and makes it a highlight in `colour`
    /// with the bar's swatch.
    fn highlight_the_cat_in(h: &mut Harness, colour: HighlightColour) {
        select_the_cat(h);
        h.frames(1, vec![]);
        let index = HighlightColour::ALL
            .iter()
            .position(|c| *c == colour)
            .expect("the colour is offered");
        let swatch = h.app.select.swatches[index].expect("the bar offers the colours");
        h.click(swatch.center());
        h.frames(1, vec![]);
    }

    #[test]
    fn colour_swatch_in_bar_creates_highlight_and_selects_it() {
        let (mut h, _) = selecting();
        highlight_the_cat(&mut h);
        // The new highlight is selected for editing, so the reader can
        // recolour or delete it without clicking it again.
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(selected_on(&h, 0), Some(0..7));
        assert!(h.app.select.bar_rect.is_some(), "the bar stays open");
        assert!(h.app.select.delete_rect.is_some(), "the bar offers delete");
        assert_eq!(
            h.app.highlights.items(),
            [Highlight {
                start: Caret { page: 0, char: 0 },
                end: Caret { page: 0, char: 7 },
                colour: HighlightColour::Yellow,
            }]
        );
    }

    #[test]
    fn highlight_is_drawn_and_follows_zoom() {
        let (mut h, _) = selecting();
        highlight_the_cat(&mut h);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Yellow, dark);
        let at = screen_pos(&h, 0, 80.0, 95.0);
        assert!(
            filled_with(&h, fill).iter().any(|r| r.contains(at)),
            "not drawn at {at:?}"
        );
        h.press(egui::Key::Equals, Modifiers::COMMAND);
        h.frames(3, vec![]);
        let zoomed = screen_pos(&h, 0, 80.0, 95.0);
        let rects = filled_with(&h, fill);
        assert!(
            rects.iter().any(|r| r.contains(zoomed)),
            "not at {zoomed:?}"
        );
        assert!(!rects.iter().any(|r| r.contains(at)), "still at {at:?}");
    }

    #[test]
    fn undo_and_redo_highlight_creation() {
        let (mut h, _) = selecting();
        highlight_the_cat(&mut h);
        assert_eq!(h.app.highlights.items().len(), 1);
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());
        h.press(egui::Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());
        h.press(egui::Key::Y, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
    }

    // Editing a highlight.

    /// Clicks the yellow highlight on "the cat", selecting it for editing.
    fn click_the_highlight(h: &mut Harness) {
        let at = screen_pos(h, 0, 85.0, 95.0);
        h.click(at);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
    }

    /// Makes the yellow highlight on "the cat" and selects it for editing.
    fn select_the_highlight(h: &mut Harness) {
        highlight_the_cat(h);
        click_the_highlight(h);
    }

    /// The fill the selection is painted with, at the theme's opacity.
    fn selection_fill(h: &Harness) -> Color32 {
        h.ctx
            .global_style()
            .visuals
            .selection
            .bg_fill
            .gamma_multiply(SELECTION_OPACITY)
    }

    #[test]
    fn click_highlight_selects_it_with_tool_off_and_shows_handles_and_delete() {
        let (mut h, _) = selecting();
        highlight_the_cat(&mut h);
        click_the_highlight(&mut h);
        assert!(!h.app.select.tool, "the select tool is off");
        assert!(
            h.app.select.handles[0].is_some(),
            "the start's handle shows"
        );
        assert!(h.app.select.handles[1].is_some(), "the end's handle shows");
        assert!(h.app.select.delete_rect.is_some(), "the bar offers delete");
        let fill = selection_fill(&h);
        assert!(
            filled_with(&h, fill).is_empty(),
            "the selected highlight is not painted blue"
        );
        let empty = screen_pos(&h, 0, 300.0, 400.0);
        h.click(empty);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, None);
        assert_eq!(h.app.select.selection, None);
    }

    #[test]
    fn click_highlight_selects_it_with_tool_on() {
        let (mut h, _) = selecting();
        highlight_the_cat(&mut h);
        h.press(egui::Key::S, Modifiers::NONE);
        h.frames(1, vec![]);
        assert!(h.app.select.tool);
        click_the_highlight(&mut h);
        assert_eq!(selected_on(&h, 0), Some(0..7));
    }

    #[test]
    fn bar_hangs_from_the_click_on_a_highlight() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        // "the cat sat": a highlight long enough that its end sits well
        // away from a click near its start.
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(11),
            colour: HighlightColour::Yellow,
        });
        let click = screen_pos(&h, 0, 75.0, 95.0);
        h.click(click);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        // Hung from the click, the gap above it.
        assert!(
            (bar.bottom() - (click.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {bar:?} does not hang from the click at {click:?}"
        );
        // Centred on the click, or as near the window's left edge as a
        // bar this wide can get.
        assert!(
            (bar.center().x - click.x).abs() <= 3.0 || bar.left() <= 1.0,
            "the bar {bar:?} is not over the click at {click:?}"
        );
        let line_top = screen_pos(&h, 0, 100.0, 90.4).y;
        assert!(
            bar.bottom() <= line_top,
            "the bar {bar:?} is not above the line at {line_top}"
        );

        // Further into the highlight there is room to centre it, and the
        // highlight's own end (its right edge) is the wrong place.
        past_double_click(&mut h);
        let into = screen_pos(&h, 0, 127.0, 95.0);
        h.click(into);
        h.frames(1, vec![]);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            (bar.center().x - into.x).abs() <= 3.0,
            "the bar {bar:?} is not over the click at {into:?}"
        );
    }

    /// The bar is drawn hanging from `at`: the gap above it and centred on
    /// it, or as near the window's left edge as a bar this wide can get.
    fn assert_bar_hangs_from(h: &Harness, at: egui::Pos2) {
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            (bar.bottom() - (at.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {bar:?} does not hang from {at:?}"
        );
        assert!(
            (bar.center().x - at.x).abs() <= 3.0 || bar.left() <= 1.0,
            "the bar {bar:?} is not over {at:?}"
        );
    }

    /// The bar is drawn above the selection's first line, centred on `x`
    /// or as near the window's left edge as a bar this wide can get.
    fn assert_bar_above_the_start(h: &Harness, x: f32) {
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        let (top, _) = h.app.select.caret_lines[0].expect("the start is placed");
        assert!(
            (bar.bottom() - (top.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {bar:?} is not above the start at {top:?}"
        );
        assert!(
            (bar.center().x - x).abs() <= 3.0 || bar.left() <= 1.0,
            "the bar {bar:?} is not over {x}"
        );
    }

    #[test]
    fn clicking_a_highlight_keeps_the_bar_by_the_mouse() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(21),
            colour: HighlightColour::Yellow,
        });
        let click = screen_pos(&h, 0, 127.0, 95.0);
        h.click(click);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_hangs_from(&h, click);
    }

    #[test]
    fn bar_goes_above_an_upward_drag() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (130.0, 115.0), (127.0, 95.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        assert_bar_above_the_start(&h, screen_pos(&h, 0, 127.0, 95.0).x);
    }

    #[test]
    fn bar_goes_above_one_line_at_the_mouses_x() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (200.0, 95.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        assert_bar_above_the_start(&h, screen_pos(&h, 0, 200.0, 95.0).x);
    }

    #[test]
    fn bar_goes_above_a_selections_start_handle_let_go() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (130.0, 95.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        let knob = h.app.select.handles[0].expect("the start's handle shows");
        let by = vec2(screen_pos(&h, 0, 114.0, 95.0).x - knob.x, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_bar_above_the_start(&h, (knob + by).x);
    }

    #[test]
    fn bar_goes_above_a_highlights_start_handle_let_go() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(11),
            colour: HighlightColour::Yellow,
        });
        h.click(screen_pos(&h, 0, 127.0, 95.0));
        h.frames(1, vec![]);
        let knob = h.app.select.handles[0].expect("the start's handle shows");
        let by = vec2(screen_pos(&h, 0, 114.0, 95.0).x - knob.x, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_above_the_start(&h, (knob + by).x);
    }

    /// The bar is drawn below the selection's last knob, and below the mouse
    /// if that is further down, as it is on a handle let go.
    fn assert_bar_below_the_end(h: &Harness, mouse: egui::Pos2) {
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        let (_, bottom) = h.app.select.caret_lines[1].expect("the end is placed");
        let clear = bottom.y.max(mouse.y) + BELOW_END;
        assert!(
            (bar.top() - clear).abs() <= 1.0,
            "the bar {bar:?} is not below the end at {bottom:?} and the mouse at {mouse:?}"
        );
    }

    /// Over more than one line, which end a handle is decides where the bar
    /// goes. On one line it goes above either way.
    #[test]
    fn bar_goes_above_a_two_line_selections_start_handle_let_go() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (127.0, 115.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        let knob = h.app.select.handles[0].expect("the start's handle shows");
        let by = vec2(12.0, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_bar_above_the_start(&h, (knob + by).x);
    }

    #[test]
    fn bar_goes_below_a_two_line_selections_end_handle_let_go() {
        let (mut h, _) = selecting();
        // Made upwards, so that the end let go of differs from the one before.
        drag_text(&mut h, 0, (127.0, 115.0), (73.0, 95.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        let knob = h.app.select.handles[1].expect("the end's handle shows");
        let by = vec2(12.0, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_bar_below_the_end(&h, knob + by);
    }

    #[test]
    fn bar_goes_above_an_end_handle_let_go_across_the_start() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 115.0), (127.0, 115.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        let knob = h.app.select.handles[1].expect("the end's handle shows");
        let to = screen_pos(&h, 0, 100.0, 95.0);
        let by = vec2(to.x - knob.x, to.y - knob.y + 6.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.bar_end(), Some(0));
        assert_bar_above_the_start(&h, (knob + by).x);
    }

    /// A highlight over the two lines, clicked so that its bar is by the mouse.
    fn two_line_highlight(h: &mut Harness, click: (f32, f32)) {
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(21),
            colour: HighlightColour::Yellow,
        });
        h.click(screen_pos(h, 0, click.0, click.1));
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.select.bar_end(), None);
    }

    #[test]
    fn bar_goes_above_a_two_line_highlights_start_handle_let_go() {
        let (mut h, _) = selecting();
        two_line_highlight(&mut h, (120.0, 115.0));
        let knob = h.app.select.handles[0].expect("the start's handle shows");
        let by = vec2(12.0, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_above_the_start(&h, (knob + by).x);
    }

    #[test]
    fn bar_goes_below_a_two_line_highlights_end_handle_let_go() {
        let (mut h, _) = selecting();
        two_line_highlight(&mut h, (75.0, 95.0));
        let knob = h.app.select.handles[1].expect("the end's handle shows");
        let by = vec2(-12.0, 0.0);
        h.drag(knob, by);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_below_the_end(&h, knob + by);
    }

    #[test]
    fn a_highlights_handle_pressed_and_let_go_keeps_the_bar_by_the_click() {
        let (mut h, _) = selecting();
        two_line_highlight(&mut h, (75.0, 95.0));
        let before = h.app.select.bar_rect.expect("the bar is drawn");
        let knob = h.app.select.handles[1].expect("the end's handle shows");
        h.click(knob);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.select.bar_end(), None);
        assert_eq!(h.app.select.bar_rect, Some(before));
    }

    #[test]
    fn bar_clears_a_knob_scrolled_out_of_view() {
        let engine = TextEngine::new(&[
            &[
                "the cat sat",
                "on the mat",
                "a third one",
                "and a fourth",
                "and a fifth",
            ],
            &["a dog"],
        ]);
        let mut h = searching(&engine);
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(55),
            colour: HighlightColour::Yellow,
        });
        h.frames(2, vec![]);
        h.click(screen_pos(&h, 0, 80.0, 155.0));
        h.frames(2, vec![]);
        let inner = h.app.last_scroll.unwrap().inner;
        let knob = |h: &Harness| {
            let (_, bottom) = h.app.select.caret_lines[0].expect("the start is placed");
            Rect::from_center_size(
                bottom + vec2(0.0, select::HANDLE_RADIUS),
                Vec2::splat(2.0 * select::HANDLE_RADIUS),
            )
        };
        // The wheel scrolls by what it is asked, with no momentum: the knob's
        // centre just above the view, so no handle shows but its lower part
        // is in view.
        let dy = knob(&h).center().y - (inner.top() - 3.0);
        h.frames(1, vec![Event::PointerMoved(inner.center())]);
        h.frames(
            30,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                -dy,
                egui::MouseWheelSource::Wheel,
            )],
        );
        h.frames(30, vec![]);
        assert!(
            knob(&h).bottom() > inner.top(),
            "the knob is wholly out of view"
        );
        // A click on the highlight where the bar above it just fits in the
        // view, reaching over the knob's lower part.
        let size = h.app.select.bar_size.expect("the bar was measured");
        let click = pos2(
            screen_pos(&h, 0, 80.0, 95.0).x,
            inner.top() + 1.0 + size.y + SELECTION_BAR_GAP,
        );
        assert!(click.y - SELECTION_BAR_GAP - size.y < knob(&h).bottom());
        past_double_click(&mut h);
        h.click(click);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.select.handles[0], None);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            !bar.intersects(knob(&h)),
            "the bar {bar:?} covers {:?}",
            knob(&h)
        );
    }

    #[test]
    fn a_highlights_handle_let_go_on_its_other_end_leaves_the_bar_where_it_is() {
        let (mut h, _) = selecting();
        two_line_highlight(&mut h, (75.0, 95.0));
        let before = h.app.select.bar_rect.expect("the bar is drawn");
        let from = h.app.select.handles[0].expect("the start's handle shows");
        let to = h.app.select.handles[1].expect("the end's handle shows");
        h.drag(from, to - from);
        h.frames(2, vec![]);
        // Nothing changed: the highlight keeps its range, and the bar its place.
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.select.bar_rect, Some(before));
    }

    #[test]
    fn bar_goes_below_a_shift_click_extending_a_selection_downwards() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (150.0, 95.0), (127.0, 95.0), Modifiers::SHIFT);
        past_double_click(&mut h);
        let at = screen_pos(&h, 0, 300.0, 115.0);
        h.modifiers = Modifiers::SHIFT;
        h.click(at);
        h.frames(2, vec![]);
        h.modifiers = Modifiers::NONE;
        assert_bar_below_the_end(&h, at);
    }

    #[test]
    fn bar_goes_below_a_shift_click_extending_a_highlight_downwards() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(7),
            colour: HighlightColour::Yellow,
        });
        h.click(screen_pos(&h, 0, 80.0, 95.0));
        h.frames(2, vec![]);
        past_double_click(&mut h);
        let at = screen_pos(&h, 0, 300.0, 115.0);
        h.modifiers = Modifiers::SHIFT;
        h.click(at);
        h.frames(2, vec![]);
        h.modifiers = Modifiers::NONE;
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_below_the_end(&h, at);
    }

    #[test]
    fn bar_goes_above_a_shift_click_extending_a_highlight_upwards() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        // Only the second line.
        h.app.highlights.add(Highlight {
            start: caret(12),
            end: caret(21),
            colour: HighlightColour::Yellow,
        });
        h.click(screen_pos(&h, 0, 110.0, 115.0));
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        past_double_click(&mut h);
        let at = screen_pos(&h, 0, 300.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.click(at);
        h.frames(2, vec![]);
        h.modifiers = Modifiers::NONE;
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_above_the_start(&h, at.x);
    }

    #[test]
    fn a_handle_pressed_and_let_go_leaves_the_bar_where_it_is() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (130.0, 115.0), (127.0, 95.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        let before = h.app.select.bar_rect.expect("the bar is drawn");
        let knob = h.app.select.handles[1].expect("the end's handle shows");
        h.click(knob);
        h.frames(2, vec![]);
        assert_eq!(h.app.select.bar_rect, Some(before));
    }

    #[test]
    fn bar_goes_above_a_double_click() {
        let (mut h, _) = selecting();
        click_times(&mut h, (127.0, 95.0), 2, Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0), Some(8..11));
        h.frames(1, vec![]);
        assert_bar_above_the_start(&h, screen_pos(&h, 0, 127.0, 95.0).x);
    }

    #[test]
    fn bar_goes_above_a_triple_click() {
        let (mut h, _) = selecting();
        click_times(&mut h, (127.0, 95.0), 3, Modifiers::SHIFT);
        assert_eq!(selected_on(&h, 0).map(|r| r.start), Some(0));
        h.frames(1, vec![]);
        assert_bar_above_the_start(&h, screen_pos(&h, 0, 127.0, 95.0).x);
    }

    #[test]
    fn bar_goes_above_a_shift_click_on_the_first_line() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 115.0), (100.0, 115.0), Modifiers::SHIFT);
        past_double_click(&mut h);
        // Off to the side of the bar, which would take the press.
        let at = screen_pos(&h, 0, 300.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.click(at);
        h.frames(2, vec![]);
        h.modifiers = Modifiers::NONE;
        assert_bar_above_the_start(&h, at.x);
    }

    #[test]
    fn no_bar_after_a_double_click_on_a_page_without_text() {
        let engine = TextEngine::new(&[&[]]);
        let mut h = searching(&engine);
        h.until(|h| matches!(h.app.texts.get(0), noido_core::PageTextState::Known(_)));
        click_times(&mut h, (300.0, 400.0), 2, Modifiers::SHIFT);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.selection, None);
        assert_eq!(h.app.select.bar_rect, None);
    }

    #[test]
    fn bar_stays_in_view_when_a_drag_is_released_past_the_windows_edge() {
        let (mut h, _) = selecting();
        let inner = h.app.last_scroll.unwrap().inner;
        let from = screen_pos(&h, 0, 73.0, 95.0);
        h.modifiers = Modifiers::SHIFT;
        h.drag(from, pos2(from.x + 200.0, inner.bottom() + 200.0) - from);
        h.modifiers = Modifiers::NONE;
        // The drag ran onto the next page; the bar waits for its text.
        h.until(|h| h.app.select.selection.is_some_and(|s| s.carets().is_some()));
        h.frames(2, vec![]);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            inner.contains_rect(bar),
            "the bar {bar:?} is not within the view {inner:?}"
        );
    }

    #[test]
    fn panning_leaves_the_bar_on_its_page_point() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 115.0), (127.0, 115.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        let offset = h.app.view.offset();
        drag_text(&mut h, 0, (300.0, 400.0), (300.0, 370.0), Modifiers::NONE);
        h.frames(1, vec![]);
        assert_ne!(h.app.view.offset(), offset, "the drag did not pan");
        let bar_at = h.app.select.bar_at.expect("the bar has its point");
        let point = screen_pos(&h, 0, 127.0, 115.0);
        assert!(
            (bar_at - point).length() <= 1.0,
            "the bar hangs from {bar_at:?}, not its page point at {point:?}"
        );
    }

    /// The knob of each end is free of the bar.
    fn assert_bar_clears_the_handles(h: &Harness) {
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        for knob in h.app.select.handles.into_iter().flatten() {
            let knob = Rect::from_center_size(knob, Vec2::splat(2.0 * select::HANDLE_RADIUS));
            assert!(!bar.intersects(knob), "the bar {bar:?} covers {knob:?}");
        }
    }

    /// The bar below the selection has its top this far under the end's
    /// caret, or the mouse past it: past the knob and the gap.
    const BELOW_END: f32 = 2.0 * select::HANDLE_RADIUS + SELECTION_BAR_GAP;

    #[test]
    fn bar_goes_past_a_mouse_let_go_below_the_text() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (130.0, 160.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        // Let go in the blank below the text: past the mouse too, so a
        // click where it let go does not land on the bar.
        let mouse = screen_pos(&h, 0, 130.0, 160.0);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            (bar.top() - (mouse.y + BELOW_END)).abs() <= 1.0,
            "the bar {bar:?} is not below the mouse at {mouse:?}"
        );
    }

    #[test]
    fn bar_goes_below_a_drag_down() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (73.0, 95.0), (127.0, 115.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        let (_, bottom) = h.app.select.caret_lines[1].expect("the end is placed");
        assert!(
            (bar.top() - (bottom.y + BELOW_END)).abs() <= 1.0,
            "the bar {bar:?} is not below the end at {bottom:?}"
        );
    }

    #[test]
    fn bar_goes_below_a_drag_down_at_any_zoom() {
        let (mut h, _) = selecting();
        // Zoomed out until the first line's knob reaches the second line.
        for _ in 0..4 {
            h.press(egui::Key::Minus, Modifiers::COMMAND);
            h.frames(3, vec![]);
        }
        assert!(h.app.view.layout().scale() < 0.7, "not zoomed out enough");
        // Zooming moved the page: back to its top, for the lines to be in view.
        h.frames(1, vec![Event::PointerMoved(pos2(600.0, 700.0))]);
        h.frames(
            2,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                2000.0,
                egui::MouseWheelSource::Wheel,
            )],
        );
        drag_text(&mut h, 0, (73.0, 95.0), (127.0, 115.0), Modifiers::SHIFT);
        h.frames(2, vec![]);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        let (_, bottom) = h.app.select.caret_lines[1].expect("the end is placed");
        assert!(
            bar.top() > bottom.y,
            "the bar {bar:?} is not below the end at {bottom:?}"
        );
    }

    #[test]
    fn clicking_a_highlight_moves_the_bar_off_a_handle_it_would_cover() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(21),
            colour: HighlightColour::Yellow,
        });
        // On the second line, the bar over the mouse lands on the first's knob.
        h.click(screen_pos(&h, 0, 110.0, 115.0));
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_bar_clears_the_handles(&h);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        let (_, bottom) = h.app.select.caret_lines[1].expect("the end is placed");
        assert!(
            (bar.top() - (bottom.y + BELOW_END)).abs() <= 1.0,
            "the bar {bar:?} is not below the end at {bottom:?}"
        );
    }

    #[test]
    fn bar_is_placed_with_its_measured_size() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(2, vec![]);
        let drawn = h.app.select.bar_rect.expect("the bar is drawn");
        assert_eq!(h.app.select.bar_size, Some(drawn.size()));
        // Told it is far wider, it is pushed that far in from the window's
        // edge, for the frame until it measures itself again.
        h.app.select.bar_size = Some(vec2(800.0, drawn.height()));
        h.frames(1, vec![]);
        let pushed = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            (pushed.center().x - 400.0).abs() <= 1.0,
            "the bar {pushed:?} did not go by its size"
        );
        h.frames(1, vec![]);
        assert_eq!(h.app.select.bar_rect, Some(drawn));
    }

    #[test]
    fn making_a_highlight_leaves_the_bar_where_it_is() {
        let (mut h, _) = selecting();
        drag_text(&mut h, 0, (130.0, 115.0), (127.0, 95.0), Modifiers::SHIFT);
        h.frames(1, vec![]);
        let before = h.app.select.bar_rect.expect("the bar is drawn");
        let swatch = h.app.select.swatches[0].expect("the bar offers the colours");
        h.click(swatch.center());
        h.frames(2, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.select.bar_rect, Some(before));
    }

    #[test]
    fn bar_follows_the_clicked_line_when_the_view_moves() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(11),
            colour: HighlightColour::Yellow,
        });
        let click = screen_pos(&h, 0, 75.0, 95.0);
        h.click(click);
        h.frames(1, vec![]);
        let before = h.app.select.bar_rect.expect("the bar is drawn");
        assert!(
            (before.bottom() - (click.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {before:?} does not hang from the click at {click:?}"
        );

        // Move the view under the bar: the page point clicked moves on
        // screen, and the bar goes with it.
        h.frames(1, vec![Event::PointerMoved(pos2(450.0, 500.0))]);
        h.frames(
            1,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                -60.0,
                egui::MouseWheelSource::Wheel,
            )],
        );
        h.frames(2, vec![]);
        let now = screen_pos(&h, 0, 75.0, 95.0);
        let after = h.app.select.bar_rect.expect("the bar is still drawn");
        assert_ne!(
            after.bottom(),
            before.bottom(),
            "the page did not move: {now:?} is still at the click {click:?}"
        );
        assert!(
            (after.bottom() - (now.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {after:?} is not above the clicked line now at {now:?}"
        );
    }

    #[test]
    fn bar_hides_when_the_clicked_line_leaves_the_view() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(11),
            colour: HighlightColour::Yellow,
        });
        h.click(screen_pos(&h, 0, 75.0, 95.0));
        h.frames(1, vec![]);
        assert!(h.app.select.bar_rect.is_some(), "the bar is drawn");

        // Scrolled well past the clicked line, the bar goes with it and
        // out of the viewport.
        h.frames(1, vec![Event::PointerMoved(pos2(450.0, 500.0))]);
        h.frames(
            2,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                -3000.0,
                egui::MouseWheelSource::Wheel,
            )],
        );
        assert!(
            h.app.select.bar_rect.is_none(),
            "the bar is still drawn at {:?}",
            h.app.select.bar_rect
        );

        // Back at the top, the bar hangs from the click again.
        for _ in 0..5 {
            h.frames(1, vec![Event::PointerMoved(pos2(450.0, 500.0))]);
            h.frames(
                2,
                vec![wheel(
                    egui::MouseWheelUnit::Point,
                    2000.0,
                    egui::MouseWheelSource::Wheel,
                )],
            );
        }
        let bar = h.app.select.bar_rect.expect("the bar is back");
        let click = screen_pos(&h, 0, 75.0, 95.0);
        assert!(
            (bar.bottom() - (click.y - SELECTION_BAR_GAP)).abs() <= 1.0,
            "the bar {bar:?} is not above the click now at {click:?}"
        );
    }

    #[test]
    fn copy_in_the_bar_keeps_the_highlight_selected() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let copy = h.text_rect("Copy").expect("the bar shows Copy");
        h.click(copy.center());
        h.frames(1, vec![]);
        assert_eq!(h.copied, vec!["the cat".to_owned()]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(selected_on(&h, 0), Some(0..7));
        assert!(h.app.select.bar_rect.is_some(), "the bar stays open");
    }

    #[test]
    fn the_bar_and_its_swatches_are_bigger() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let bar = h.app.select.bar_rect.expect("the bar is drawn");
        // The sizes as literals: a test against the constants would pass
        // whatever they were shrunk to.
        for swatch in h.app.select.swatches {
            let swatch = swatch.expect("the bar offers the colours");
            assert!(
                swatch.width() >= 27.0 && swatch.height() >= 27.0,
                "the swatch {swatch:?} is smaller than 27 points"
            );
        }
        assert!(
            bar.height() >= 48.0,
            "the bar is {} tall, not 48: {bar:?}",
            bar.height()
        );
    }

    #[test]
    fn the_bar_keeps_its_delete_button_until_there_is_a_highlight() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        let fresh = h.app.select.bar_rect.expect("the bar is drawn");
        // The button is there from the start, disabled, so the bar does
        // not widen under the pointer once a highlight is made.
        let delete = h
            .app
            .select
            .delete_rect
            .expect("the bar offers Delete before a highlight exists");
        h.click(delete.center());
        h.frames(1, vec![]);
        assert!(
            h.app.highlights.items().is_empty(),
            "a disabled Delete acted"
        );
        assert_eq!(selected_on(&h, 0), Some(0..7));
        assert!(h.app.select.delete_rect.is_some(), "the bar stays open");

        let swatch = h.app.select.swatches[0].expect("the bar offers the colours");
        h.click(swatch.center());
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        let made = h.app.select.bar_rect.expect("the bar is still drawn");
        assert_eq!(
            made.width(),
            fresh.width(),
            "the bar changed width when the highlight was made: {fresh:?} then {made:?}"
        );
    }

    #[test]
    fn the_bar_offers_a_swatch_for_every_colour() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        assert!(
            h.app.select.swatches.iter().all(Option::is_some),
            "the bar leaves a colour out: {:?}",
            h.app.select.swatches
        );
    }

    #[test]
    fn the_bar_rings_the_swatch_of_the_selected_colour() {
        let (mut h, _) = selecting();
        // Not Yellow: the first swatch, which a bar that always rings the
        // first one would still get right.
        highlight_the_cat_in(&mut h, HighlightColour::Blue);
        click_the_highlight(&mut h);
        // The bar fades in; let it reach full strength before reading it.
        h.frames(20, vec![]);
        let ring = h.ctx.global_style().visuals.selection.stroke.color;
        let ringed = |h: &Harness, colour: HighlightColour| {
            let index = HighlightColour::ALL
                .iter()
                .position(|c| *c == colour)
                .expect("the colour is offered");
            let swatch = h.app.select.swatches[index].expect("the bar offers the colours");
            stroked_circles(h, ring)
                .iter()
                .any(|(centre, radius)| swatch.contains(*centre) && *radius > select::SWATCH_RADIUS)
        };
        assert!(ringed(&h, HighlightColour::Blue), "blue is ringed");
        assert!(!ringed(&h, HighlightColour::Yellow), "yellow is not ringed");
        assert!(!ringed(&h, HighlightColour::Green), "green is not ringed");
    }

    #[test]
    fn hovering_a_swatch_names_its_colour() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.frames(1, vec![]);
        // The names as literals: a mis-mapped colour_name would still match
        // itself, so two colours sharing a name must fail here.
        let names = ["Yellow", "Green", "Blue", "Pink", "Purple"];
        for ((index, colour), name) in HighlightColour::ALL.into_iter().enumerate().zip(names) {
            let swatch = h.app.select.swatches[index].expect("the bar offers the colours");
            h.frames(1, vec![Event::PointerMoved(swatch.center())]);
            h.frames(40, vec![]);
            let Some(tip) = h.text_rect(name) else {
                panic!("{colour:?} is not named {name:?}");
            };
            // The bar sits over the selection, so a tooltip under the swatch
            // lands on the pointer and is unreadable.
            let bar = h.app.select.bar_rect.expect("the bar is drawn");
            assert!(
                tip.bottom() <= bar.top() + 0.5,
                "{name:?}'s tooltip {tip:?} is not above the bar {bar:?} (0.5px rounding allowed)"
            );
        }
    }

    #[test]
    fn drag_highlight_handle_resizes_it_and_undoes_in_one_step() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        let step = screen_pos(&h, 0, 126.0, 95.0).x - screen_pos(&h, 0, 114.0, 95.0).x;
        h.drag(end, vec2(step, 0.0));
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.get(0).unwrap().end.char, 9);
        assert_eq!(h.app.select.editing, Some(0));
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.get(0).unwrap().end.char, 7);
        assert_eq!(h.app.highlights.get(0).unwrap().start.char, 0);
        assert_eq!(h.app.select.editing, None);
    }

    #[test]
    fn dragging_a_handle_draws_the_live_range() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let end = h.app.select.handles[1].expect("the end's handle shows");
        let step = screen_pos(&h, 0, 126.0, 95.0).x - screen_pos(&h, 0, 114.0, 95.0).x;
        // Hold the end's handle and drag it two characters on, without
        // letting go.
        h.frames(1, vec![Event::PointerMoved(end)]);
        h.frames(
            1,
            vec![Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
        );
        h.frames(1, vec![Event::PointerMoved(end + vec2(step, 0.0))]);
        assert_eq!(h.app.highlights.get(0).unwrap().end.char, 7);
        let dark = h.ctx.global_style().visuals.dark_mode;
        let fill = select::highlight_fill(HighlightColour::Yellow, dark);
        let live = screen_pos(&h, 0, 120.0, 95.0);
        assert!(
            filled_with(&h, fill).iter().any(|r| r.contains(live)),
            "the live range is not drawn at {live:?}"
        );
    }

    #[test]
    fn a_fresh_selection_drag_lets_go_of_the_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        h.press(egui::Key::S, Modifiers::NONE);
        h.frames(1, vec![]);
        assert!(h.app.select.tool);
        drag_text(&mut h, 0, (90.0, 95.0), (112.0, 95.0), Modifiers::NONE);
        assert_eq!(h.app.select.editing, None);
        assert_eq!(selected_on(&h, 0), Some(3..7));
        let highlight = h.app.highlights.get(0).expect("the highlight is kept");
        assert_eq!((highlight.start.char, highlight.end.char), (0, 7));
    }

    #[test]
    fn collapsing_highlight_handle_reverts_to_stored_range() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let start = h.app.select.handles[0].expect("the start's handle shows");
        let end = h.app.select.handles[1].expect("the end's handle shows");
        h.drag(end, start - end);
        h.frames(1, vec![]);
        let highlight = h.app.highlights.get(0).expect("the highlight is kept");
        assert_eq!((highlight.start.char, highlight.end.char), (0, 7));
        assert_eq!(selected_on(&h, 0), Some(0..7));
    }

    #[test]
    fn colour_swatch_recolours_selected_highlight_and_keeps_it_selected() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let swatch = h.app.select.swatches[2].expect("the bar offers the colours");
        h.click(swatch.center());
        h.frames(1, vec![]);
        assert_eq!(
            h.app.highlights.get(0).unwrap().colour,
            HighlightColour::Blue
        );
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(selected_on(&h, 0), Some(0..7));
        assert!(h.app.select.bar_rect.is_some(), "the bar stays open");
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(
            h.app.highlights.get(0).unwrap().colour,
            HighlightColour::Yellow
        );
    }

    #[test]
    fn delete_button_and_backspace_remove_selected_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        let delete = h.app.select.delete_rect.expect("the bar offers delete");
        h.click(delete.center());
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());
        assert_eq!(h.app.select.editing, None);
        assert_eq!(h.app.select.selection, None);

        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
        click_the_highlight(&mut h);
        h.press(egui::Key::Backspace, Modifiers::NONE);
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());

        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        click_the_highlight(&mut h);
        h.press(egui::Key::Delete, Modifiers::NONE);
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());
    }

    #[test]
    fn undo_with_no_history_keeps_the_selection() {
        let (mut h, _) = selecting();
        select_the_cat(&mut h);
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(selected_on(&h, 0), Some(0..7));
    }

    #[test]
    fn backspace_behind_the_settings_page_keeps_the_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        open_settings(&mut h);
        h.press(egui::Key::Backspace, Modifiers::NONE);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
    }

    #[test]
    fn delete_behind_the_settings_page_keeps_the_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        open_settings(&mut h);
        h.press(egui::Key::Delete, Modifiers::NONE);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
    }

    #[test]
    fn undo_and_redo_behind_the_settings_page_keep_the_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        open_settings(&mut h);
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
        h.app.highlights.undo();
        h.press(egui::Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        h.frames(1, vec![]);
        assert!(h.app.highlights.items().is_empty());
    }

    #[test]
    fn undo_from_the_menu_behind_the_settings_page_keeps_the_highlight() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        open_settings(&mut h);
        h.app.queued.push(Action::Undo);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
    }

    #[test]
    fn backspace_and_undo_in_search_field_leave_highlight_alone() {
        let (mut h, _) = selecting();
        select_the_highlight(&mut h);
        focus_search(&mut h);
        h.typed("cat");
        h.press(egui::Key::Backspace, Modifiers::NONE);
        h.press(egui::Key::Z, Modifiers::COMMAND);
        h.frames(1, vec![]);
        assert_eq!(h.app.highlights.items().len(), 1);
    }

    #[test]
    fn copy_copies_selected_highlight_across_pages() {
        let engine =
            TextEngine::new(&[&["the cat sat", "on the mat"], &["a dog"], &["one more"]]).gated();
        let mut h = searching(&engine);
        let at = |page, char| noido_core::Spot::At(Caret { page, char });
        let index = h
            .app
            .highlights
            .add(Highlight {
                start: Caret { page: 0, char: 0 },
                end: Caret { page: 1, char: 3 },
                colour: HighlightColour::Yellow,
            })
            .expect("the highlight is added");
        h.app.select.editing = Some(index);
        h.app.select.selection = Some(noido_core::Selection {
            anchor: at(0, 0),
            focus: at(1, 3),
        });
        h.frames(1, vec![Event::Copy]);
        assert!(h.copied.is_empty());
        engine.open_gate();
        h.until(|h| !h.copied.is_empty());
        assert_eq!(h.copied, vec!["the cat sat on the mat a d".to_owned()]);
    }

    #[test]
    fn click_overlapping_highlights_picks_shorter_and_double_click_selects_word() {
        let (mut h, _) = selecting();
        let caret = |char| Caret { page: 0, char };
        let colour = HighlightColour::Yellow;
        h.app.highlights.add(Highlight {
            start: caret(0),
            end: caret(11),
            colour,
        });
        h.app.highlights.add(Highlight {
            start: caret(4),
            end: caret(7),
            colour: HighlightColour::Green,
        });
        // A point inside both highlights picks the shorter one.
        let cat = screen_pos(&h, 0, 99.0, 95.0);
        h.click(cat);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, Some(1));
        assert_eq!(selected_on(&h, 0), Some(4..7));
        // A point only the longer one covers picks it.
        let the = screen_pos(&h, 0, 75.0, 95.0);
        h.click(the);
        h.frames(1, vec![]);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(selected_on(&h, 0), Some(0..11));
        // These two count as a double click rather than a third and
        // fourth.
        past_double_click(&mut h);
        // A double click selects the word instead, not the highlight.
        click_times(&mut h, (123.0, 95.0), 2, Modifiers::SHIFT);
        assert_eq!(h.app.select.editing, None);
        assert_eq!(selected_on(&h, 0), Some(8..11));
    }

    #[test]
    fn link_and_highlight_plain_click_follows_link_shift_click_selects_highlight() {
        let mut h = linking_text();
        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 0 },
            end: Caret { page: 0, char: 10 },
            colour: HighlightColour::Yellow,
        });
        click_link(&mut h, 0, 100.0, 96.0);
        assert_eq!(h.app.chosen, Some(5));
        assert_eq!(h.app.select.editing, None);

        // Back to the link's page; the clicks below are single ones.
        for _ in 0..20 {
            if h.centre().page == 0 {
                break;
            }
            h.press(egui::Key::PageUp, Modifiers::NONE);
            h.frames(1, vec![]);
        }
        h.app.chosen = None;
        past_double_click(&mut h);
        assert_eq!(h.centre().page, 0);
        click_times(&mut h, (100.0, 96.0), 1, Modifiers::SHIFT);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.chosen, None);

        past_double_click(&mut h);
        click_times(&mut h, (120.0, 96.0), 1, Modifiers::SHIFT);
        assert_eq!(h.app.select.editing, Some(0));
        assert_eq!(h.app.chosen, None);
        assert_eq!(selected_on(&h, 0), Some(0..8));
        // The extend reached the stored highlight, not just the selection.
        assert_eq!(h.app.highlights.get(0).unwrap().end.char, 8);

        // A Shift+click back onto the start swaps the selection's ends
        // rather than collapsing it, so the stored range stays put.
        past_double_click(&mut h);
        click_times(&mut h, (73.0, 95.0), 1, Modifiers::SHIFT);
        let highlight = h.app.highlights.get(0).expect("the highlight is kept");
        assert_eq!((highlight.start.char, highlight.end.char), (0, 8));
        assert_eq!(selected_on(&h, 0), Some(0..8));

        // A Shift+click while the selection has collapsed to one caret
        // restores the stored range instead of shrinking the highlight to
        // nothing.
        let caret = noido_core::Spot::At(Caret { page: 0, char: 8 });
        h.app.select.selection = Some(noido_core::Selection {
            anchor: caret,
            focus: caret,
        });
        past_double_click(&mut h);
        click_times(&mut h, (121.0, 95.0), 1, Modifiers::SHIFT);
        let highlight = h.app.highlights.get(0).expect("the highlight is kept");
        assert_eq!((highlight.start.char, highlight.end.char), (0, 8));
        assert_eq!(selected_on(&h, 0), Some(0..8));
    }

    #[test]
    fn highlights_persist_in_doc_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::at(dir.path());
        let mut h = Harness::build(|app| {
            app.with_persistence(Store::at(dir.path()), test_slot(dir.path()), None)
        });
        h.app.highlights.add(Highlight {
            start: Caret { page: 0, char: 0 },
            end: Caret { page: 0, char: 7 },
            colour: HighlightColour::Yellow,
        });
        h.frames(150, vec![]);
        assert_eq!(
            store.load(test_key()).1.unwrap().highlights,
            h.app.highlights.items()
        );
    }

    // ---- Outline ----

    const NO_OUTLINE: &str = "This document has no outline";

    fn entry(title: &str, target: Option<noido_engine::LinkTarget>) -> noido_engine::OutlineEntry {
        noido_engine::OutlineEntry {
            title: title.into(),
            target,
            open: false,
            children: Vec::new(),
        }
    }

    /// An entry going to page `page`, `y` points down.
    fn heading_at(title: &str, page: usize, y: f32) -> noido_engine::OutlineEntry {
        entry(
            title,
            Some(noido_engine::LinkTarget::Internal(
                noido_engine::Destination {
                    page,
                    x: None,
                    y: Some(y),
                },
            )),
        )
    }

    fn heading(title: &str, page: usize) -> noido_engine::OutlineEntry {
        heading_at(title, page, 100.0)
    }

    fn untargeted(title: &str) -> noido_engine::OutlineEntry {
        entry(title, None)
    }

    fn address(title: &str, uri: &str) -> noido_engine::OutlineEntry {
        entry(title, Some(noido_engine::LinkTarget::Web(uri.into())))
    }

    fn branch(
        mut parent: noido_engine::OutlineEntry,
        open: bool,
        children: Vec<noido_engine::OutlineEntry>,
    ) -> noido_engine::OutlineEntry {
        parent.open = open;
        parent.children = children;
        parent
    }

    /// Ten pages, page 5 labelled "61", the rest not. The outline, entries
    /// 0 to 7 in tree order: Preface, Part One (no target, open), Chapter 1
    /// (folded), Section 1.1, Chapter 2, Web, Script, Void (no target).
    fn outline_engine() -> TextEngine {
        let mut labels = [None; 10];
        labels[5] = Some("61");
        TextEngine::new(&[&["x"] as &[&str]; 10])
            .with_labels(&labels)
            .with_outline(vec![
                heading("Preface", 0),
                branch(
                    untargeted("Part One"),
                    true,
                    vec![
                        branch(
                            heading("Chapter 1", 2),
                            false,
                            vec![heading("Section 1.1", 3)],
                        ),
                        heading("Chapter 2", 5),
                    ],
                ),
                address("Web", "https://example.org/"),
                address("Script", "javascript:alert(1)"),
                untargeted("Void"),
            ])
    }

    /// `count` pages, one heading on each.
    fn long_outline(count: usize) -> TextEngine {
        let headings = (0..count)
            .map(|i| heading(&format!("Heading {i}"), i))
            .collect();
        TextEngine::new(&vec![&["x"] as &[&str]; count]).with_outline(headings)
    }

    /// A harness on the outline tab, its document and links thread `engine`.
    fn outline_harness(engine: &TextEngine) -> Harness {
        outline_harness_at(engine, None)
    }

    /// Like [`outline_harness`], the sidebar `width` wide from its first
    /// frame (later changes to the saved width don't reach the panel).
    fn outline_harness_at(engine: &TextEngine, width: Option<f32>) -> Harness {
        let linker = engine.clone();
        Harness::build_with(Arc::new(engine.clone()), move |mut app| {
            app.sidebar.open = true;
            if let Some(width) = width {
                app.sidebar.width = width;
            }
            app.sidebar.choose(SidebarTab::Outline);
            app.with_linker(noido_render::Linker::start(
                Arc::new(linker),
                Arc::new(Vec::new()),
                Box::new(|| {}),
            ))
        })
    }

    /// Like [`outline_harness`], once the outline has come and been drawn.
    fn outline_ready(engine: &TextEngine) -> Harness {
        let mut h = outline_harness(engine);
        h.until(|h| h.app.links.outline.is_some());
        h.frames(3, vec![]);
        h
    }

    fn outline_rows(h: &Harness) -> Vec<crate::sidebar::OutlineRowRects> {
        h.app.sidebar_view.outline_rows().to_vec()
    }

    fn row_titles(h: &Harness) -> Vec<String> {
        outline_rows(h).into_iter().map(|r| r.title).collect()
    }

    fn row_entries(h: &Harness) -> Vec<usize> {
        outline_rows(h).into_iter().map(|r| r.entry).collect()
    }

    fn row_of(h: &Harness, title: &str) -> crate::sidebar::OutlineRowRects {
        outline_rows(h)
            .into_iter()
            .find(|r| r.title == title)
            .unwrap_or_else(|| panic!("no row {title:?}: {:?}", row_titles(h)))
    }

    /// The titles of the rows filled as the current entry's.
    fn marked_rows(h: &Harness) -> Vec<String> {
        let fill = h.ctx.global_style().visuals.selection.bg_fill;
        outline_rows(h)
            .into_iter()
            .filter(|r| {
                h.shapes.iter().any(|c| {
                    matches!(&c.shape, egui::Shape::Rect(s) if s.fill == fill && r.row.contains_rect(s.rect))
                })
            })
            .map(|r| r.title)
            .collect()
    }

    fn click_row(h: &mut Harness, title: &str) {
        let row = row_of(h, title);
        h.click(row.row.center());
        h.frames(2, vec![]);
    }

    /// Moves the pointer onto the row and runs a frame there.
    fn hover_row(h: &mut Harness, title: &str) {
        let row = row_of(h, title);
        h.frames(1, vec![Event::PointerMoved(row.row.center())]);
        h.frames(1, vec![]);
    }

    /// Where `text` was drawn last frame, within `row`'s height.
    fn text_in_row(h: &Harness, text: &str, row: Rect) -> Option<Rect> {
        h.latest_texts()
            .into_iter()
            .find(|(t, r)| t == text && row.y_range().contains(r.center().y))
            .map(|(_, r)| r)
    }

    fn shows_no_outline(h: &Harness) -> bool {
        h.texts().iter().any(|t| t == NO_OUTLINE)
    }

    #[test]
    fn row_shows_title_and_number() {
        let h = outline_ready(&outline_engine());
        assert_eq!(
            row_titles(&h),
            [
                "Preface",
                "Part One",
                "Chapter 1",
                "Chapter 2",
                "Web",
                "Script",
                "Void"
            ]
        );
        let number = |title: &str| row_of(&h, title).number;
        // The short form: a printed number alone, a file number where the
        // page has no label.
        assert_eq!(number("Chapter 2"), "61");
        assert_eq!(number("Preface"), "(1)");
        // Web and no-target rows have none.
        assert_eq!(number("Web"), "");
        assert_eq!(number("Void"), "");
        assert_eq!(number("Part One"), "");
        assert!(h.texts().contains(&"61".to_owned()), "{:?}", h.texts());
        assert!(h.texts().contains(&"Chapter 2".to_owned()));
        // Deeper rows are indented, and the number is the weak colour.
        let (preface, chapter) = (row_of(&h, "Preface"), row_of(&h, "Chapter 2"));
        let left = |title: &str, row: &crate::sidebar::OutlineRowRects| {
            text_in_row(&h, title, row.row).unwrap().min.x
        };
        assert!(left("Chapter 2", &chapter) > left("Preface", &preface) + 5.0);
        assert_ne!(h.text_color("61"), h.text_color("Chapter 2"));
    }

    #[test]
    fn click_jumps_flashes_and_adds_history() {
        let mut h = outline_ready(&outline_engine());
        hover_row(&mut h, "Chapter 2");
        assert_eq!(h.cursor, egui::CursorIcon::PointingHand);
        click_row(&mut h, "Chapter 2");
        assert_eq!(h.app.chosen, Some(5));
        assert_eq!(h.app.clicked_entry, Some(4));
        assert_eq!(h.centre().page, 5);
        assert_eq!(h.app.history.back_entries().len(), 1);
        h.until(|h| !flash_outlines(h).is_empty());
        assert!(!flash_outlines(&h).is_empty());
        assert_eq!(marked_rows(&h), ["Chapter 2"]);
        h.press(egui::Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(h.centre().page, 0);
    }

    #[test]
    fn click_on_chevron_folds_without_jumping() {
        let mut h = outline_ready(&outline_engine());
        let row = row_of(&h, "Chapter 1");
        let chevron = row.chevron.expect("a branch has a chevron");
        assert!(chevron.width() >= row.row.height(), "{chevron:?}");
        assert!(row_of(&h, "Chapter 2").chevron.is_none());
        assert!(row_of(&h, "Preface").chevron.is_none());
        h.click(chevron.center());
        h.frames(2, vec![]);
        assert!(row_titles(&h).contains(&"Section 1.1".to_owned()));
        assert_eq!(h.app.chosen, None);
        assert_eq!(h.app.clicked_entry, None);
        assert!(h.app.history.back_entries().is_empty());
        assert_eq!(h.centre().page, 0);
        let chevron = row_of(&h, "Chapter 1").chevron.unwrap();
        h.click(chevron.center());
        h.frames(2, vec![]);
        assert!(!row_titles(&h).contains(&"Section 1.1".to_owned()));
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn click_on_targetless_parent_toggles() {
        let mut h = outline_ready(&outline_engine());
        hover_row(&mut h, "Part One");
        assert_eq!(h.cursor, egui::CursorIcon::PointingHand);
        click_row(&mut h, "Part One");
        assert_eq!(
            row_titles(&h),
            ["Preface", "Part One", "Web", "Script", "Void"]
        );
        assert_eq!(h.app.chosen, None);
        assert!(h.app.history.back_entries().is_empty());
        click_row(&mut h, "Part One");
        assert!(row_titles(&h).contains(&"Chapter 2".to_owned()));
    }

    #[test]
    fn click_on_targetless_leaf_does_nothing() {
        let mut h = outline_ready(&outline_engine());
        let before = row_titles(&h);
        hover_row(&mut h, "Void");
        assert_ne!(h.cursor, egui::CursorIcon::PointingHand);
        click_row(&mut h, "Void");
        assert_eq!(row_titles(&h), before);
        assert_eq!(h.app.chosen, None);
        assert_eq!(h.app.clicked_entry, None);
        assert!(h.app.history.back_entries().is_empty());
        assert_eq!(h.centre().page, 0);
    }

    #[test]
    fn drag_scrolls_without_jumping() {
        let mut h = outline_ready(&long_outline(100));
        let first = outline_rows(&h)[0].row.center();
        assert_eq!(h.app.sidebar_view.outline_offset(), 0.0);
        h.drag(first + vec2(0.0, 300.0), vec2(0.0, -300.0));
        h.frames(30, vec![]);
        assert!(h.app.sidebar_view.outline_offset() > 100.0);
        assert_eq!(h.app.chosen, None);
        assert_eq!(h.app.clicked_entry, None);
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn web_entry_opens_only_allowed_schemes() {
        let mut h = outline_ready(&outline_engine());
        click_row(&mut h, "Web");
        assert_eq!(h.opened, ["https://example.org/".to_owned()]);
        click_row(&mut h, "Script");
        assert_eq!(h.opened.len(), 1, "{:?}", h.opened);
        assert_eq!(h.app.chosen, None);
        assert!(h.app.history.back_entries().is_empty());
    }

    /// A reader with branches of each kind, folded and open: with and
    /// without targets, one a web address.
    fn double_click_engine() -> TextEngine {
        TextEngine::new(&[&["x"] as &[&str]; 10]).with_outline(vec![
            heading("Top", 0),
            branch(heading("Chap", 3), false, vec![heading("Kid", 4)]),
            branch(heading("Open", 6), true, vec![heading("OKid", 7)]),
            branch(untargeted("Folder"), false, vec![heading("FKid", 8)]),
            branch(untargeted("OpenFolder"), true, vec![heading("OFKid", 9)]),
            branch(
                address("WebB", "https://example.org/"),
                false,
                vec![heading("WKid", 9)],
            ),
            heading("Leaf", 8),
        ])
    }

    /// Two presses in quick succession at `at`.
    fn double_click(h: &mut Harness, at: egui::Pos2) {
        h.click(at);
        h.click(at);
        h.frames(2, vec![]);
    }

    fn double_click_row(h: &mut Harness, title: &str) {
        let at = row_of(h, title).row.center();
        double_click(h, at);
    }

    #[test]
    fn double_click_on_a_folded_branch_with_a_target_jumps_once_and_unfolds() {
        let mut h = outline_ready(&double_click_engine());
        let row = row_of(&h, "Chap").row;
        let offset = h.app.sidebar_view.outline_offset();
        double_click(&mut h, row.center());
        assert_eq!(h.app.history.back_entries().len(), 1);
        assert_eq!(h.app.chosen, Some(3));
        assert!(row_titles(&h).contains(&"Kid".to_owned()));
        assert_eq!(row_of(&h, "Chap").row, row);
        assert_eq!(h.app.sidebar_view.outline_offset(), offset);
    }

    #[test]
    fn double_click_on_an_open_branch_with_a_target_keeps_it_open() {
        let mut h = outline_ready(&double_click_engine());
        double_click_row(&mut h, "Open");
        assert_eq!(h.app.history.back_entries().len(), 1);
        assert!(row_titles(&h).contains(&"OKid".to_owned()));
    }

    #[test]
    fn double_click_on_a_branch_without_a_target_ends_unfolded() {
        let mut h = outline_ready(&double_click_engine());
        double_click_row(&mut h, "Folder");
        assert!(row_titles(&h).contains(&"FKid".to_owned()));
        double_click_row(&mut h, "OpenFolder");
        assert!(row_titles(&h).contains(&"OFKid".to_owned()));
        assert!(h.app.history.back_entries().is_empty());
    }

    #[test]
    fn double_click_on_a_leaf_jumps_once() {
        let mut h = outline_ready(&double_click_engine());
        double_click_row(&mut h, "Leaf");
        assert_eq!(h.app.history.back_entries().len(), 1);
        assert_eq!(h.app.chosen, Some(8));
    }

    #[test]
    fn double_click_on_the_chevron_toggles_twice() {
        let mut h = outline_ready(&double_click_engine());
        let at = row_of(&h, "Chap").chevron.unwrap().center();
        double_click(&mut h, at);
        assert!(!row_titles(&h).contains(&"Kid".to_owned()));
        assert!(h.app.history.back_entries().is_empty());
        let at = row_of(&h, "Open").chevron.unwrap().center();
        double_click(&mut h, at);
        assert!(row_titles(&h).contains(&"OKid".to_owned()));
    }

    #[test]
    fn double_click_on_a_web_branch_opens_once_and_unfolds() {
        let mut h = outline_ready(&double_click_engine());
        double_click_row(&mut h, "WebB");
        assert_eq!(h.opened, ["https://example.org/".to_owned()]);
        assert!(row_titles(&h).contains(&"WKid".to_owned()));
    }

    #[test]
    fn mark_follows_the_pages_without_unfolding() {
        let mut h = outline_ready(&outline_engine());
        let before = row_titles(&h);
        go_to(&mut h, 4);
        h.frames(2, vec![]);
        // Section 1.1 is inside the folded Chapter 1: it stands for it.
        assert_eq!(marked_rows(&h), ["Chapter 1"]);
        assert_eq!(row_titles(&h), before);
        go_to(&mut h, 6);
        h.frames(2, vec![]);
        assert_eq!(marked_rows(&h), ["Chapter 2"]);
        let selection = h.ctx.global_style().visuals.selection.stroke.color;
        assert_eq!(h.text_color("Chapter 2"), Some(selection));
        assert_eq!(h.text_color("61"), Some(selection));
        assert_eq!(row_titles(&h), before);
    }

    #[test]
    fn mark_lands_on_visible_ancestor() {
        let mut h = outline_ready(&outline_engine());
        let chevron = row_of(&h, "Chapter 1").chevron.unwrap();
        h.click(chevron.center());
        go_to(&mut h, 4);
        h.frames(3, vec![]);
        assert_eq!(marked_rows(&h), ["Section 1.1"]);
        let chevron = row_of(&h, "Chapter 1").chevron.unwrap();
        h.click(chevron.center());
        h.frames(2, vec![]);
        assert_eq!(marked_rows(&h), ["Chapter 1"]);
    }

    #[test]
    fn tree_scrolls_to_the_marked_row() {
        let mut h = outline_ready(&long_outline(80));
        assert!(!row_entries(&h).contains(&70));
        go_to(&mut h, 70);
        h.frames(4, vec![]);
        let current = h.app.current_entry().expect("an entry is current");
        assert!(current >= 69, "{current}");
        assert!(row_entries(&h).contains(&current), "{:?}", row_entries(&h));
        assert!(!marked_rows(&h).is_empty());
    }

    #[test]
    fn folding_does_not_scroll() {
        let headings = (0..100)
            .map(|i| match i {
                60 => branch(
                    heading("Heading 60", 60),
                    true,
                    vec![heading_at("60.1", 60, 300.0), heading_at("60.2", 60, 500.0)],
                ),
                i => heading(&format!("Heading {i}"), i),
            })
            .collect();
        let engine = TextEngine::new(&[&["x"] as &[&str]; 100]).with_outline(headings);
        let mut h = outline_ready(&engine);
        go_to(&mut h, 62);
        h.frames(4, vec![]);
        // The reader scrolls the outline until the current entry is just
        // out of sight below, the branch still in view.
        let rows = outline_rows(&h);
        let height = rows[1].row.min.y - rows[0].row.min.y;
        let current = h.app.current_entry().unwrap();
        h.frames(1, vec![Event::PointerMoved(rows[5].row.center())]);
        for _ in 0..40 {
            if !row_entries(&h).contains(&current) {
                break;
            }
            h.frames(
                1,
                vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, height),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                }],
            );
            h.frames(30, vec![]);
        }
        assert!(!row_entries(&h).contains(&current));
        assert!(row_of(&h, "Heading 60").chevron.is_some());
        let offset = h.app.sidebar_view.outline_offset();
        assert!(offset > 100.0, "{offset}");
        let chevron = row_of(&h, "Heading 60").chevron.unwrap();
        h.click(chevron.center());
        h.frames(4, vec![]);
        assert!(!row_titles(&h).contains(&"60.1".to_owned()));
        assert_eq!(h.app.sidebar_view.outline_offset(), offset);
    }

    #[test]
    fn reader_scrolling_stands_until_the_entry_changes() {
        let mut h = outline_ready(&long_outline(100));
        let at = outline_rows(&h)[0].row.center() + vec2(0.0, 300.0);
        h.drag(at, vec2(0.0, -300.0));
        h.frames(120, vec![]);
        let offset = h.app.sidebar_view.outline_offset();
        assert!(offset > 100.0, "{offset}");
        h.frames(30, vec![]);
        assert_eq!(h.app.sidebar_view.outline_offset(), offset);
        // The entry (the first) is out of sight and stays so.
        assert!(!row_entries(&h).contains(&0));
        go_to(&mut h, 90);
        h.frames(4, vec![]);
        let current = h.app.current_entry().unwrap();
        assert!(row_entries(&h).contains(&current), "{current}");
    }

    #[test]
    fn clicked_entry_stays_marked_near_the_end() {
        let engine = TextEngine::new(&[&["x"] as &[&str]; 10]).with_outline(vec![
            heading("Intro", 0),
            heading("Last", 9),
            heading_at("Back", 9, 700.0),
        ]);
        let mut h = outline_ready(&engine);
        click_row(&mut h, "Back");
        h.frames(3, vec![]);
        // The jump stops short of the target at the end of the document.
        assert_eq!(marked_rows(&h), ["Back"]);
    }

    #[test]
    fn clicked_entry_mark_ends_when_the_reader_moves() {
        let mut h = outline_ready(&outline_engine());
        click_row(&mut h, "Chapter 1");
        assert_eq!(marked_rows(&h), ["Chapter 1"]);
        assert_eq!(h.app.clicked_entry, Some(2));
        go_to(&mut h, 6);
        h.frames(2, vec![]);
        assert_eq!(h.app.clicked_entry, None);
        assert_eq!(marked_rows(&h), ["Chapter 2"]);
        // Another way of choosing a page ends it too. A pause first: two
        // clicks on a row in quick succession are a double click.
        h.frames(30, vec![]);
        click_row(&mut h, "Chapter 1");
        assert_eq!(marked_rows(&h), ["Chapter 1"]);
        h.press(egui::Key::Home, Modifiers::NONE);
        h.frames(2, vec![]);
        assert_eq!(h.app.clicked_entry, None);
        assert_eq!(marked_rows(&h), ["Preface"]);
    }

    #[test]
    fn dual_mode_marks_a_right_hand_chapter() {
        let headings = (0..10)
            .map(|i| heading(&format!("P{}", i + 1), i))
            .collect();
        let engine = TextEngine::new(&[&["x"] as &[&str]; 10]).with_outline(headings);
        let mut h = outline_ready(&engine);
        h.press(egui::Key::D, Modifiers::NONE);
        assert!(h.app.view.dual());
        let mut right_hand = 0;
        for spread in 0..h.app.view.layout().spread_count() {
            let pages = h.app.view.layout().spread_pages(spread);
            if pages.len() != 2 {
                continue;
            }
            h.app.view.go_to_page(pages.start);
            h.frames(4, vec![]);
            let point = h.app.view.reading_point(None).unwrap();
            right_hand += usize::from(point.page == pages.end - 1);
            assert_eq!(
                marked_rows(&h),
                [format!("P{}", point.page + 1)],
                "spread {pages:?}"
            );
        }
        assert!(right_hand > 0, "no spread read on its right-hand page");
    }

    #[test]
    fn empty_state_after_the_reply_not_before() {
        let engine = TextEngine::new(&[&["x"] as &[&str]; 3]).gated_links();
        let mut h = outline_harness(&engine);
        h.frames(4, vec![]);
        assert!(h.app.links.outline.is_none());
        assert!(!shows_no_outline(&h), "{:?}", h.texts());
        engine.open_link_gate();
        h.until(shows_no_outline);
        assert!(shows_no_outline(&h), "{:?}", h.texts());
    }

    /// The empty-state text wraps in a narrow sidebar rather than widening it.
    #[test]
    fn empty_outline_text_keeps_a_narrow_sidebar_narrow() {
        let bare = TextEngine::new(&[&["x"] as &[&str]; 3]);
        let mut h = outline_harness_at(&bare, Some(noido_core::SIDEBAR_MIN_WIDTH));
        h.until(shows_no_outline);
        h.frames(3, vec![]);
        assert!(shows_no_outline(&h), "{:?}", h.texts());
        assert_eq!(h.app.sidebar.width, 160.0);
    }

    #[test]
    fn no_search_text_keeps_a_narrow_sidebar_narrow() {
        let mut h = Harness::build_with(Arc::new(BlankEngine), |mut app| {
            app.sidebar.open = true;
            app.sidebar.choose(SidebarTab::Results);
            app.sidebar.width = noido_core::SIDEBAR_MIN_WIDTH;
            app
        });
        h.frames(3, vec![]);
        assert!(h.texts().iter().any(|t| t == "No search"));
        assert_eq!(h.app.sidebar.width, 160.0);
    }

    #[test]
    fn saved_outline_tab_with_no_outline_shows_the_empty_state() {
        let mut h = outline_ready(&outline_engine());
        assert_eq!(h.app.sidebar.last, SidebarTab::Outline);
        let bare = TextEngine::new(&[&["x"] as &[&str]; 10]).gated_links();
        h.reload(Arc::new(bare.clone()), recompiled_key());
        h.frames(2, vec![]);
        // The old tree stands until the reply.
        assert!(row_titles(&h).contains(&"Preface".to_owned()));
        bare.open_link_gate();
        h.until(shows_no_outline);
        assert!(shows_no_outline(&h), "{:?}", h.texts());
        assert!(outline_rows(&h).is_empty());
        assert_eq!(h.app.sidebar.tab, SidebarTab::Outline);
    }

    #[test]
    fn reload_same_outline_keeps_folding() {
        let mut h = outline_ready(&outline_engine());
        click_row(&mut h, "Part One");
        let folded = row_titles(&h);
        assert!(!folded.contains(&"Chapter 2".to_owned()));
        let same = outline_engine().gated_links();
        h.reload(Arc::new(same.clone()), recompiled_key());
        same.open_link_gate();
        h.until(|h| h.app.links.outline_replies() > 0);
        h.frames(3, vec![]);
        assert_eq!(row_titles(&h), folded);
    }

    /// The outline's rows and page numbers end left of the scroll bar when
    /// the list overflows, solid or floating; a list that fits gets the
    /// whole width.
    #[test]
    fn outline_rows_leave_the_scroll_bar_clear() {
        for (solid, bar) in [(false, 12.0), (true, 17.0)] {
            let make = |engine: &TextEngine| {
                let mut h = outline_harness(engine);
                if solid {
                    h = h.solid_bars();
                }
                h.until(|h| h.app.links.outline.is_some());
                h.frames(4, vec![]);
                h
            };
            let h = make(&long_outline(200));
            // Inside the panel's 1-point border on its right.
            let bar_left = h.app.sidebar.width - 1.0 - bar;
            let rows = outline_rows(&h);
            assert!(rows.len() > 5, "{}", rows.len());
            for row in &rows {
                assert!(
                    row.row.max.x <= bar_left + 0.5,
                    "solid {solid}: row {} ends at {}, the bar starts at {bar_left}",
                    row.title,
                    row.row.max.x
                );
                let number = text_in_row(&h, &row.number, row.row)
                    .unwrap_or_else(|| panic!("no number {:?}", row.number));
                assert!(number.max.x <= bar_left + 0.5, "solid {solid}: {number:?}");
            }
            let short = make(&long_outline(3));
            let rows = outline_rows(&short);
            assert_eq!(rows.len(), 3);
            for row in &rows {
                assert!(
                    row.row.max.x >= short.app.sidebar.width - 1.0 - 0.5,
                    "solid {solid}: a list that fits ends at {}",
                    row.row.max.x
                );
            }
        }
    }

    /// Folding starts over with a changed tree, so rows move under an
    /// entry index that stays: the sidebar follows the mark again.
    #[test]
    fn reload_changed_outline_follows_the_mark_again() {
        let tree = |changed: &str| {
            let kids = (0..200)
                .map(|i| {
                    let title = if i == 50 {
                        changed.to_owned()
                    } else {
                        format!("Kid {i}")
                    };
                    heading(&title, 1)
                })
                .collect();
            TextEngine::new(&[&["x"] as &[&str]; 10]).with_outline(vec![
                branch(untargeted("Big"), true, kids),
                heading("After", 5),
            ])
        };
        let mut h = outline_ready(&tree("Kid 50"));
        h.click(row_of(&h, "Big").chevron.unwrap().center());
        h.frames(2, vec![]);
        go_to(&mut h, 5);
        h.frames(3, vec![]);
        assert_eq!(marked_rows(&h), ["After"]);
        let changed = tree("Kid fifty").gated_links();
        h.reload(Arc::new(changed.clone()), recompiled_key());
        changed.open_link_gate();
        h.until(|h| h.app.links.outline_replies() > 0);
        h.frames(4, vec![]);
        assert!(
            h.app.sidebar_view.outline_offset() > 0.0,
            "scrolled to the mark"
        );
        assert_eq!(marked_rows(&h), ["After"], "{:?}", row_titles(&h));
    }

    #[test]
    fn reload_shorter_outline_drops_stale_mark() {
        let mut h = outline_ready(&outline_engine());
        let short = TextEngine::new(&[&["x"] as &[&str]; 3])
            .with_outline(vec![heading("A", 0), heading("B", 1)])
            .gated_links();
        h.reload(Arc::new(short.clone()), recompiled_key());
        h.frames(2, vec![]);
        // The old tree is still shown, and the reader clicks in it.
        click_row(&mut h, "Chapter 2");
        assert_eq!(h.app.clicked_entry, Some(4));
        short.open_link_gate();
        h.until(|h| h.app.links.outline_replies() > 0);
        h.frames(3, vec![]);
        assert_eq!(row_titles(&h), ["A", "B"]);
        assert_eq!(h.app.clicked_entry, None);
        assert!(h.app.chosen <= Some(2), "{:?}", h.app.chosen);
        assert!(h.app.current_entry().is_none_or(|entry| entry < 2));
        assert!(marked_rows(&h).len() <= 1);
    }

    #[test]
    fn target_past_the_end_goes_to_the_last_page() {
        let mut labels = [None; 10];
        labels[9] = Some("end");
        let engine = TextEngine::new(&[&["x"] as &[&str]; 10])
            .with_labels(&labels)
            .with_outline(vec![heading("Intro", 0), heading("Beyond", 99)]);
        let mut h = outline_ready(&engine);
        assert_eq!(row_of(&h, "Beyond").number, "end");
        click_row(&mut h, "Beyond");
        assert_eq!(h.app.chosen, Some(9));
        assert_eq!(h.centre().page, 9);
        assert_eq!(marked_rows(&h), ["Beyond"]);
    }

    #[test]
    fn narrow_sidebar_keeps_chevron_and_number() {
        const LONG: &str = "with a title far too long to fit into any outline row";
        let mut node = heading(&format!("Depth 64 {LONG}"), 9);
        for depth in (0..64).rev() {
            node = branch(
                heading(&format!("Depth {depth} {LONG}"), 9),
                true,
                vec![node],
            );
        }
        let mut labels = [None; 10];
        labels[9] = Some("A label far too long for a row");
        let engine = TextEngine::new(&[&["x"] as &[&str]; 10])
            .with_labels(&labels)
            .with_outline(vec![node]);
        let mut h = outline_harness_at(&engine, Some(noido_core::SIDEBAR_MIN_WIDTH));
        h.size = vec2(900.0, 2400.0);
        h.until(|h| h.app.links.outline.is_some() && numbers_known(h));
        h.frames(4, vec![]);
        let rows = outline_rows(&h);
        assert!(rows.len() > 60, "{}", rows.len());
        assert_eq!(h.app.sidebar.width, 160.0);
        assert!(rows[0].row.width() <= 160.0, "{:?}", rows[0].row);
        assert!(rows.last().unwrap().row.width() <= 160.0);
        for row in &rows {
            let inside = |rect: Rect| row.row.contains_rect(rect.expand(-0.01));
            let number = text_in_row(&h, &row.number, row.row)
                .unwrap_or_else(|| panic!("no number {:?}", row.number));
            assert!(inside(number), "{number:?} in {:?}", row.row);
            assert!(number.width() <= row.row.width() / 3.0 + 1.0, "{number:?}");
            let title = text_in_row(&h, &row.title, row.row)
                .unwrap_or_else(|| panic!("no title {:?}", row.title));
            assert!(inside(title), "{title:?} in {:?}", row.row);
            assert!(title.max.x <= number.min.x + 0.5, "{title:?} {number:?}");
            if let Some(chevron) = row.chevron {
                assert!(inside(chevron), "{chevron:?} in {:?}", row.row);
                assert!(chevron.max.x <= title.min.x + 0.5, "{chevron:?} {title:?}");
            }
        }
        let deepest = rows.last().unwrap();
        assert!(deepest.title.starts_with("Depth 64"), "{}", deepest.title);
    }

    #[test]
    fn hundred_thousand_rows_draw_only_the_visible() {
        let headings = (0..100_000)
            .map(|i| heading(&format!("Heading {i}"), i % 10))
            .collect();
        let engine = TextEngine::new(&[&["x"] as &[&str]; 10]).with_outline(headings);
        let h = outline_ready(&engine);
        let drawn = outline_rows(&h).len();
        assert!(drawn > 0 && drawn < 100, "{drawn}");
        assert_eq!(h.app.links.outline.as_ref().unwrap().rows().len(), 100_000);
    }
}
