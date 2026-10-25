//! A page's text, assembled from the glyphs an engine draws, and search on
//! it. Pure: no threads, no engine calls.

mod page;
mod search;
#[cfg(test)]
mod testing;

pub use page::*;
pub use search::*;
