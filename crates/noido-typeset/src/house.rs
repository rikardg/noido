//! The house style: a B-format book page.
// Unused until the later modules (layout, pages) use them.
#![allow(dead_code)]

use noido_engine::Size;

/// B-format, 129 x 198 mm, in points.
pub(crate) const PAPER: Size = Size {
    width: 365.67,
    height: 561.26,
};
pub(crate) const MEASURE: f32 = 270.0;
pub(crate) const LEFT: f32 = (PAPER.width - MEASURE) / 2.0;
pub(crate) const TOP: f32 = 56.0;
pub(crate) const LEADING: f32 = 14.0;
pub(crate) const LINES: usize = 31;
pub(crate) const BODY: f32 = 11.0;
pub(crate) const HEADING: [f32; 6] = [18.0, 14.0, 12.0, 11.0, 11.0, 11.0];
pub(crate) const CODE_SCALE: f32 = 0.85;
pub(crate) const INDENT_EM: f32 = 1.0;
pub(crate) const LAYOUT_KEY: &str = "b-format 11/14";
