//! noido EPUB: an EPUB's container, package and spine, read from a ZIP
//! held in memory into parts. Pure: bytes in, parts out.
//!
//! A part is a spine item and its number is its place in the spine
//! (`linear="no"` is ignored), so every itemref yields exactly one part.
//! Images and fonts are never read.

use std::fmt;
use std::io::{Cursor, Read};

use noido_blocks::xhtml::{self, Xhtml};
use noido_blocks::{Block, BlockKind, Style, Text};
use percent_encoding::percent_decode_str;
use roxmltree::{Document, Node, ParsingOptions};
use zip::ZipArchive;

mod toc;

/// The container, package and encryption files are small; this bounds
/// their reads whatever a header says.
const MAX_XML: u64 = 4 * 1024 * 1024;

pub struct Epub {
    pub meta: Meta,
    pub parts: Vec<Part>,
    /// From the nav document, else the NCX; empty when there is neither.
    pub toc: Vec<TocEntry>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub creators: Vec<String>,
    pub language: Option<String>,
    pub subject: Option<String>,
}

pub struct Part {
    /// Its path in the container, percent-decoded ("OEBPS/text/ch 2.xhtml").
    /// Empty when the itemref names nothing in the manifest.
    pub path: String,
    pub kind: PartKind,
}

pub enum PartKind {
    Text(Xhtml),
    /// The first spine item when it is an image itself, or has no
    /// text (only whitespace) and holds an image (an `<img>` or an svg
    /// `<image>`) or is marked as the cover (the
    /// package's guide, or `epub:type`). A marked cover with text stays
    /// `Text`.
    Cover,
    /// Couldn't be read: missing from the manifest or the ZIP, not
    /// UTF-8, not well-formed; an image straight in the spine, if not
    /// first.
    Unreadable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub children: Vec<TocEntry>,
}

#[derive(Debug, PartialEq)]
pub enum EpubError {
    NotZip,
    NoContainer,
    NoPackage,
    EmptySpine,
    Drm,
    /// The spine's XHTML, uncompressed, in bytes.
    TooLarge(usize),
}

impl fmt::Display for EpubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotZip => f.write_str("not a readable EPUB: not a ZIP archive"),
            Self::NoContainer => f.write_str("not a readable EPUB: no container"),
            Self::NoPackage => f.write_str("not a readable EPUB: no package"),
            Self::EmptySpine => f.write_str("not a readable EPUB: the spine is empty"),
            Self::Drm => f.write_str("this EPUB is DRM-protected"),
            Self::TooLarge(n) => write!(
                f,
                "too large to typeset ({} MB; the limit is 8 MB)",
                n.div_ceil(1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for EpubError {}

const CONTAINER: &str = "META-INF/container.xml";

/// Cheap: the first entry is `mimetype` reading application/epub+zip,
/// or the archive holds META-INF/container.xml.
pub fn is_epub(bytes: &[u8]) -> bool {
    let Ok(mut zip) = ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    if let Ok(mut first) = zip.by_index(0)
        && first.name_raw() == b"mimetype"
        && let Some(data) = read_capped(&mut first, 64)
        && data.starts_with(b"application/epub+zip")
    {
        return true;
    }
    zip.by_name(CONTAINER).is_ok()
}

/// `limit`: the most uncompressed spine XHTML read, in bytes.
pub fn open(bytes: &[u8], limit: usize) -> Result<Epub, EpubError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| EpubError::NotZip)?;
    let container = read_text(&mut zip, CONTAINER).ok_or(EpubError::NoContainer)?;
    let package_path = package_path(&container).ok_or(EpubError::NoPackage)?;
    let package = read_text(&mut zip, &package_path).ok_or(EpubError::NoPackage)?;
    let doc = parse_xml(&package).map_err(|_| EpubError::NoPackage)?;
    let root = doc.root_element();
    if root.tag_name().name() != "package" {
        return Err(EpubError::NoPackage);
    }
    let dir = package_path.rsplit_once('/').map_or("", |(d, _)| d);

    let mut manifest: Vec<Item> = Vec::new();
    for item in descendants(root, "item") {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            manifest.push(Item {
                id,
                path: in_dir(dir, href),
                image: item
                    .attribute("media-type")
                    .is_some_and(|t| t.starts_with("image/")),
                nav: item
                    .attribute("properties")
                    .is_some_and(|p| p.split_whitespace().any(|t| t == "nav")),
            });
        }
    }
    let spine: Vec<Option<&Item>> = descendants(root, "itemref")
        .filter_map(|r| r.attribute("idref"))
        .map(|idref| manifest.iter().find(|i| i.id == idref))
        .collect();
    if spine.is_empty() {
        return Err(EpubError::EmptySpine);
    }

    if let Some(encryption) = read_text(&mut zip, "META-INF/encryption.xml")
        && let Ok(doc) = parse_xml(&encryption)
    {
        for reference in doc
            .descendants()
            .filter(|n| n.tag_name().name() == "CipherReference")
        {
            let Some(uri) = reference.attribute("URI") else {
                continue;
            };
            let path = in_dir("", uri);
            if spine.iter().flatten().any(|i| i.path == path) {
                return Err(EpubError::Drm);
            }
        }
    }

    // The declared sizes first, so nothing is inflated for a book that is
    // too large anyway.
    let mut declared = 0usize;
    for item in spine.iter().flatten().filter(|i| !i.image) {
        if let Ok(entry) = zip.by_name(&item.path) {
            declared = declared.saturating_add(entry.size() as usize);
        }
    }
    if declared > limit {
        return Err(EpubError::TooLarge(declared));
    }

    let guide_cover = descendants(root, "reference")
        .find(|r| r.attribute("type") == Some("cover"))
        .and_then(|r| r.attribute("href"))
        .map(|href| in_dir(dir, href));

    let mut parts = Vec::with_capacity(spine.len());
    let mut read = 0usize;
    for (place, item) in spine.iter().enumerate() {
        let first = place == 0;
        let Some(item) = item else {
            parts.push(Part {
                path: String::new(),
                kind: PartKind::Unreadable,
            });
            continue;
        };
        let kind = if item.image {
            if first {
                PartKind::Cover
            } else {
                PartKind::Unreadable
            }
        } else {
            // A lying header can't inflate past what is left of the limit.
            let left = (limit - read) as u64;
            let data = zip
                .by_name(&item.path)
                .ok()
                .and_then(|mut entry| read_capped(&mut entry, left + 1));
            match data {
                Some(data) => {
                    read += data.len();
                    if read > limit {
                        return Err(EpubError::TooLarge(read));
                    }
                    match xhtml::parse(&data) {
                        Ok(x) => {
                            let marked = x.cover || guide_cover.as_deref() == Some(&item.path);
                            if first && !x.has_text && (marked || x.svg_image || has_image(&x)) {
                                PartKind::Cover
                            } else {
                                PartKind::Text(x)
                            }
                        }
                        Err(()) => PartKind::Unreadable,
                    }
                }
                None => PartKind::Unreadable,
            }
        };
        parts.push(Part {
            path: item.path.clone(),
            kind,
        });
    }

    let ncx = descendants(root, "spine")
        .next()
        .and_then(|s| s.attribute("toc"))
        .and_then(|id| manifest.iter().find(|i| i.id == id));
    let mut toc = manifest
        .iter()
        .find(|i| i.nav)
        .and_then(|nav| read_text(&mut zip, &nav.path).map(|t| toc::from_nav(&t, &nav.path)))
        .unwrap_or_default();
    if toc.is_empty()
        && let Some(ncx) = ncx
        && let Some(text) = read_text(&mut zip, &ncx.path)
    {
        toc = toc::from_ncx(&text, &ncx.path);
    }

    Ok(Epub {
        meta: metadata(root),
        parts,
        toc,
    })
}

struct Item<'a> {
    id: &'a str,
    path: String,
    image: bool,
    nav: bool,
}

fn read_capped(reader: &mut impl Read, cap: u64) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    reader.take(cap).read_to_end(&mut data).ok()?;
    Some(data)
}

fn read_text(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let data = read_capped(&mut entry, MAX_XML)?;
    String::from_utf8(data)
        .ok()
        .map(|s| s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s))
}

/// An XML file of the container. Real books carry a DOCTYPE (the OEB
/// package, the NCX); roxmltree reads it without fetching anything.
fn parse_xml(text: &str) -> Result<Document<'_>, roxmltree::Error> {
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(text, options)
}

fn package_path(container: &str) -> Option<String> {
    let doc = parse_xml(container).ok()?;
    descendants(doc.root_element(), "rootfile")
        .find(|r| r.attribute("media-type") == Some("application/oebps-package+xml"))
        .and_then(|r| r.attribute("full-path"))
        .map(|p| in_dir("", p))
}

fn descendants<'a, 'i>(
    root: Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    root.descendants()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}

fn metadata(root: Node) -> Meta {
    let mut meta = Meta::default();
    let Some(md) = descendants(root, "metadata").next() else {
        return meta;
    };
    for node in md.children().filter(Node::is_element) {
        let text = node.text().map(str::trim).filter(|t| !t.is_empty());
        let Some(text) = text else { continue };
        match node.tag_name().name() {
            "title" if meta.title.is_none() => meta.title = Some(text.to_string()),
            "creator" => meta.creators.push(text.to_string()),
            "language" if meta.language.is_none() => meta.language = Some(text.to_string()),
            "subject" if meta.subject.is_none() => meta.subject = Some(text.to_string()),
            _ => {}
        }
    }
    meta
}

/// `href`, as written in the file at `base_path`, as a container path
/// with its fragment kept as written: "path" or "path#fragment". A bare
/// "#fragment" is `base_path`'s own. The path is percent-decoded and
/// `..` resolved.
pub fn resolve(base_path: &str, href: &str) -> String {
    let (path, fragment) = match href.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (href, None),
    };
    let dir = base_path.rsplit_once('/').map_or("", |(d, _)| d);
    let mut out = if path.is_empty() {
        base_path.to_string()
    } else {
        in_dir(dir, path)
    };
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

/// A path in the container: `href` against `dir`, its fragment dropped,
/// percent-decoded, `.` and `..` resolved.
fn in_dir(dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or_default();
    let decoded = percent_decode_str(href).decode_utf8_lossy();
    let mut out: Vec<&str> = if decoded.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn has_image(x: &Xhtml) -> bool {
    x.tree.blocks.iter().any(block_has_image)
}

fn block_has_image(block: &Block) -> bool {
    match &block.kind {
        BlockKind::Heading { text, .. } | BlockKind::Paragraph(text) => text_has_image(text),
        BlockKind::Quote(blocks) | BlockKind::Footnote { blocks, .. } => {
            blocks.iter().any(block_has_image)
        }
        BlockKind::List { items, .. } => items.iter().any(|i| i.blocks.iter().any(block_has_image)),
        BlockKind::Table { head, rows, .. } => head
            .iter()
            .chain(rows.iter().flatten())
            .any(|c| text_has_image(&c.text)),
        _ => false,
    }
}

fn text_has_image(text: &Text) -> bool {
    text.styles
        .iter()
        .any(|s| matches!(s.style, Style::Image { .. }))
}
