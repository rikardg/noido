//! noido comic-archive engine: CBZ (a ZIP of images), read with `zip` and
//! decoded with `image`.
//!
//! Pages are the archive's images, in natural order of their names, and a
//! page is as many points as the image is pixels. Renders above that size
//! return the image's own pixels rather than an upscale: a raster page has
//! no more detail, and the frontend's texture budget should not pay for it.

use std::cmp::Ordering;
use std::io::{Cursor, Read};

use noido_engine::{
    Date, Document, EngineError, PdfBytes, PdfEngine, PixelRect, Properties, Rgba8Image, Size,
};
use zip::ZipArchive;

/// A comic archive larger than this is refused: a page is decoded whole.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// A page's image with more pixels than this is refused, so a zip bomb or a
/// stray poster scan cannot take the render thread down.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
/// A ridiculous number of entries is not a comic, and listing them all would
/// be work an archive should not be able to ask for.
const MAX_ENTRIES: usize = 20_000;

/// A CBZ document engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct ComicEngine;

impl PdfEngine for ComicEngine {
    fn open(&self, bytes: PdfBytes) -> Result<Box<dyn Document>, EngineError> {
        let mut zip = archive(&bytes)?;
        if zip.len() > MAX_ENTRIES {
            return Err(EngineError::Open(format!(
                "the archive has {} entries, more than a comic",
                zip.len()
            )));
        }
        let mut pages = Vec::new();
        let mut properties = Properties::default();
        let mut read_info = false;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(zip_error)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().map_err(zip_error)?.into_owned();
            if junk(&name) {
                continue;
            }
            if is_comic_info(&name) {
                if !read_info {
                    let mut xml = String::new();
                    if entry.read_to_string(&mut xml).is_ok() {
                        properties = parse_comic_info(&xml);
                    }
                    read_info = true;
                }
                continue;
            }
            if entry.size() > MAX_ENTRY_BYTES {
                continue;
            }
            let Some((width, height)) = image_size(&mut entry) else {
                continue;
            };
            pages.push(Page {
                index,
                name,
                size: Size {
                    width: width as f32,
                    height: height as f32,
                },
            });
        }
        if pages.is_empty() {
            return Err(EngineError::Open("no images in the archive".into()));
        }
        pages.sort_by(|a, b| natural_cmp(&a.name, &b.name).then(a.index.cmp(&b.index)));
        Ok(Box::new(ComicDocument {
            bytes,
            pages,
            properties,
        }))
    }
}

struct Page {
    /// The entry's index in the archive.
    index: usize,
    name: String,
    size: Size,
}

struct ComicDocument {
    bytes: PdfBytes,
    pages: Vec<Page>,
    properties: Properties,
}

impl ComicDocument {
    fn page(&self, page: usize) -> Result<&Page, EngineError> {
        self.pages.get(page).ok_or(EngineError::PageOutOfRange {
            page,
            count: self.pages.len(),
        })
    }

    fn archive(&self) -> Result<ZipArchive<Cursor<&[u8]>>, EngineError> {
        archive(&self.bytes)
    }

    fn decode(&self, page: &Page) -> Result<image::RgbaImage, EngineError> {
        let mut zip = self.archive()?;
        let mut entry = zip.by_index(page.index).map_err(zip_error)?;
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(EngineError::Render(format!(
                "page {} is {} MB, too large",
                page.index + 1,
                entry.size() / (1024 * 1024)
            )));
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| EngineError::Render(format!("page {}: {e}", page.index + 1)))?;
        let decoded = image::load_from_memory(&data)
            .map_err(|e| EngineError::Render(format!("page {}: {e}", page.index + 1)))?;
        let mut rgba = decoded.into_rgba8();
        let pixels = u64::from(rgba.width()) * u64::from(rgba.height());
        if pixels > MAX_PIXELS {
            return Err(EngineError::Render(format!(
                "page {} is {} megapixels, too large",
                page.index + 1,
                pixels / 1_000_000
            )));
        }
        premultiply(&mut rgba);
        Ok(rgba)
    }
}

impl Document for ComicDocument {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> Result<Size, EngineError> {
        Ok(self.page(page)?.size)
    }

    fn render_tile(
        &self,
        page: usize,
        scale: f32,
        rect: PixelRect,
    ) -> Result<Rgba8Image, EngineError> {
        let image = self.decode(self.page(page)?)?;
        let scale = scale.max(0.0);
        let width = ((image.width() as f32 * scale).round() as u32).max(1);
        let height = ((image.height() as f32 * scale).round() as u32).max(1);
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(EngineError::TooLarge { width, height });
        }
        let full = resized(image, width, height);
        let x = rect.x.min(full.width().saturating_sub(1));
        let y = rect.y.min(full.height().saturating_sub(1));
        let width = rect.width.min(full.width() - x);
        let height = rect.height.min(full.height() - y);
        if width == 0 || height == 0 {
            return Err(EngineError::Render("empty tile".into()));
        }
        let stride = full.width() as usize * 4;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for row in y..y + height {
            let start = row as usize * stride + x as usize * 4;
            pixels.extend_from_slice(&full.as_raw()[start..start + width as usize * 4]);
        }
        Rgba8Image::from_premultiplied(width, height, pixels)
            .ok_or_else(|| EngineError::Render("tile size".into()))
    }

    fn render_page(&self, page: usize, width: u32, height: u32) -> Result<Rgba8Image, EngineError> {
        let image = self.decode(self.page(page)?)?;
        let (source_width, source_height) = image.dimensions();
        // Never upscale: the extra pixels carry no detail, and the reader
        // pays for them in texture budget.
        let scale = (width.max(1) as f32 / source_width as f32)
            .min(height.max(1) as f32 / source_height as f32)
            .min(1.0);
        let width = ((source_width as f32 * scale).round() as u32).max(1);
        let height = ((source_height as f32 * scale).round() as u32).max(1);
        let image = resized(image, width, height);
        Rgba8Image::from_premultiplied(width, height, image.into_raw())
            .ok_or_else(|| EngineError::Render("page size".into()))
    }

    fn properties(&self) -> Properties {
        self.properties.clone()
    }
}

fn archive(bytes: &PdfBytes) -> Result<ZipArchive<Cursor<&[u8]>>, EngineError> {
    ZipArchive::new(Cursor::new(&bytes[..]))
        .map_err(|e| EngineError::Open(format!("not a readable CBZ: {e}")))
}

fn zip_error(error: zip::result::ZipError) -> EngineError {
    EngineError::Open(format!("not a readable CBZ: {error}"))
}

fn resized(image: image::RgbaImage, width: u32, height: u32) -> image::RgbaImage {
    if image.width() == width && image.height() == height {
        return image;
    }
    let (source_width, source_height) = image.dimensions();
    let source = fast_image_resize::images::Image::from_vec_u8(
        source_width,
        source_height,
        image.into_raw(),
        fast_image_resize::PixelType::U8x4,
    )
    .expect("an RGBA image is the right size");
    let mut destination =
        fast_image_resize::images::Image::new(width, height, fast_image_resize::PixelType::U8x4);
    // The pixels are already premultiplied; convolving them as they are is
    // the right thing, so the resizer must not scale them by alpha again.
    let mut options = fast_image_resize::ResizeOptions::new().resize_alg(
        fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::CatmullRom),
    );
    options.mul_div_alpha = false;
    fast_image_resize::Resizer::new()
        .resize(&source, &mut destination, Some(&options))
        .expect("both images are RGBA");
    image::RgbaImage::from_raw(width, height, destination.into_vec())
        .expect("the resizer made the right size")
}

/// `image`'s pixels in the premultiplied form [`Rgba8Image`] carries.
fn premultiply(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha != 255 {
            for channel in &mut pixel.0[..3] {
                *channel = (u32::from(*channel) * alpha / 255) as u8;
            }
        }
    }
}

/// Files a comic archive carries around its pages, not pages themselves.
fn junk(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("__macosx/")
        || lower == ".ds_store"
        || lower.ends_with("/.ds_store")
        || name
            .rsplit('/')
            .next()
            .is_some_and(|file| file.starts_with("._"))
}

fn is_comic_info(name: &str) -> bool {
    name.rsplit('/')
        .next()
        .is_some_and(|file| file.eq_ignore_ascii_case("ComicInfo.xml"))
}

/// The image's size from as much of its header as it takes, without decoding
/// the pixels. An entry that is not an image gives `None`.
fn image_size(entry: &mut impl Read) -> Option<(u32, u32)> {
    let mut data = Vec::new();
    for more in [4_096u64, 64 * 1024, 4 * 1024 * 1024] {
        let before = data.len();
        entry.take(more).read_to_end(&mut data).ok()?;
        if let Some(size) = dimensions(&data) {
            return Some(size);
        }
        // The entry ended: reading more would tell us nothing.
        if data.len() == before {
            break;
        }
    }
    entry.read_to_end(&mut data).ok()?;
    dimensions(&data)
}

fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// The text of the first `tag` in a ComicInfo document.
fn parse_comic_info(xml: &str) -> Properties {
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return Properties::default();
    };
    let text = |tag: &str| {
        document
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == tag)
            .and_then(|node| node.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let number = |tag: &str| text(tag).and_then(|text| text.parse::<u16>().ok());
    let created = number("Year").map(|year| Date {
        year,
        month: number("Month").unwrap_or(1).clamp(1, 12) as u8,
        day: number("Day").unwrap_or(1).clamp(1, 31) as u8,
        hour: 0,
        minute: 0,
        second: 0,
        utc_offset: None,
    });
    Properties {
        title: text("Title"),
        author: text("Writer"),
        subject: text("Summary"),
        keywords: text("Genre").or_else(|| text("Tags")),
        created,
        ..Properties::default()
    }
}

/// Natural order, case-insensitively: `page2` before `page10`.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a_bytes[i].is_ascii_digit() && b_bytes[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < a.len() && a_bytes[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b_bytes[j].is_ascii_digit() {
                j += 1;
            }
            let (run_a, run_b) = (&a_bytes[start_i..i], &b_bytes[start_j..j]);
            let trimmed_a = trim_zeros(run_a);
            let trimmed_b = trim_zeros(run_b);
            let order = trimmed_a
                .len()
                .cmp(&trimmed_b.len())
                .then_with(|| trimmed_a.cmp(trimmed_b))
                .then_with(|| run_a.len().cmp(&run_b.len()));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            let (char_a, char_b) = (a[i..].chars().next(), b[j..].chars().next());
            let (Some(char_a), Some(char_b)) = (char_a, char_b) else {
                break;
            };
            let order = char_a.cmp(&char_b);
            if order != Ordering::Equal {
                return order;
            }
            i += char_a.len_utf8();
            j += char_b.len_utf8();
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn trim_zeros(run: &[u8]) -> &[u8] {
    let first = run.iter().position(|&byte| byte != b'0');
    first.map_or(&run[run.len()..], |first| &run[first..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_counts_runs_of_digits() {
        assert_eq!(natural_cmp("page2", "page10"), Ordering::Less);
        assert_eq!(natural_cmp("page10", "page2"), Ordering::Greater);
        assert_eq!(natural_cmp("Page2", "page10"), Ordering::Less);
        assert_eq!(natural_cmp("page02", "page2"), Ordering::Greater);
        assert_eq!(natural_cmp("page2", "page02"), Ordering::Less);
        assert_eq!(natural_cmp("pages/1.png", "pages/10.png"), Ordering::Less);
        assert_eq!(natural_cmp("page2a", "page2b"), Ordering::Less);
    }

    #[test]
    fn junk_is_not_a_page() {
        assert!(junk("__MACOSX/._1.png"));
        assert!(junk("pages/.DS_Store"));
        assert!(junk("._1.png"));
        assert!(!junk("pages/1.png"));
    }
}
