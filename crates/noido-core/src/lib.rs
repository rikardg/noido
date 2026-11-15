//! noido viewer state, independent of any UI toolkit.

mod anchor;
mod bars;
mod column;
mod defaults;
mod fonts;
mod glide;
mod highlights;
mod history;
mod hits;
mod keymap;
mod layout;
mod menu;
mod outline;
mod page_numbers;
mod paper;
mod pins;
mod place;
mod preview;
#[cfg(test)]
mod reflow_tests;
mod selection;
mod sidebar;
mod target;
mod thumbnails;
mod tracker;
mod transforms;
mod view;
mod window;

// The menu has a `Check` of its own, so this one leaves as `AnchorCheck`.
pub use anchor::{Anchored, Check as AnchorCheck, Walk, anchor, check, requote};
pub use bars::*;
pub use column::*;
pub use defaults::*;
pub use fonts::*;
pub use glide::*;
pub use highlights::*;
pub use history::*;
pub use hits::*;
pub use keymap::*;
pub use layout::*;
pub use menu::*;
pub use outline::*;
pub use page_numbers::*;
pub use paper::*;
pub use pins::*;
pub use place::*;
pub use preview::*;
pub use selection::*;
pub use sidebar::*;
pub use target::*;
pub use thumbnails::*;
pub use tracker::*;
pub use transforms::*;
pub use view::*;
pub use window::*;
