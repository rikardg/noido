//! Page labels: the number or text a PDF declares for each page, read from
//! the catalog's `/PageLabels` number tree.

use std::collections::HashSet;

use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::{
    Array, Dict, MaybeRef, Name, Number, Object, ObjectIdentifier, String as PdfString,
};

use crate::pdf_string::decode_untrimmed;

/// Number trees deeper than this are treated as broken.
const MAX_TREE_DEPTH: usize = 32;
/// The longest label prefix kept, in characters.
const MAX_PREFIX_CHARS: usize = 256;
/// The largest number standard roman numerals write.
const MAX_ROMAN: u64 = 3999;
/// Letter labels repeating more than this show as decimal: a huge `/St`
/// would otherwise make a huge string.
const MAX_LETTER_REPEATS: u64 = 16;

/// One label per page, `None` for a page without one; empty when no page
/// has one.
pub(crate) fn page_labels(pdf: &Pdf) -> Vec<Option<String>> {
    let Some(tree) = pdf
        .xref()
        .get::<Dict>(pdf.xref().root_id())
        .and_then(|catalog| catalog.get::<Dict>(b"PageLabels"))
    else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut visited: HashSet<ObjectIdentifier> = tree.obj_id().into_iter().collect();
    collect(pdf, &tree, 0, &mut visited, &mut ranges);
    ranges.sort_by_key(|(first, _)| *first);

    let count = pdf.pages().len();
    let mut labels = vec![None; count];
    for (i, (first, range)) in ranges.iter().enumerate() {
        let end = ranges
            .get(i + 1)
            .map_or(count, |(next, _)| *next)
            .min(count);
        for (page, slot) in labels.iter_mut().enumerate().take(end).skip(*first) {
            *slot = label(range, (page - first) as u64);
        }
    }
    if labels.iter().all(Option::is_none) {
        return Vec::new();
    }
    labels
}

/// Gathers the `(first page, range)` pairs under `node`. Each node is visited
/// once, so shared nodes or cycles can't make it run long.
fn collect<'a>(
    pdf: &'a Pdf,
    node: &Dict<'a>,
    depth: usize,
    visited: &mut HashSet<ObjectIdentifier>,
    ranges: &mut Vec<(usize, Dict<'a>)>,
) {
    if depth > MAX_TREE_DEPTH {
        return;
    }
    if let Some(nums) = node.get::<Array>(b"Nums") {
        let mut entries = nums.iter::<Object>();
        while let (Some(key), Some(value)) = (entries.next(), entries.next()) {
            if let (Object::Number(key), Object::Dict(range)) = (key, value)
                && let Ok(first) = usize::try_from(key.as_i64())
            {
                ranges.push((first, range));
            }
        }
    }
    let Some(kids) = node.get::<Array>(b"Kids") else {
        return;
    };
    for kid in kids.raw_iter() {
        let kid = match kid {
            MaybeRef::Ref(r) => {
                let id = ObjectIdentifier::from(r);
                if !visited.insert(id) {
                    continue;
                }
                pdf.xref().get::<Dict>(id)
            }
            MaybeRef::NotRef(Object::Dict(dict)) => Some(dict),
            MaybeRef::NotRef(_) => None,
        };
        if let Some(kid) = kid {
            collect(pdf, &kid, depth + 1, visited, ranges);
        }
    }
}

/// `prefix` cut to `MAX_PREFIX_CHARS` characters.
fn capped(mut prefix: String) -> String {
    if let Some((end, _)) = prefix.char_indices().nth(MAX_PREFIX_CHARS) {
        prefix.truncate(end);
    }
    prefix
}

/// The label of the page `offset` pages into `range`.
fn label(range: &Dict<'_>, offset: u64) -> Option<String> {
    let mut text = range
        .get::<PdfString>(b"P")
        .map(|prefix| capped(decode_untrimmed(prefix.as_bytes())))
        .unwrap_or_default();
    if let Some(style) = range.get::<Name>(b"S") {
        let start = range
            .get::<Number>(b"St")
            .map_or(1, |start| start.as_i64().max(1)) as u64;
        let number = start.saturating_add(offset);
        match &*style {
            b"D" => text.push_str(&number.to_string()),
            b"r" => text.push_str(&roman(number)),
            b"R" => text.push_str(&roman(number).to_uppercase()),
            b"a" => text.push_str(&letters(number)),
            b"A" => text.push_str(&letters(number).to_uppercase()),
            _ => {}
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// `number` in lower-case roman numerals, or decimal past what they write.
fn roman(number: u64) -> String {
    const DIGITS: [(u64, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    if number > MAX_ROMAN {
        return number.to_string();
    }
    let mut left = number;
    let mut text = String::new();
    for (value, digit) in DIGITS {
        while left >= value {
            text.push_str(digit);
            left -= value;
        }
    }
    text
}

/// `number` in lower-case letters: a…z, aa, bb, … zz, aaa, by the PDF spec;
/// decimal when it would repeat too often.
fn letters(number: u64) -> String {
    let repeats = (number - 1) / 26 + 1;
    if repeats > MAX_LETTER_REPEATS {
        return number.to_string();
    }
    let letter = char::from(b'a' + ((number - 1) % 26) as u8);
    std::iter::repeat_n(letter, repeats as usize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_prefixes_are_cut_on_a_char_boundary() {
        let long = "é".repeat(MAX_PREFIX_CHARS + 50);
        let cut = capped(long);
        assert_eq!(cut.chars().count(), MAX_PREFIX_CHARS);
        assert!(cut.chars().all(|c| c == 'é'));
        assert_eq!(capped("A-".to_owned()), "A-");
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(14), "xiv");
        assert_eq!(roman(3999), "mmmcmxcix");
        assert_eq!(roman(4000), "4000");
    }

    #[test]
    fn letter_labels() {
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa");
        assert_eq!(letters(28), "bb");
        assert_eq!(letters(53), "aaa");
    }

    #[test]
    fn letters_past_the_repeat_limit_are_decimal() {
        assert_eq!(letters(26 * 16), "z".repeat(16));
        assert_eq!(letters(26 * 16 + 1), "417");
        assert_eq!(letters(u64::MAX), u64::MAX.to_string());
    }
}
