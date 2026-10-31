//! Recovered from a recorded edit command, before rustfmt; not build-wired.
//! Canonical app marks, synced unchanged from Link's design kit.
use eframe::egui::{self, Color32, Painter, Pos2};
pub fn svg(app: &str) -> &'static str {
    match app {
        "arcade.box" => include_str!("../../../../assets/glyphs/arcade.box.svg"),
        "arcade.look" => include_str!("../../../../assets/glyphs/arcade.look.svg"),
        "arcade.wheel" => include_str!("../../../../assets/glyphs/arcade.wheel.svg"),
        "arcade.clipboard" => include_str!("../../../../assets/glyphs/arcade.clipboard.svg"),
        "arcade.shelf" => include_str!("../../../../assets/glyphs/arcade.shelf.svg"),
        "arcade.find" => include_str!("../../../../assets/glyphs/arcade.find.svg"),
        "arcade.tools" => include_str!("../../../../assets/glyphs/arcade.tools.svg"),
        _ => include_str!("../../../../assets/glyphs/arcade.lens.svg"),
    }
}
pub fn texture(ctx: &egui::Context, app: &str) -> egui::TextureId {
    let id = egui::Id::new(("arcade-glyph", app));
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) { return texture.id(); }
    let mask = arcade_link::design::palette(arcade_link::design::Theme::Light, crate::link::ME).accent_text;
    let rgba = crate::icon::render(svg(app), 64, mask);
    let texture = ctx.load_texture(app, egui::ColorImage::from_rgba_unmultiplied([64,64], &rgba), egui::TextureOptions::LINEAR);
    let result = texture.id(); ctx.data_mut(|d| d.insert_temp(id, texture)); result
}
pub fn paint(painter: &Painter, center: Pos2, app: &str, color: Color32) {
    painter.image(texture(painter.ctx(), app), egui::Rect::from_center_size(center, egui::Vec2::splat(16.0)), egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0,1.0)), color);
}
pub fn badge(ui: &mut egui::Ui, app: &str) {
    if app.starts_with("arcade.") { let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0,20.0), egui::Sense::hover()); paint(ui.painter(), rect.center(), app, super::theme::colors(ui.ctx()).muted); }
}
