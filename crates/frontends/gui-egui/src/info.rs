//! The document info page: the file, what the document says about itself,
//! its format and its fonts.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui;
use noido_core::{Family, families};
use noido_engine::{Date, PdfBytes, PdfEngine, Properties, Size};
use noido_render::read_properties;

use crate::icons;

/// The settings row's copy button, by the id a test clicks.
pub const COPY_BUTTON: &str = "info-settings-copy";
/// The settings row's folder button, by the id a test clicks.
pub const FOLDER_BUTTON: &str = "info-settings-folder";

/// What the reader asked of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Close the page.
    Close,
    /// Put the settings file's path on the clipboard.
    CopyPath,
    /// Show the settings file's folder.
    ShowFolder,
}

/// The open document's file, to read its properties from.
pub struct Source {
    pub engine: Arc<dyn PdfEngine>,
    pub bytes: PdfBytes,
    /// As the reader gave it: neither absolute nor tidied; the page tidies
    /// it when it shows it.
    pub path: PathBuf,
}

/// The open info page.
pub struct Page {
    reply: Receiver<Properties>,
    /// The file's path as the page shows it, worked out when the page opens
    /// so that what it says does not depend on the folder noido is in by the
    /// time it is drawn.
    shown: PathBuf,
    /// Once read: the properties, and their fonts as families.
    read: Option<(Properties, Vec<Family>)>,
}

impl Page {
    /// Opens the page and starts reading `source`'s properties.
    pub fn open(source: &Source, ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        let reply = read_properties(
            source.engine.clone(),
            source.bytes.clone(),
            Box::new(move || ctx.request_repaint()),
        );
        Self {
            reply,
            shown: shown_path(&source.path),
            read: None,
        }
    }

    fn receive(&mut self) {
        if self.read.is_none()
            && let Ok(properties) = self.reply.try_recv()
        {
            let families = families(&properties.fonts);
            self.read = Some((properties, families));
        }
    }
}

/// Shows the page over the window; returns what the reader asked of it.
/// `sizes` are the pages' sizes, and `settings` the file the document's
/// settings are saved in, shown when there is one.
pub fn show(
    page: &mut Page,
    ctx: &egui::Context,
    source: &Source,
    sizes: &[Size],
    settings: Option<&Path>,
) -> Option<Asked> {
    page.receive();
    let mut asked = None;
    let max_height = ctx.content_rect().height() * 0.7;
    // The frame has no right margin, so the scroll bar runs along the
    // page's edge; everything else keeps the margin itself.
    let theme = crate::theme::Theme::current(ctx);
    let frame = theme.frame(
        ctx.theme(),
        crate::theme::Surface::Dialog,
        &ctx.global_style(),
    );
    let margin = frame.inner_margin.right;
    let frame = frame.inner_margin(egui::Margin {
        right: 0,
        ..frame.inner_margin
    });
    let response = egui::Modal::new(egui::Id::unique("document-info"))
        .frame(frame)
        .show(ctx, |ui| {
            crate::theme::scope(ui, crate::theme::Role::Dialog, |ui| {
                ui.set_width(WIDTH + f32::from(margin));
                right(ui, margin, |ui| {
                    ui.heading("Document info");
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        asked = right(ui, margin, |ui| contents(ui, page, source, sizes, settings));
                    });
                ui.add_space(8.0);
                right(ui, margin, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            asked = Some(Asked::Close);
                        }
                    });
                });
            })
        });
    if response.should_close() {
        asked = Some(Asked::Close);
    }
    asked
}

/// `add`'s part of the page, with the margin the frame leaves it.
fn right<R>(ui: &mut egui::Ui, margin: i8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            right: margin,
            ..egui::Margin::ZERO
        })
        .show(ui, add)
        .inner
}

fn contents(
    ui: &mut egui::Ui,
    page: &Page,
    source: &Source,
    sizes: &[Size],
    settings: Option<&Path>,
) -> Option<Asked> {
    let read = page.read.as_ref();
    let properties = read.map(|(p, _)| p);
    let mut asked = None;
    section(ui, "File", |ui| {
        let name = page
            .shown
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let folder = page.shown.parent().map(|p| p.display().to_string());
        row(ui, "Name", name);
        row(ui, "Folder", folder.filter(|f| !f.is_empty()));
        row(ui, "Size", Some(file_size_text(source.bytes.len() as u64)));
        if let Some(path) = settings {
            asked = settings_row(ui, path);
        }
    });
    section(ui, "Document", |ui| {
        let Some(p) = properties else {
            row(ui, "", Some("Reading…".into()));
            return;
        };
        row(ui, "Title", p.title.clone());
        row(ui, "Author", p.author.clone());
        row(ui, "Subject", p.subject.clone());
        row(ui, "Keywords", p.keywords.clone());
        row(ui, "Creator", p.creator.clone());
        row(ui, "Producer", p.producer.clone());
        row(ui, "Created", p.created.map(date_text));
        row(ui, "Modified", p.modified.map(date_text));
    });
    section(ui, "Format", |ui| {
        row(ui, "Pages", Some(sizes.len().to_string()));
        row(
            ui,
            "Page size",
            Some(page_size_text(sizes)).filter(|s| !s.is_empty()),
        );
        let yes = |on: bool| on.then(|| "Yes".to_owned());
        row(
            ui,
            "PDF version",
            properties.and_then(|p| p.version.clone()),
        );
        row(ui, "Tagged", properties.and_then(|p| yes(p.tagged)));
        row(ui, "Outline", properties.and_then(|p| yes(p.has_outline)));
    });
    section(ui, "Fonts", |ui| match read {
        None => row(ui, "", Some("Reading…".into())),
        Some((_, families)) if families.is_empty() => row(ui, "", Some("None".into())),
        Some((_, families)) => {
            for family in families {
                let styles: Vec<String> = family
                    .styles
                    .iter()
                    .map(|s| {
                        if s.embedded {
                            s.name.clone()
                        } else {
                            format!("{} (not embedded)", s.name)
                        }
                    })
                    .collect();
                let raw: Vec<&str> = family
                    .styles
                    .iter()
                    .flat_map(|s| s.raw.iter().map(String::as_str))
                    .collect();
                crate::theme::hover_text(
                    ui.add(egui::Label::new(egui::RichText::new(&family.name).strong())),
                    raw.join("\n"),
                );
                ui.indent(&family.name, |ui| {
                    ui.add(egui::Label::new(styles.join(", ")).wrap());
                });
            }
        }
    });
    asked
}

/// The path the page shows: made absolute where it can be, with `.` and
/// `..` folded away, and symlinks left as they were written. A `..` is a way
/// of getting somewhere, not part of a folder's name. The folding is
/// lexical, as Windows' `GetFullPathNameW` is, so a `..` after a symlink
/// names the folder that was written, not the one the link resolves to.
/// A path that cannot be made absolute is shown as it was given.
fn shown_path(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut kept: Vec<Component> = Vec::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                // A name goes back over what `..` follows.
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                // Above the root there is nothing to go back over.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => kept.push(component),
            },
            _ => kept.push(component),
        }
    }
    let mut shown = PathBuf::new();
    for component in kept {
        shown.push(component.as_os_str());
    }
    shown
}

/// The width of the page's text.
pub(crate) const WIDTH: f32 = 440.0;

/// The width of the keys' column. Fixed, not an `egui::Grid`: a grid lays
/// out with last frame's column widths, so when the rows arrive the page
/// is too wide for a frame and jumps sideways as it re-centres.
const KEY_WIDTH: f32 = 96.0;

/// A heading and rows under it.
fn section<R>(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong());
    ui.add_space(2.0);
    ui.spacing_mut().item_spacing.y = 4.0;
    rows(ui)
}

/// The row's key: a narrow column, so the values line up.
fn key_column(ui: &mut egui::Ui, key: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(KEY_WIDTH, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(KEY_WIDTH);
            ui.add(egui::Label::new(egui::RichText::new(key).weak()).wrap());
        },
    );
}

/// A row of a section, left out without a value.
fn row(ui: &mut egui::Ui, key: &str, value: Option<String>) {
    let Some(value) = value else {
        return;
    };
    ui.horizontal_top(|ui| {
        key_column(ui, key);
        ui.add(egui::Label::new(value).wrap());
    });
}

/// The width of a button beside the settings path.
const BUTTON: f32 = 24.0;

/// The settings file's path, with a button that copies it and one that
/// shows its folder; returns what was clicked.
fn settings_row(ui: &mut egui::Ui, path: &Path) -> Option<Asked> {
    let mut asked = None;
    ui.horizontal_top(|ui| {
        key_column(ui, "Settings");
        // The buttons take their room first, with the gaps either side of
        // them, so the path wraps beside them and the row ends where the
        // page's other rows do.
        let gap = ui.spacing().item_spacing.x;
        let room = 2.0 * BUTTON + 2.0 * gap;
        let width = (ui.available_width() - room).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.add(egui::Label::new(path.display().to_string()).wrap());
            },
        );
        let copy = icons::button(
            ui,
            egui::Id::unique(COPY_BUTTON),
            BUTTON,
            false,
            icons::paint_copy_icon,
        );
        if crate::theme::hover_text(copy, "Copy path").clicked() {
            asked = Some(Asked::CopyPath);
        }
        let folder = icons::button(
            ui,
            egui::Id::unique(FOLDER_BUTTON),
            BUTTON,
            false,
            icons::paint_folder_icon,
        );
        if crate::theme::hover_text(folder, "Open folder").clicked() {
            asked = Some(Asked::ShowFolder);
        }
    });
    asked
}

/// "216 × 279 mm (612 × 792 pt)"; "mixed" when the pages differ by a
/// point or more.
fn page_size_text(sizes: &[Size]) -> String {
    let Some(first) = sizes.first() else {
        return String::new();
    };
    let same = |a: f32, b: f32| (a - b).abs() < 1.0;
    if !sizes
        .iter()
        .all(|s| same(s.width, first.width) && same(s.height, first.height))
    {
        return "mixed".into();
    }
    let mm = |points: f32| (points / 72.0 * 25.4).round();
    format!(
        "{} × {} mm ({} × {} pt)",
        mm(first.width),
        mm(first.height),
        first.width.round(),
        first.height.round()
    )
}

/// "2024-03-05 14:22 +01:00": as the document wrote it.
fn date_text(date: Date) -> String {
    let Date {
        year,
        month,
        day,
        hour,
        minute,
        ..
    } = date;
    let when = format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}");
    match date.utc_offset {
        None => when,
        Some(0) => format!("{when} UTC"),
        Some(offset) => {
            let sign = if offset < 0 { '−' } else { '+' };
            let offset = offset.unsigned_abs();
            format!("{when} {sign}{:02}:{:02}", offset / 60, offset % 60)
        }
    }
}

/// "2.4 MB", "830 kB", "512 bytes".
fn file_size_text(bytes: u64) -> String {
    let scaled = |unit: f64, name: &str| {
        let value = bytes as f64 / unit;
        if value < 10.0 {
            format!("{value:.1} {name}")
        } else {
            format!("{value:.0} {name}")
        }
    };
    match bytes {
        0..1_000 => format!("{bytes} bytes"),
        1_000..1_000_000 => scaled(1e3, "kB"),
        _ => scaled(1e6, "MB"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    #[test]
    fn page_size_in_mm_and_points() {
        assert_eq!(
            page_size_text(&[size(612.0, 792.0), size(612.0, 792.0)]),
            "216 × 279 mm (612 × 792 pt)"
        );
        assert_eq!(
            page_size_text(&[size(595.276, 841.89)]),
            "210 × 297 mm (595 × 842 pt)"
        );
    }

    #[test]
    fn pages_of_different_sizes_are_mixed() {
        assert_eq!(
            page_size_text(&[size(612.0, 792.0), size(595.0, 842.0)]),
            "mixed"
        );
        assert_eq!(
            page_size_text(&[size(612.0, 792.0), size(612.4, 792.0)]),
            "216 × 279 mm (612 × 792 pt)"
        );
    }

    fn date(utc_offset: Option<i16>) -> Date {
        Date {
            year: 2024,
            month: 3,
            day: 5,
            hour: 14,
            minute: 22,
            second: 1,
            utc_offset,
        }
    }

    #[test]
    fn dates_show_their_offset() {
        assert_eq!(date_text(date(Some(60))), "2024-03-05 14:22 +01:00");
        assert_eq!(date_text(date(Some(-270))), "2024-03-05 14:22 −04:30");
        assert_eq!(date_text(date(Some(0))), "2024-03-05 14:22 UTC");
        assert_eq!(date_text(date(None)), "2024-03-05 14:22");
    }

    fn shown(path: &str) -> PathBuf {
        shown_path(Path::new(path))
    }

    #[test]
    fn the_folder_is_shown_without_the_way_there() {
        let cwd = std::env::current_dir().unwrap();
        // `noido ../papers/paper.pdf`: the folder, not the way to it. The
        // `..` here goes up from the crate noido is opened in.
        assert_eq!(
            shown("../papers/paper.pdf"),
            cwd.parent().unwrap().join("papers/paper.pdf")
        );
        assert_eq!(
            shown("/docs/scans/../papers/paper.pdf"),
            Path::new("/docs/papers/paper.pdf")
        );
    }

    #[test]
    fn a_parent_dir_goes_back_over_the_folder_it_follows() {
        let cwd = std::env::current_dir().unwrap();
        // The first `..` takes back `a`, the second the folder noido is in.
        assert_eq!(shown("a/../../b.pdf"), cwd.parent().unwrap().join("b.pdf"));
        // Above the root there is nothing to take back.
        assert_eq!(shown("/.."), Path::new("/"));
        assert_eq!(shown("/docs/../.."), Path::new("/"));
    }

    #[test]
    fn the_path_is_tidied() {
        assert_eq!(
            shown("/docs//papers/./paper.pdf"),
            Path::new("/docs/papers/paper.pdf")
        );
    }

    /// A folder reached through a symlink stays the one it was opened
    /// through, not the one it resolves to.
    #[cfg(unix)]
    #[test]
    fn the_folder_is_shown_as_it_was_written_not_as_it_resolves() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        std::fs::write(dir.path().join("real").join("paper.pdf"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("link")).unwrap();

        let through_the_link = dir.path().join("link").join("paper.pdf");
        assert_eq!(shown_path(&through_the_link), through_the_link);
    }

    #[test]
    fn file_sizes() {
        assert_eq!(file_size_text(512), "512 bytes");
        assert_eq!(file_size_text(830_000), "830 kB");
        assert_eq!(file_size_text(2_400_000), "2.4 MB");
        assert_eq!(file_size_text(24_000_000), "24 MB");
    }
}
