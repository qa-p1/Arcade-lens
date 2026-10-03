//! Pins: captured regions floating above everything.
//!
//! * drag to move, scroll to zoom, Ctrl/⌘+scroll for opacity
//! * double-click or Esc to close, right-click for the menu
//! * Live pins re-capture their source region a few times per second, but
//!   only while the pin does not cover that region (it would capture itself).

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Sense, Stroke, StrokeKind, Vec2, ViewportBuilder, ViewportCommand, ViewportId};
use image::RgbaImage;
use lens_core::geometry::Rect;

use super::theme::{self, ACCENT_SOFT, TEXT};

pub struct Pin {
    pub id: u64,
    pub image: Arc<RgbaImage>,
    texture: Option<egui::TextureHandle>,
    /// Where it came from, in physical virtual-desktop pixels.
    pub source: Option<Rect>,
    pub zoom: f32,
    pub opacity: f32,
    pub live: bool,
    last_live: Instant,
    pub closed: bool,
    initial_pos: Pos2,
    scale: f32,
}

pub enum PinRequest {
    Copy(Arc<RgbaImage>),
    Save(Arc<RgbaImage>),
    Annotate(Arc<RgbaImage>),
    CloseAll,
}

impl Pin {
    /// `scale` converts physical pixels to the logical points of the target monitor.
    pub fn new(id: u64, image: Arc<RgbaImage>, source: Option<Rect>, scale: f32) -> Pin {
        let initial_pos = source.map_or(Pos2::new(120.0, 120.0), |r| Pos2::new(r.x as f32 / scale, r.y as f32 / scale));
        Pin { id, image, texture: None, source, zoom: 1.0, opacity: 1.0, live: false, last_live: Instant::now(), closed: false, initial_pos, scale }
    }

    pub fn viewport_id(&self) -> ViewportId {
        ViewportId::from_hash_of(("lens-pin", self.id))
    }

    pub fn builder(&self) -> ViewportBuilder {
        let size = Vec2::new(self.image.width() as f32, self.image.height() as f32) / self.scale * self.zoom;
        ViewportBuilder::default()
            .with_title("Arcade Lens Pin")
            .with_position(self.initial_pos)
            .with_inner_size(size.max(Vec2::splat(16.0)))
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_app_id("arcade-lens-pin")
    }

    fn refresh_live(&mut self, ctx: &egui::Context) {
        let Some(src) = self.source else { return };
        if self.last_live.elapsed() < Duration::from_millis(200) {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        self.last_live = Instant::now();
        ctx.request_repaint_after(Duration::from_millis(200));
        let ppp = ctx.pixels_per_point();
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
            let me = Rect::new((outer.min.x * ppp) as i32, (outer.min.y * ppp) as i32, (outer.width() * ppp) as u32, (outer.height() * ppp) as u32);
            if me.intersection(&src).is_some() {
                return; // Covering our own source; showing ourselves recursively is useless.
            }
        }
        if let Ok(img) = lens_platform::capture_rect(src) {
            self.image = Arc::new(img);
            self.texture = None;
        }
    }

    /// Draws the pin's window contents. Returns requests for the app.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<PinRequest> {
        let ctx = ui.ctx().clone();
        let mut out = Vec::new();
        if self.live {
            self.refresh_live(&ctx);
        }
        let tex = self
            .texture
            .get_or_insert_with(|| {
                let ci = egui::ColorImage::from_rgba_unmultiplied([self.image.width() as usize, self.image.height() as usize], self.image.as_raw());
                ctx.load_texture(format!("pin-{}", self.id), ci, egui::TextureOptions::LINEAR)
            })
            .clone();
        let rect = ui.max_rect();
        let resp = ui.interact(rect, egui::Id::new(("pin-body", self.id)), Sense::click_and_drag());
        let alpha = (self.opacity * 255.0) as u8;
        ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(alpha));

        if resp.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if resp.double_clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.closed = true;
        }
        let (scroll, ctrl) = ctx.input(|i| (i.smooth_scroll_delta.y, i.modifiers.ctrl || i.modifiers.command));
        if resp.hovered() && scroll.abs() > 0.5 {
            if ctrl {
                self.opacity = (self.opacity + scroll.signum() * 0.05).clamp(0.15, 1.0);
            } else {
                self.set_zoom(&ctx, self.zoom * if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 });
            }
        }
        // A pin over its own source looks exactly like the screen; a hairline
        // border makes it recognizable as a floating object.
        ui.painter().rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, Color32::from_white_alpha(60)), StrokeKind::Inside);
        if resp.hovered() {
            ui.painter().rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, ACCENT_SOFT), StrokeKind::Inside);
            let close = egui::Rect::from_min_size(Pos2::new(rect.max.x - 22.0, rect.min.y + 4.0), Vec2::splat(18.0));
            ui.painter().rect_filled(close, CornerRadius::same(9), theme::SURFACE);
            let c = close.center();
            let s = Stroke::new(1.4, TEXT);
            ui.painter().line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(4.0, 4.0)], s);
            ui.painter().line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(-4.0, 4.0)], s);
            if resp.clicked() && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| close.contains(p)) {
                self.closed = true;
            }
            if self.zoom != 1.0 || self.opacity < 1.0 || self.live {
                let mut tags = Vec::new();
                if self.zoom != 1.0 {
                    tags.push(format!("{:.0}%", self.zoom * 100.0));
                }
                if self.opacity < 1.0 {
                    tags.push(format!("opacity {:.0}%", self.opacity * 100.0));
                }
                if self.live {
                    tags.push("live".into());
                }
                theme::pill(ui.painter(), rect.left_bottom() + Vec2::new(4.0, -4.0), Align2::LEFT_BOTTOM, &tags.join(" · "), TEXT, 10.5);
            }
        }
        resp.context_menu(|ui| {
            if ui.button("Copy").clicked() {
                out.push(PinRequest::Copy(self.image.clone()));
                ui.close();
            }
            if ui.button("Save…").clicked() {
                out.push(PinRequest::Save(self.image.clone()));
                ui.close();
            }
            if ui.button("Annotate").clicked() {
                out.push(PinRequest::Annotate(self.image.clone()));
                ui.close();
            }
            ui.separator();
            ui.menu_button("Opacity", |ui| {
                for o in [1.0, 0.75, 0.5, 0.25] {
                    if ui.radio(self.opacity == o, format!("{:.0}%", o * 100.0)).clicked() {
                        self.opacity = o;
                        ui.close();
                    }
                }
            });
            if ui.button("Actual size").clicked() {
                self.set_zoom(&ctx, 1.0);
                ui.close();
            }
            if self.source.is_some() && lens_platform::live_capture_supported() && ui.checkbox(&mut self.live, "Live").changed() {
                self.last_live = Instant::now() - Duration::from_secs(1);
            }
            ui.separator();
            if ui.button("Close").clicked() {
                self.closed = true;
                ui.close();
            }
            if ui.button("Close all pins").clicked() {
                out.push(PinRequest::CloseAll);
                ui.close();
            }
        });
        out
    }

    fn set_zoom(&mut self, ctx: &egui::Context, z: f32) {
        self.zoom = z.clamp(0.1, 8.0);
        let size = Vec2::new(self.image.width() as f32, self.image.height() as f32) / self.scale * self.zoom;
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size.max(Vec2::splat(16.0))));
    }
}

/// Small floating indicator shown while a window is being recorded.
pub struct Recording {
    pub child: std::process::Child,
    pub path: std::path::PathBuf,
    pub started: Instant,
    pub title: String,
}

impl Recording {
    pub fn start(window: &lens_core::selection::WindowInfo, dir: &std::path::Path) -> Result<Recording, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = crate::host::unique_path(dir, &format!("Recording {}.mp4", chrono::Local::now().format("%Y-%m-%d at %H.%M.%S")));
        let r = window.rect;
        // Even dimensions are required by H.264.
        let (w, h) = (r.width & !1, r.height & !1);
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-framerate", "30"]);
        if cfg!(windows) {
            cmd.args(["-f", "gdigrab", "-offset_x", &r.x.to_string(), "-offset_y", &r.y.to_string(), "-video_size", &format!("{w}x{h}"), "-i", "desktop"]);
        } else {
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
            cmd.args(["-f", "x11grab", "-video_size", &format!("{w}x{h}"), "-i", &format!("{display}+{},{}", r.x, r.y)]);
        }
        cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"]).arg(&path);
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let child = cmd.spawn().map_err(|e| format!("ffmpeg: {e}"))?;
        Ok(Recording { child, path, started: Instant::now(), title: window.title.clone() })
    }

    /// Asks ffmpeg to finish the file cleanly.
    pub fn stop(mut self) -> std::path::PathBuf {
        use std::io::Write;
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return self.path;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        self.path
    }

    pub fn ui(&self, ui: &mut egui::Ui) -> bool {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(500));
        let mut stop = false;
        egui::Frame::new().fill(theme::SURFACE).inner_margin(8).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                let blink = (self.started.elapsed().as_millis() / 500).is_multiple_of(2);
                ui.painter().circle_filled(r.center(), 5.0, if blink { theme::DANGER } else { theme::DANGER.gamma_multiply(0.4) });
                let s = self.started.elapsed().as_secs();
                ui.label(egui::RichText::new(format!("REC {:02}:{:02}", s / 60, s % 60)).color(TEXT).monospace()).on_hover_text(&self.title);
                if ui.button("Stop").clicked() {
                    stop = true;
                }
            });
        });
        stop
    }
}
