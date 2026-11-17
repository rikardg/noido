//! The typesetter behind Document: house style, fonts, and (later) the
//! line breaker and page maker.

mod fonts;
#[allow(dead_code)] // until the page maker uses all of it
mod house;
mod kp;
#[allow(dead_code)] // until the page maker uses it
mod lines;
#[allow(dead_code)] // until the page maker calls paragraphs()
mod paragraphs;
