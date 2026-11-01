//! Arcade app glyphs from the design kit (`assets/glyphs/`), drawn as white
//! masks tinted at paint time, so one texture serves every theme and state.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, TextureHandle, TextureId, TextureOptions, Vec2};

/// Rendered at 2× so glyphs stay sharp at 125 % and on HiDPI screens.
const OVERSAMPLE: f32 = 2.0;

/// The glyph's white mask texture at `px` logical pixels, cached on `ctx`.
pub fn texture(ctx: &egui::Context, app: &str, px: f32) -> Option<TextureId> {
    let svg = crate::icon::glyph_svg(app)?;
    let size = (px * OVERSAMPLE * ctx.pixels_per_point()).round().max(8.0) as u32;
    let id = egui::Id::new(("lens-glyph", app, size));
    if let Some(handle) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(handle.id());
    }
    let rgba = crate::icon::render_svg(svg, size, [255, 255, 255]);
    let image = egui::ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba);
    let handle = ctx.load_texture(format!("glyph-{app}-{size}"), image, TextureOptions::LINEAR);
    let tex = handle.id();
    ctx.data_mut(|d| d.insert_temp(id, handle));
    Some(tex)
}

/// Paints `app`'s glyph at 16 px, centred on `center`, in `color`.
pub fn paint(p: &Painter, center: Pos2, app: &str, color: Color32) {
    paint_sized(p, center, app, 16.0, color);
}

pub fn paint_sized(p: &Painter, center: Pos2, app: &str, px: f32, color: Color32) {
    if let Some(tex) = texture(p.ctx(), app, px) {
        p.image(tex, Rect::from_center_size(center, Vec2::splat(px)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), color);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn textures_are_cached_per_size() {
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let a = super::texture(ctx, "arcade.box", 16.0).unwrap();
            assert_eq!(super::texture(ctx, "arcade.box", 16.0), Some(a));
            assert_ne!(super::texture(ctx, "arcade.box", 36.0), Some(a));
            assert!(super::texture(ctx, "arcade.nothing", 16.0).is_none());
        });
    }
}
