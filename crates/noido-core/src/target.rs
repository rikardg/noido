//! What noido is asked to open: a path, or a `file:` or `noido:` link to a
//! page or a named place (RFC 8118 fragments, `#page=12`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use url::Url;

/// Where in the document to arrive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A page, from 0.
    Page(usize),
    /// A named destination.
    Named(String),
}

/// A document to open, and where in it.
#[derive(Debug, PartialEq)]
pub struct Opening {
    pub path: PathBuf,
    pub target: Option<Target>,
}

/// The schemes read as links; anything else is a plain path.
const SCHEMES: [&str; 2] = ["file://", "noido://"];

/// What is encoded in a fragment value: enough that it parses back.
const FRAGMENT_VALUE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'=')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// Reads the command line's document argument: a `file://` or `noido://`
/// link, or else a plain path taken as is.
pub fn parse_argument(arg: &OsStr) -> Result<Opening, String> {
    let plain = || Opening {
        path: PathBuf::from(arg),
        target: None,
    };
    let Some(text) = arg.to_str() else {
        return Ok(plain());
    };
    let Some(scheme) = SCHEMES.iter().find(|scheme| {
        text.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    }) else {
        if other_scheme(text) {
            return Err(format!("{text}: only local files can be opened"));
        }
        return Ok(plain());
    };
    let not_local = || format!("{text}: not a local file");
    // A noido: link is a file: link with the scheme swapped.
    let file = format!("file://{}", &text[scheme.len()..]);
    let url = Url::parse(&file).map_err(|_| not_local())?;
    if url.query().is_some() {
        return Err(format!(
            "{text}: has a ?…, which a file link can't; encode ? in the file name as %3F"
        ));
    }
    let path = url.to_file_path().map_err(|()| not_local())?;
    Ok(Opening {
        path,
        target: url.fragment().and_then(parse_fragment),
    })
}

/// Whether `text` starts with a URI scheme and `//` (RFC 3986: a letter,
/// then letters, digits, `+`, `-` or `.`): a link, as file names hardly
/// ever are.
fn other_scheme(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    rest.starts_with("//")
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The target in an RFC 8118 fragment (without the `#`); `None` when it
/// names none that noido understands. Parameters are taken left to right;
/// a later one replaces an earlier one, and ones not understood are skipped.
pub fn parse_fragment(fragment: &str) -> Option<Target> {
    // The last parameter that parses wins: look from the end.
    fragment.rsplit(['&', '#']).find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        let value = percent_decode_str(value).decode_utf8_lossy();
        match name {
            "page" => match value.parse::<usize>() {
                Ok(page @ 1..) => Some(Target::Page(page - 1)),
                _ => None,
            },
            "nameddest" if !value.is_empty() => Some(Target::Named(value.into_owned())),
            _ => None,
        }
    })
}

/// `target` as a fragment, without the `#`.
pub fn to_fragment(target: &Target) -> String {
    match target {
        Target::Page(page) => format!("page={}", page + 1),
        Target::Named(name) => format!("nameddest={}", utf8_percent_encode(name, FRAGMENT_VALUE)),
    }
}

/// The `noido:` link to `page` (from 0) of the document at `canonical`;
/// `None` for a relative path.
pub fn page_link(canonical: &Path, page: usize) -> Option<String> {
    let file = Url::from_file_path(canonical).ok()?;
    // Encoded too, so the link survives as a Markdown link's target.
    let file = file.as_str().replace('(', "%28").replace(')', "%29");
    let rest = file.strip_prefix("file:")?;
    Some(format!("noido:{rest}#{}", to_fragment(&Target::Page(page))))
}

/// [`page_link`] as a Markdown link labelled with `name` (the file name)
/// and the page: `[Some Book, p. 51](noido:///…#page=51)`.
pub fn markdown_page_link(canonical: &Path, name: &str, page: usize) -> Option<String> {
    let link = page_link(canonical, page)?;
    let title = match name.len().checked_sub(4) {
        Some(end) if name.is_char_boundary(end) && name[end..].eq_ignore_ascii_case(".pdf") => {
            &name[..end]
        }
        _ => name,
    };
    let mut label = String::new();
    for c in title.chars() {
        if matches!(c, '\\' | '[' | ']') {
            label.push('\\');
        }
        label.push(c);
    }
    Some(format!("[{label}, p. {}]({link})", page + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arg: &str) -> Result<Opening, String> {
        parse_argument(OsStr::new(arg))
    }

    fn opening(path: &str, target: Option<Target>) -> Opening {
        Opening {
            path: PathBuf::from(path),
            target,
        }
    }

    #[test]
    fn plain_path_is_taken_as_is() {
        for arg in [
            "dir/a#page=3.pdf",
            "noido:x.pdf",
            "file:/abs.pdf",
            "a%20b.pdf",
        ] {
            assert_eq!(parse(arg), Ok(opening(arg, None)), "{arg}");
        }
    }

    #[test]
    fn file_uri_with_page() {
        assert_eq!(
            parse("file:///home/u/my%20doc.pdf#page=3"),
            Ok(opening("/home/u/my doc.pdf", Some(Target::Page(2))))
        );
    }

    #[test]
    fn file_uri_without_fragment() {
        assert_eq!(
            parse("file:///home/u/x.pdf"),
            Ok(opening("/home/u/x.pdf", None))
        );
    }

    #[test]
    fn noido_uri_is_the_same() {
        let expected = Ok(opening("/home/u/x.pdf", Some(Target::Page(2))));
        assert_eq!(parse("noido:///home/u/x.pdf#page=3"), expected);
        assert_eq!(parse("NOIDO:///home/u/x.pdf#page=3"), expected);
        assert_eq!(parse("File:///home/u/x.pdf#page=3"), expected);
    }

    #[test]
    fn localhost_is_local() {
        assert_eq!(
            parse("file://localhost/home/u/x.pdf"),
            Ok(opening("/home/u/x.pdf", None))
        );
    }

    #[test]
    fn other_schemes_are_an_error() {
        for arg in [
            "sftp://host/x.pdf",
            "smb://server/share/x.pdf",
            "HTTPS://a/b.pdf",
        ] {
            let error = parse(arg).unwrap_err();
            assert!(error.contains("only local files"), "{arg}: {error}");
        }
    }

    #[test]
    fn things_that_only_look_like_schemes_are_paths() {
        for arg in [
            "C:\\x.pdf",
            "a:b.pdf",
            "1x://y.pdf",
            "a b://c.pdf",
            "://x.pdf",
        ] {
            assert_eq!(parse(arg), Ok(opening(arg, None)), "{arg}");
        }
    }

    #[test]
    fn a_link_with_a_query_is_an_error() {
        for arg in ["file:///a?b.pdf", "noido:///a?b.pdf#page=2"] {
            let error = parse(arg).unwrap_err();
            assert!(error.contains("%3F"), "{arg}: {error}");
        }
    }

    #[test]
    fn other_host_is_an_error() {
        let error = parse("file://server/x.pdf").unwrap_err();
        assert!(error.contains("not a local file"), "{error}");
    }

    #[test]
    fn fragment_rules() {
        use Target::{Named, Page};
        let cases: [(&str, Option<Target>); 11] = [
            ("page=3", Some(Page(2))),
            ("nameddest=sec%202", Some(Named("sec 2".into()))),
            ("page=3&nameddest=a", Some(Named("a".into()))),
            ("nameddest=a#page=4", Some(Page(3))),
            ("zoom=200&page=2", Some(Page(1))),
            ("page=3&page=0", Some(Page(2))),
            ("page=0", None),
            ("page=x", None),
            ("", None),
            ("zoom=50", None),
            ("nameddest=", None),
        ];
        for (fragment, expected) in cases {
            assert_eq!(parse_fragment(fragment), expected, "{fragment}");
        }
    }

    #[test]
    fn fragment_round_trip() {
        for target in [
            Target::Page(0),
            Target::Page(51),
            Target::Named("a b&c#d%=e".into()),
            Target::Named("Smörgås".into()),
        ] {
            assert_eq!(parse_fragment(&to_fragment(&target)), Some(target.clone()));
        }
        assert_eq!(to_fragment(&Target::Page(51)), "page=52");
    }

    #[test]
    fn page_link_form() {
        assert_eq!(
            page_link(Path::new("/home/u/books/Some Book.pdf"), 50).as_deref(),
            Some("noido:///home/u/books/Some%20Book.pdf#page=51")
        );
    }

    #[test]
    fn parentheses_are_encoded() {
        let link = page_link(Path::new("/b/x (2).pdf"), 0).unwrap();
        assert!(link.contains("x%20%282%29.pdf"), "{link}");
        assert!(!link.contains(['(', ')']), "{link}");
    }

    #[test]
    fn copied_links_open_the_same_file() {
        for path in [
            "/b/a#b.pdf",
            "/b/100%.pdf",
            "/b/q?.pdf",
            "/b/(x).pdf",
            "/b/Smörgås ö.pdf",
            "/b/a&b=c.pdf",
            "/b/a%20b.pdf",
        ] {
            let link = page_link(Path::new(path), 4).unwrap();
            assert_eq!(
                parse(&link),
                Ok(opening(path, Some(Target::Page(4)))),
                "{path} → {link}"
            );
        }
    }

    #[test]
    fn relative_path_has_no_link() {
        assert_eq!(page_link(Path::new("x.pdf"), 0), None);
    }

    #[test]
    fn markdown_form() {
        let book = Path::new("/b/Some Book.pdf");
        assert_eq!(
            markdown_page_link(book, "Some Book.pdf", 50).as_deref(),
            Some("[Some Book, p. 51](noido:///b/Some%20Book.pdf#page=51)")
        );
        let label = |name| {
            let link = markdown_page_link(book, name, 0).unwrap();
            link[..link.find("](").unwrap() + 1].to_owned()
        };
        assert_eq!(label("Notes.PDF"), "[Notes, p. 1]");
        assert_eq!(label("a.pdf.txt"), "[a.pdf.txt, p. 1]");
        assert_eq!(label("x [draft] \\ y.pdf"), "[x \\[draft\\] \\\\ y, p. 1]");
    }
}
