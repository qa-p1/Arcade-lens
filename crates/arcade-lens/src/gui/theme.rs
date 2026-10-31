//! Lens's look, from the Arcade design kit: the kit palette with Lens's teal
//! accent in the current theme, the kit type scale and kit-styled canvas
//! pieces (key chips, badges, label chips, icons, the selection outline).
//!
//! Colours are read per frame: [`sync`] records the theme egui resolved for
//! this frame (the Appearance setting, or the system theme), so every surface
//! follows a theme change live.

use std::sync::atomic::{AtomicBool, Ordering};

use arcade_link::design::egui::components::{Kit, Tone};
use arcade_link::design::{self, radius, size, text, PathOp, Theme, TypeStyle};
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use lens_core::SafetyClass;

pub const APP_ID: &str = "arcade.lens";

static DARK: AtomicBool = AtomicBool::new(true);

/// Records the theme egui resolved for this frame. Call once per frame before
/// painting; the settings window and every overlay share the same context.
pub fn sync(ctx: &egui::Context) {
    DARK.store(ctx.style().visuals.dark_mode, Ordering::Relaxed);
}

pub fn current() -> Theme {
    if DARK.load(Ordering::Relaxed) {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// The kit components for the current theme.
pub fn kit() -> Kit {
    Kit::for_theme(current(), APP_ID)
}

/// One theme's colours as egui colours. Status foregrounds (`ok`, `warn`,
/// `danger`, `accent_text`) are the kit's readable inks; `*_fill` are the
/// filled status colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    pub bg: Color32,
    pub panel: Color32,
    pub sunken: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub ok: Color32,
    pub ok_soft: Color32,
    pub warn: Color32,
    pub warn_soft: Color32,
    pub danger: Color32,
    pub danger_fill: Color32,
    pub danger_soft: Color32,
    pub on_fill: Color32,
    pub scrim: Color32,
}

pub fn colours(theme: Theme) -> Colours {
    let p = design::palette(theme, APP_ID);
    let ink = design::ink(theme, APP_ID);
    let c = design::egui::colour;
    let [r, g, b, a] = design::rgba(p.scrim);
    Colours {
        bg: c(p.bg),
        panel: c(p.panel),
        sunken: c(p.sunken),
        hover: c(p.hover),
        line: c(p.line),
        line_strong: c(p.line_strong),
        text: c(p.text),
        muted: c(p.muted),
        faint: c(p.faint),
        accent: c(p.accent),
        accent_text: c(ink.accent),
        accent_soft: c(p.accent_soft),
        ok: c(ink.ok),
        ok_soft: c(p.ok_soft),
        warn: c(ink.warn),
        warn_soft: c(p.warn_soft),
        danger: c(ink.danger),
        danger_fill: c(p.danger),
        danger_soft: c(p.danger_soft),
        on_fill: c(p.on_fill),
        scrim: Color32::from_rgba_unmultiplied(r, g, b, a),
    }
}

/// The current theme's colours.
pub fn c() -> Colours {
    colours(current())
}

/// A kit type style (Inter, with the kit's medium and semibold instances).
pub fn font(style: TypeStyle) -> FontId {
    design::egui::font(style)
}

/// Body size at a kit weight, e.g. palette labels at 500.
pub fn font_weight(style: TypeStyle, weight: u16) -> FontId {
    design::egui::font_weight(style.size, weight)
}

/// The system monospace face at the kit's mono size (sizes, coordinates, code).
pub fn mono() -> FontId {
    FontId::monospace(text::MONO.size)
}

/// Mono at the key-chip size, for labels drawn in key-chip style.
pub fn mono_caption() -> FontId {
    FontId::monospace(text::CAPTION.size)
}

/// Installs Inter once and applies the kit style for the Appearance setting
/// (`"system"`, `"light"` or `"dark"`). Call again when the setting changes.
pub fn apply_style(ctx: &egui::Context, appearance: &str) {
    let installed = egui::Id::new("lens-inter-installed");
    if !ctx.data(|d| d.get_temp::<bool>(installed).unwrap_or(false)) {
        design::egui::install_inter(ctx);
        ctx.data_mut(|d| d.insert_temp(installed, true));
    }
    design::apply_egui(ctx, appearance, APP_ID);
    sync(ctx);
}

/// The kit's `shadow-pop` for the current theme.
pub fn shadow_pop() -> egui::epaint::Shadow {
    design::egui::visuals(current(), APP_ID).popup_shadow
}

/// Template B surface: `panel` fill, 1 px `line`, radius 12, `shadow-pop`,
/// 8 px padding. Opaque: no translucency over the frozen frame.
pub fn panel_frame() -> egui::Frame {
    let c = c();
    egui::Frame::new()
        .fill(c.panel)
        .stroke(Stroke::new(1.0_f32, c.line))
        .corner_radius(CornerRadius::same(radius::LG as u8))
        .inner_margin(egui::Margin::same(8))
        .shadow(shadow_pop())
}

/// A kit icon drawn with any painter (canvas surfaces have no `Ui`).
pub fn paint_icon(painter: &egui::Painter, name: &str, center: Pos2, px: f32, color: Color32) {
    let Some(icon) = design::icon(name) else { return };
    let origin = center - Vec2::splat(px * 0.5);
    let point = |x: f32, y: f32| origin + Vec2::new(x, y);
    let stroke = Stroke::new(size::ICON_STROKE * (px / 16.0).max(1.0), color);
    let fill = if icon.filled { color } else { Color32::TRANSPARENT };
    let mut points: Vec<Pos2> = Vec::new();
    let mut current = origin;
    let flush = |points: &mut Vec<Pos2>, closed: bool| {
        if points.len() > 1 {
            painter.add(egui::epaint::PathShape { points: std::mem::take(points), closed, fill, stroke: stroke.into() });
        } else {
            points.clear();
        }
    };
    for op in icon.path(px / 24.0) {
        match op {
            PathOp::Move(x, y) => {
                flush(&mut points, false);
                current = point(x, y);
                points.push(current);
            }
            PathOp::Line(x, y) => {
                current = point(x, y);
                points.push(current);
            }
            PathOp::Cubic(a, b, cx, cy, x, y) => {
                let (p0, p1, p2, p3) = (current, point(a, b), point(cx, cy), point(x, y));
                for step in 1..=12 {
                    let t = step as f32 / 12.0;
                    let s = 1.0 - t;
                    points.push((p0.to_vec2() * s.powi(3) + p1.to_vec2() * (3.0 * s * s * t) + p2.to_vec2() * (3.0 * s * t * t) + p3.to_vec2() * t.powi(3)).to_pos2());
                }
                current = p3;
            }
            PathOp::Close => flush(&mut points, true),
        }
    }
    flush(&mut points, false);
}

/// Width a key chip for `key` takes.
pub fn key_chip_width(painter: &egui::Painter, key: &str) -> f32 {
    let g = painter.layout_no_wrap(key.to_string(), font(text::CAPTION), Color32::WHITE);
    (g.size().x + 12.0).max(size::KEY_CHIP)
}

/// The kit key chip (11.5/500, 22 px high, `line-strong` border with a 2 px
/// bottom edge), drawn from its left-centre. Returns its width.
pub fn key_chip(painter: &egui::Painter, left_center: Pos2, key: &str) -> f32 {
    let c = c();
    let g = painter.layout_no_wrap(key.to_string(), font(text::CAPTION), c.text);
    let rect = Rect::from_min_size(Pos2::new(left_center.x, left_center.y - size::KEY_CHIP / 2.0), Vec2::new((g.size().x + 12.0).max(size::KEY_CHIP), size::KEY_CHIP));
    let edge = Stroke::new(1.0_f32, c.line_strong);
    painter.rect(rect, radius::XS, c.sunken, edge, StrokeKind::Inside);
    painter.hline(rect.x_range().shrink(radius::XS), rect.bottom() - 1.5, edge);
    painter.galley(rect.center() - g.size() * 0.5 - Vec2::new(0.0, 0.5), g, c.text);
    rect.width()
}

/// Foreground and fill of a kit badge tone.
pub fn tone(t: Tone) -> (Color32, Color32) {
    kit().tones(t)
}

/// A kit badge (20 px pill, caption text) drawn from its left-centre.
/// Returns its width.
pub fn badge(painter: &egui::Painter, left_center: Pos2, label: &str, t: Tone) -> f32 {
    let (fg, bg) = tone(t);
    let g = painter.layout_no_wrap(label.to_string(), font(text::CAPTION), fg);
    let rect = Rect::from_min_size(Pos2::new(left_center.x, left_center.y - 10.0), Vec2::new(g.size().x + 14.0, 20.0));
    let stroke = if t == Tone::Neutral { Stroke::new(1.0_f32, c().line) } else { Stroke::NONE };
    painter.rect(rect, radius::PILL, bg, stroke, StrokeKind::Inside);
    painter.galley(rect.center() - g.size() * 0.5, g, fg);
    rect.width()
}

/// The safety badge for an action: `warn` "online" for actions that leave
/// this computer, `danger` "asks first" for dangerous ones.
pub fn safety_badge(safety: SafetyClass) -> Option<(&'static str, Tone)> {
    match safety {
        SafetyClass::External => Some(("online", Tone::Warn)),
        SafetyClass::Dangerous => Some(("asks first", Tone::Danger)),
        _ => None,
    }
}

pub fn safety_badge_width(painter: &egui::Painter, safety: SafetyClass) -> f32 {
    safety_badge(safety).map_or(0.0, |(label, _)| painter.layout_no_wrap(label.into(), font(text::CAPTION), Color32::WHITE).size().x + 14.0)
}

/// Draws [`safety_badge`] from its left-centre; returns its width (0 for none).
pub fn paint_safety(painter: &egui::Painter, left_center: Pos2, safety: SafetyClass) -> f32 {
    match safety_badge(safety) {
        Some((label, t)) => badge(painter, left_center, label, t),
        None => 0.0,
    }
}

/// A label on the frozen frame (size, coordinates, hints) in key-chip style:
/// `sunken` fill, `line-strong` border, readable over any screenshot.
pub fn label_chip(painter: &egui::Painter, anchor: Pos2, align: egui::Align2, label: &str, monospace: bool) -> Rect {
    let c = c();
    let f = if monospace { mono_caption() } else { font(text::CAPTION) };
    let g = painter.layout_no_wrap(label.to_string(), f, c.text);
    let rect = align.anchor_size(anchor, Vec2::new(g.size().x + 14.0, size::KEY_CHIP));
    let edge = Stroke::new(1.0_f32, c.line_strong);
    painter.rect(rect, radius::XS, c.panel, edge, StrokeKind::Inside);
    painter.galley(rect.center() - g.size() * 0.5, g, c.text);
    rect
}

/// The selection outline: 1.5 px accent between a white and a black halo, so
/// it stays visible over very light and very dark screenshots in both themes.
pub fn selection_outline(painter: &egui::Painter, rect: Rect, pixel: f32) {
    let px = pixel.max(1.0);
    painter.rect_stroke(rect.expand(px * 0.75), 0.0, Stroke::new(px * 4.5, Color32::from_black_alpha(120)), StrokeKind::Middle);
    painter.rect_stroke(rect.expand(px * 0.75), 0.0, Stroke::new(px * 3.0, Color32::from_white_alpha(200)), StrokeKind::Middle);
    painter.rect_stroke(rect.expand(px * 0.75), 0.0, Stroke::new(px * 1.5, c().accent), StrokeKind::Middle);
}

/// A 1 px line drawn over the frozen screenshot (crosshair): white with a
/// dark halo underneath, so it reads on any image.
pub fn on_image_lines(painter: &egui::Painter, segments: &[[Pos2; 2]]) {
    for (width, colour) in [(3.0, Color32::from_black_alpha(90)), (1.0, Color32::from_white_alpha(170))] {
        for s in segments {
            painter.line_segment(*s, Stroke::new(width, colour));
        }
    }
}

/// Measure guides: 1.5 px accent over a dark halo.
pub fn guide_strokes() -> [Stroke; 2] {
    [Stroke::new(3.5_f32, Color32::from_black_alpha(150)), Stroke::new(1.5_f32, c().accent)]
}

/// The tint over a measured box: the accent at low opacity.
pub fn guide_fill() -> Color32 {
    c().accent.gamma_multiply(0.14)
}

/// Template B footer hint bar (28 px, `caption`; labels in `muted`, which
/// stays ≥ 4.5:1 on the panel) as a floating
/// chip row on the canvas: an optional lead text, then key chips each
/// followed by what the key does ("↵ Run", "Esc Close").
pub fn hint_bar(painter: &egui::Painter, anchor: Pos2, align: egui::Align2, lead: Option<&str>, keys: &[(&str, &str)]) -> Rect {
    let c = c();
    let caption = font(text::CAPTION);
    let lead_g = lead.map(|t| painter.layout_no_wrap(t.to_string(), caption.clone(), c.muted));
    let items: Vec<(f32, std::sync::Arc<egui::Galley>)> =
        keys.iter().map(|(k, label)| (key_chip_width(painter, k), painter.layout_no_wrap((*label).to_string(), caption.clone(), c.muted))).collect();
    let mut width = 20.0 + lead_g.as_ref().map_or(0.0, |g| g.size().x + 14.0);
    width += items.iter().map(|(kw, g)| kw + 6.0 + g.size().x + 14.0).sum::<f32>();
    let rect = align.anchor_size(anchor, Vec2::new(width - 14.0, size::HINT_BAR + 8.0));
    painter.rect(rect, radius::MD, c.panel, Stroke::new(1.0_f32, c.line), StrokeKind::Inside);
    let y = rect.center().y;
    let mut x = rect.left() + 10.0;
    if let Some(g) = lead_g {
        let w = g.size().x;
        painter.galley(Pos2::new(x, y - g.size().y / 2.0), g, c.muted);
        x += w + 14.0;
    }
    for ((key, _), (_, g)) in keys.iter().zip(items) {
        x += key_chip(painter, Pos2::new(x, y), key) + 6.0;
        let w = g.size().x;
        painter.galley(Pos2::new(x, y - g.size().y / 2.0), g, c.muted);
        x += w + 14.0;
    }
    rect
}

/// Kit resize handle: 8 px, radius 2, `panel` fill with an accent border.
pub fn handle(painter: &egui::Painter, center: Pos2) {
    let c = c();
    let r = Rect::from_center_size(center, Vec2::splat(8.0));
    painter.rect(r, 2.0, c.panel, Stroke::new(1.5_f32, c.accent), StrokeKind::Middle);
}

pub struct ChipState {
    pub is_default: bool,
    pub focused: bool,
}

/// A chip in the compact palette row: label plus key chip.
pub fn chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, safety: SafetyClass, st: ChipState) -> egui::Response {
    action_chip(ui, label, key, safety, st, None)
}

/// A compact palette chip: optional peer glyph, label (body 500), safety
/// badge, and a key chip ("↵" on the default action). The default action
/// carries `accent-soft` with an accent edge; hover uses `hover`.
pub fn action_chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, safety: SafetyClass, st: ChipState, peer: Option<&str>) -> egui::Response {
    let c = c();
    let painter = ui.painter().clone();
    let galley = painter.layout_no_wrap(label.to_string(), font_weight(text::BODY, 500), c.text);
    let shown_key = if st.is_default { Some("↵") } else { key };
    let key_w = shown_key.map_or(0.0, |k| key_chip_width(&painter, k) + 6.0);
    let badge_w = if peer.is_some() && safety == SafetyClass::External { 0.0 } else { safety_badge_width(&painter, safety) };
    let badge_w = if badge_w > 0.0 { badge_w + 6.0 } else { 0.0 };
    let glyph_w = if peer.is_some() { 22.0 } else { 0.0 };
    let size = Vec2::new(galley.size().x + key_w + badge_w + glyph_w + 20.0, size::CONTROL);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered() || st.focused;
    let (fill, stroke) = if st.is_default {
        (c.accent_soft, Stroke::new(1.0_f32, c.accent))
    } else if hovered {
        (c.hover, Stroke::new(1.0_f32, c.line))
    } else {
        (Color32::TRANSPARENT, Stroke::NONE)
    };
    painter.rect(rect, radius::SM, fill, stroke, StrokeKind::Inside);
    let y = rect.center().y;
    let mut x = rect.left() + 10.0;
    if let Some(peer) = peer {
        super::glyphs::paint(&painter, Pos2::new(x + 8.0, y), peer, c.muted);
        x += glyph_w;
    }
    painter.galley(Pos2::new(x, y - galley.size().y / 2.0), galley.clone(), c.text);
    x += galley.size().x + 6.0;
    if badge_w > 0.0 {
        x += paint_safety(&painter, Pos2::new(x, y), safety) + 6.0;
    }
    if let Some(k) = shown_key {
        key_chip(&painter, Pos2::new(x, y), k);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f64 {
        let f = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn lens_uses_its_teal_accent_in_both_themes() {
        assert_eq!(colours(Theme::Light).accent, Color32::from_rgb(0x1e, 0x7e, 0x72));
        assert_eq!(colours(Theme::Dark).accent, Color32::from_rgb(0x2b, 0xb3, 0xa3));
    }

    #[test]
    fn overlay_text_is_readable_in_both_themes() {
        for theme in [Theme::Light, Theme::Dark] {
            let c = colours(theme);
            for (name, fg) in [("text", c.text), ("muted", c.muted), ("accent", c.accent_text), ("warn", c.warn), ("danger", c.danger), ("ok", c.ok)] {
                for (surface, bg) in [("panel", c.panel), ("hover", c.hover), ("sunken", c.sunken)] {
                    let ratio = contrast(fg, bg);
                    assert!(ratio >= 4.5, "{theme:?} {name} on {surface}: {ratio:.2}");
                }
            }
            let (fg, bg) = Kit::for_theme(theme, APP_ID).tones(Tone::Accent);
            assert!(contrast(fg, bg) >= 4.5, "{theme:?} accent badge");
            assert!(contrast(c.text, c.accent_soft) >= 4.5, "{theme:?} text on the selected row");
        }
    }

    #[test]
    fn safety_is_a_badge_not_coloured_text() {
        assert_eq!(safety_badge(SafetyClass::External), Some(("online", Tone::Warn)));
        assert_eq!(safety_badge(SafetyClass::Dangerous), Some(("asks first", Tone::Danger)));
        assert_eq!(safety_badge(SafetyClass::Pure), None);
    }

    #[test]
    fn theme_follows_the_context() {
        let ctx = egui::Context::default();
        apply_style(&ctx, "light");
        let _ = ctx.run(egui::RawInput::default(), |ctx| sync(ctx));
        assert_eq!(current(), Theme::Light);
        apply_style(&ctx, "dark");
        let _ = ctx.run(egui::RawInput::default(), |ctx| sync(ctx));
        assert_eq!(current(), Theme::Dark);
    }
}
