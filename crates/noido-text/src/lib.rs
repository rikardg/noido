//! A page's text, assembled from the glyphs an engine draws, and search on
//! it. Pure: no threads, no engine calls.

mod copy;
mod page;
mod search;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use copy::*;
pub use page::*;
pub use search::*;
