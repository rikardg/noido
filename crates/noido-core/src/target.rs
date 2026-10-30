//! What noido is asked to open: a path, or a `file:` or `noido:` link to a
//! page or a named place (RFC 8118 fragments, `#page=12`).

use std::ffi::OsStr;
use std::path::PathBuf;

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
        return Ok(plain());
    };
    let not_local = || format!("{text}: not a local file");
    // A noido: link is a file: link with the scheme swapped.
    let file = format!("file://{}", &text[scheme.len()..]);
    let url = Url::parse(&file).map_err(|_| not_local())?;
    let path = url.to_file_path().map_err(|()| not_local())?;
    Ok(Opening {
        path,
        target: url.fragment().and_then(parse_fragment),
    })
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
}
