//! Recovered from a recorded edit command, before rustfmt; not build-wired.
//! Lens's canvas helpers resolve the shared kit on every frame. Shared
//! controls are rendered by Link; this module defines no widget styling.
use arcade_link::design::{self, egui::{colour, components::{ButtonKind, Kit, Tone}}, Theme};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Vec2};
use lens_core::SafetyClass;
pub struct Colors {
    pub surface: Color32, pub surface_hi: Color32, pub border: Color32,
    pub text: Color32, pub muted: Color32, pub accent: Color32,
    pub accent_soft: Color32, pub accent_ink: Color32, pub external: Color32,
    pub danger: Color32, pub success: Color32, pub dim: Color32,
}
pub fn colors(ctx: &egui::Context) -> Colors {
    let theme = if ctx.style().visuals.dark_mode { Theme::Dark } else { Theme::Light };
    let p = design::palette(theme, "arcade.lens"); let ink = design::ink(theme, "arcade.lens");
    let [r,g,b,a] = p.scrim.to_be_bytes();
    Colors { surface: colour(p.panel), surface_hi: colour(p.hover), border: colour(p.line), text: colour(p.text), muted: colour(p.muted),
        accent: colour(p.accent), accent_soft: colour(p.accent_soft), accent_ink: colour(ink.accent), external: colour(ink.warn),
        danger: colour(ink.danger), success: colour(ink.ok), dim: Color32::from_rgba_unmultiplied(r,g,b,a) }
}
pub fn font(size: f32) -> FontId { design::egui::font_weight(size, 400) }
pub fn mono(size: f32) -> FontId { FontId::monospace(size) }
pub fn panel_frame(ctx: &egui::Context) -> egui::Frame { egui::Frame::popup(&ctx.style()).inner_margin(8) }
pub fn apply_style(ctx: &egui::Context, setting: &str) { design::apply_egui(ctx, setting, "arcade.lens"); }
pub struct ChipState { pub is_default: bool, pub focused: bool }
pub fn chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, safety: SafetyClass, state: ChipState) -> egui::Response { action_chip(ui, label, key, safety, state, None) }
pub fn action_chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, safety: SafetyClass, state: ChipState, peer: Option<&str>) -> egui::Response {
    let kit = Kit::new(ui, "arcade.lens");
    ui.horizontal(|ui| {
        if let Some(peer) = peer { super::glyphs::badge(ui, peer); }
        let response = kit.button(ui, label, if state.is_default || state.focused { ButtonKind::Primary } else { ButtonKind::Ghost });
        match safety { SafetyClass::External => { kit.badge(ui, "↗ online", Tone::Warn); }, SafetyClass::Dangerous => { kit.badge(ui, "asks first", Tone::Danger); }, _ => {} }
        if let Some(key) = key { kit.key_chip(ui, key); }
        response
    }).inner
}
pub fn pill(painter: &egui::Painter, anchor: Pos2, align: egui::Align2, text: &str, _fg: Color32, _size: f32) -> Rect {
    let ctx = painter.ctx();
    let mut ui = egui::Ui::new(ctx.clone(), painter.layer_id().id.with(("lens-chip", text, anchor.x.to_bits(), anchor.y.to_bits())),
        egui::UiBuilder::new().layer_id(painter.layer_id()).max_rect(Rect::from_min_size(anchor, Vec2::splat(1024.0))));
    let kit = Kit::new(&ui, "arcade.lens");
    let galley = painter.layout_no_wrap(text.into(), design::egui::font(design::text::CAPTION), colors(ctx).text);
    let rect = align.anchor_size(anchor, galley.size() + Vec2::new(12.0, 8.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| kit.key_chip(ui, text)).inner.rect
}
pub fn key_badge(painter: &egui::Painter, anchor: Pos2, key: &str, color: Color32) -> f32 { pill(painter, anchor, egui::Align2::LEFT_CENTER, key, color, design::text::CAPTION.size).width() }
pub fn safety_marker(painter: &egui::Painter, center: Pos2, safety: SafetyClass) -> f32 {
    let (label, tone) = match safety { SafetyClass::External => ("↗ online", Tone::Warn), SafetyClass::Dangerous => ("asks first", Tone::Danger), _ => return 0.0 };
    let mut ui = egui::Ui::new(painter.ctx().clone(), painter.layer_id().id.with(("lens-safety", center.x.to_bits(), center.y.to_bits())), egui::UiBuilder::new().layer_id(painter.layer_id()).max_rect(Rect::from_min_size(center - Vec2::new(0.0, 12.0), Vec2::new(100.0, 28.0))));
    let kit = Kit::new(&ui, "arcade.lens"); kit.badge(&mut ui, label, tone).rect.width()
}
