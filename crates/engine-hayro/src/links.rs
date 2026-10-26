//! Link annotations and their destinations, read from the page's raw
//! dictionaries: hayro has no annotation API.

use std::collections::HashMap;

use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{
    Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, String as PdfString,
};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, Point};
use noido_engine::{Destination, Link, LinkTarget, Rect};

/// Name trees deeper than this are treated as broken (or cyclic).
const MAX_TREE_DEPTH: usize = 32;

/// Each page's object identifier, to turn destination references into
/// page indices.
pub(crate) type PageIndex = HashMap<ObjectIdentifier, usize>;

pub(crate) fn page_index(pdf: &Pdf) -> PageIndex {
    pdf.pages()
        .iter()
        .enumerate()
        .filter_map(|(i, page)| page.raw().obj_id().map(|id| (id, i)))
        .collect()
}

/// The links on page `page`, in annotation order. Links that can't be
/// read, or that go anywhere but a place in this document or a URI, are
/// left out.
pub(crate) fn page_links(pdf: &Pdf, index: &PageIndex, page: usize) -> Vec<Link> {
    let Some(this) = pdf.pages().get(page) else {
        return Vec::new();
    };
    let Some(annots) = this.raw().get::<Array>(b"Annots") else {
        return Vec::new();
    };
    let transform = this.initial_transform(true).to_kurbo();
    let (width, height) = this.render_dimensions();
    annots
        .iter::<Dict>()
        .filter(|annot| {
            annot
                .get::<Name>(b"Subtype")
                .is_some_and(|s| &*s == b"Link")
        })
        .filter_map(|annot| {
            let target = target(pdf, index, &annot)?;
            let areas: Vec<Rect> = areas(&annot)
                .into_iter()
                .filter_map(|(min, max)| page_rect(transform, min, max, width, height))
                .collect();
            (!areas.is_empty()).then_some(Link { areas, target })
        })
        .collect()
}

/// The link's areas in PDF space, as (min, max) corners: from
/// `/QuadPoints` when it has a usable one, else from `/Rect`.
fn areas(annot: &Dict<'_>) -> Vec<(Point, Point)> {
    let quads: Vec<f64> = annot
        .get::<Array>(b"QuadPoints")
        .map(|a| a.iter::<f64>().collect())
        .unwrap_or_default();
    if !quads.is_empty() && quads.len().is_multiple_of(8) {
        return quads
            .chunks(8)
            .map(|q| {
                let xs = [q[0], q[2], q[4], q[6]];
                let ys = [q[1], q[3], q[5], q[7]];
                let min = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
                let max = |v: [f64; 4]| v.into_iter().fold(f64::NEG_INFINITY, f64::max);
                (Point::new(min(xs), min(ys)), Point::new(max(xs), max(ys)))
            })
            .collect();
    }
    let rect: Vec<f64> = annot
        .get::<Array>(b"Rect")
        .map(|a| a.iter::<f64>().collect())
        .unwrap_or_default();
    match rect[..] {
        [x0, y0, x1, y1] => vec![(
            Point::new(x0.min(x1), y0.min(y1)),
            Point::new(x0.max(x1), y0.max(y1)),
        )],
        _ => Vec::new(),
    }
}

/// A PDF-space box in page points, upright and clipped to the page;
/// `None` when nothing of it is on the page.
fn page_rect(transform: Affine, min: Point, max: Point, width: f32, height: f32) -> Option<Rect> {
    let corners = [
        transform * min,
        transform * Point::new(max.x, min.y),
        transform * max,
        transform * Point::new(min.x, max.y),
    ];
    let left = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) as f32;
    let right = corners
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max) as f32;
    let top = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) as f32;
    let bottom = corners
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max) as f32;
    let (left, right) = (left.max(0.0), right.min(width));
    let (top, bottom) = (top.max(0.0), bottom.min(height));
    (left < right && top < bottom).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn target(pdf: &Pdf, index: &PageIndex, annot: &Dict<'_>) -> Option<LinkTarget> {
    if let Some(dest) = annot.get::<Object>(b"Dest") {
        return destination(pdf, index, dest).map(LinkTarget::Internal);
    }
    let action = annot.get::<Dict>(b"A")?;
    let kind = action.get::<Name>(b"S")?;
    match &*kind {
        b"GoTo" => destination(pdf, index, action.get::<Object>(b"D")?).map(LinkTarget::Internal),
        b"URI" => {
            let uri = action.get::<PdfString>(b"URI")?;
            Some(LinkTarget::Web(
                std::string::String::from_utf8_lossy(uri.as_bytes()).into_owned(),
            ))
        }
        _ => None,
    }
}

/// Resolves a destination: an array, or a name looked up in the document.
fn destination(pdf: &Pdf, index: &PageIndex, dest: Object<'_>) -> Option<Destination> {
    match dest {
        Object::Array(array) => explicit(pdf, index, &array),
        Object::String(name) => explicit(pdf, index, &named(pdf, name.as_bytes())?),
        Object::Name(name) => explicit(pdf, index, &named(pdf, &name)?),
        _ => None,
    }
}

/// The destination array for `name`: from the `/Names /Dests` tree, else
/// the catalog's older `/Dests` dictionary.
fn named<'a>(pdf: &'a Pdf, name: &[u8]) -> Option<Array<'a>> {
    let catalog = pdf.xref().get::<Dict>(pdf.xref().root_id())?;
    let value = catalog
        .get::<Dict>(b"Names")
        .and_then(|names| names.get::<Dict>(b"Dests"))
        .and_then(|tree| lookup(&tree, name, 0))
        .or_else(|| catalog.get::<Dict>(b"Dests")?.get::<Object>(name))?;
    match value {
        Object::Array(array) => Some(array),
        Object::Dict(dict) => dict.get::<Array>(b"D"),
        _ => None,
    }
}

/// Looks `name` up in a name tree node.
fn lookup<'a>(node: &Dict<'a>, name: &[u8], depth: usize) -> Option<Object<'a>> {
    if depth > MAX_TREE_DEPTH {
        return None;
    }
    if let Some(limits) = node.get::<Array>(b"Limits") {
        let limits: Vec<PdfString> = limits.iter::<PdfString>().collect();
        if let [low, high] = &limits[..]
            && (name < low.as_bytes() || name > high.as_bytes())
        {
            return None;
        }
    }
    if let Some(names) = node.get::<Array>(b"Names") {
        let mut entries = names.iter::<Object>();
        while let (Some(key), Some(value)) = (entries.next(), entries.next()) {
            if let Object::String(key) = key
                && key.as_bytes() == name
            {
                return Some(value);
            }
        }
    }
    node.get::<Array>(b"Kids")?
        .iter::<Dict>()
        .find_map(|kid| lookup(&kid, name, depth + 1))
}

/// An explicit destination: `[page kind args…]`.
fn explicit(pdf: &Pdf, index: &PageIndex, array: &Array<'_>) -> Option<Destination> {
    let mut items = array.raw_iter();
    let page = match items.next()? {
        MaybeRef::Ref(r) => *index.get(&ObjectIdentifier::from(r))?,
        MaybeRef::NotRef(Object::Number(n)) => usize::try_from(n.as_i64()).ok()?,
        MaybeRef::NotRef(_) => return None,
    };
    let target = pdf.pages().get(page)?;
    let kind = array.iter::<Object>().nth(1);
    let args: Vec<Option<f64>> = array
        .iter::<Object>()
        .skip(2)
        .map(|o| match o {
            Object::Number(n) => Some(n.as_f64()),
            _ => None,
        })
        .collect();
    let arg = |i: usize| args.get(i).copied().flatten();
    let (x, y) = match kind {
        Some(Object::Name(kind)) => match &*kind {
            b"XYZ" => (arg(0), arg(1)),
            b"FitH" | b"FitBH" => (None, arg(0)),
            b"FitV" | b"FitBV" => (arg(0), None),
            b"FitR" => (arg(0), arg(3)),
            _ => (None, None),
        },
        _ => (None, None),
    };
    let (x, y) = to_page(target, x, y);
    Some(Destination { page, x, y })
}

/// A PDF-space point with coordinates possibly missing, in page points.
/// An output coordinate is kept only if it doesn't depend on a missing
/// input (rotated pages swap the axes).
fn to_page(page: &Page<'_>, x: Option<f64>, y: Option<f64>) -> (Option<f32>, Option<f32>) {
    let [a, b, c, d, e, f] = page.initial_transform(true).to_kurbo().as_coeffs();
    let (px, py) = (x.unwrap_or(0.0), y.unwrap_or(0.0));
    let known = |coeff_x: f64, coeff_y: f64| {
        (coeff_x == 0.0 || x.is_some()) && (coeff_y == 0.0 || y.is_some())
    };
    let out_x = known(a, c).then_some((a * px + c * py + e) as f32);
    let out_y = known(b, d).then_some((b * px + d * py + f) as f32);
    (out_x, out_y)
}
