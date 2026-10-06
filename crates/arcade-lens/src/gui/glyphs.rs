//! Vector copies of Arcade-link/assets/glyphs (16×16, stroke 1.5).
//! Tokens from assets/tokens.json apply only to integration surfaces.
use eframe::egui::{self, Color32, Painter, Pos2, Stroke};

pub const SURFACE: Color32 = Color32::from_rgb(20, 23, 28);
pub const BORDER: Color32 = Color32::from_rgb(42, 47, 56);
pub const TEXT: Color32 = Color32::from_rgb(231, 234, 240);
pub const MUTED: Color32 = Color32::from_rgb(154, 163, 178);

pub fn paint(p: &Painter, center: Pos2, app: &str, color: Color32) {
    let point = |x, y| center + egui::vec2(x - 8.0, y - 8.0);
    let stroke = Stroke::new(1.5, color);
    let line = |points: &[(f32, f32)]| p.add(egui::Shape::line(points.iter().map(|&(x, y)| point(x, y)).collect(), stroke));
    let curve = |points: [(f32, f32); 4]| {
        p.add(egui::epaint::CubicBezierShape::from_points_stroke(points.map(|(x, y)| point(x, y)), false, Color32::TRANSPARENT, stroke));
    };
    match app {
        "arcade.box" => {
            line(&[(8.0, 1.75), (14.0, 4.75), (14.0, 11.25), (8.0, 14.25), (2.0, 11.25), (2.0, 4.75), (8.0, 1.75)]);
            line(&[(2.0, 4.75), (8.0, 7.75), (14.0, 4.75)]);
            line(&[(8.0, 7.75), (8.0, 14.25)]);
        }
        "arcade.look" => {
            curve([(1.25, 8.0), (1.25, 8.0), (3.75, 3.25), (8.0, 3.25)]);
            curve([(8.0, 3.25), (12.25, 3.25), (14.75, 8.0), (14.75, 8.0)]);
            curve([(14.75, 8.0), (14.75, 8.0), (12.25, 12.75), (8.0, 12.75)]);
            curve([(8.0, 12.75), (3.75, 12.75), (1.25, 8.0), (1.25, 8.0)]);
            p.circle_stroke(center, 2.0, stroke);
        }
        "arcade.wheel" => {
            p.circle_stroke(center, 6.25, stroke);
            p.circle_stroke(center, 1.75, stroke);
            for (a, b) in [((8.0, 1.75), (8.0, 6.25)), ((8.0, 9.75), (8.0, 14.25)), ((1.75, 8.0), (6.25, 8.0)), ((9.75, 8.0), (14.25, 8.0))] {
                line(&[a, b]);
            }
        }
        "arcade.clipboard" => {
            line(&[(5.5, 2.75), (4.0, 2.75)]);
            curve([(4.0, 2.75), (3.31, 2.75), (2.75, 3.31), (2.75, 4.0)]);
            line(&[(2.75, 4.0), (2.75, 13.0)]);
            curve([(2.75, 13.0), (2.75, 13.69), (3.31, 14.25), (4.0, 14.25)]);
            line(&[(4.0, 14.25), (12.0, 14.25)]);
            curve([(12.0, 14.25), (12.69, 14.25), (13.25, 13.69), (13.25, 13.0)]);
            line(&[(13.25, 13.0), (13.25, 4.0)]);
            curve([(13.25, 4.0), (13.25, 3.31), (12.69, 2.75), (12.0, 2.75)]);
            line(&[(12.0, 2.75), (10.5, 2.75)]);
            p.rect_stroke(egui::Rect::from_min_max(point(5.5, 1.5), point(10.5, 4.0)), 1, stroke, egui::StrokeKind::Middle);
            line(&[(5.5, 8.0), (10.5, 8.0)]);
            line(&[(5.5, 10.75), (9.0, 10.75)]);
        }
        "arcade.tools" => {
            curve([(10.5, 1.9), (7.7, 1.0), (5.3, 3.8), (6.6, 6.6)]);
            line(&[(6.6, 6.6), (1.9, 11.3)]);
            curve([(1.9, 11.3), (0.6, 12.6), (2.6, 14.6), (3.9, 13.3)]);
            line(&[(3.9, 13.3), (8.6, 8.6)]);
            curve([(8.6, 8.6), (11.4, 9.9), (14.2, 7.5), (13.3, 4.7)]);
            line(&[(13.3, 4.7), (11.3, 6.7), (9.4, 6.3), (9.0, 4.4), (10.5, 1.9)]);
        }
        _ => {}
    }
}

pub fn badge(ui: &mut egui::Ui, app: &str) {
    if app.starts_with("arcade.") {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        paint(ui.painter(), rect.center(), app, MUTED);
    }
}
