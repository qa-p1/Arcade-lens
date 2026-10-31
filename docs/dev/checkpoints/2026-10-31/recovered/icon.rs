//! Recovered from a recorded edit command, before rustfmt; not build-wired.
//! Launcher and monochrome tray marks from the pinned Arcade design kit.
use std::sync::{Arc, OnceLock};
use eframe::egui::IconData;
pub const THEME_SIZES: [u32; 6] = [16,32,48,64,128,256];
pub fn render(svg: &str, size: u32, color: u32) -> Vec<u8> {
    let svg = svg.replace("currentColor", &format!("#{color:06x}"));
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("kit SVG");
    let mut pixels = tiny_skia::Pixmap::new(size,size).expect("icon dimensions");
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale,scale), &mut pixels.as_mut());
    pixels.pixels().iter().flat_map(|p| { let p = p.demultiply(); [p.red(),p.green(),p.blue(),p.alpha()] }).collect()
}
pub fn rgba(size: u32) -> Vec<u8> { render(include_str!("../../../packaging/icons/icon.svg"), size, arcade_link::design::palette(arcade_link::design::Theme::Light, crate::link::ME).text) }
pub fn png(size: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(size,size,rgba(size)).expect("icon buffer"); let mut bytes = std::io::Cursor::new(Vec::new()); img.write_to(&mut bytes,image::ImageFormat::Png).expect("PNG icon"); bytes.into_inner()
}
pub fn window_icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new(); ICON.get_or_init(|| Arc::new(IconData { rgba: rgba(64), width:64, height:64 })).clone()
}
pub fn tray_icons() -> Vec<lens_platform::tray::TrayIcon> {
    let palette = arcade_link::design::palette(if cfg!(target_os="macos") { arcade_link::design::Theme::Light } else { arcade_link::design::Theme::Dark },crate::link::ME);
    [22,32,48,64].into_iter().map(|size| lens_platform::tray::TrayIcon { size, rgba: render(crate::gui::glyphs::svg(crate::link::ME),size,palette.text) }).collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn renders_every_size() { for size in super::THEME_SIZES { let px=super::rgba(size); assert_eq!(px.len(),(size*size*4) as usize); assert!(px.chunks_exact(4).any(|p| p[3]>200)); assert_eq!(px[3],0); } }
}
