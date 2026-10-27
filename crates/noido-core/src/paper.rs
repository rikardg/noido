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
    Grey,
}

impl Paper {
    /// Every paper, in the order the settings page shows them.
    pub const ALL: [Paper; 4] = [Paper::White, Paper::Sepia, Paper::Bisque, Paper::Grey];

    /// The colour as red, green, blue.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Paper::White => [0xFF, 0xFF, 0xFF],
            Paper::Sepia => [0xF4, 0xEC, 0xD8],
            Paper::Bisque => [0xFF, 0xE4, 0xC4],
            // Light enough that black text keeps about 13:1 contrast.
            Paper::Grey => [0xD4, 0xD4, 0xD4],
        }
    }

    /// The name the settings page shows.
    pub fn name(self) -> &'static str {
        match self {
            Paper::White => "Original",
            Paper::Sepia => "Sepia",
            Paper::Bisque => "Bisque",
            Paper::Grey => "Grey",
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
    }

    #[test]
    fn names() {
        let names: Vec<_> = Paper::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(names, ["Original", "Sepia", "Bisque", "Grey"]);
    }
}
