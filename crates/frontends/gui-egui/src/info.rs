//! The document info page: the file, what the document says about itself,
//! its format and its fonts.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui;
use noido_core::{Family, families};
use noido_engine::{Date, PdfBytes, PdfEngine, Properties, Size};
use noido_render::read_properties;

/// The open document's file, to read its properties from.
pub struct Source {
    pub engine: Arc<dyn PdfEngine>,
    pub bytes: PdfBytes,
    pub path: PathBuf,
}

/// The open info page.
pub struct Page {
    reply: Receiver<Properties>,
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
        Self { reply, read: None }
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

/// Shows the page over the window; returns whether it should close.
/// `sizes` are the pages' sizes.
pub fn show(page: &mut Page, ctx: &egui::Context, source: &Source, sizes: &[Size]) -> bool {
    page.receive();
    let mut close = false;
    let max_height = ctx.content_rect().height() * 0.7;
    let response = egui::Modal::new(egui::Id::unique("document-info")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading("Document info");
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(max_height)
            .show(ui, |ui| contents(ui, page, source, sizes));
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button("Close").clicked();
        });
    });
    close || response.should_close()
}

fn contents(ui: &mut egui::Ui, page: &Page, source: &Source, sizes: &[Size]) {
    let read = page.read.as_ref();
    let properties = read.map(|(p, _)| p);
    section(ui, "File", |ui| {
        let name = source
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let folder = source.path.parent().map(|p| p.display().to_string());
        row(ui, "Name", name);
        row(ui, "Folder", folder.filter(|f| !f.is_empty()));
        row(ui, "Size", Some(file_size_text(source.bytes.len() as u64)));
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
                ui.add(egui::Label::new(egui::RichText::new(&family.name).strong()))
                    .on_hover_text(raw.join("\n"));
                ui.add(egui::Label::new(styles.join(", ")).wrap());
                ui.end_row();
            }
        }
    });
}

/// A heading and a grid of rows under it.
fn section(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong());
    ui.add_space(2.0);
    egui::Grid::new(title)
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, rows);
}

/// A row of a section, left out without a value.
fn row(ui: &mut egui::Ui, key: &str, value: Option<String>) {
    let Some(value) = value else {
        return;
    };
    ui.label(egui::RichText::new(key).weak());
    ui.add(egui::Label::new(value).wrap());
    ui.end_row();
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

    #[test]
    fn file_sizes() {
        assert_eq!(file_size_text(512), "512 bytes");
        assert_eq!(file_size_text(830_000), "830 kB");
        assert_eq!(file_size_text(2_400_000), "2.4 MB");
        assert_eq!(file_size_text(24_000_000), "24 MB");
    }
}
