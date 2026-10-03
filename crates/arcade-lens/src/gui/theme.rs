//! Visual language: restrained dark surfaces, one accent, crisp 1px lines.

use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use lens_core::SafetyClass;

pub const SURFACE: Color32 = Color32::from_rgba_premultiplied(22, 22, 25, 245);
pub const SURFACE_HI: Color32 = Color32::from_rgb(39, 39, 42);
pub const BORDER: Color32 = Color32::from_rgb(63, 63, 70);
pub const TEXT: Color32 = Color32::from_rgb(250, 250, 250);
pub const MUTED: Color32 = Color32::from_rgb(161, 161, 170);
pub const FAINT: Color32 = Color32::from_rgb(113, 113, 122);
pub const ACCENT: Color32 = Color32::from_rgb(139, 92, 246);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(167, 139, 250);
pub const EXTERNAL: Color32 = Color32::from_rgb(96, 165, 250);
pub const DANGER: Color32 = Color32::from_rgb(248, 113, 113);
pub const SUCCESS: Color32 = Color32::from_rgb(74, 222, 128);
pub const DIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 110);

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(6))
        .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(90) })
}

pub fn apply_style(ctx: &egui::Context) {
    ctx.global_style_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.window_fill = Color32::from_rgb(24, 24, 27);
        s.visuals.panel_fill = Color32::from_rgb(24, 24, 27);
        s.visuals.extreme_bg_color = Color32::from_rgb(9, 9, 11);
        s.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
        s.visuals.selection.stroke = Stroke::new(1.0, ACCENT_SOFT);
        s.visuals.hyperlink_color = ACCENT_SOFT;
        s.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(212, 212, 216));
        s.visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
        s.visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
        s.visuals.widgets.active.corner_radius = CornerRadius::same(6);
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.interaction.selectable_labels = false;
    });
}

/// Small key hint, e.g. `C` or `↵`, drawn as a rounded badge.
pub fn key_badge(painter: &egui::Painter, center_left: Pos2, key: &str, color: Color32) -> f32 {
    let galley = painter.layout_no_wrap(key.to_string(), mono(10.5), color);
    let w = (galley.size().x + 8.0).max(16.0);
    let r = Rect::from_min_size(Pos2::new(center_left.x, center_left.y - 8.0), Vec2::new(w, 16.0));
    painter.rect(r, CornerRadius::same(4), Color32::from_white_alpha(10), Stroke::new(1.0, Color32::from_white_alpha(28)), StrokeKind::Inside);
    painter.galley(Pos2::new(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, color);
    w
}

/// Enter-key glyph drawn with lines (no font dependency).
pub fn enter_icon(painter: &egui::Painter, center: Pos2, color: Color32) {
    let s = Stroke::new(1.3, color);
    let (x, y) = (center.x, center.y);
    painter.line_segment([Pos2::new(x + 4.0, y - 4.0), Pos2::new(x + 4.0, y + 1.5)], s);
    painter.line_segment([Pos2::new(x + 4.0, y + 1.5), Pos2::new(x - 4.0, y + 1.5)], s);
    painter.line_segment([Pos2::new(x - 4.0, y + 1.5), Pos2::new(x - 1.5, y - 1.0)], s);
    painter.line_segment([Pos2::new(x - 4.0, y + 1.5), Pos2::new(x - 1.5, y + 4.0)], s);
}

/// Marker for actions that leave the machine (↗) or are dangerous (!).
pub fn safety_marker(painter: &egui::Painter, center: Pos2, safety: SafetyClass) -> f32 {
    match safety {
        SafetyClass::External => {
            let s = Stroke::new(1.2, EXTERNAL);
            painter.line_segment([Pos2::new(center.x - 3.0, center.y + 3.0), Pos2::new(center.x + 3.0, center.y - 3.0)], s);
            painter.line_segment([Pos2::new(center.x, center.y - 3.0), Pos2::new(center.x + 3.0, center.y - 3.0)], s);
            painter.line_segment([Pos2::new(center.x + 3.0, center.y - 3.0), Pos2::new(center.x + 3.0, center.y)], s);
            10.0
        }
        SafetyClass::Dangerous => {
            let pts = vec![Pos2::new(center.x, center.y - 4.5), Pos2::new(center.x + 5.0, center.y + 4.0), Pos2::new(center.x - 5.0, center.y + 4.0)];
            painter.add(Shape::convex_polygon(pts, DANGER.gamma_multiply(0.25), Stroke::new(1.0, DANGER)));
            painter.line_segment([Pos2::new(center.x, center.y - 1.5), Pos2::new(center.x, center.y + 1.2)], Stroke::new(1.2, DANGER));
            12.0
        }
        _ => 0.0,
    }
}

pub struct ChipState {
    pub is_default: bool,
    pub focused: bool,
}

/// A palette chip: label plus key hint. Returns the click response.
pub fn chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, safety: SafetyClass, st: ChipState) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), font(13.5), TEXT);
    let key_w = key.map_or(0.0, |k| ui.painter().layout_no_wrap(k.to_string(), mono(10.5), MUTED).size().x.max(8.0) + 8.0 + 6.0);
    let marker_w = match safety {
        SafetyClass::External => 10.0,
        SafetyClass::Dangerous => 12.0,
        _ => 0.0,
    };
    let enter_w = if st.is_default { 14.0 } else { 0.0 };
    let size = Vec2::new(galley.size().x + key_w + marker_w + enter_w + 20.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered() || st.focused;
    let (fill, stroke) = if st.is_default {
        (ACCENT.gamma_multiply(if hovered { 0.38 } else { 0.26 }), Stroke::new(1.0, ACCENT_SOFT.gamma_multiply(0.8)))
    } else if hovered {
        (SURFACE_HI, Stroke::new(1.0, BORDER))
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.0, Color32::TRANSPARENT))
    };
    p.rect(rect, CornerRadius::same(7), fill, stroke, StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    p.galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley.clone(), TEXT);
    x += galley.size().x + 6.0;
    if marker_w > 0.0 {
        safety_marker(p, Pos2::new(x + 3.0, rect.center().y), safety);
        x += marker_w;
    }
    if st.is_default {
        enter_icon(p, Pos2::new(x + 5.0, rect.center().y), ACCENT_SOFT);
        x += enter_w;
    }
    if let Some(k) = key {
        key_badge(p, Pos2::new(x, rect.center().y), k, MUTED);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small rounded label pill drawn directly with the painter.
pub fn pill(painter: &egui::Painter, anchor: Pos2, align: egui::Align2, text: &str, fg: Color32, size: f32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), font(size), fg);
    let r = align.anchor_size(anchor, galley.size() + Vec2::new(14.0, 8.0));
    painter.rect(r, CornerRadius::same(6), SURFACE, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    painter.galley(r.min + Vec2::new(7.0, 4.0), galley, fg);
    r
}
