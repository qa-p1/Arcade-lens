//! Lightweight annotation: mark up a capture, then copy, save or pin it.
//!
//! Marks are stored in image pixel coordinates, drawn live with the egui
//! painter, and rasterized for export with tiny-skia (shapes) and ab_glyph
//! (text), so the exported image matches what was on screen.

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect as ERect, RichText, Sense, Shape, Stroke, Vec2, ViewportBuilder, ViewportId};
use image::{Rgba, RgbaImage};

use super::theme::{self, ACCENT, MUTED, TEXT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Highlighter,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Text,
    Pixelate,
}

impl Tool {
    const ALL: [Tool; 8] = [Tool::Pen, Tool::Highlighter, Tool::Line, Tool::Arrow, Tool::Rect, Tool::Ellipse, Tool::Text, Tool::Pixelate];
    fn label(&self) -> &'static str {
        match self {
            Tool::Pen => "Pen",
            Tool::Highlighter => "Highlight",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Rect => "Box",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::Pixelate => "Redact",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Stroke { points: Vec<[f32; 2]>, color: [u8; 4], width: f32, highlight: bool },
    Line { a: [f32; 2], b: [f32; 2], color: [u8; 4], width: f32, arrow: bool },
    Rect { min: [f32; 2], max: [f32; 2], color: [u8; 4], width: f32 },
    Ellipse { min: [f32; 2], max: [f32; 2], color: [u8; 4], width: f32 },
    Text { pos: [f32; 2], text: String, color: [u8; 4], size: f32 },
    Pixelate { min: [f32; 2], max: [f32; 2] },
}

const COLORS: [[u8; 4]; 8] = [
    [239, 68, 68, 255],
    [249, 115, 22, 255],
    [234, 179, 8, 255],
    [34, 197, 94, 255],
    [59, 130, 246, 255],
    [139, 92, 246, 255],
    [250, 250, 250, 255],
    [24, 24, 27, 255],
];

pub enum EditorRequest {
    Copy(Arc<RgbaImage>),
    Save(Arc<RgbaImage>),
    Pin(Arc<RgbaImage>),
}

pub struct Editor {
    pub id: u64,
    original: Arc<RgbaImage>,
    base: RgbaImage,
    texture: Option<egui::TextureHandle>,
    marks: Vec<Mark>,
    redo: Vec<Mark>,
    tool: Tool,
    color: [u8; 4],
    width: f32,
    current: Option<Mark>,
    text_at: Option<([f32; 2], String)>,
    pub closed: bool,
}

/// Applies mosaic pixelation to an area (in place).
pub fn pixelate(img: &mut RgbaImage, min: [f32; 2], max: [f32; 2]) {
    let block = ((max[0] - min[0]).abs().min((max[1] - min[1]).abs()) / 6.0).clamp(6.0, 16.0) as u32;
    let (x0, y0) = (min[0].min(max[0]).max(0.0) as u32, min[1].min(max[1]).max(0.0) as u32);
    let (x1, y1) = ((min[0].max(max[0]) as u32).min(img.width()), (min[1].max(max[1]) as u32).min(img.height()));
    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let (ex, ey) = ((bx + block).min(x1), (by + block).min(y1));
            let mut sum = [0u64; 4];
            let mut n = 0;
            for y in by..ey {
                for x in bx..ex {
                    let p = img.get_pixel(x, y);
                    for i in 0..4 {
                        sum[i] += p[i] as u64;
                    }
                    n += 1;
                }
            }
            if let Some(n) = std::num::NonZeroU64::new(n) {
                let avg = Rgba([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, (sum[3] / n) as u8]);
                for y in by..ey {
                    for x in bx..ex {
                        img.put_pixel(x, y, avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

fn paint(c: [u8; 4], alpha: f32) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], (c[3] as f32 * alpha) as u8);
    p.anti_alias = true;
    p
}

fn stroke(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke { width, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() }
}

fn arrow_head(a: [f32; 2], b: [f32; 2], width: f32) -> [[f32; 2]; 3] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let (ux, uy) = (dx / len, dy / len);
    let size = (width * 4.0).max(10.0);
    let base = [b[0] - ux * size, b[1] - uy * size];
    let (px, py) = (-uy * size * 0.5, ux * size * 0.5);
    [b, [base[0] + px, base[1] + py], [base[0] - px, base[1] - py]]
}

/// Draws text with the bundled UI font onto `img`.
fn draw_text(img: &mut RgbaImage, pos: [f32; 2], text: &str, color: [u8; 4], size: f32) {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
    let defs = egui::FontDefinitions::default();
    let Some(data) = defs.font_data.get("Ubuntu-Light").or_else(|| defs.font_data.values().next()) else { return };
    let Ok(font) = FontRef::try_from_slice(&data.font) else { return };
    let scaled = font.as_scaled(PxScale::from(size));
    let mut caret = ab_glyph::point(pos[0], pos[1] + scaled.ascent());
    let mut prev = None;
    for ch in text.chars() {
        if ch == '\n' {
            caret = ab_glyph::point(pos[0], caret.y + scaled.height() + scaled.line_gap());
            prev = None;
            continue;
        }
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            caret.x += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(size), caret);
        caret.x += scaled.h_advance(id);
        prev = Some(id);
        if let Some(outline) = font.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|x, y, cov| {
                let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    return;
                }
                let dst = img.get_pixel_mut(px as u32, py as u32);
                let a = cov * color[3] as f32 / 255.0;
                for i in 0..3 {
                    dst[i] = (color[i] as f32 * a + dst[i] as f32 * (1.0 - a)).round() as u8;
                }
                dst[3] = dst[3].max((a * 255.0) as u8);
            });
        }
    }
}

/// Rasterizes `marks` over `base` (which already has pixelation applied).
pub fn render(base: &RgbaImage, marks: &[Mark]) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut pm = tiny_skia::Pixmap::new(w, h).expect("non-empty image");
    for (dst, src) in pm.pixels_mut().iter_mut().zip(base.pixels()) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    let t = tiny_skia::Transform::identity();
    let mut texts = Vec::new();
    for m in marks {
        match m {
            Mark::Stroke { points, color, width, highlight } => {
                let mut pb = tiny_skia::PathBuilder::new();
                if let Some(f) = points.first() {
                    pb.move_to(f[0], f[1]);
                    for p in points.iter().skip(1) {
                        pb.line_to(p[0], p[1]);
                    }
                    if points.len() == 1 {
                        pb.line_to(f[0] + 0.1, f[1]);
                    }
                }
                if let Some(path) = pb.finish() {
                    let (alpha, wmul) = if *highlight { (0.35, 3.0) } else { (1.0, 1.0) };
                    pm.stroke_path(&path, &paint(*color, alpha), &stroke(width * wmul), t, None);
                }
            }
            Mark::Line { a, b, color, width, arrow } => {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(a[0], a[1]);
                pb.line_to(b[0], b[1]);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint(*color, 1.0), &stroke(*width), t, None);
                }
                if *arrow {
                    let hd = arrow_head(*a, *b, *width);
                    let mut pb = tiny_skia::PathBuilder::new();
                    pb.move_to(hd[0][0], hd[0][1]);
                    pb.line_to(hd[1][0], hd[1][1]);
                    pb.line_to(hd[2][0], hd[2][1]);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        pm.fill_path(&path, &paint(*color, 1.0), tiny_skia::FillRule::Winding, t, None);
                    }
                }
            }
            Mark::Rect { min, max, color, width } | Mark::Ellipse { min, max, color, width } => {
                let r = tiny_skia::Rect::from_ltrb(min[0].min(max[0]), min[1].min(max[1]), min[0].max(max[0]), min[1].max(max[1]));
                if let Some(r) = r {
                    let path = if matches!(m, Mark::Rect { .. }) { Some(tiny_skia::PathBuilder::from_rect(r)) } else { tiny_skia::PathBuilder::from_oval(r) };
                    if let Some(path) = path {
                        pm.stroke_path(&path, &paint(*color, 1.0), &stroke(*width), t, None);
                    }
                }
            }
            Mark::Text { pos, text, color, size } => texts.push((*pos, text.clone(), *color, *size)),
            Mark::Pixelate { .. } => {}
        }
    }
    let mut out = RgbaImage::from_fn(w, h, |x, y| {
        let c = pm.pixel(x, y).unwrap().demultiply();
        Rgba([c.red(), c.green(), c.blue(), c.alpha()])
    });
    for (pos, text, color, size) in texts {
        draw_text(&mut out, pos, &text, color, size);
    }
    out
}

impl Editor {
    pub fn new(id: u64, image: Arc<RgbaImage>) -> Editor {
        Editor {
            id,
            base: (*image).clone(),
            original: image,
            texture: None,
            marks: Vec::new(),
            redo: Vec::new(),
            tool: Tool::Arrow,
            color: COLORS[0],
            width: 4.0,
            current: None,
            text_at: None,
            closed: false,
        }
    }

    pub fn viewport_id(&self) -> ViewportId {
        ViewportId::from_hash_of(("lens-annotate", self.id))
    }

    pub fn builder(&self) -> ViewportBuilder {
        let (w, h) = (self.original.width() as f32, self.original.height() as f32);
        let size = Vec2::new(w.clamp(900.0, 1400.0), (h + 90.0).clamp(360.0, 900.0));
        ViewportBuilder::default()
            .with_title("Annotate — Arcade Lens")
            .with_inner_size(size)
            .with_min_inner_size([480.0, 300.0])
            .with_always_on_top()
            .with_app_id("arcade-lens-annotate")
            .with_icon(crate::icon::window_icon())
    }

    fn rebuild_base(&mut self) {
        self.base = (*self.original).clone();
        for m in &self.marks {
            if let Mark::Pixelate { min, max } = m {
                pixelate(&mut self.base, *min, *max);
            }
        }
        self.texture = None;
    }

    fn push(&mut self, m: Mark) {
        let pix = matches!(m, Mark::Pixelate { .. });
        self.marks.push(m);
        self.redo.clear();
        if pix {
            self.rebuild_base();
        }
    }

    fn undo(&mut self) {
        if let Some(m) = self.marks.pop() {
            let pix = matches!(m, Mark::Pixelate { .. });
            self.redo.push(m);
            if pix {
                self.rebuild_base();
            }
        }
    }

    fn redo(&mut self) {
        if let Some(m) = self.redo.pop() {
            let pix = matches!(m, Mark::Pixelate { .. });
            self.marks.push(m);
            if pix {
                self.rebuild_base();
            }
        }
    }

    pub fn export(&self) -> Arc<RgbaImage> {
        Arc::new(render(&self.base, &self.marks))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<EditorRequest> {
        let ctx = ui.ctx().clone();
        let mut out = Vec::new();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.closed = true;
        }
        let typing = self.text_at.is_some();
        ctx.input_mut(|i| {
            let cmd = egui::Modifiers::COMMAND;
            if i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::Z) || i.consume_key(cmd, egui::Key::Y) {
                self.redo();
            } else if i.consume_key(cmd, egui::Key::Z) {
                self.undo();
            }
            if i.consume_key(cmd, egui::Key::C) {
                out.push(EditorRequest::Copy(self.export()));
            }
            if i.consume_key(cmd, egui::Key::S) {
                out.push(EditorRequest::Save(self.export()));
            }
            if !typing {
                for (n, t) in Tool::ALL.iter().enumerate() {
                    let key = [
                        egui::Key::Num1,
                        egui::Key::Num2,
                        egui::Key::Num3,
                        egui::Key::Num4,
                        egui::Key::Num5,
                        egui::Key::Num6,
                        egui::Key::Num7,
                        egui::Key::Num8,
                    ][n];
                    if i.consume_key(egui::Modifiers::NONE, key) {
                        self.tool = *t;
                    }
                }
            }
        });

        egui::Panel::top("toolbar").frame(egui::Frame::new().fill(Color32::from_rgb(24, 24, 27)).inner_margin(8)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (n, t) in Tool::ALL.iter().enumerate() {
                    let sel = self.tool == *t;
                    let b = egui::Button::new(RichText::new(t.label()).size(12.5).color(if sel { Color32::WHITE } else { TEXT })).fill(if sel {
                        ACCENT.gamma_multiply(0.7)
                    } else {
                        theme::SURFACE_HI
                    });
                    if ui.add(b).on_hover_text(format!("{} ({})", t.label(), n + 1)).clicked() {
                        self.tool = *t;
                    }
                }
                ui.separator();
                for c in COLORS {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 7.5, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
                    if self.color == c {
                        ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, TEXT));
                    }
                    if resp.clicked() {
                        self.color = c;
                    }
                }
                ui.separator();
                ui.add(egui::Slider::new(&mut self.width, 1.0..=16.0).show_value(false)).on_hover_text("Stroke width");
                ui.separator();
                if ui.add_enabled(!self.marks.is_empty(), egui::Button::new("Undo")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo")).clicked() {
                    self.redo();
                }
                ui.separator();
                if ui.add(egui::Button::new(RichText::new("Copy").color(Color32::WHITE)).fill(ACCENT.gamma_multiply(0.85))).clicked() {
                    out.push(EditorRequest::Copy(self.export()));
                }
                if ui.button("Save").clicked() {
                    out.push(EditorRequest::Save(self.export()));
                }
                if ui.button("Pin").clicked() {
                    out.push(EditorRequest::Pin(self.export()));
                }
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(9, 9, 11))).show(ui, |ui| {
            let avail = ui.max_rect();
            let (iw, ih) = (self.base.width() as f32, self.base.height() as f32);
            // Fit inside the window, never upscale past 1:1 physical pixels.
            let scale = (avail.width() / iw).min(avail.height() / ih).min(1.0 / ctx.pixels_per_point());
            let size = Vec2::new(iw, ih) * scale;
            let canvas = ERect::from_center_size(avail.center(), size);
            let tex = self
                .texture
                .get_or_insert_with(|| {
                    ctx.load_texture(
                        format!("annotate-{}", self.id),
                        egui::ColorImage::from_rgba_unmultiplied([iw as usize, ih as usize], self.base.as_raw()),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .clone();
            let resp = ui.allocate_rect(canvas, Sense::click_and_drag());
            let painter = ui.painter_at(avail);
            painter.image(tex.id(), canvas, ERect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let to_img = |p: Pos2| [((p.x - canvas.min.x) / scale).clamp(0.0, iw), ((p.y - canvas.min.y) / scale).clamp(0.0, ih)];
            let to_scr = |p: [f32; 2]| Pos2::new(canvas.min.x + p[0] * scale, canvas.min.y + p[1] * scale);

            if let Some(pos) = resp.interact_pointer_pos() {
                let ip = to_img(pos);
                if resp.drag_started() {
                    self.current = Some(match self.tool {
                        Tool::Pen | Tool::Highlighter => {
                            Mark::Stroke { points: vec![ip], color: self.color, width: self.width, highlight: self.tool == Tool::Highlighter }
                        }
                        Tool::Line | Tool::Arrow => Mark::Line { a: ip, b: ip, color: self.color, width: self.width, arrow: self.tool == Tool::Arrow },
                        Tool::Rect => Mark::Rect { min: ip, max: ip, color: self.color, width: self.width },
                        Tool::Ellipse => Mark::Ellipse { min: ip, max: ip, color: self.color, width: self.width },
                        Tool::Pixelate => Mark::Pixelate { min: ip, max: ip },
                        Tool::Text => Mark::Text { pos: ip, text: String::new(), color: self.color, size: 18.0 },
                    });
                } else if resp.dragged() {
                    match &mut self.current {
                        Some(Mark::Stroke { points, .. }) => points.push(ip),
                        Some(Mark::Line { b, .. }) => *b = ip,
                        Some(Mark::Rect { max, .. } | Mark::Ellipse { max, .. } | Mark::Pixelate { max, .. }) => *max = ip,
                        _ => {}
                    }
                }
                if resp.clicked() && self.tool == Tool::Text {
                    self.text_at = Some((ip, String::new()));
                }
            }
            if resp.drag_stopped() {
                if let Some(m) = self.current.take() {
                    if !matches!(m, Mark::Text { .. }) {
                        self.push(m);
                    }
                }
            }
            for m in self.marks.iter().chain(self.current.iter()) {
                draw_mark(&painter, m, &to_scr, scale);
            }
            if let Some((pos, text)) = &mut self.text_at {
                let p = to_scr(*pos);
                let mut commit = false;
                let mut cancel = false;
                egui::Area::new(egui::Id::new(("annotate-text", self.id))).fixed_pos(p).order(egui::Order::Foreground).show(&ctx, |ui| {
                    theme::panel_frame().show(ui, |ui| {
                        let r = ui.add(egui::TextEdit::singleline(text).hint_text("Type, then Enter").desired_width(220.0));
                        r.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            commit = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                    });
                });
                if commit {
                    let (pos, text) = self.text_at.take().unwrap();
                    if !text.trim().is_empty() {
                        let size = (18.0 / scale.max(0.1)).clamp(12.0, 64.0);
                        self.push(Mark::Text { pos, text, color: self.color, size });
                    }
                } else if cancel {
                    self.text_at = None;
                }
            }
            if self.marks.is_empty() && self.current.is_none() {
                painter.text(
                    canvas.center_bottom() + Vec2::new(0.0, -12.0),
                    egui::Align2::CENTER_BOTTOM,
                    "Draw on the image · 1–8 tools · Ctrl+Z undo · Ctrl+C copy",
                    theme::font(12.0),
                    MUTED,
                );
            }
        });
        out
    }
}

fn draw_mark(p: &egui::Painter, m: &Mark, to_scr: &dyn Fn([f32; 2]) -> Pos2, scale: f32) {
    let col = |c: &[u8; 4], a: f32| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (c[3] as f32 * a) as u8);
    match m {
        Mark::Stroke { points, color, width, highlight } => {
            let (a, wm) = if *highlight { (0.35, 3.0) } else { (1.0, 1.0) };
            let pts: Vec<Pos2> = points.iter().map(|q| to_scr(*q)).collect();
            p.add(Shape::line(pts, Stroke::new(width * wm * scale, col(color, a))));
        }
        Mark::Line { a, b, color, width, arrow } => {
            p.line_segment([to_scr(*a), to_scr(*b)], Stroke::new(width * scale, col(color, 1.0)));
            if *arrow {
                let h = arrow_head(*a, *b, *width);
                p.add(Shape::convex_polygon(h.iter().map(|q| to_scr(*q)).collect(), col(color, 1.0), Stroke::NONE));
            }
        }
        Mark::Rect { min, max, color, width } => {
            p.rect_stroke(ERect::from_two_pos(to_scr(*min), to_scr(*max)), 0.0, Stroke::new(width * scale, col(color, 1.0)), egui::StrokeKind::Middle);
        }
        Mark::Ellipse { min, max, color, width } => {
            let r = ERect::from_two_pos(to_scr(*min), to_scr(*max));
            p.add(Shape::ellipse_stroke(r.center(), r.size() / 2.0, Stroke::new(width * scale, col(color, 1.0))));
        }
        Mark::Text { pos, text, color, size } => {
            p.text(to_scr(*pos), egui::Align2::LEFT_TOP, text, theme::font(size * scale), col(color, 1.0));
        }
        Mark::Pixelate { min, max } => {
            p.rect_stroke(ERect::from_two_pos(to_scr(*min), to_scr(*max)), 0.0, Stroke::new(1.0, Color32::from_white_alpha(90)), egui::StrokeKind::Middle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_draws_shapes_and_pixelates() {
        let mut base = RgbaImage::from_fn(100, 60, |x, _| if x < 50 { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) });
        pixelate(&mut base, [40.0, 0.0], [60.0, 60.0]);
        // The mosaic blends the black/white boundary.
        assert!((1..255).contains(&base.get_pixel(49, 10)[0]));
        let out = render(
            &base,
            &[
                Mark::Line { a: [5.0, 30.0], b: [35.0, 30.0], color: [239, 68, 68, 255], width: 4.0, arrow: true },
                Mark::Text { pos: [70.0, 10.0], text: "Hi".into(), color: [250, 250, 250, 255], size: 20.0 },
            ],
        );
        assert_eq!(*out.get_pixel(20, 30), Rgba([239, 68, 68, 255]));
        assert_eq!(*out.get_pixel(20, 10), Rgba([255, 255, 255, 255]));
        // Some text pixels were drawn on the black half.
        assert!((70..95).any(|x| (10..35).any(|y| out.get_pixel(x, y)[0] > 128)));
    }
}
