//! Lens's icons from the Arcade design kit: the launcher icon (teal tile with
//! the white Lens glyph, `packaging/icons/png/`) and the app glyphs
//! (`assets/glyphs/`), all synced unchanged by `scripts/sync-design`.

use std::sync::{Arc, OnceLock};

use eframe::egui::IconData;
use tiny_skia::{Pixmap, Transform};

/// Sizes written to the icon theme.
pub const THEME_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

/// The kit's launcher icon PNG at a size it ships.
fn kit_png(size: u32) -> Option<&'static [u8]> {
    Some(match size {
        16 => include_bytes!("../../../packaging/icons/png/16.png"),
        24 => include_bytes!("../../../packaging/icons/png/24.png"),
        32 => include_bytes!("../../../packaging/icons/png/32.png"),
        48 => include_bytes!("../../../packaging/icons/png/48.png"),
        64 => include_bytes!("../../../packaging/icons/png/64.png"),
        128 => include_bytes!("../../../packaging/icons/png/128.png"),
        256 => include_bytes!("../../../packaging/icons/png/256.png"),
        512 => include_bytes!("../../../packaging/icons/png/512.png"),
        1024 => include_bytes!("../../../packaging/icons/png/1024.png"),
        _ => return None,
    })
}

/// An Arcade app's glyph SVG (`currentColor` strokes on a 16 grid).
pub fn glyph_svg(app_id: &str) -> Option<&'static str> {
    Some(match app_id {
        "arcade.box" => include_str!("../../../assets/glyphs/arcade.box.svg"),
        "arcade.clipboard" => include_str!("../../../assets/glyphs/arcade.clipboard.svg"),
        "arcade.find" => include_str!("../../../assets/glyphs/arcade.find.svg"),
        "arcade.lens" => include_str!("../../../assets/glyphs/arcade.lens.svg"),
        "arcade.look" => include_str!("../../../assets/glyphs/arcade.look.svg"),
        "arcade.shelf" => include_str!("../../../assets/glyphs/arcade.shelf.svg"),
        "arcade.tools" => include_str!("../../../assets/glyphs/arcade.tools.svg"),
        "arcade.wheel" => include_str!("../../../assets/glyphs/arcade.wheel.svg"),
        _ => return None,
    })
}

/// An SVG rendered at `size`×`size` with `currentColor` set to `rgb`, as
/// unpremultiplied RGBA. Unknown or broken SVGs give a transparent image.
pub fn render_svg(svg: &str, size: u32, rgb: [u8; 3]) -> Vec<u8> {
    let svg = svg.replace("currentColor", &format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
    let mut out = vec![0; (size * size * 4) as usize];
    let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()) else { return out };
    let Some(mut pm) = Pixmap::new(size.max(1), size.max(1)) else { return out };
    let s = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, Transform::from_scale(s, s), &mut pm.as_mut());
    for (i, p) in pm.pixels().iter().enumerate() {
        let c = p.demultiply();
        out[i * 4..i * 4 + 4].copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// The launcher icon as unpremultiplied RGBA, `size`×`size`: the kit PNG at
/// that size, or the nearest larger one scaled down.
pub fn rgba(size: u32) -> Vec<u8> {
    let source = [16, 24, 32, 48, 64, 128, 256, 512, 1024].into_iter().find(|&s| s >= size).unwrap_or(1024);
    let img = image::load_from_memory(kit_png(source).expect("kit icon size")).expect("kit icon decodes").to_rgba8();
    if source == size {
        return img.into_raw();
    }
    image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3).into_raw()
}

/// PNG bytes of the launcher icon.
pub fn png(size: u32) -> Vec<u8> {
    if let Some(bytes) = kit_png(size) {
        return bytes.to_vec();
    }
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

/// The tray icon: the Lens glyph, monochrome. Black for the macOS menu bar
/// (shown as a template image, which follows the bar's appearance); white on
/// Linux and Windows panels, which are dark by default.
pub fn tray_icons() -> Vec<lens_platform::tray::TrayIcon> {
    let rgb = if cfg!(target_os = "macos") { [0, 0, 0] } else { [255, 255, 255] };
    let glyph = glyph_svg("arcade.lens").expect("lens glyph");
    [22, 32, 48, 64].into_iter().map(|size| lens_platform::tray::TrayIcon { size, rgba: render_svg(glyph, size, rgb) }).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn launcher_icon_renders_every_size() {
        for size in super::THEME_SIZES.into_iter().chain([22, 100]) {
            let px = super::rgba(size);
            assert_eq!(px.len(), (size * size * 4) as usize);
            // Opaque in the middle, transparent in the corner.
            let mid = ((size / 2 * size + size / 2) * 4 + 3) as usize;
            assert!(px[mid] > 200 && px[3] == 0, "size {size}");
        }
    }

    #[test]
    fn launcher_icon_is_the_teal_kit_tile() {
        let px = super::rgba(64);
        // A tile pixel inside the rounded corner, away from the glyph.
        let at = ((6 * 64 + 32) * 4) as usize;
        let [r, g, b] = [px[at], px[at + 1], px[at + 2]];
        assert!(g > r && b > r, "teal, not violet: {r},{g},{b}");
    }

    #[test]
    fn every_app_glyph_renders() {
        for app in ["arcade.box", "arcade.clipboard", "arcade.find", "arcade.lens", "arcade.look", "arcade.shelf", "arcade.tools", "arcade.wheel"] {
            let px = super::render_svg(super::glyph_svg(app).unwrap(), 32, [255, 255, 255]);
            assert!(px.chunks(4).any(|p| p[3] > 128), "{app} draws something");
        }
        assert!(super::glyph_svg("arcade.unknown").is_none());
    }

    #[test]
    fn tray_glyph_is_monochrome() {
        for icon in super::tray_icons() {
            assert!(icon.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[0] == p[1] && p[1] == p[2]), "size {}", icon.size);
        }
    }
}
