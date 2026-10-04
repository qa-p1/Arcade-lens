//! The app icon, drawn in code so the tray, window icons and the launcher
//! entry share one design at every size: selection corners around a lens
//! on a violet tile.

use std::sync::{Arc, OnceLock};

use eframe::egui::IconData;
use tiny_skia::{Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode, Stroke, Transform};

/// Sizes written to the icon theme.
pub const THEME_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    // Cubic approximation of a quarter circle.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().expect("rounded rect path")
}

/// The icon as unpremultiplied RGBA, `size`×`size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut pm = Pixmap::new(size, size).expect("icon size");
    // Small sizes get a fuller tile and bolder strokes so they stay legible.
    let small = size <= 24;
    let inset = if small { 0.0 } else { s * 0.04 };
    let tile = rounded_rect(inset, inset, s - 2.0 * inset, s - 2.0 * inset, s * 0.22);
    let mut bg = Paint { anti_alias: true, ..Default::default() };
    bg.shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(s, s),
        vec![GradientStop::new(0.0, Color::from_rgba8(167, 139, 250, 255)), GradientStop::new(1.0, Color::from_rgba8(109, 40, 217, 255))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    pm.fill_path(&tile, &bg, FillRule::Winding, Transform::identity(), None);

    let mut fg = Paint { anti_alias: true, ..Default::default() };
    fg.set_color_rgba8(255, 255, 255, 255);
    let width = s * if small { 0.095 } else { 0.072 };
    let stroke = Stroke { width, line_cap: LineCap::Round, ..Default::default() };

    // Selection corners.
    let (a, b, len) = (s * 0.22, s * 0.78, s * 0.15);
    let mut pb = PathBuilder::new();
    for (cx, cy, dx, dy) in [(a, a, 1.0, 1.0), (b, a, -1.0, 1.0), (a, b, 1.0, -1.0), (b, b, -1.0, -1.0)] {
        pb.move_to(cx + dx * len, cy);
        pb.line_to(cx, cy);
        pb.line_to(cx, cy + dy * len);
    }
    pm.stroke_path(&pb.finish().expect("corners"), &fg, &stroke, Transform::identity(), None);

    // The lens: a ring and its handle.
    let (cx, cy, r) = (s * 0.46, s * 0.46, s * 0.135);
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    let d = r * std::f32::consts::FRAC_1_SQRT_2;
    pb.move_to(cx + d, cy + d);
    pb.line_to(s * 0.63, s * 0.63);
    pm.stroke_path(&pb.finish().expect("lens"), &fg, &stroke, Transform::identity(), None);

    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

pub fn png(size: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(size, size, rgba(size)).expect("icon buffer");
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).expect("encode icon");
    out.into_inner()
}

/// The icon for window title bars and task switchers.
pub fn window_icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(IconData { rgba: rgba(64), width: 64, height: 64 })).clone()
}

/// Tray icon sizes; the tray picks the closest.
pub fn tray_icons() -> Vec<lens_platform::tray::TrayIcon> {
    [22, 32, 48, 64].into_iter().map(|size| lens_platform::tray::TrayIcon { size, rgba: rgba(size) }).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_every_size() {
        for size in super::THEME_SIZES {
            let px = super::rgba(size);
            assert_eq!(px.len(), (size * size * 4) as usize);
            // Opaque in the middle, transparent in the corner.
            let mid = ((size / 2 * size + size / 2) * 4 + 3) as usize;
            assert!(px[mid] > 200 && px[3] == 0, "size {size}");
        }
    }
}
