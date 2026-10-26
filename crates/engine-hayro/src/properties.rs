//! What the document says about itself: the info dictionary, then XMP for
//! what it lacks; flags from the catalog; and the fonts its pages use.

use std::collections::HashSet;

use hayro::hayro_syntax::object::{Array, DateTime, Dict, Name, ObjectIdentifier, Stream};
use hayro::hayro_syntax::page::Resources;
use hayro::hayro_syntax::{Pdf, PdfVersion};
use noido_engine::{Date, Font, Properties};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";

pub(crate) fn properties(pdf: &Pdf) -> Properties {
    let info = pdf.metadata();
    let text =
        |bytes: &Option<Vec<u8>>| bytes.as_deref().map(decode_text).filter(|s| !s.is_empty());
    let mut p = Properties {
        title: text(&info.title),
        author: text(&info.author),
        subject: text(&info.subject),
        keywords: text(&info.keywords),
        creator: text(&info.creator),
        producer: text(&info.producer),
        created: info.creation_date.map(info_date),
        modified: info.modification_date.map(info_date),
        version: Some(version(pdf.version()).into()),
        ..Properties::default()
    };
    if let Some(catalog) = pdf.xref().get::<Dict>(pdf.xref().root_id()) {
        p.tagged = catalog
            .get::<Dict>(b"MarkInfo")
            .and_then(|m| m.get::<bool>(b"Marked"))
            .unwrap_or(false);
        p.has_outline = catalog
            .get::<Dict>(b"Outlines")
            .is_some_and(|o| o.contains_key(b"First"));
        if let Some(xmp) = catalog
            .get::<Stream>(b"Metadata")
            .and_then(|s| s.decoded().ok())
            .and_then(|bytes| String::from_utf8(bytes.into_owned()).ok())
        {
            fill_from_xmp(&mut p, &xmp);
        }
    }
    p.fonts = fonts(pdf);
    p
}

fn version(version: PdfVersion) -> &'static str {
    match version {
        PdfVersion::Pdf10 => "1.0",
        PdfVersion::Pdf11 => "1.1",
        PdfVersion::Pdf12 => "1.2",
        PdfVersion::Pdf13 => "1.3",
        PdfVersion::Pdf14 => "1.4",
        PdfVersion::Pdf15 => "1.5",
        PdfVersion::Pdf16 => "1.6",
        PdfVersion::Pdf17 => "1.7",
        PdfVersion::Pdf20 => "2.0",
    }
}

fn info_date(d: DateTime) -> Date {
    // hayro keeps the sign on the hours only.
    let minutes = i16::from(d.utc_offset_minute);
    let offset = i16::from(d.utc_offset_hour) * 60
        + if d.utc_offset_hour < 0 {
            -minutes
        } else {
            minutes
        };
    Date {
        year: d.year,
        month: d.month,
        day: d.day,
        hour: d.hour,
        minute: d.minute,
        second: d.second,
        utc_offset: Some(offset),
    }
}

/// A PDF text string: UTF-16BE or UTF-8 with a byte-order mark, else
/// PDFDocEncoding.
fn decode_text(bytes: &[u8]) -> String {
    let text = if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_be_bytes(pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(utf8) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(utf8).into_owned()
    } else {
        bytes.iter().map(|&b| pdf_doc_char(b)).collect()
    };
    text.trim().to_owned()
}

/// PDFDocEncoding: Latin-1 but for 0x80–0xA0.
fn pdf_doc_char(byte: u8) -> char {
    const HIGH: [char; 33] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\u{FFFD}', '€',
    ];
    match byte {
        0x80..=0xA0 => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// Fills the fields `p` lacks from the XMP packet `xmp`.
fn fill_from_xmp(p: &mut Properties, xmp: &str) {
    let Ok(doc) = roxmltree::Document::parse(xmp.trim_start_matches('\u{FEFF}')) else {
        return;
    };
    let value = |ns: &str, name: &str| xmp_value(&doc, ns, name);
    let fill = |field: &mut Option<String>, ns: &str, name: &str| {
        if field.is_none() {
            *field = value(ns, name);
        }
    };
    fill(&mut p.title, DC, "title");
    fill(&mut p.author, DC, "creator");
    fill(&mut p.subject, DC, "description");
    fill(&mut p.keywords, PDF, "Keywords");
    fill(&mut p.creator, XMP, "CreatorTool");
    fill(&mut p.producer, PDF, "Producer");
    if p.created.is_none() {
        p.created = value(XMP, "CreateDate").as_deref().and_then(xmp_date);
    }
    if p.modified.is_none() {
        p.modified = value(XMP, "ModifyDate").as_deref().and_then(xmp_date);
    }
}

/// A property, written as an attribute or an element: an element's
/// `rdf:Alt` gives its default (else first) item, `rdf:Seq` and `rdf:Bag`
/// all items, joined.
fn xmp_value(doc: &roxmltree::Document<'_>, ns: &str, name: &str) -> Option<String> {
    let value = doc
        .descendants()
        .filter(|n| n.is_element())
        .find_map(|node| {
            if let Some(value) = node.attribute((ns, name)) {
                return Some(value.to_owned());
            }
            let tag = node.tag_name();
            if tag.namespace() != Some(ns) || tag.name() != name {
                return None;
            }
            let rdf = |n: &roxmltree::Node<'_, '_>, name: &str| {
                n.is_element()
                    && n.tag_name().namespace() == Some(RDF)
                    && n.tag_name().name() == name
            };
            let Some(container) = node.children().find(|c| c.is_element()) else {
                return node.text().map(str::to_owned);
            };
            let items: Vec<roxmltree::Node<'_, '_>> =
                container.children().filter(|c| rdf(c, "li")).collect();
            if rdf(&container, "Alt") {
                let default = items.iter().find(|li| {
                    li.attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                        == Some("x-default")
                });
                default
                    .or(items.first())
                    .and_then(|li| li.text())
                    .map(str::to_owned)
            } else {
                let texts: Vec<&str> = items.iter().filter_map(|li| li.text()).collect();
                Some(texts.join(", "))
            }
        })?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// An XMP date: `YYYY[-MM[-DD[Thh:mm[:ss[.s+]][Z|±hh:mm]]]]`.
fn xmp_date(text: &str) -> Option<Date> {
    let num = |s: &str| s.parse::<u16>().ok();
    let (date, time) = text.split_once('T').unwrap_or((text, ""));
    let mut parts = date.split('-');
    let year = num(parts.next()?)?;
    let month = parts.next().map_or(Some(1), num)?;
    let day = parts.next().map_or(Some(1), num)?;
    let (clock, utc_offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, Some(0))
    } else if let Some(i) = time.rfind(['+', '-']) {
        let sign = if time[i..].starts_with('-') { -1 } else { 1 };
        let (h, m) = time[i + 1..].split_once(':')?;
        let minutes = i16::try_from(num(h)? * 60 + num(m)?).ok()?;
        (&time[..i], Some(sign * minutes))
    } else {
        (time, None)
    };
    let mut clock = clock.split(':');
    let mut next = || clock.next().map_or(Some(0), |s| num(s.split('.').next()?));
    let (hour, minute, second) = (next()?, next()?, next()?);
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    valid.then_some(Date {
        year,
        month: month as u8,
        day: day as u8,
        hour: hour as u8,
        minute: minute as u8,
        second: second as u8,
        utc_offset,
    })
}

/// Each named font once, by name, from every page and the forms they draw
/// (each form once, so forms that draw each other end).
fn fonts(pdf: &Pdf) -> Vec<Font> {
    let mut walk = FontWalk::default();
    for page in pdf.pages().iter() {
        walk.resources(page.resources());
    }
    walk.fonts.sort_by(|a, b| a.name.cmp(&b.name));
    walk.fonts
}

#[derive(Default)]
struct FontWalk {
    fonts: Vec<Font>,
    seen_fonts: HashSet<Font>,
    seen_forms: HashSet<ObjectIdentifier>,
}

impl FontWalk {
    fn resources(&mut self, resources: &Resources<'_>) {
        for key in resources.fonts.keys() {
            if let Some(font) = resources.fonts.get::<Dict>(&key).and_then(|d| font(&d))
                && self.seen_fonts.insert(font.clone())
            {
                self.fonts.push(font);
            }
        }
        for key in resources.x_objects.keys() {
            let Some(form) = resources.x_objects.get::<Stream>(&key) else {
                continue;
            };
            let dict = form.dict();
            let is_form = dict.get::<Name>(b"Subtype").is_some_and(|s| &*s == b"Form");
            if !is_form || !self.seen_forms.insert(form.obj_id()) {
                continue;
            }
            if let Some(inner) = dict.get::<Dict>(b"Resources") {
                self.resources(&Resources::new(inner));
            }
        }
    }
}

/// A font dictionary's name and whether it carries its font program: a
/// composite font's program is in its descendant.
fn font(dict: &Dict<'_>) -> Option<Font> {
    let name = dict.get::<Name>(b"BaseFont")?;
    let program_holder = dict
        .get::<Array>(b"DescendantFonts")
        .and_then(|kids| kids.iter::<Dict>().next())
        .unwrap_or_else(|| dict.clone());
    let embedded = program_holder
        .get::<Dict>(b"FontDescriptor")
        .is_some_and(|d| {
            [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
                .iter()
                .any(|key| d.contains_key(key))
        });
    Some(Font {
        name: decode_text(&name),
        embedded,
    })
}
