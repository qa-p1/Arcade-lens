//! Measure mode geometry: edge detection on the frozen frame.
//!
//! From the cursor, scan outward in each direction until the pixel differs
//! from the starting pixel by more than a tolerance. That finds the bounds
//! of the flat area under the cursor (a button, a gap between elements),
//! which is what designers measure. Snapping moves a dragged edge onto the
//! nearest strong color boundary.

use image::RgbaImage;

/// Per-channel difference that counts as an edge.
pub const TOLERANCE: i32 = 18;

fn differs(a: &image::Rgba<u8>, b: &image::Rgba<u8>) -> bool {
    (0..3).any(|i| (a[i] as i32 - b[i] as i32).abs() > TOLERANCE)
}

/// Distances (in px) from `(x, y)` to the first differing pixel in each
/// direction: (left, right, up, down). Bounded by the image edges.
pub fn extents(img: &RgbaImage, x: u32, y: u32) -> (u32, u32, u32, u32) {
    let (w, h) = img.dimensions();
    if x >= w || y >= h {
        return (0, 0, 0, 0);
    }
    let start = *img.get_pixel(x, y);
    let mut left = 0;
    while x > left && !differs(img.get_pixel(x - left - 1, y), &start) {
        left += 1;
    }
    let mut right = 0;
    while x + right + 1 < w && !differs(img.get_pixel(x + right + 1, y), &start) {
        right += 1;
    }
    let mut up = 0;
    while y > up && !differs(img.get_pixel(x, y - up - 1), &start) {
        up += 1;
    }
    let mut down = 0;
    while y + down + 1 < h && !differs(img.get_pixel(x, y + down + 1), &start) {
        down += 1;
    }
    (left, right, up, down)
}

/// Strength of the vertical boundary between columns `x-1` and `x`, sampled
/// over rows `y0..y1`.
fn column_edge(img: &RgbaImage, x: u32, y0: u32, y1: u32) -> u32 {
    if x == 0 || x >= img.width() {
        return 0;
    }
    (y0..y1.min(img.height())).step_by(2).filter(|&y| differs(img.get_pixel(x - 1, y), img.get_pixel(x, y))).count() as u32
}

fn row_edge(img: &RgbaImage, y: u32, x0: u32, x1: u32) -> u32 {
    if y == 0 || y >= img.height() {
        return 0;
    }
    (x0..x1.min(img.width())).step_by(2).filter(|&x| differs(img.get_pixel(x, y - 1), img.get_pixel(x, y))).count() as u32
}

/// Moves a vertical edge at `x` to the strongest boundary within `radius`
/// px, if one is clearly present along `y0..y1`.
pub fn snap_x(img: &RgbaImage, x: i32, y0: u32, y1: u32, radius: i32) -> i32 {
    let span = (y1.saturating_sub(y0) / 2).max(1);
    (x - radius..=x + radius)
        .filter(|c| *c > 0 && (*c as u32) < img.width())
        .map(|c| (c, column_edge(img, c as u32, y0, y1)))
        .filter(|(_, s)| *s * 2 >= span)
        .max_by_key(|(c, s)| (*s, -(c - x).abs()))
        .map_or(x, |(c, _)| c)
}

pub fn snap_y(img: &RgbaImage, y: i32, x0: u32, x1: u32, radius: i32) -> i32 {
    let span = (x1.saturating_sub(x0) / 2).max(1);
    (y - radius..=y + radius)
        .filter(|r| *r > 0 && (*r as u32) < img.height())
        .map(|r| (r, row_edge(img, r as u32, x0, x1)))
        .filter(|(_, s)| *s * 2 >= span)
        .max_by_key(|(r, s)| (*s, -(r - y).abs()))
        .map_or(y, |(r, _)| r)
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    /// 200×100 white canvas with a dark "button" at x 50..150, y 30..70.
    fn button() -> RgbaImage {
        RgbaImage::from_fn(200, 100, |x, y| if (50..150).contains(&x) && (30..70).contains(&y) { Rgba([24, 24, 27, 255]) } else { Rgba([250, 250, 250, 255]) })
    }

    #[test]
    fn extents_find_the_button_bounds() {
        let img = button();
        // Inside the button at (60, 40): 10 px to its left edge, 89 to the right, 10 up, 29 down.
        assert_eq!(extents(&img, 60, 40), (10, 89, 10, 29));
        // In the margin left of it: the white area spans to the image edge and the button.
        assert_eq!(extents(&img, 20, 50), (20, 29, 50, 49));
    }

    #[test]
    fn snapping_moves_to_strong_edges() {
        let img = button();
        assert_eq!(snap_x(&img, 47, 30, 70, 6), 50);
        assert_eq!(snap_x(&img, 153, 30, 70, 6), 150);
        assert_eq!(snap_y(&img, 33, 50, 150, 6), 30);
        // No edge nearby: unchanged.
        assert_eq!(snap_x(&img, 100, 30, 70, 6), 100);
    }
}
