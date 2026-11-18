//! The typesetter behind Document: house style, fonts, and (later) the
//! line breaker and page maker.

mod book;
mod fonts;
#[allow(dead_code)] // until the page maker uses all of it
mod house;
mod kp;
#[allow(dead_code)] // until the page maker uses it
mod lines;
mod links;
#[allow(dead_code)] // until the page maker uses it
mod pages;
#[allow(dead_code)] // until the page maker calls paragraphs()
mod paragraphs;
mod runs;

pub use book::{Book, typeset};
