//! The block tree that EPUB and markdown are parsed into and the
//! typesetter reads: nested blocks, each holding its text flat, with
//! style ranges and a map from drawn text back to the source. Offsets
//! into the source are characters of one part. Pure: no I/O.

mod entities;
pub mod markdown;
mod text;
mod tree;
pub mod xhtml;

pub use text::*;
pub use tree::*;
