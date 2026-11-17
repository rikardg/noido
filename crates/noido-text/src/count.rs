//! A page's size in words and characters.

use crate::search::line_end_hyphen;

/// A text's size in words and characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub words: usize,
    /// Characters the page draws: line breaks are noido's and are not
    /// counted, and a line-end hyphen is drawn, so it is.
    pub chars: usize,
    /// The same without the spaces between words.
    pub chars_no_spaces: usize,
}

impl Counts {
    /// Counts `text`, one page's text as `PageText::text` has it: lines
    /// joined with newlines, spaces inferred from the gaps. A line-end
    /// hyphen inside a word joins the lines rather than splitting the word.
    pub fn of(text: &str) -> Counts {
        let chars: Vec<char> = text.chars().collect();
        let mut counts = Counts::default();
        let mut in_word = false;
        let mut i = 0;
        while i < chars.len() {
            // A hyphen at the line's end joins those lines; it and the
            // line break do not end the word. A soft hyphen joins without
            // being drawn; every other hyphen is on the page.
            if in_word && let Some((_, next)) = line_end_hyphen(&chars, i, false) {
                if chars[i] != '\u{ad}' {
                    counts.chars += 1;
                    counts.chars_no_spaces += 1;
                }
                i = next;
                continue;
            }
            let c = chars[i];
            // The line breaks are noido's, not the document's.
            if c != '\n' {
                counts.chars += 1;
            }
            if c.is_whitespace() {
                in_word = false;
            } else {
                if !in_word {
                    counts.words += 1;
                    in_word = true;
                }
                counts.chars_no_spaces += 1;
            }
            i += 1;
        }
        counts
    }
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, other: Counts) {
        self.words += other.words;
        self.chars += other.chars;
        self.chars_no_spaces += other.chars_no_spaces;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_runs_of_non_space_characters() {
        assert_eq!(
            Counts::of("the cat sat\non a mat"),
            Counts {
                words: 6,
                chars: 19,
                chars_no_spaces: 15
            }
        );
    }

    #[test]
    fn the_line_breaks_and_spaces_are_not_characters_but_spaces_are() {
        // The newline is noido's, not the document's.
        assert_eq!(
            Counts::of("a b\nc"),
            Counts {
                words: 3,
                chars: 4,
                chars_no_spaces: 3
            }
        );
    }

    #[test]
    fn a_line_end_hyphen_does_not_split_a_word() {
        // "statis-"/"tics" is one word; the hyphen is drawn, so it is a
        // character, and it is not a space.
        assert_eq!(
            Counts::of("statis-\ntics"),
            Counts {
                words: 1,
                chars: 11,
                chars_no_spaces: 11
            }
        );
        // A hyphen the word owns joins it the same way.
        assert_eq!(
            Counts::of("Anglo-\nSaxon"),
            Counts {
                words: 1,
                chars: 11,
                chars_no_spaces: 11
            }
        );
    }

    #[test]
    fn a_soft_hyphen_is_not_a_drawn_character() {
        // A soft hyphen marks where a word may break; it joins the word
        // like a line-end hyphen, but it is not drawn, so it is no
        // character of the page.
        assert_eq!(
            Counts::of("statis\u{ad}\ntics"),
            Counts {
                words: 1,
                chars: 10,
                chars_no_spaces: 10
            }
        );
    }

    #[test]
    fn a_hyphen_that_is_not_at_a_line_end_ends_nothing() {
        assert_eq!(
            Counts::of("well-known"),
            Counts {
                words: 1,
                chars: 10,
                chars_no_spaces: 10
            }
        );
    }

    #[test]
    fn punctuation_and_digits_are_words() {
        // A lone dash is a word, as it is in Word; nothing here guesses at
        // the language to do better.
        assert_eq!(Counts::of("1997 — ok").words, 3);
    }

    #[test]
    fn empty_text_has_nothing() {
        assert_eq!(Counts::of(""), Counts::default());
    }

    #[test]
    fn pages_add_up() {
        let mut total = Counts::default();
        total += Counts::of("the cat sat");
        total += Counts::of("a dog");
        assert_eq!(
            total,
            Counts {
                words: 5,
                chars: 16,
                chars_no_spaces: 13
            }
        );
    }
}
