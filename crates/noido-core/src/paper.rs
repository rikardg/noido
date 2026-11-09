//! The colour pages are shown on.

/// The paper colour. Pages are drawn tinted with it: white paper takes the
/// colour, black text stays black.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    /// The page as the document has it.
    #[default]
    White,
    Sepia,
    /// CSS bisque: warmer and a little darker than sepia.
    Bisque,
    /// Bisque after dark: the same warmth, less saturated and darker.
    Sand,
    /// `#D4D4D4`: light enough that black text keeps about 14:1 contrast.
    Grey,
    /// `#C8C8C8`: a step darker, about 13:1.
    DarkGrey,
    /// `#C0C0C0`: a step darker again, the dimmest paper, about 11.5:1.
    DarkerGrey,
}

impl Paper {
    /// Every paper, in the order the settings page shows them.
    pub const ALL: [Paper; 7] = [
        Paper::White,
        Paper::Sepia,
        Paper::Bisque,
        Paper::Sand,
        Paper::Grey,
        Paper::DarkGrey,
        Paper::DarkerGrey,
    ];

    /// The colour as red, green, blue.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Paper::White => [0xFF, 0xFF, 0xFF],
            Paper::Sepia => [0xF4, 0xEC, 0xD8],
            Paper::Bisque => [0xFF, 0xE4, 0xC4],
            Paper::Sand => [0xEB, 0xCD, 0xA8],
            Paper::Grey => [0xD4, 0xD4, 0xD4],
            Paper::DarkGrey => [0xC8, 0xC8, 0xC8],
            Paper::DarkerGrey => [0xC0, 0xC0, 0xC0],
        }
    }

    /// The name the settings page shows.
    pub fn name(self) -> &'static str {
        match self {
            Paper::White => "Original",
            Paper::Sepia => "Sepia",
            Paper::Bisque => "Bisque",
            Paper::Sand => "Sand",
            Paper::Grey => "Grey",
            Paper::DarkGrey => "Dark Grey",
            Paper::DarkerGrey => "Darker Grey",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_leaves_the_page_alone() {
        assert_eq!(Paper::White.rgb(), [255, 255, 255]);
        assert_eq!(Paper::default(), Paper::White);
    }

    #[test]
    fn colours() {
        assert_eq!(Paper::Sepia.rgb(), [0xF4, 0xEC, 0xD8]);
        assert_eq!(Paper::Grey.rgb(), [0xD4, 0xD4, 0xD4]);
        assert_eq!(Paper::Bisque.rgb(), [0xFF, 0xE4, 0xC4]);
        assert_eq!(Paper::Sand.rgb(), [0xEB, 0xCD, 0xA8]);
        assert_eq!(Paper::DarkGrey.rgb(), [0xC8, 0xC8, 0xC8]);
        assert_eq!(Paper::DarkerGrey.rgb(), [0xC0, 0xC0, 0xC0]);
    }

    #[test]
    fn names() {
        let names: Vec<_> = Paper::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(
            names,
            [
                "Original",
                "Sepia",
                "Bisque",
                "Sand",
                "Grey",
                "Dark Grey",
                "Darker Grey",
            ]
        );
    }

    #[test]
    fn added_papers_are_darker_than_the_one_they_follow() {
        assert!(luminance(Paper::Sand) < luminance(Paper::Bisque));
        assert!(luminance(Paper::DarkGrey) < luminance(Paper::Grey));
        assert!(luminance(Paper::DarkerGrey) < luminance(Paper::DarkGrey));
    }

    #[test]
    fn added_papers_keep_black_text_comfortable() {
        for paper in [Paper::Sand, Paper::DarkGrey, Paper::DarkerGrey] {
            let contrast = (luminance(paper) + 0.05) / 0.05;
            assert!(contrast >= 11.0, "{paper:?} is {contrast:.1}:1");
        }
    }

    /// Relative luminance as WCAG defines it.
    fn luminance(paper: Paper) -> f64 {
        let linear = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b] = paper.rgb();
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }
}
