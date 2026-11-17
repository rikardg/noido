//! The typesetter behind Document: house style, fonts, and (later) the
//! line breaker and page maker.

mod fonts;
mod house;
#[allow(dead_code)] // until the paragraph setter (Task 8) uses it
mod kp;
