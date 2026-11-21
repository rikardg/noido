//! The desktop's recent list on Linux: `$XDG_DATA_HOME/recently-used.xbel`.
//!
//! The file is shared by every GTK application on the machine, so we only
//! ever touch the bookmark for the file being noted and the `noido`
//! application entry inside it; everything else is left as it is.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use url::Url;

/// The namespace of the desktop-bookmark metadata.
const BOOKMARK_NS: &str = "http://www.freedesktop.org/standards/desktop-bookmarks";
/// The namespace of the MIME type element.
const MIME_NS: &str = "http://www.freedesktop.org/standards/shared-mime-info";

#[cfg(target_os = "linux")]
const FILE_NAME: &str = "recently-used.xbel";
const ROOT: &str = "xbel";
const APP_NAME: &str = "noido";
const APP_EXEC: &str = "'noido %u'";
const OWNER: &str = "http://freedesktop.org";

/// The desktop's recent list.
#[derive(Clone, Debug)]
pub struct Recents {
    path: PathBuf,
}

impl Recents {
    /// The list at `path`; tests use one of their own.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The list this desktop keeps, or none when there is no home directory
    /// to keep one in.
    #[cfg(target_os = "linux")]
    pub fn for_desktop() -> Option<Self> {
        let dirs = directories::BaseDirs::new()?;
        Some(Self::at(dirs.data_dir().join(FILE_NAME)))
    }

    /// Noting `path` as opened now: its bookmark gains (or moves up with)
    /// the `noido` application entry.
    pub fn note(&self, path: &Path) -> io::Result<()> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let uri = Url::from_file_path(&path).map_err(|()| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not an absolute path", path.display()),
            )
        })?;
        let uri = uri.as_str();
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => fresh(),
            Err(e) => return Err(e),
        };
        let doc = parse(&text)?;
        let root = doc.root_element();
        if root.tag_name().name() != ROOT {
            return Err(invalid("the desktop's recent list is not an XBEL document"));
        }
        // Our elements are written with these two prefixes; inserting them
        // where the file does not bind them would make the whole list
        // unreadable for every application.
        if root.lookup_namespace_uri(Some("bookmark")) != Some(BOOKMARK_NS)
            || root.lookup_namespace_uri(Some("mime")) != Some(MIME_NS)
        {
            return Err(invalid(
                "the desktop's recent list does not declare the bookmark and mime namespaces",
            ));
        }
        let now = timestamp(SystemTime::now());
        let mut edits = Vec::new();
        match find_bookmark(&doc, uri) {
            Some(bookmark) => {
                set_attribute(&mut edits, bookmark, "modified", &now);
                let applications = bookmark
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("metadata") && node.attribute("owner") == Some(OWNER)
                    })
                    .and_then(|metadata| {
                        metadata
                            .children()
                            .find(|node| node.has_tag_name((BOOKMARK_NS, "applications")))
                    });
                match applications {
                    Some(applications) => {
                        let ours = applications.children().find(|node| {
                            node.has_tag_name((BOOKMARK_NS, "application"))
                                && node.attribute("name") == Some(APP_NAME)
                        });
                        if let Some(ours) = ours {
                            let count = ours
                                .attribute("count")
                                .and_then(|count| count.parse::<u32>().ok())
                                .unwrap_or(1)
                                .saturating_add(1);
                            set_attribute(&mut edits, ours, "count", &count.to_string());
                            set_attribute(&mut edits, ours, "modified", &now);
                            set_attribute(&mut edits, ours, "exec", APP_EXEC);
                        } else {
                            insert_application(&mut edits, applications, &text, &now);
                        }
                    }
                    None => add_metadata(&mut edits, bookmark, &text, &path, &now),
                }
            }
            None => {
                let bookmark = new_bookmark(uri, &path, &now);
                append_before_close(&mut edits, doc.root_element(), &text, &bookmark);
            }
        }
        let out = apply(&text, edits);
        write_atomic(&self.path, &out)
    }

    /// Forgetting what noido opened: its application entry goes from every
    /// bookmark, and a bookmark left with none goes too. Other applications'
    /// entries stay.
    pub fn clear(&self) -> io::Result<()> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let doc = parse(&text)?;
        if doc.root_element().tag_name().name() != ROOT {
            return Err(invalid("the desktop's recent list is not an XBEL document"));
        }
        let now = timestamp(SystemTime::now());
        let mut edits = Vec::new();
        for bookmark in doc
            .root_element()
            .children()
            .filter(|node| node.has_tag_name("bookmark"))
        {
            let applications: Vec<_> = bookmark
                .descendants()
                .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
                .collect();
            let Some(ours) = applications
                .iter()
                .find(|node| node.attribute("name") == Some(APP_NAME))
            else {
                continue;
            };
            if applications.len() == 1 {
                // Nobody else wants the file: the bookmark goes with us.
                let range = bookmark.range();
                edits.push(Edit {
                    start: range.start,
                    end: range.end,
                    text: String::new(),
                });
            } else {
                let range = ours.range();
                edits.push(Edit {
                    start: range.start,
                    end: range.end,
                    text: String::new(),
                });
                // GTK moves a bookmark whose application list changed.
                set_attribute(&mut edits, bookmark, "modified", &now);
            }
        }
        if edits.is_empty() {
            return Ok(());
        }
        let out = apply(&text, edits);
        write_atomic(&self.path, &out)
    }
}

/// The file for a list that had none: the XBEL root and both namespaces.
fn fresh() -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<xbel version=\"1.0\"\n",
            "      xmlns:bookmark=\"{}\"\n",
            "      xmlns:mime=\"{}\"\n",
            ">\n",
            "</xbel>\n",
        ),
        BOOKMARK_NS, MIME_NS
    )
}

/// The bookmark for a file the list has never seen.
fn new_bookmark(uri: &str, path: &Path, now: &str) -> String {
    format!(
        r#"  <bookmark href="{}" added="{now}" modified="{now}" visited="{now}">
    <info>
      <metadata owner="{OWNER}">
        <mime:mime-type type="{}"/>
        <bookmark:applications>
          <bookmark:application name="{APP_NAME}" exec="{}" modified="{now}" count="1"/>
        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
"#,
        escape(uri),
        mime(path),
        escape(APP_EXEC),
    )
}

/// The MIME type the desktop will show for `path`, by its extension.
fn mime(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        "cbz" => "application/vnd.comicbook+zip",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// `time` as GTK writes it: UTC, to the microsecond.
fn timestamp(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs() as i64;
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        rest / 3600,
        (rest / 60) % 60,
        rest % 60,
        since.subsec_micros()
    )
}

/// The civil date `days` after 1970-01-01, from Howard Hinnant's algorithm.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// `value` escaped for a double-quoted XML attribute.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn parse(text: &str) -> io::Result<roxmltree::Document<'_>> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| invalid(&format!("the desktop's recent list cannot be read: {e}")))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn find_bookmark<'a, 'i>(
    doc: &'a roxmltree::Document<'i>,
    uri: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    doc.root_element().children().find(|node| {
        node.is_element() && node.has_tag_name("bookmark") && node.attribute("href") == Some(uri)
    })
}

/// Where `node`'s closing tag begins in `source`.
fn before_end_tag(node: roxmltree::Node<'_, '_>, source: &str) -> usize {
    assert!(node.is_element(), "only elements have a closing tag");
    source[..node.range().end]
        .rfind('<')
        .expect("an element's range ends with its closing tag")
}

/// Replaces `node`'s `name` attribute with `value`, or adds it after the
/// first one.
fn set_attribute(edits: &mut Vec<Edit>, node: roxmltree::Node<'_, '_>, name: &str, value: &str) {
    let value = escape(value);
    if let Some(attribute) = node.attribute_node(name) {
        let range = attribute.range_value();
        edits.push(Edit {
            start: range.start,
            end: range.end,
            text: value,
        });
    } else if let Some(first) = node.attributes().next() {
        let at = first.range().end;
        edits.push(Edit {
            start: at,
            end: at,
            text: format!(" {name}=\"{value}\""),
        });
    }
}

/// Adds noido's application entry to `applications`, first in the list as
/// GTK adds a new application.
fn insert_application(
    edits: &mut Vec<Edit>,
    applications: roxmltree::Node<'_, '_>,
    source: &str,
    now: &str,
) {
    let application = format!(
        "<bookmark:application name=\"{APP_NAME}\" exec=\"{}\" modified=\"{now}\" count=\"1\"/>",
        escape(APP_EXEC)
    );
    let end = start_tag_end(applications, source);
    let range = applications.range();
    if source[range.start..end].ends_with("/>") {
        // An empty element has no room inside: give it two tags, without
        // touching the start tag's attributes.
        let name = element_name(applications, source);
        edits.push(Edit {
            start: end - 2,
            end: end - 1,
            text: String::new(),
        });
        edits.push(Edit {
            start: range.end,
            end: range.end,
            text: format!("\n          {application}\n        </{name}>"),
        });
    } else {
        edits.push(Edit {
            start: end,
            end,
            text: format!("\n          {application}"),
        });
    }
}

/// Gives a bookmark that has no metadata (or metadata without applications)
/// the missing parts: the MIME type too, which the format requires.
fn add_metadata(
    edits: &mut Vec<Edit>,
    bookmark: roxmltree::Node<'_, '_>,
    source: &str,
    path: &Path,
    now: &str,
) {
    let metadata = bookmark
        .descendants()
        .find(|node| node.has_tag_name("metadata") && node.attribute("owner") == Some(OWNER));
    let applications = format!(
        "\n        <bookmark:applications>\n          <bookmark:application name=\"{APP_NAME}\" exec=\"{}\" modified=\"{now}\" count=\"1\"/>\n        </bookmark:applications>\n      ",
        escape(APP_EXEC)
    );
    match metadata {
        Some(metadata) => {
            let mut block = String::new();
            if !metadata
                .children()
                .any(|node| node.has_tag_name((MIME_NS, "mime-type")))
            {
                block.push_str(&format!(
                    "\n        <mime:mime-type type=\"{}\"/>",
                    mime(path)
                ));
            }
            block.push_str(&applications);
            append_before_close(edits, metadata, source, &block);
        }
        None => {
            let block = format!(
                "\n    <info>\n      <metadata owner=\"{OWNER}\">\n        <mime:mime-type type=\"{}\"/>{applications}</metadata>\n    </info>\n  ",
                mime(path)
            );
            append_before_close(edits, bookmark, source, &block);
        }
    }
}

/// Puts `children` at the end of `node`'s content, giving a self-closing
/// element its closing tag first.
fn append_before_close(
    edits: &mut Vec<Edit>,
    node: roxmltree::Node<'_, '_>,
    source: &str,
    children: &str,
) {
    let end = start_tag_end(node, source);
    let range = node.range();
    if source[range.start..end].ends_with("/>") {
        let name = element_name(node, source);
        edits.push(Edit {
            start: end - 2,
            end: end - 1,
            text: String::new(),
        });
        edits.push(Edit {
            start: range.end,
            end: range.end,
            text: format!("{children}</{name}>"),
        });
    } else {
        let at = before_end_tag(node, source);
        edits.push(Edit {
            start: at,
            end: at,
            text: children.to_owned(),
        });
    }
}

/// Where `node`'s start tag ends in `source` (just past its `>`).
fn start_tag_end(node: roxmltree::Node<'_, '_>, source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut quote = 0;
    for (i, byte) in bytes.iter().enumerate().skip(node.range().start) {
        match *byte {
            b'\'' | b'"' if quote == 0 => quote = *byte,
            byte if byte == quote => quote = 0,
            b'>' if quote == 0 => return i + 1,
            _ => {}
        }
    }
    source.len()
}

/// The element's name as written, prefix and all.
fn element_name<'a>(node: roxmltree::Node<'_, '_>, source: &'a str) -> &'a str {
    let rest = &source[node.range().start + 1..];
    let end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// One replacement in the source text; `start == end` inserts.
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// `source` with `edits` applied, last first so earlier offsets stay good.
fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut out = source.to_owned();
    for edit in edits {
        out.replace_range(edit.start..edit.end, &edit.text);
    }
    out
}

/// Replaces `path` with `text` through a temporary file in its directory, so
/// a reader never sees a half-written list.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;

    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    // `tempfile` makes the file readable by its owner alone before any of
    // the list is written; a plain create would use the umask first.
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The list is the user's alone, as GTK leaves it.
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o600);
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use url::Url;

    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap()
    }

    fn a_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"document").unwrap();
        path
    }

    fn recents(dir: &Path) -> Recents {
        Recents::at(dir.join("recently-used.xbel"))
    }

    fn text(recents: &Recents) -> String {
        fs::read_to_string(&recents.path).unwrap()
    }

    fn parse(text: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(text).unwrap()
    }

    fn href(path: &Path) -> String {
        let path = fs::canonicalize(path).unwrap();
        Url::from_file_path(path).unwrap().to_string()
    }

    fn bookmarks<'a, 'i>(doc: &'a roxmltree::Document<'i>) -> Vec<roxmltree::Node<'a, 'i>> {
        doc.root_element()
            .children()
            .filter(|node| node.has_tag_name("bookmark"))
            .collect()
    }

    const OLD: &str = "2020-01-02T03:04:05.000006Z";

    fn xbel(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0"
      xmlns:bookmark="{BOOKMARK_NS}"
      xmlns:mime="{MIME_NS}"
>
{body}</xbel>
"#
        )
    }

    /// One application entry, as GTK writes it.
    fn app(name: &str, exec: &str, count: u32, modified: &str) -> String {
        format!(
            "          <bookmark:application name=\"{name}\" exec=\"{exec}\" modified=\"{modified}\" count=\"{count}\"/>\n"
        )
    }

    /// A bookmark with `apps` under metadata, all its times in 2020.
    fn bookmark(href: &str, apps: &str) -> String {
        format!(
            r#"  <bookmark href="{href}" added="{OLD}" modified="{OLD}" visited="{OLD}">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="application/pdf"/>
        <bookmark:applications>
{apps}        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
"#
        )
    }

    #[test]
    fn a_new_file_starts_the_list() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());

        recents.note(&file).unwrap();

        let text = text(&recents);
        assert!(
            text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"")
        );
        let doc = parse(&text);
        assert_eq!(doc.root_element().tag_name().name(), "xbel");
        assert_eq!(doc.root_element().attribute("version"), Some("1.0"));
        let books = bookmarks(&doc);
        assert_eq!(books.len(), 1);
        let bookmark = books[0];
        let uri = href(&file);
        assert_eq!(bookmark.attribute("href"), Some(uri.as_str()));
        let added = bookmark.attribute("added").unwrap();
        assert_eq!(bookmark.attribute("modified"), Some(added));
        assert_eq!(bookmark.attribute("visited"), Some(added));
        assert!(added.ends_with('Z'), "{added} is not UTC");
        let metadata = bookmark
            .descendants()
            .find(|node| node.has_tag_name("metadata"))
            .unwrap();
        assert_eq!(metadata.attribute("owner"), Some("http://freedesktop.org"));
        let mime = bookmark
            .descendants()
            .find(|node| node.has_tag_name((super::MIME_NS, "mime-type")))
            .unwrap();
        assert_eq!(mime.attribute("type"), Some("application/pdf"));
        let apps: Vec<_> = bookmark
            .descendants()
            .filter(|node| node.has_tag_name((super::BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("name"), Some("noido"));
        assert_eq!(apps[0].attribute("exec"), Some("'noido %u'"));
        assert_eq!(apps[0].attribute("count"), Some("1"));
        assert_eq!(apps[0].attribute("modified"), Some(added));
    }

    #[test]
    fn a_second_open_bumps_the_count() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let noido = app("noido", "'noido %u'", 1, OLD);
        fs::write(&recents.path, xbel(&bookmark(&uri, &noido))).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        assert_eq!(text.matches(&format!("href=\"{uri}\"")).count(), 1);
        let doc = parse(&text);
        let books = bookmarks(&doc);
        assert_eq!(books.len(), 1);
        let bookmark = books[0];
        assert_eq!(bookmark.attribute("added"), Some(OLD));
        assert_eq!(bookmark.attribute("visited"), Some(OLD));
        assert_ne!(bookmark.attribute("modified"), Some(OLD));
        let apps: Vec<_> = bookmark
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("count"), Some("2"));
        assert_ne!(apps[0].attribute("modified"), Some(OLD));
    }

    #[test]
    fn a_count_that_cannot_grow_stays_at_its_maximum() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let full = app("noido", "'noido %u'", u32::MAX, OLD);
        fs::write(&recents.path, xbel(&bookmark(&uri, &full))).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        let apps: Vec<_> = bookmarks(&doc)[0]
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps[0].attribute("count"), Some("4294967295"));
    }

    #[test]
    fn metadata_without_applications_gains_them() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let body = format!(
            r#"  <bookmark href="{uri}" added="{OLD}" modified="{OLD}" visited="{OLD}">
    <info>
      <metadata owner="http://freedesktop.org"/>
    </info>
  </bookmark>
"#
        );
        fs::write(&recents.path, xbel(&body)).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        let bookmark = bookmarks(&doc)[0];
        assert_eq!(
            bookmark
                .descendants()
                .filter(|node| node.has_tag_name("metadata"))
                .count(),
            1,
            "the metadata was duplicated"
        );
        let mime = bookmark
            .descendants()
            .find(|node| node.has_tag_name((MIME_NS, "mime-type")))
            .unwrap();
        assert_eq!(mime.attribute("type"), Some("application/pdf"));
        let apps: Vec<_> = bookmark
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("name"), Some("noido"));
    }

    #[test]
    fn an_application_without_its_attributes_gains_them() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let bare = "          <bookmark:application name=\"noido\"/>\n";
        fs::write(&recents.path, xbel(&bookmark(&uri, bare))).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        let apps: Vec<_> = bookmarks(&doc)[0]
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("count"), Some("2"));
        assert_eq!(apps[0].attribute("exec"), Some("'noido %u'"));
        assert!(apps[0].attribute("modified").is_some());
    }

    #[test]
    fn a_self_closing_applications_list_gains_an_entry() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let body = format!(
            r#"  <bookmark href="{uri}" added="{OLD}" modified="{OLD}" visited="{OLD}">
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="application/pdf"/>
        <bookmark:applications/>
      </metadata>
    </info>
  </bookmark>
"#
        );
        fs::write(&recents.path, xbel(&body)).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        let apps: Vec<_> = bookmarks(&doc)[0]
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("name"), Some("noido"));
    }

    #[cfg(unix)]
    #[test]
    fn the_list_is_private_and_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());

        recents.note(&file).unwrap();
        let mode = fs::metadata(&recents.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a new list is readable by others");

        fs::set_permissions(&recents.path, fs::Permissions::from_mode(0o640)).unwrap();
        recents.note(&file).unwrap();
        let mode = fs::metadata(&recents.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640, "the list's mode was changed");
    }

    #[test]
    fn another_applications_entry_is_kept() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let evince = app(
            "Document Viewer",
            "'evince %u'",
            4,
            "2019-05-06T07:08:09.000000Z",
        );
        let body = format!(
            r#"  <bookmark href="{uri}" added="{OLD}" modified="{OLD}" visited="{OLD}">
    <title>A paper</title>
    <info>
      <metadata owner="http://freedesktop.org">
        <mime:mime-type type="application/pdf"/>
        <bookmark:groups>
          <bookmark:group>Office</bookmark:group>
        </bookmark:groups>
        <bookmark:applications>
{evince}        </bookmark:applications>
      </metadata>
    </info>
  </bookmark>
"#
        );
        fs::write(&recents.path, xbel(&body)).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        assert!(
            text.contains(&evince),
            "another application's entry changed:\n{text}"
        );
        assert!(text.contains("<title>A paper</title>"));
        assert!(text.contains("<bookmark:group>Office</bookmark:group>"));
        let doc = parse(&text);
        let bookmark = bookmarks(&doc)[0];
        assert_eq!(bookmark.attribute("added"), Some(OLD));
        assert_eq!(bookmark.attribute("visited"), Some(OLD));
        let mime = bookmark
            .descendants()
            .find(|node| node.has_tag_name((MIME_NS, "mime-type")))
            .unwrap();
        assert_eq!(mime.attribute("type"), Some("application/pdf"));
        let apps: Vec<_> = bookmark
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 2);
        let evince = apps
            .iter()
            .find(|node| node.attribute("name") == Some("Document Viewer"))
            .unwrap();
        assert_eq!(evince.attribute("count"), Some("4"));
        assert_eq!(
            evince.attribute("modified"),
            Some("2019-05-06T07:08:09.000000Z")
        );
        let ours = apps
            .iter()
            .find(|node| node.attribute("name") == Some("noido"))
            .unwrap();
        assert_eq!(ours.attribute("count"), Some("1"));
    }

    #[test]
    fn a_bookmark_without_metadata_gains_it() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        let body = format!(
            "  <bookmark href=\"{uri}\" added=\"{OLD}\" modified=\"{OLD}\" visited=\"{OLD}\"/>\n"
        );
        fs::write(&recents.path, xbel(&body)).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        let bookmark = bookmarks(&doc)[0];
        assert_eq!(bookmark.attribute("added"), Some(OLD));
        assert_ne!(bookmark.attribute("modified"), Some(OLD));
        let mime = bookmark
            .descendants()
            .find(|node| node.has_tag_name((MIME_NS, "mime-type")))
            .unwrap();
        assert_eq!(mime.attribute("type"), Some("application/pdf"));
        let apps: Vec<_> = bookmark
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("name"), Some("noido"));
    }

    #[test]
    fn clearing_removes_only_noido() {
        let dir = temp();
        let shared = a_file(dir.path(), "shared.pdf");
        let ours = a_file(dir.path(), "ours.pdf");
        let theirs = a_file(dir.path(), "theirs.pdf");
        let recents = recents(dir.path());
        let evince = app(
            "Document Viewer",
            "'evince %u'",
            4,
            "2019-05-06T07:08:09.000000Z",
        );
        let noido = app("noido", "'noido %u'", 2, OLD);
        let body = [
            bookmark(&href(&shared), &format!("{noido}{evince}")),
            bookmark(&href(&ours), &noido),
            bookmark(&href(&theirs), &evince),
        ]
        .concat();
        fs::write(&recents.path, xbel(&body)).unwrap();

        recents.clear().unwrap();

        let text = text(&recents);
        assert!(!text.contains("name=\"noido\""), "{text}");
        let doc = parse(&text);
        let books = bookmarks(&doc);
        assert_eq!(books.len(), 2);
        let (shared_uri, theirs_uri, ours_uri) = (href(&shared), href(&theirs), href(&ours));
        let kept: Vec<_> = books
            .iter()
            .map(|bookmark| bookmark.attribute("href").unwrap())
            .collect();
        assert!(kept.contains(&shared_uri.as_str()));
        assert!(kept.contains(&theirs_uri.as_str()));
        assert!(!kept.contains(&ours_uri.as_str()));
        assert!(
            text.contains(&evince),
            "the other application's entry changed"
        );
        let shared_book = books
            .iter()
            .find(|bookmark| bookmark.attribute("href") == Some(shared_uri.as_str()))
            .unwrap();
        assert_ne!(shared_book.attribute("modified"), Some(OLD));
        let apps: Vec<_> = shared_book
            .descendants()
            .filter(|node| node.has_tag_name((BOOKMARK_NS, "application")))
            .collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].attribute("name"), Some("Document Viewer"));
        let theirs_book = books
            .iter()
            .find(|bookmark| bookmark.attribute("href") == Some(theirs_uri.as_str()))
            .unwrap();
        assert_eq!(theirs_book.attribute("modified"), Some(OLD));
    }

    #[test]
    fn paths_become_escaped_uris() {
        let dir = temp();
        let file = a_file(dir.path(), "a b&c#d%e-å.pdf");
        let recents = recents(dir.path());

        recents.note(&file).unwrap();

        let text = text(&recents);
        let uri = href(&file);
        assert!(uri.contains("%20"), "{uri}");
        assert!(
            text.contains("&amp;"),
            "the URI's ampersand was not escaped:\n{text}"
        );
        let doc = parse(&text);
        let bookmark = bookmarks(&doc)[0];
        assert_eq!(bookmark.attribute("href"), Some(uri.as_str()));
        let mime = bookmark
            .descendants()
            .find(|node| node.has_tag_name((MIME_NS, "mime-type")))
            .unwrap();
        assert_eq!(mime.attribute("type"), Some("application/pdf"));
    }

    #[test]
    fn a_new_bookmark_joins_the_list() {
        let dir = temp();
        let old = a_file(dir.path(), "old.pdf");
        let new = a_file(dir.path(), "new.pdf");
        let recents = recents(dir.path());
        let old_book = bookmark(&href(&old), &app("Document Viewer", "'evince %u'", 1, OLD));
        fs::write(&recents.path, xbel(&old_book)).unwrap();

        recents.note(&new).unwrap();

        let text = text(&recents);
        assert!(text.contains(&old_book), "the old bookmark changed");
        let doc = parse(&text);
        let books = bookmarks(&doc);
        assert_eq!(books.len(), 2);
        let new_uri = href(&new);
        assert!(
            books
                .iter()
                .any(|bookmark| bookmark.attribute("href") == Some(new_uri.as_str()))
        );
    }

    #[test]
    fn an_empty_list_gains_its_first_bookmark() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let empty = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\" xmlns:bookmark=\"{BOOKMARK_NS}\" xmlns:mime=\"{MIME_NS}\"/>\n"
        );
        fs::write(&recents.path, empty).unwrap();

        recents.note(&file).unwrap();

        let text = text(&recents);
        let doc = parse(&text);
        assert_eq!(bookmarks(&doc).len(), 1);
    }

    #[test]
    fn a_file_that_is_not_the_list_is_left_alone() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        for contents in ["not a list", "<other/>"] {
            fs::write(&recents.path, contents).unwrap();
            assert!(recents.note(&file).is_err(), "{contents:?} was accepted");
            assert!(recents.clear().is_err(), "{contents:?} was cleared");
            assert_eq!(fs::read_to_string(&recents.path).unwrap(), contents);
        }
    }

    #[test]
    fn a_list_without_the_namespaces_is_left_alone() {
        let dir = temp();
        let file = a_file(dir.path(), "paper.pdf");
        let recents = recents(dir.path());
        let uri = href(&file);
        // Valid XBEL, but without the two prefixes our elements use: writing
        // them would make the file unreadable for everyone.
        let contents = format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n  <bookmark href=\"{uri}\" added=\"{OLD}\" modified=\"{OLD}\" visited=\"{OLD}\"/>\n</xbel>\n"
        );
        fs::write(&recents.path, &contents).unwrap();

        assert!(recents.note(&file).is_err());

        assert_eq!(text(&recents), contents);
    }

    #[test]
    fn clearing_a_missing_list_is_fine() {
        let dir = temp();
        let recents = recents(dir.path());

        recents.clear().unwrap();

        assert!(!recents.path.exists());
    }

    #[test]
    fn timestamps_are_utc_to_the_microsecond() {
        use std::time::{Duration, UNIX_EPOCH};
        let at = |seconds: u64, micros: u32| UNIX_EPOCH + Duration::new(seconds, micros * 1000);
        assert_eq!(timestamp(at(0, 0)), "1970-01-01T00:00:00.000000Z");
        assert_eq!(
            timestamp(at(951_782_400, 123_456)),
            "2000-02-29T00:00:00.123456Z"
        );
    }
}
