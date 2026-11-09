//! The pagelet's page: fitted inside the panel with its aspect kept, and
//! the way back to page points for a pointer on it.

use noido_engine::Size;

use crate::PageRect;

/// Whether `size` can be fitted: positive and finite on both sides.
fn usable(width: f32, height: f32) -> bool {
    width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0
}

/// `page` centred in `area` with its aspect kept; a page without a usable
/// size fills `area`.
pub fn fit_page(page: Size, area: PageRect) -> PageRect {
    if !usable(page.width, page.height) || !usable(area.width, area.height) {
        return area;
    }
    let scale = (area.width / page.width).min(area.height / page.height);
    let (width, height) = (page.width * scale, page.height * scale);
    PageRect {
        x: area.x + (area.width - width) / 2.0,
        y: area.y + (area.height - height) / 2.0,
        width,
        height,
    }
}

/// The page point (page points) under (`x`, `y`) inside `fitted`, or `None`
/// outside the page.
pub fn page_point(page: Size, fitted: PageRect, x: f32, y: f32) -> Option<(f32, f32)> {
    if !usable(page.width, page.height) || !usable(fitted.width, fitted.height) {
        return None;
    }
    let inside = x >= fitted.x
        && x <= fitted.x + fitted.width
        && y >= fitted.y
        && y <= fitted.y + fitted.height;
    if !inside {
        return None;
    }
    Some((
        (x - fitted.x) / fitted.width * page.width,
        (y - fitted.y) / fitted.height * page.height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter() -> Size {
        Size {
            width: 612.0,
            height: 792.0,
        }
    }

    fn area(x: f32, y: f32, width: f32, height: f32) -> PageRect {
        PageRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn fit_page_letterboxes_and_centres() {
        let fitted = fit_page(letter(), area(10.0, 20.0, 400.0, 400.0));
        assert!((fitted.height - 400.0).abs() < 0.01, "{fitted:?}");
        assert!((fitted.width - 309.0909).abs() < 0.01, "{fitted:?}");
        assert!((fitted.x - 55.4545).abs() < 0.01, "{fitted:?}");
        assert!((fitted.y - 20.0).abs() < 0.01, "{fitted:?}");
    }

    #[test]
    fn fit_page_fills_a_matching_area() {
        let fitted = fit_page(letter(), area(0.0, 0.0, 306.0, 396.0));
        assert!((fitted.x).abs() < 0.01 && (fitted.y).abs() < 0.01);
        assert!((fitted.width - 306.0).abs() < 0.01);
        assert!((fitted.height - 396.0).abs() < 0.01);
    }

    #[test]
    fn page_point_maps_back_to_page_points() {
        let page = letter();
        let fitted = fit_page(page, area(0.0, 0.0, 306.0, 396.0));
        assert_eq!(page_point(page, fitted, 0.0, 0.0), Some((0.0, 0.0)));
        assert_eq!(page_point(page, fitted, 153.0, 198.0), Some((306.0, 396.0)));
        assert_eq!(page_point(page, fitted, -1.0, 10.0), None);
        assert_eq!(page_point(page, fitted, 307.0, 10.0), None);
    }

    #[test]
    fn a_page_without_a_size_fills_the_area() {
        let bad = Size {
            width: 0.0,
            height: f32::NAN,
        };
        let fitted = fit_page(bad, area(5.0, 6.0, 70.0, 80.0));
        assert_eq!(fitted, area(5.0, 6.0, 70.0, 80.0));
        assert_eq!(page_point(bad, fitted, 5.0, 6.0), None);
    }
}
