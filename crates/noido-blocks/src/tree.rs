//! The blocks: nested where the format nests them, each holding its
//! text flat.

use std::ops::Range;

use crate::Text;

/// One part's blocks: for markdown, the whole file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// The whole construct in the source, markers included
    /// (`## Title\n`, `> quoted\n> lines`), in characters of the part.
    pub span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        text: Text,
    },
    Paragraph(Text),
    /// Its text as written: no styles, whitespace kept.
    Code {
        info: Option<String>,
        text: Text,
    },
    Quote(Vec<Block>),
    List {
        /// The first number of an ordered list; `None` for bullets.
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        align: Vec<Align>,
        head: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
    },
    Rule,
    /// A footnote's body, where it was defined.
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
    /// Raw HTML: not drawn in typeset mode; raw mode styles its span.
    Html,
    /// YAML front matter: not drawn in typeset mode; raw mode styles it.
    FrontMatter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub span: Range<usize>,
    /// `Some` for a task list item: checked or not.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub span: Range<usize>,
    pub text: Text,
}

/// A table column's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}
