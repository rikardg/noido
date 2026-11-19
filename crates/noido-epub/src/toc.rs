//! The table of contents: the EPUB 3 nav document, else the EPUB 2 NCX.
//! A document that can't be read gives an empty outline, never an error.

use roxmltree::Node;

use crate::{TocEntry, resolve};

/// `doc_path`: where the document sits in the container; its hrefs are
/// resolved against that directory.
pub(crate) fn from_nav(text: &str, doc_path: &str) -> Vec<TocEntry> {
    // A nav document is XHTML: HTML's named entities (`&nbsp;`) are common.
    let text = noido_blocks::xhtml::replace_entities(text);
    let Ok(doc) = crate::parse_xml(&text) else {
        return Vec::new();
    };
    let dir = dir_of(doc_path);
    let navs: Vec<Node> = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "nav")
        .collect();
    let is_toc = |n: &Node| {
        n.attributes()
            .any(|a| a.name() == "type" && a.value().split_whitespace().any(|t| t == "toc"))
    };
    let Some(nav) = navs.iter().find(|n| is_toc(n)).or(navs.first()) else {
        return Vec::new();
    };
    child(*nav, "ol").map_or_else(Vec::new, |ol| nav_list(ol, dir))
}

pub(crate) fn from_ncx(text: &str, doc_path: &str) -> Vec<TocEntry> {
    let Ok(doc) = crate::parse_xml(text) else {
        return Vec::new();
    };
    let Some(map) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "navMap")
    else {
        return Vec::new();
    };
    ncx_points(map, dir_of(doc_path))
}

fn nav_list(ol: Node, dir: &str) -> Vec<TocEntry> {
    let mut out = Vec::new();
    for li in children(ol, "li") {
        let Some(label) = li
            .children()
            .find(|n| n.is_element() && matches!(n.tag_name().name(), "a" | "span"))
        else {
            continue;
        };
        let href = label
            .attribute("href")
            .filter(|_| label.tag_name().name() == "a")
            .map(|h| href_of(dir, h))
            .unwrap_or_default();
        out.push(TocEntry {
            title: collapse(&label),
            href,
            children: child(li, "ol").map_or_else(Vec::new, |ol| nav_list(ol, dir)),
        });
    }
    out
}

fn ncx_points(parent: Node, dir: &str) -> Vec<TocEntry> {
    children(parent, "navPoint")
        .map(|point| {
            let title = child(point, "navLabel")
                .and_then(|l| child(l, "text"))
                .map(|t| collapse(&t))
                .unwrap_or_default();
            let href = child(point, "content")
                .and_then(|c| c.attribute("src"))
                .map(|s| href_of(dir, s))
                .unwrap_or_default();
            TocEntry {
                title,
                href,
                children: ncx_points(point, dir),
            }
        })
        .collect()
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The container path of `href`, with its `#fragment` kept as written.
fn href_of(dir: &str, href: &str) -> String {
    match href.split_once('#') {
        Some((_, frag)) => format!("{}#{frag}", resolve(dir, href)),
        None => resolve(dir, href),
    }
}

fn collapse(node: &Node) -> String {
    let mut text = String::new();
    for t in node.descendants().filter(Node::is_text) {
        text.push_str(t.text().unwrap_or_default());
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn children<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> Option<Node<'a, 'i>> {
    children(node, name).next()
}
