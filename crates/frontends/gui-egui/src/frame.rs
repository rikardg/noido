//! The window frame noido draws itself where the system's is turned off
//! (Linux and Windows): resize edges and a border.

use eframe::egui::{
    self, CursorIcon, Id, LayerId, Order, Pos2, Rect, ResizeDirection, Stroke, StrokeKind,
    ViewportCommand,
};

/// Width of the band along the window's edges that resizes it, in UI points.
const RESIZE_BAND: f32 = 5.0;

/// The resize the window edge at `pointer` starts: corners where two bands
/// meet, `None` away from the edges.
pub fn resize_direction(window: Rect, pointer: Pos2, band: f32) -> Option<ResizeDirection> {
    let west = pointer.x - window.left() < band;
    let east = window.right() - pointer.x < band;
    let north = pointer.y - window.top() < band;
    let south = window.bottom() - pointer.y < band;
    match (north, south, west, east) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::NorthEast),
        (_, true, true, _) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, ..) => Some(ResizeDirection::North),
        (_, true, ..) => Some(ResizeDirection::South),
        (_, _, true, _) => Some(ResizeDirection::West),
        (_, _, _, true) => Some(ResizeDirection::East),
        _ => None,
    }
}

/// Whether the window is maximised or fullscreen, so it has no edges to
/// resize or border to draw.
fn fills_screen(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let viewport = i.viewport();
        viewport.maximized.unwrap_or(false) || viewport.fullscreen.unwrap_or(false)
    })
}

/// Resizes the window from its edges: the matching cursor on hover, a native
/// resize on press.
pub fn resize_edges(ctx: &egui::Context) {
    if fills_screen(ctx) {
        return;
    }
    let (window, pointer, pressed) = ctx.input(|i| {
        (
            i.content_rect(),
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
        )
    });
    let Some(direction) = pointer.and_then(|p| resize_direction(window, p, RESIZE_BAND)) else {
        return;
    };
    ctx.set_cursor_icon(cursor(direction));
    if pressed {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// A thin line around the window, so its edge shows against windows of the
/// same colour.
pub fn border(ctx: &egui::Context) {
    if fills_screen(ctx) {
        return;
    }
    let window = ctx.content_rect();
    let colour = ctx.global_style().visuals.window_stroke.color;
    ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("window-border")))
        .rect_stroke(window, 0.0, Stroke::new(1.0, colour), StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn window() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(900.0, 1000.0))
    }

    #[test]
    fn edges_and_corners() {
        let at = |x, y| resize_direction(window(), pos2(x, y), 5.0);
        assert_eq!(at(2.0, 500.0), Some(ResizeDirection::West));
        assert_eq!(at(898.0, 500.0), Some(ResizeDirection::East));
        assert_eq!(at(450.0, 1.0), Some(ResizeDirection::North));
        assert_eq!(at(450.0, 999.0), Some(ResizeDirection::South));
        assert_eq!(at(898.0, 2.0), Some(ResizeDirection::NorthEast));
        assert_eq!(at(1.0, 998.0), Some(ResizeDirection::SouthWest));
        assert_eq!(at(20.0, 500.0), None);
        assert_eq!(at(5.0, 500.0), None);
    }
}
