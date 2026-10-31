//! Pins: captured regions floating above everything.
//!
//! * drag to move, scroll to zoom, Ctrl/⌘+scroll for opacity
//! * hover for close, copy and opacity; zoom and opacity show as a readout
//! * double-click or Esc to close, right-click for the menu
//! * Live pins re-capture their source region a few times per second, but
//!   only while the pin does not cover that region (it would capture itself).

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Pos2, Sense, Stroke, StrokeKind, Vec2, ViewportBuilder, ViewportCommand, ViewportId};
use image::RgbaImage;
use lens_core::geometry::Rect;

use arcade_link::design::{motion, radius, size, text};

use super::theme;

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
    readout: Option<(String, Instant)>,
    /// The pointer is on the mini toolbar (keeps it shown at the edge).
    toolbar_hot: bool,
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
        // The image, not the window, lines up with where it came from.
        let initial_pos = source.map_or(Pos2::new(120.0, 120.0), |r| Pos2::new(r.x as f32 / scale, r.y as f32 / scale)) - Vec2::splat(MARGIN);
        Pin {
            id,
            image,
            texture: None,
            source,
            zoom: 1.0,
            opacity: 1.0,
            live: false,
            last_live: Instant::now(),
            closed: false,
            initial_pos,
            scale,
            readout: None,
            toolbar_hot: false,
        }
    }

    pub fn viewport_id(&self) -> ViewportId {
        ViewportId::from_hash_of(("lens-pin", self.id))
    }

    pub fn builder(&self) -> ViewportBuilder {
        ViewportBuilder::default()
            .with_title("Pin — Arcade Lens")
            .with_position(self.initial_pos)
            .with_inner_size(self.window_size())
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_app_id(crate::link::OS_ID)
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
        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect).map(|r| r.shrink(MARGIN)) {
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
        let c = theme::c();
        let kit = theme::kit();
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
        let rect = ui.max_rect().shrink(MARGIN);
        let corner = CornerRadius::same(radius::MD as u8);
        // The window is transparent around the image; the shadow lives there.
        ui.painter().add(pin_shadow().as_shape(rect, corner));
        let resp = ui.interact(rect, egui::Id::new(("pin-body", self.id)), Sense::click_and_drag());
        let alpha = (self.opacity * 255.0) as u8;
        egui::Image::from_texture((tex.id(), rect.size())).corner_radius(corner).tint(Color32::from_white_alpha(alpha)).paint_at(ui, rect);
        // A pin over its own source looks exactly like the screen; the border
        // makes it recognizable as a floating object.
        let hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| rect.contains(p));
        ui.painter().rect_stroke(rect, corner, Stroke::new(1.0_f32, if hovered { c.line_strong } else { c.line }), StrokeKind::Inside);

        if resp.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if resp.double_clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.closed = true;
        }
        let (scroll, ctrl) = ctx.input(|i| (i.smooth_scroll_delta.y, i.modifiers.ctrl || i.modifiers.command));
        if hovered && scroll.abs() > 0.5 {
            if ctrl {
                self.opacity = (self.opacity + scroll.signum() * 0.05).clamp(0.15, 1.0);
                self.readout(format!("Opacity {}", percent(self.opacity)));
            } else {
                self.set_zoom(&ctx, self.zoom * if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 });
                self.readout(format!("Zoom {}", percent(self.zoom)));
            }
        }

        // Hover mini toolbar: close, copy, opacity. Only where it fits.
        let bar = egui::Rect::from_min_size(
            rect.right_top() + Vec2::new(-(3.0 * size::CONTROL_SM + 2.0 * 2.0 + 12.0) - 6.0, 6.0),
            Vec2::new(3.0 * size::CONTROL_SM + 2.0 * 2.0 + 12.0, size::CONTROL_SM + 8.0),
        );
        let toolbar_fits = rect.width() >= bar.width() + 12.0 && rect.height() >= bar.height() + 12.0;
        if (hovered || self.toolbar_hot) && toolbar_fits {
            let mut hot = false;
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                ui.painter().add(theme::shadow_pop().as_shape(bar, radius::SM));
                ui.painter().rect(bar, radius::SM, c.panel, Stroke::new(1.0_f32, c.line), StrokeKind::Inside);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let opacity = kit.icon_button(ui, "eye", "Opacity");
                    let copy = kit.icon_button(ui, "copy", "Copy");
                    let close = kit.icon_button(ui, "x", "Close pin");
                    hot = opacity.hovered() || copy.hovered() || close.hovered();
                    if opacity.clicked() {
                        self.opacity = next_opacity(self.opacity);
                        self.readout(format!("Opacity {}", percent(self.opacity)));
                    }
                    if copy.clicked() {
                        out.push(PinRequest::Copy(self.image.clone()));
                        self.readout("Copied".into());
                    }
                    if close.clicked() {
                        self.closed = true;
                    }
                });
            });
            self.toolbar_hot = hot;
        } else {
            self.toolbar_hot = false;
        }
        if self.live {
            theme::badge(ui.painter(), rect.left_top() + Vec2::new(6.0, 16.0), "Live", arcade_link::design::egui::components::Tone::Accent);
        }
        self.paint_readout(ui, rect);

        resp.context_menu(|ui| {
            if ui.button("Copy").clicked() {
                out.push(PinRequest::Copy(self.image.clone()));
                self.readout("Copied".into());
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
                for o in OPACITY_STEPS {
                    if ui.radio(self.opacity == o, percent(o)).clicked() {
                        self.opacity = o;
                        ui.close();
                    }
                }
            });
            if ui.button("Actual size").clicked() {
                self.set_zoom(&ctx, 1.0);
                self.readout("Zoom 100%".into());
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

    /// Shows a short readout (zoom, opacity, "Copied") as a toast on the pin.
    fn readout(&mut self, message: String) {
        self.readout = Some((message, Instant::now()));
    }

    fn paint_readout(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let Some((message, at)) = &self.readout else { return };
        let life = Duration::from_millis(motion::COPIED_MS);
        if at.elapsed() >= life {
            self.readout = None;
            return;
        }
        let c = theme::c();
        let p = ui.painter();
        let g = p.layout_no_wrap(message.clone(), theme::font(text::SMALL), c.text);
        let size = Vec2::new(g.size().x + 24.0, size::CONTROL_SM);
        if rect.width() < size.x + 8.0 || rect.height() < size.y + 8.0 {
            // Too small to show anything inside; the change is still applied.
            return;
        }
        let toast = egui::Rect::from_center_size(rect.center_bottom() - Vec2::new(0.0, size.y / 2.0 + 8.0), size);
        p.add(theme::shadow_pop().as_shape(toast, radius::MD));
        p.rect(toast, radius::MD, c.panel, Stroke::new(1.0_f32, c.line), StrokeKind::Inside);
        p.galley(toast.center() - g.size() * 0.5, g, c.text);
        ui.ctx().request_repaint_after(life.saturating_sub(at.elapsed()));
    }

    fn set_zoom(&mut self, ctx: &egui::Context, z: f32) {
        self.zoom = z.clamp(0.1, 8.0);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.window_size()));
    }

    fn window_size(&self) -> Vec2 {
        let image = Vec2::new(self.image.width() as f32, self.image.height() as f32) / self.scale * self.zoom;
        image.max(Vec2::splat(16.0)) + Vec2::splat(MARGIN * 2.0)
    }
}

/// Transparent space around the image for the pin's shadow, in points.
const MARGIN: f32 = 10.0;

const OPACITY_STEPS: [f32; 4] = [1.0, 0.75, 0.5, 0.25];

/// The kit's `shadow-pop` colour with its blur kept inside [`MARGIN`]: the
/// full 48 px blur would need a wide transparent ring around every pin, and
/// that ring would take clicks meant for the windows underneath.
fn pin_shadow() -> egui::epaint::Shadow {
    let pop = theme::shadow_pop();
    egui::epaint::Shadow { offset: [0, 3], blur: (MARGIN as u8).saturating_sub(2), spread: 0, color: pop.color }
}

fn next_opacity(current: f32) -> f32 {
    OPACITY_STEPS.iter().copied().find(|o| *o < current - 0.01).unwrap_or(1.0)
}

fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
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
        let c = theme::c();
        let kit = theme::kit();
        let mut stop = false;
        theme::panel_frame().show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                let blink = (self.started.elapsed().as_millis() / 500).is_multiple_of(2);
                ui.painter().circle_filled(r.center(), 5.0, if blink { c.danger_fill } else { c.danger_fill.gamma_multiply(0.4) });
                let s = self.started.elapsed().as_secs();
                ui.label(egui::RichText::new(format!("Recording {:02}:{:02}", s / 60, s % 60)).font(theme::mono()).color(c.text)).on_hover_text(&self.title);
                if kit.button_state(ui, "Stop", arcade_link::design::egui::components::ButtonKind::Danger, true, false, true).clicked() {
                    stop = true;
                }
            });
        });
        stop
    }
}
