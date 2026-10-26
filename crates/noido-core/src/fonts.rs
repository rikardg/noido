//! Font names as people know them: "ABCDEF+MinionPro-It" is Minion Pro,
//! Italic.

use noido_engine::Font;

/// A typeface and the styles of it a document uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    /// Lightest first, upright before slanted.
    pub styles: Vec<Style>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    pub name: String,
    /// Whether every font of this style is in the document.
    pub embedded: bool,
    /// The names the document gives it, without subset prefixes.
    pub raw: Vec<String>,
}

/// `fonts` grouped into families, by name.
pub fn families(fonts: &[Font]) -> Vec<Family> {
    let mut families: Vec<Family> = Vec::new();
    for font in fonts {
        let raw = without_subset(&font.name);
        let (family, style) = family_and_style(raw);
        let index = match families.iter().position(|f| f.name == family) {
            Some(i) => i,
            None => {
                families.push(Family {
                    name: family,
                    styles: Vec::new(),
                });
                families.len() - 1
            }
        };
        let styles = &mut families[index].styles;
        match styles.iter_mut().find(|s| s.name == style) {
            Some(existing) => {
                existing.embedded &= font.embedded;
                if !existing.raw.iter().any(|r| r == raw) {
                    existing.raw.push(raw.to_owned());
                }
            }
            None => styles.push(Style {
                name: style,
                embedded: font.embedded,
                raw: vec![raw.to_owned()],
            }),
        }
    }
    families
        .sort_by(|a, b| (a.name.to_lowercase(), &a.name).cmp(&(b.name.to_lowercase(), &b.name)));
    for family in &mut families {
        family
            .styles
            .sort_by_cached_key(|s| (weight(&s.name), slanted(&s.name), s.name.clone()));
    }
    families
}

/// A font name's family and style: ("Minion Pro", "Italic").
pub fn family_and_style(name: &str) -> (String, String) {
    let name = without_subset(name);
    if let Some((family, style)) = tex_name(name) {
        return (family.to_owned(), style.to_owned());
    }
    let (family, style) = match name.find(['-', ',']) {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    };
    let mut family_words = words(without_vendor_suffix(family));
    let mut style_words: Vec<&str> = Vec::new();
    if style.is_empty() {
        // "ArialBold": style words at the end, but never the whole name,
        // and never "Roman" ("TimesNewRoman").
        while family_words.len() > 1
            && style_word(family_words.last().unwrap()).is_some_and(|w| w != "Regular")
        {
            style_words.insert(0, family_words.pop().unwrap());
        }
    } else {
        let style = style.split('_').next().unwrap_or_default();
        style_words = style
            .split(['-', ','])
            .flat_map(|part| words(without_vendor_suffix(part)))
            .collect();
    }
    let family = if family_words.is_empty() {
        name.to_owned()
    } else {
        family_words.join(" ")
    };
    (family, style_name(&style_words))
}

/// Classic TeX fonts, by the abbreviation their names start with; a
/// design size follows ("CMR9"), dropped here.
const TEX_FONTS: &[(&str, &str, &str)] = &[
    ("CMR", "Computer Modern", "Roman"),
    ("CMB", "Computer Modern", "Bold"),
    ("CMBX", "Computer Modern", "Bold Extended"),
    ("CMBXTI", "Computer Modern", "Bold Extended Italic"),
    ("CMBXSL", "Computer Modern", "Bold Extended Slanted"),
    ("CMTI", "Computer Modern", "Italic"),
    ("CMSL", "Computer Modern", "Slanted"),
    ("CMU", "Computer Modern", "Unslanted Italic"),
    ("CMCSC", "Computer Modern", "Caps and Small Caps"),
    ("CMMI", "Computer Modern", "Math Italic"),
    ("CMMIB", "Computer Modern", "Math Bold Italic"),
    ("CMSY", "Computer Modern", "Symbol"),
    ("CMBSY", "Computer Modern", "Bold Symbol"),
    ("CMEX", "Computer Modern", "Math Extension"),
    ("CMTT", "Computer Modern", "Typewriter"),
    ("CMSLTT", "Computer Modern", "Slanted Typewriter"),
    ("CMITT", "Computer Modern", "Italic Typewriter"),
    (
        "CMTCSC",
        "Computer Modern",
        "Typewriter Caps and Small Caps",
    ),
    ("CMVTT", "Computer Modern", "Variable-width Typewriter"),
    ("CMTEX", "Computer Modern", "TeX Extended ASCII"),
    ("CMSS", "Computer Modern", "Sans Serif"),
    ("CMSSBX", "Computer Modern", "Sans Serif Bold Extended"),
    ("CMSSI", "Computer Modern", "Sans Serif Italic"),
    ("CMSSDC", "Computer Modern", "Sans Serif Demibold Condensed"),
    ("CMSSQ", "Computer Modern", "Sans Serif Quotation"),
    ("CMSSQI", "Computer Modern", "Sans Serif Quotation Italic"),
    ("CMDUNH", "Computer Modern", "Dunhill"),
    ("CMFF", "Computer Modern", "Funny"),
    ("CMFI", "Computer Modern", "Funny Italic"),
    ("CMFIB", "Computer Modern", "Fibonacci"),
    ("CMINCH", "Computer Modern", "Inch"),
    // CM-Super, Computer Modern in T1 encoding: "SFRM1000".
    ("SFRM", "Computer Modern", "Roman"),
    ("SFBX", "Computer Modern", "Bold Extended"),
    ("SFBI", "Computer Modern", "Bold Extended Italic"),
    ("SFTI", "Computer Modern", "Italic"),
    ("SFSL", "Computer Modern", "Slanted"),
    ("SFCC", "Computer Modern", "Caps and Small Caps"),
    ("SFTT", "Computer Modern", "Typewriter"),
    ("SFSS", "Computer Modern", "Sans Serif"),
    ("SFSX", "Computer Modern", "Sans Serif Bold Extended"),
    ("SFSI", "Computer Modern", "Sans Serif Italic"),
    ("SFRB", "Computer Modern", "Bold"),
    ("SFBL", "Computer Modern", "Bold Extended Slanted"),
    (
        "SFXC",
        "Computer Modern",
        "Bold Extended Caps and Small Caps",
    ),
    ("SFIT", "Computer Modern", "Italic Typewriter"),
    ("SFST", "Computer Modern", "Slanted Typewriter"),
    ("SFUI", "Computer Modern", "Unslanted Italic"),
    ("MSAM", "AMS Symbols", "A"),
    ("MSBM", "AMS Symbols", "B"),
    ("EUFM", "Euler Fraktur", "Regular"),
    ("EUFB", "Euler Fraktur", "Bold"),
    ("EURM", "Euler Roman", "Regular"),
    ("EURB", "Euler Roman", "Bold"),
    ("EUSM", "Euler Script", "Regular"),
    ("EUSB", "Euler Script", "Bold"),
    ("EUEX", "Euler", "Extension"),
];

/// A classic TeX font's family and style: "CMMI9" is Computer Modern, Math
/// Italic. Only a known abbreviation followed by a size of one to four
/// digits.
fn tex_name(name: &str) -> Option<(&'static str, &'static str)> {
    let letters = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let size = name.len() - letters.len();
    if !(1..=4).contains(&size) {
        return None;
    }
    TEX_FONTS
        .iter()
        .find(|(abbreviation, _, _)| *abbreviation == letters)
        .map(|&(_, family, style)| (family, style))
}

/// The name without a subset prefix: six capitals and a plus.
fn without_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// Without the "PS" and "MT" that PostScript and Monotype names end in.
fn without_vendor_suffix(part: &str) -> &str {
    let part = part.strip_suffix("MT").unwrap_or(part);
    part.strip_suffix("PS").unwrap_or(part)
}

/// `text` split into words: before a capital that follows a small letter,
/// before the last of a run of capitals that starts a word ("OCRBStd"),
/// and around numbers that follow a small letter ("Serif4Caption"). Runs
/// of capitals and numbers stay whole ("CMR9").
fn words(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut words = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (prev, cur) = (chars[i - 1].1, chars[i].1);
        let next_small = chars.get(i + 1).is_some_and(|&(_, c)| c.is_lowercase());
        let split = (prev.is_lowercase() && (cur.is_uppercase() || cur.is_ascii_digit()))
            || (prev.is_uppercase() && cur.is_uppercase() && next_small)
            || (prev.is_ascii_digit() && cur.is_uppercase() && next_small);
        if split {
            words.push(&text[start..chars[i].0]);
            start = chars[i].0;
        }
    }
    if start < text.len() {
        words.push(&text[start..]);
    }
    words
}

/// The style word `word` stands for, if it is one.
fn style_word(word: &str) -> Option<&'static str> {
    Some(match word.to_lowercase().as_str() {
        "regular" | "reg" | "regu" | "roman" | "normal" | "plain" => "Regular",
        "italic" | "ital" | "it" => "Italic",
        "oblique" | "obl" | "oblq" => "Oblique",
        "bold" | "bd" => "Bold",
        "semibold" | "smbd" | "sb" => "Semibold",
        "demibold" => "Demibold",
        "medium" | "med" | "medi" => "Medium",
        "light" | "lt" => "Light",
        "thin" => "Thin",
        "book" | "bk" => "Book",
        "black" | "blk" => "Black",
        "heavy" | "hv" => "Heavy",
        "condensed" | "cond" | "cn" => "Condensed",
        "subhead" | "subh" => "Subhead",
        "caption" | "capt" => "Caption",
        "display" | "disp" => "Display",
        "semi" => "Semi",
        "demi" => "Demi",
        "extra" => "Extra",
        "ultra" => "Ultra",
        _ => return None,
    })
}

/// Style words spelled out and joined: "Regular" only alone.
fn style_name(words: &[&str]) -> String {
    let mut names: Vec<String> = Vec::new();
    for word in words {
        let name = style_word(word).map_or_else(|| (*word).to_owned(), str::to_owned);
        match (names.last().map(String::as_str), name.as_str()) {
            (Some(prefix @ ("Semi" | "Demi")), "Bold") => {
                let merged = format!("{prefix}bold");
                *names.last_mut().unwrap() = merged;
            }
            _ if !names.contains(&name) => names.push(name),
            _ => {}
        }
    }
    if names.len() > 1 {
        names.retain(|n| n != "Regular");
    }
    if names.is_empty() {
        "Regular".into()
    } else {
        names.join(" ")
    }
}

/// How heavy a style is, as a CSS weight; Regular when it doesn't say.
fn weight(style: &str) -> u16 {
    let words: Vec<&str> = style.split(' ').collect();
    let extra = words.iter().any(|w| matches!(*w, "Extra" | "Ultra"));
    let has = |w: &str| words.contains(&w);
    match () {
        _ if has("Thin") => 100,
        _ if has("Light") && extra => 200,
        _ if has("Light") => 300,
        _ if has("Book") => 350,
        _ if has("Medium") => 500,
        _ if has("Semibold") || has("Demibold") => 600,
        _ if has("Bold") && extra => 800,
        _ if has("Bold") => 700,
        _ if has("Heavy") => 800,
        _ if has("Black") => 900,
        _ => 400,
    }
}

fn slanted(style: &str) -> bool {
    style
        .split(' ')
        .any(|w| matches!(w, "Italic" | "Oblique" | "Slant"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(name: &str) -> (String, String) {
        family_and_style(name)
    }

    fn pair(family: &str, style: &str) -> (String, String) {
        (family.into(), style.into())
    }

    #[test]
    fn subset_prefix_is_dropped() {
        assert_eq!(split("ABCDEF+MinionPro-It"), pair("Minion Pro", "Italic"));
    }

    #[test]
    fn a_plus_that_is_no_subset_prefix_stays() {
        assert_eq!(split("Abc+Def"), pair("Abc+Def", "Regular"));
    }

    #[test]
    fn a_name_without_style_is_regular() {
        assert_eq!(split("Helvetica"), pair("Helvetica", "Regular"));
        assert_eq!(split("Calibri"), pair("Calibri", "Regular"));
    }

    #[test]
    fn words_are_split_at_capitals_and_numbers() {
        assert_eq!(
            split("SourceSerif4Caption-It"),
            pair("Source Serif 4 Caption", "Italic")
        );
        assert_eq!(split("BellMTStd-SemiBold"), pair("Bell MT Std", "Semibold"));
        assert_eq!(split("OCRBStd"), pair("OCRB Std", "Regular"));
        assert_eq!(split("LMRoman10-Regular"), pair("LM Roman 10", "Regular"));
    }

    #[test]
    fn tex_names_are_spelled_out_without_their_size() {
        assert_eq!(split("ACEGLD+CMR6"), pair("Computer Modern", "Roman"));
        assert_eq!(split("CMMI9"), pair("Computer Modern", "Math Italic"));
        assert_eq!(
            split("CMMIB10"),
            pair("Computer Modern", "Math Bold Italic")
        );
        assert_eq!(split("CMSY8"), pair("Computer Modern", "Symbol"));
        assert_eq!(split("CMTT9"), pair("Computer Modern", "Typewriter"));
        assert_eq!(split("CMBX12"), pair("Computer Modern", "Bold Extended"));
        assert_eq!(split("SFRM1000"), pair("Computer Modern", "Roman"));
        assert_eq!(
            split("SFXC1000"),
            pair("Computer Modern", "Bold Extended Caps and Small Caps")
        );
        assert_eq!(
            split("SFIT1000"),
            pair("Computer Modern", "Italic Typewriter")
        );
        assert_eq!(split("MSBM10"), pair("AMS Symbols", "B"));
        assert_eq!(split("EUFM10"), pair("Euler Fraktur", "Regular"));
        assert_eq!(split("EUSB10"), pair("Euler Script", "Bold"));
    }

    #[test]
    fn unknown_or_sizeless_tex_like_names_stay_whole() {
        assert_eq!(split("CMXYZ9"), pair("CMXYZ9", "Regular"));
        assert_eq!(split("CMR"), pair("CMR", "Regular"));
        assert_eq!(split("CMR12345"), pair("CMR12345", "Regular"));
    }

    #[test]
    fn tex_sizes_of_one_style_are_one_style() {
        let fonts = [font("CMR9", true), font("CMR6", true), font("CMMI9", true)];
        assert_eq!(
            families(&fonts),
            vec![Family {
                name: "Computer Modern".into(),
                styles: vec![
                    style("Roman", true, &["CMR9", "CMR6"]),
                    style("Math Italic", true, &["CMMI9"]),
                ],
            }]
        );
    }

    #[test]
    fn monotype_and_postscript_suffixes_are_dropped() {
        assert_eq!(
            split("TimesNewRomanPSMT"),
            pair("Times New Roman", "Regular")
        );
        assert_eq!(
            split("TimesNewRomanPS-BoldItalicMT"),
            pair("Times New Roman", "Bold Italic")
        );
        assert_eq!(split("ArialMT"), pair("Arial", "Regular"));
    }

    #[test]
    fn a_comma_separates_the_style_too() {
        assert_eq!(split("Arial,BoldItalic"), pair("Arial", "Bold Italic"));
    }

    #[test]
    fn style_abbreviations_are_spelled_out() {
        assert_eq!(
            split("Intervogue-BlackOblq"),
            pair("Intervogue", "Black Oblique")
        );
        assert_eq!(split("Intervogue-Med"), pair("Intervogue", "Medium"));
        assert_eq!(split("Intervogue-Reg"), pair("Intervogue", "Regular"));
        assert_eq!(
            split("MinionPro-SemiboldSubh"),
            pair("Minion Pro", "Semibold Subhead")
        );
        assert_eq!(split("Times-Roman"), pair("Times", "Regular"));
        assert_eq!(
            split("Helvetica-BoldOblique"),
            pair("Helvetica", "Bold Oblique")
        );
    }

    #[test]
    fn regular_is_dropped_beside_another_style() {
        assert_eq!(
            split("NimbusRomNo9L-ReguItal"),
            pair("Nimbus Rom No 9L", "Italic")
        );
        assert_eq!(
            split("NimbusRomNo9L-Regu"),
            pair("Nimbus Rom No 9L", "Regular")
        );
    }

    #[test]
    fn a_generated_instance_suffix_is_dropped() {
        assert_eq!(
            split("NimbusRomNo9L-Regu-Slant_167"),
            pair("Nimbus Rom No 9L", "Slant")
        );
    }

    #[test]
    fn style_words_at_the_end_of_a_name_without_separator() {
        assert_eq!(split("ArialBold"), pair("Arial", "Bold"));
        assert_eq!(split("SegoeUIBoldItalic"), pair("Segoe UI", "Bold Italic"));
        assert_eq!(split("BaskOldFace"), pair("Bask Old Face", "Regular"));
    }

    fn font(name: &str, embedded: bool) -> Font {
        Font {
            name: name.into(),
            embedded,
        }
    }

    fn style(name: &str, embedded: bool, raw: &[&str]) -> Style {
        Style {
            name: name.into(),
            embedded,
            raw: raw.iter().map(|&r| r.into()).collect(),
        }
    }

    #[test]
    fn fonts_are_grouped_by_family_in_name_order() {
        let fonts = [
            font("AAAAAA+MinionPro-It", true),
            font("Helvetica", false),
            font("BBBBBB+MinionPro-Bold", true),
            font("CCCCCC+MinionPro-Regular", true),
            font("arial", true),
        ];
        assert_eq!(
            families(&fonts),
            vec![
                Family {
                    name: "arial".into(),
                    styles: vec![style("Regular", true, &["arial"])],
                },
                Family {
                    name: "Helvetica".into(),
                    styles: vec![style("Regular", false, &["Helvetica"])],
                },
                Family {
                    name: "Minion Pro".into(),
                    styles: vec![
                        style("Regular", true, &["MinionPro-Regular"]),
                        style("Italic", true, &["MinionPro-It"]),
                        style("Bold", true, &["MinionPro-Bold"]),
                    ],
                },
            ]
        );
    }

    #[test]
    fn styles_go_lightest_first_and_upright_before_slanted() {
        let fonts = [
            font("Intervogue-BlackOblq", true),
            font("Intervogue-Black", true),
            font("Intervogue-Light", true),
            font("Intervogue-MedOblq", true),
            font("Intervogue-Thin", true),
            font("Intervogue-Med", true),
        ];
        let names: Vec<String> = families(&fonts)[0]
            .styles
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(
            names,
            [
                "Thin",
                "Light",
                "Medium",
                "Medium Oblique",
                "Black",
                "Black Oblique"
            ]
        );
    }

    #[test]
    fn subsets_of_one_font_are_one_style() {
        let fonts = [
            font("AAAAAA+Calibri-Bold", true),
            font("BBBBBB+Calibri-Bold", true),
            font("CCCCCC+Calibri-Bold", false),
        ];
        assert_eq!(
            families(&fonts),
            vec![Family {
                name: "Calibri".into(),
                styles: vec![style("Bold", false, &["Calibri-Bold"])],
            }]
        );
    }
}
