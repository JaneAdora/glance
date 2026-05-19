use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub compact: bool,
    pub too_narrow: bool,
}

pub fn choose_columns(width: u16) -> Columns {
    if width < 30 {
        Columns { compact: true, too_narrow: true }
    } else if width < 60 {
        Columns { compact: true, too_narrow: false }
    } else {
        Columns { compact: false, too_narrow: false }
    }
}

/// Compute Canvas x_bounds and y_bounds for a Rect rendered with the Braille
/// marker (2 subpixels wide × 4 subpixels tall per terminal cell), such that
/// content of half-extent (data_half_x, data_half_y) keeps its visual aspect
/// ratio regardless of how the area is resized.
///
/// The shorter axis anchors to the data; the longer axis is padded (letterbox
/// / pillarbox). Content stays centered around (0, 0).
///
/// For a unit circle: `braille_aspect_bounds(area, 1.0, 1.0)`.
/// For the world map (-180..180, -90..90):
/// `braille_aspect_bounds(area, 180.0, 90.0)`.
pub fn braille_aspect_bounds(
    area: Rect,
    data_half_x: f64,
    data_half_y: f64,
) -> ([f64; 2], [f64; 2]) {
    let sub_w = (area.width as f64) * 2.0;
    let sub_h = (area.height as f64) * 4.0;
    if sub_w <= 0.0 || sub_h <= 0.0 || data_half_x <= 0.0 || data_half_y <= 0.0 {
        return (
            [-data_half_x.max(1.0), data_half_x.max(1.0)],
            [-data_half_y.max(1.0), data_half_y.max(1.0)],
        );
    }
    // area_ratio > data_ratio  → area is wider than data needs → pillarbox (widen x)
    // area_ratio < data_ratio  → area is taller than data needs → letterbox (widen y)
    let area_ratio = sub_w / sub_h;
    let data_ratio = data_half_x / data_half_y;
    if area_ratio >= data_ratio {
        let half_x = data_half_y * area_ratio;
        ([-half_x, half_x], [-data_half_y, data_half_y])
    } else {
        let half_y = data_half_x / area_ratio;
        ([-data_half_x, data_half_x], [-half_y, half_y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn square_data_in_wide_area_pillarboxes() {
        // 40×5 cells with Braille → 80×20 subpixels, ratio 4:1
        // Unit circle (1:1) should pillarbox: y stays ±1, x widens.
        let r = Rect::new(0, 0, 40, 5);
        let (x, y) = braille_aspect_bounds(r, 1.0, 1.0);
        assert_eq!(y, [-1.0, 1.0]);
        assert!(x[1] > 1.0, "x should widen; got {:?}", x);
    }

    #[test]
    fn square_data_in_tall_area_letterboxes() {
        // 10×20 cells with Braille → 20×80 subpixels, ratio 0.25:1 (tall)
        // Unit circle should letterbox: x stays ±1, y widens.
        let r = Rect::new(0, 0, 10, 20);
        let (x, y) = braille_aspect_bounds(r, 1.0, 1.0);
        assert_eq!(x, [-1.0, 1.0]);
        assert!(y[1] > 1.0, "y should widen; got {:?}", y);
    }

    #[test]
    fn world_2to1_in_balanced_area() {
        // 80×20 cells → 160×80 subpixels, ratio 2:1 (matches world)
        // No letterbox or pillarbox needed.
        let r = Rect::new(0, 0, 80, 20);
        let (x, y) = braille_aspect_bounds(r, 180.0, 90.0);
        assert_eq!(x, [-180.0, 180.0]);
        assert_eq!(y, [-90.0, 90.0]);
    }
}
