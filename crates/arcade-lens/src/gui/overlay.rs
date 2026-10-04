//! The selection overlay: frozen frame → draw → spatial, progressive palette.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, CursorIcon, Key, Pos2, Sense, Stroke, StrokeKind, Vec2};
use image::RgbaImage;
use lens_core::action::{ActionOutcome, Choice, ConfirmRequest, Params};
use lens_core::cancel::CancelToken;
use lens_core::chain::ChainRunContext;
use lens_core::geometry::{MonitorInfo, Point, Rect};
use lens_core::host::Host;
use lens_core::palette::{build_palette, invoke, stabilize, Invocation, InvokeContext, Palette, PaletteEntry, PaletteInput, Target};
use lens_core::selection::{Selection, SelectionContext, WindowInfo};
use lens_core::usage::UsageStore;
use lens_core::{Analysis, AnalysisEvent, AnalysisReport, Capability, Finding, Value};

use super::ghost::GuiHost;
use super::measure;
use super::runtime::Runtime;
use super::theme::{self, ACCENT, ACCENT_SOFT, BORDER, DANGER, MUTED, SUCCESS, TEXT};

/// Shared services the overlay needs to analyze and act.
pub struct Env {
    pub rt: Arc<Runtime>,
    pub host: Arc<GuiHost>,
    pub usage: Arc<Mutex<UsageStore>>,
}

pub struct MonitorView {
    pub monitor: MonitorInfo,
    pub image: Arc<RgbaImage>,
    pub texture: egui::TextureHandle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

#[derive(Clone, Copy, Debug)]
enum DragKind {
    New,
    Move,
    Resize(Handle),
    Measure,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    mon: usize,
    kind: DragKind,
    start: (i32, i32),
    orig: Rect,
}

pub struct Sel {
    pub mon: usize,
    /// Local to the monitor, physical pixels.
    pub rect: Rect,
    pub selection: Selection,
}

pub enum Panel {
    Primary,
    Expanded { filter: String, cursor: usize },
    Confirm { entry: PaletteEntry, req: ConfirmRequest, params: Params },
    Choice { entry: PaletteEntry, choices: Vec<Choice>, cursor: usize, params: Params },
    Result { title: String, body: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
    pub then_close: bool,
}

enum JobOutcome {
    Invocation(lens_core::Result<Invocation>),
    Chain(lens_core::Result<Vec<ActionOutcome>>),
}

struct Job {
    rx: Receiver<JobOutcome>,
    started: Instant,
    entry: PaletteEntry,
    params: Params,
}

pub struct Overlay {
    pub views: Vec<MonitorView>,
    pub windows: Vec<WindowInfo>,
    pub root_monitor: usize,
    pub measure: bool,
    drag: Option<Drag>,
    pub sel: Option<Sel>,
    analysis: Option<Analysis>,
    pub report: AnalysisReport,
    pub palette: Palette,
    analysis_started: Instant,
    pub analysis_done: bool,
    pub panel: Panel,
    job: Option<Job>,
    pub toast: Option<Toast>,
    pub close_requested: bool,
    palette_size: Vec2,
    measure_rect: Option<(usize, Rect)>,
    reanalyze_at: Option<Instant>,
    pub history_saved: bool,
}

pub fn cap_label(c: &Capability) -> String {
    match c.as_str() {
        "region" => "Region",
        "image" => "Image",
        "icon" => "Icon",
        "color" => "Color",
        "palette" => "Palette",
        "ui-element" => "Element",
        "window" => "Window",
        "qr-code" => "QR code",
        "barcode" => "Barcode",
        "text" => "Text",
        "table" => "Table",
        "code" => "Code",
        "command" => "Command",
        "error" => "Error",
        "url" => "Link",
        "email" => "Email",
        "phone" => "Phone",
        "address" => "Address",
        "date-time" => "Date",
        "timecode" => "Timecode",
        "path" => "Path",
        "ip-address" => "IP address",
        "domain" => "Domain",
        "hash" => "Hash",
        "uuid" => "UUID",
        "secret" => "Secret",
        "currency" => "Amount",
        "quantity" => "Measurement",
        "coordinates" => "Location",
        "git-commit" => "Commit",
        "document" => "Document",
        "subtitle" => "Subtitle",
        other => return other.rsplit('.').next().unwrap_or(other).replace('-', " "),
    }
    .to_string()
}

fn key_name(k: Key) -> Option<char> {
    let n = k.name();
    let mut c = n.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) if ch.is_ascii_alphanumeric() => Some(ch.to_ascii_lowercase()),
        _ => None,
    }
}

impl Overlay {
    pub fn new(views: Vec<MonitorView>, windows: Vec<WindowInfo>, cursor: Option<Point>) -> Overlay {
        let root_monitor =
            cursor.and_then(|c| views.iter().position(|v| v.monitor.rect.contains(c))).or_else(|| views.iter().position(|v| v.monitor.is_primary)).unwrap_or(0);
        Overlay {
            views,
            windows,
            root_monitor,
            measure: false,
            drag: None,
            sel: None,
            analysis: None,
            report: AnalysisReport::default(),
            palette: Palette::default(),
            analysis_started: Instant::now(),
            analysis_done: false,
            panel: Panel::Primary,
            job: None,
            toast: None,
            close_requested: false,
            palette_size: Vec2::new(420.0, 64.0),
            measure_rect: None,
            reanalyze_at: None,
            history_saved: false,
        }
    }

    fn view(&self, mon: usize) -> &MonitorView {
        &self.views[mon]
    }

    pub fn set_selection(&mut self, env: &Env, mon: usize, rect: Rect) {
        let v = &self.views[mon];
        let bounds = Rect::new(0, 0, v.image.width(), v.image.height());
        let Some(rect) = rect.intersection(&bounds) else { return };
        if rect.width < 1 || rect.height < 1 {
            return;
        }
        let crop = image::imageops::crop_imm(&*v.image, rect.x as u32, rect.y as u32, rect.width, rect.height).to_image();
        let global = Rect::new(v.monitor.rect.x + rect.x, v.monitor.rect.y + rect.y, rect.width, rect.height);
        let mut selection = Selection::new(global, crop);
        selection.context = SelectionContext {
            monitor: Some(v.monitor.clone()),
            windows: self.windows.clone(),
            focused_app: self.windows.first().and_then(|w| w.app_name.clone()),
        };
        // Dropping the previous analysis cancels it.
        self.analysis = Some(env.rt.engine.analyze(selection.clone()));
        self.report = AnalysisReport::default();
        self.palette = Palette::default();
        self.analysis_started = Instant::now();
        self.analysis_done = false;
        self.history_saved = false;
        self.panel = Panel::Primary;
        self.sel = Some(Sel { mon, rect, selection });
        self.measure = false;
        self.measure_rect = None;
    }

    fn clear_selection(&mut self) {
        self.analysis = None;
        self.sel = None;
        self.report = AnalysisReport::default();
        self.palette = Palette::default();
        self.panel = Panel::Primary;
    }

    /// Drains progressive analysis results and finished jobs. Call once per frame.
    pub fn poll(&mut self, env: &Env, ctx: &egui::Context) {
        if let Some(at) = self.reanalyze_at {
            if at.elapsed() > Duration::from_millis(250) {
                self.reanalyze_at = None;
                if let Some(s) = &self.sel {
                    let (mon, rect) = (s.mon, s.rect);
                    self.set_selection(env, mon, rect);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        let mut changed = false;
        if let Some(a) = &self.analysis {
            while let Ok(e) = a.events().try_recv() {
                match &e {
                    AnalysisEvent::Finished { elapsed, .. } => {
                        self.analysis_done = true;
                        crate::lens_debug!("analysis finished in {elapsed:?}");
                    }
                    AnalysisEvent::Findings(f) => crate::lens_debug!("findings: {:?}", f.iter().map(|f| f.capability.as_str()).collect::<Vec<_>>()),
                    AnalysisEvent::RecognizerFailed { recognizer, error } => crate::lens_debug!("recognizer {recognizer} failed: {error}"),
                }
                self.report.apply(e);
                changed = true;
            }
            if !self.analysis_done {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        if changed {
            let usage = env.usage.lock().unwrap();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let next = build_palette(&PaletteInput {
                registry: &env.rt.registry,
                findings: &self.report.findings,
                settings: &env.rt.settings,
                usage: &usage,
                host: env.host.features(),
                chains: &env.rt.chains,
                now,
            });
            // Free reordering for the first moments; after that, keep what the user sees in place.
            let settled = self.analysis_started.elapsed() > Duration::from_millis(350);
            self.palette = stabilize(&self.palette.primary, next, settled, &env.rt.settings, &env.rt.registry);
        }
        if let Some(job) = &self.job {
            if let Ok(out) = job.rx.try_recv() {
                let job = self.job.take().unwrap();
                self.finish_job(env, job, out);
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        if let Some(t) = &self.toast {
            if Instant::now() >= t.until {
                if t.then_close {
                    self.close_requested = true;
                }
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
    }

    fn toast(&mut self, text: impl Into<String>, kind: ToastKind, then_close: bool) {
        let ms = if kind == ToastKind::Error {
            2600
        } else if then_close {
            650
        } else {
            1400
        };
        self.toast = Some(Toast { text: text.into(), kind, until: Instant::now() + Duration::from_millis(ms), then_close });
    }

    pub fn run_entry(&mut self, env: &Env, entry: PaletteEntry, params: Params, confirmed: bool) {
        if self.job.is_some() {
            return;
        }
        let Some(sel) = &self.sel else { return };
        let (tx, rx) = mpsc::channel();
        let registry = env.rt.registry.clone();
        let settings = env.rt.settings.clone();
        let findings = self.report.findings.clone();
        let selection = sel.selection.clone();
        let host: Arc<GuiHost> = env.host.clone();
        match &entry.target {
            Target::Chain(id) => {
                let Some(chain) = env.rt.chains.iter().find(|c| &c.id == id).cloned() else { return };
                let plan = match chain.validate(&registry) {
                    Ok(p) => p,
                    Err(e) => return self.toast(e.to_string(), ToastKind::Error, false),
                };
                if plan.needs_confirmation && !confirmed {
                    let reasons = vec![format!("This chain may: {}", effects_text(plan.effects))];
                    let req = ConfirmRequest { title: format!("Run \"{}\"?", chain.name), subject: chain_summary(&chain), reasons };
                    self.panel = Panel::Confirm { entry, req, params };
                    return;
                }
                std::thread::spawn(move || {
                    let r = chain.run(ChainRunContext {
                        registry: &registry,
                        findings: &findings,
                        host: &*host,
                        settings: &settings,
                        selection: Some(&selection),
                        cancel: &CancelToken::new(),
                        confirmed: true,
                    });
                    let _ = tx.send(JobOutcome::Chain(r));
                });
            }
            Target::Action(id) => {
                let id = id.clone();
                let finding = entry.finding;
                let p = params.clone();
                std::thread::spawn(move || {
                    let cx = InvokeContext {
                        registry: &registry,
                        findings: &findings,
                        host: &*host,
                        settings: &settings,
                        selection: Some(&selection),
                        params: &p,
                        confirmed,
                    };
                    let _ = tx.send(JobOutcome::Invocation(invoke(&id, finding, &cx)));
                });
            }
        }
        self.job = Some(Job { rx, started: Instant::now(), entry, params });
    }

    fn finish_job(&mut self, env: &Env, job: Job, out: JobOutcome) {
        let record = |env: &Env, e: &PaletteEntry| {
            if env.rt.settings.privacy.learn_action_usage {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                let mut u = env.usage.lock().unwrap();
                u.record(e.capability.as_str(), &e.target.usage_key(), now);
                let _ = crate::config::save_usage(&env.rt.paths, &u);
            }
        };
        let messages: Vec<String> = match out {
            JobOutcome::Invocation(Ok(Invocation::Done(o))) => {
                record(env, &job.entry);
                o.message.into_iter().collect()
            }
            JobOutcome::Invocation(Ok(Invocation::NeedsConfirmation(req))) => {
                self.panel = Panel::Confirm { entry: job.entry, req, params: job.params };
                return;
            }
            JobOutcome::Invocation(Ok(Invocation::NeedsChoice(choices))) => {
                self.panel = Panel::Choice { entry: job.entry, choices, cursor: 0, params: job.params };
                return;
            }
            JobOutcome::Chain(Ok(outs)) => {
                record(env, &job.entry);
                outs.into_iter().filter_map(|o| o.message).collect()
            }
            JobOutcome::Invocation(Err(e)) | JobOutcome::Chain(Err(e)) => {
                self.panel = Panel::Primary;
                return self.toast(e.to_string(), ToastKind::Error, false);
            }
        };
        let body = messages.join("\n");
        let inspect = job.entry.group == lens_core::ActionGroup::Inspect && job.entry.target.usage_key() != "core.ui.measure";
        if self.measure {
            // The action switched to measure mode; keep the overlay open.
            self.panel = Panel::Primary;
        } else if inspect || body.contains('\n') || body.chars().count() > 90 {
            self.panel = Panel::Result { title: job.entry.label.clone(), body };
        } else {
            let text = if body.is_empty() { job.entry.label.clone() } else { body };
            self.toast(text, ToastKind::Success, true);
        }
    }

    pub fn enter_measure(&mut self, global: Rect) {
        let mon = self.views.iter().position(|v| v.monitor.rect.contains(Point { x: global.x, y: global.y })).unwrap_or(self.root_monitor);
        let m = &self.views[mon].monitor.rect;
        self.measure = true;
        self.measure_rect = Some((mon, Rect::new(global.x - m.x, global.y - m.y, global.width, global.height)));
        self.panel = Panel::Primary;
    }

    /// Draws one monitor's overlay. `primary` is true for the window that owns keyboard focus.
    pub fn ui(&mut self, ui: &mut egui::Ui, env: &Env, mon: usize, primary: bool) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let screen = ui.max_rect();
        let view_size = {
            let v = self.view(mon);
            (v.image.width() as i32, v.image.height() as i32)
        };
        let to_px = |p: Pos2| -> (i32, i32) { (((p.x - screen.min.x) * ppp).floor() as i32, ((p.y - screen.min.y) * ppp).floor() as i32) };
        let to_pt = |x: f32, y: f32| -> Pos2 { Pos2::new(screen.min.x + x / ppp, screen.min.y + y / ppp) };
        let rect_pt = |r: &Rect| egui::Rect::from_min_max(to_pt(r.x as f32, r.y as f32), to_pt(r.right() as f32, r.bottom() as f32));

        if primary {
            self.poll(env, &ctx);
            self.keyboard(&ctx, env);
        }

        // Background interaction (Areas drawn later sit above and take their own clicks).
        let bg = ui.interact(screen, egui::Id::new(("lens-overlay-bg", mon)), Sense::click_and_drag());
        let hover = ctx.input(|i| i.pointer.hover_pos()).filter(|p| screen.contains(*p));
        let hover_px = hover.map(to_px).map(|(x, y)| (x.clamp(0, view_size.0 - 1), y.clamp(0, view_size.1 - 1)));
        self.pointer(env, &ctx, &bg, mon, hover_px, view_size, ppp);

        // --- Paint ---
        let painter = ui.painter_at(screen);
        let v = self.view(mon);
        painter.image(v.texture.id(), screen, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

        let sel_rect = self.current_rect(mon);
        match sel_rect.map(|r| rect_pt(&r)) {
            Some(r) => {
                for d in [
                    egui::Rect::from_min_max(screen.min, Pos2::new(screen.max.x, r.min.y)),
                    egui::Rect::from_min_max(Pos2::new(screen.min.x, r.max.y), screen.max),
                    egui::Rect::from_min_max(Pos2::new(screen.min.x, r.min.y), Pos2::new(r.min.x, r.max.y)),
                    egui::Rect::from_min_max(Pos2::new(r.max.x, r.min.y), Pos2::new(screen.max.x, r.max.y)),
                ] {
                    painter.rect_filled(d, 0.0, theme::DIM);
                }
                let px1 = 1.0 / ppp;
                painter.rect_stroke(r.expand(px1 * 0.5), 0.0, Stroke::new(px1.max(1.0), ACCENT_SOFT), StrokeKind::Middle);
                if self.drag.is_none() && self.sel.as_ref().is_some_and(|s| s.mon == mon) && !self.measure {
                    for h in handle_points(r) {
                        painter.rect(
                            egui::Rect::from_center_size(h.1, Vec2::splat(6.0)),
                            CornerRadius::same(1),
                            TEXT,
                            Stroke::new(1.0, ACCENT),
                            StrokeKind::Middle,
                        );
                    }
                }
                let size = sel_rect.unwrap();
                let label = format!("{} × {}", size.width, size.height);
                let anchor = if r.min.y > 28.0 {
                    (r.left_top() + Vec2::new(0.0, -6.0), Align2::LEFT_BOTTOM)
                } else {
                    (r.left_top() + Vec2::new(6.0, 6.0), Align2::LEFT_TOP)
                };
                theme::pill(&painter, anchor.0, anchor.1, &label, TEXT, 11.5);
            }
            None => {
                painter.rect_filled(screen, 0.0, theme::DIM.gamma_multiply(0.55));
            }
        }

        // Window snapping hint.
        if self.drag.is_none() && self.sel.is_none() && !self.measure {
            if let Some((hx, hy)) = hover_px {
                if let Some(w) = self.window_at(mon, hx, hy) {
                    let m = &self.view(mon).monitor.rect;
                    let local = Rect::new(w.rect.x - m.x, w.rect.y - m.y, w.rect.width, w.rect.height);
                    let r = rect_pt(&local).intersect(screen);
                    painter.rect_stroke(r, 2.0, Stroke::new(2.0, ACCENT.gamma_multiply(0.85)), StrokeKind::Inside);
                    let name = w.app_name.clone().filter(|a| !a.is_empty()).unwrap_or_else(|| w.title.clone());
                    theme::pill(&painter, r.left_top() + Vec2::new(8.0, 8.0), Align2::LEFT_TOP, &format!("{name} · click to select"), TEXT, 11.0);
                }
            }
        }

        if self.measure {
            self.paint_measure(&painter, mon, hover_px, &rect_pt, &to_pt, ppp);
        } else if self.sel.is_none() || self.drag.is_some() {
            if let (Some(p), Some((x, y))) = (hover, hover_px) {
                if self.drag.is_none() {
                    let s = Stroke::new(1.0, Color32::from_white_alpha(70));
                    painter.line_segment([Pos2::new(screen.min.x, p.y), Pos2::new(screen.max.x, p.y)], s);
                    painter.line_segment([Pos2::new(p.x, screen.min.y), Pos2::new(p.x, screen.max.y)], s);
                }
                self.paint_loupe(&painter, screen, p, (x, y), mon);
            }
        }

        // HUD hint.
        if primary && self.sel.is_none() && self.drag.is_none() {
            let hint = if self.measure {
                "Measure · drag to measure a box · C copy · M back · Esc close"
            } else if self.windows.is_empty() {
                "Drag to select · M measure · Esc close"
            } else {
                "Drag to select · Click a window · M measure · Esc close"
            };
            theme::pill(&painter, Pos2::new(screen.center().x, screen.min.y + 14.0), Align2::CENTER_TOP, hint, MUTED, 12.0);
        }

        if bg.hovered() {
            let icon = match self.hover_handle(mon, hover_px) {
                Some(Handle::N | Handle::S) => CursorIcon::ResizeVertical,
                Some(Handle::E | Handle::W) => CursorIcon::ResizeHorizontal,
                Some(Handle::NE | Handle::SW) => CursorIcon::ResizeNeSw,
                Some(Handle::NW | Handle::SE) => CursorIcon::ResizeNwSe,
                None if self.inside_selection(mon, hover_px) => CursorIcon::Move,
                None => CursorIcon::Crosshair,
            };
            ctx.set_cursor_icon(icon);
        }

        // Panels live next to the selection on its monitor.
        if let Some(r) = self.sel.as_ref().filter(|s| s.mon == mon && self.drag.is_none() && !self.measure).map(|s| rect_pt(&s.rect)) {
            super::panels::show(self, &ctx, env, screen, r);
        }
        if let Some(t) = &self.toast {
            // Above the selection, clear of the palette next to it; otherwise inside the
            // selection's top edge, and for a small selection at the screen top, past the palette.
            let (anchor, pivot) = match self.sel.as_ref().filter(|s| s.mon == mon).map(|s| rect_pt(&s.rect)) {
                Some(r) if r.min.y - screen.min.y > 70.0 => (Pos2::new(r.center().x, r.min.y - 30.0), Align2::CENTER_BOTTOM),
                Some(r) if r.height() > 60.0 => (Pos2::new(r.center().x, r.min.y + 12.0), Align2::CENTER_TOP),
                Some(r) => (Pos2::new(r.center().x, (r.max.y + 100.0).min(screen.max.y - 30.0)), Align2::CENTER_TOP),
                None => (screen.center(), Align2::CENTER_CENTER),
            };
            let color = match t.kind {
                ToastKind::Success => SUCCESS,
                ToastKind::Error => DANGER,
                ToastKind::Info => TEXT,
            };
            let text = t.text.clone();
            egui::Area::new(egui::Id::new("lens-toast")).order(egui::Order::Tooltip).pivot(pivot).fixed_pos(anchor).constrain_to(screen).show(&ctx, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    // Short messages stay on one line; long errors wrap at a readable width.
                    let galley_width = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), egui::FontId::proportional(13.5), color).size().x);
                    ui.set_width(galley_width.min(420.0));
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color).size(13.5)).wrap());
                });
            });
        }
    }

    fn current_rect(&self, mon: usize) -> Option<Rect> {
        if self.measure {
            return self.measure_rect.filter(|(m, _)| *m == mon).map(|(_, r)| r);
        }
        self.sel.as_ref().filter(|s| s.mon == mon).map(|s| s.rect)
    }

    fn window_at(&self, mon: usize, x: i32, y: i32) -> Option<&WindowInfo> {
        let m = &self.view(mon).monitor.rect;
        let p = Point { x: m.x + x, y: m.y + y };
        self.windows.iter().find(|w| w.rect.contains(p))
    }

    fn inside_selection(&self, mon: usize, p: Option<(i32, i32)>) -> bool {
        match (&self.sel, p) {
            (Some(s), Some((x, y))) if s.mon == mon && !self.measure => s.rect.contains(Point { x, y }),
            _ => false,
        }
    }

    fn hover_handle(&self, mon: usize, p: Option<(i32, i32)>) -> Option<Handle> {
        let (s, (x, y)) = (self.sel.as_ref().filter(|s| s.mon == mon && !self.measure)?, p?);
        let r = s.rect;
        let tol = 6;
        let near = |a: i32, b: i64| (a as i64 - b).abs() <= tol;
        let in_x = x as i64 >= r.x as i64 - tol && (x as i64) <= r.right() + tol;
        let in_y = y as i64 >= r.y as i64 - tol && (y as i64) <= r.bottom() + tol;
        let (l, rt, t, b) = (near(x, r.x as i64), near(x, r.right()), near(y, r.y as i64), near(y, r.bottom()));
        Some(match (l, rt, t, b) {
            (true, _, true, _) => Handle::NW,
            (_, true, true, _) => Handle::NE,
            (true, _, _, true) => Handle::SW,
            (_, true, _, true) => Handle::SE,
            (true, _, _, _) if in_y => Handle::W,
            (_, true, _, _) if in_y => Handle::E,
            (_, _, true, _) if in_x => Handle::N,
            (_, _, _, true) if in_x => Handle::S,
            _ => return None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn pointer(&mut self, env: &Env, ctx: &egui::Context, bg: &egui::Response, mon: usize, hover_px: Option<(i32, i32)>, size: (i32, i32), _ppp: f32) {
        let clamp = |(x, y): (i32, i32)| (x.clamp(0, size.0), y.clamp(0, size.1));
        if bg.drag_started() {
            // Start where the button went down, not where the drag threshold was crossed.
            let origin = ctx.input(|i| i.pointer.press_origin()).map(|o| {
                let ppp = ctx.pixels_per_point();
                clamp((((o.x - bg.rect.min.x) * ppp).floor() as i32, ((o.y - bg.rect.min.y) * ppp).floor() as i32))
            });
            if let Some(p) = origin.or(hover_px) {
                let kind = if self.measure {
                    DragKind::Measure
                } else if let Some(h) = self.hover_handle(mon, hover_px) {
                    DragKind::Resize(h)
                } else if self.inside_selection(mon, hover_px) {
                    DragKind::Move
                } else {
                    DragKind::New
                };
                let orig = self.sel.as_ref().map_or(Rect::default(), |s| s.rect);
                if matches!(kind, DragKind::New) {
                    self.clear_selection();
                }
                self.drag = Some(Drag { mon, kind, start: p, orig });
            }
        }
        if let (Some(d), Some(p)) = (self.drag, ctx.input(|i| i.pointer.interact_pos()).filter(|_| bg.dragged() || bg.drag_stopped())) {
            if d.mon == mon {
                let screen = bg.rect;
                let ppp = ctx.pixels_per_point();
                let cur = clamp((((p.x - screen.min.x) * ppp).round() as i32, ((p.y - screen.min.y) * ppp).round() as i32));
                let (dx, dy) = (cur.0 - d.start.0, cur.1 - d.start.1);
                let rect = match d.kind {
                    DragKind::New | DragKind::Measure => Rect::from_corners(Point { x: d.start.0, y: d.start.1 }, Point { x: cur.0, y: cur.1 }),
                    DragKind::Move => {
                        let o = d.orig;
                        Rect::new((o.x + dx).clamp(0, size.0 - o.width as i32), (o.y + dy).clamp(0, size.1 - o.height as i32), o.width, o.height)
                    }
                    DragKind::Resize(h) => resize(d.orig, h, dx, dy),
                };
                let rect = if matches!(d.kind, DragKind::Measure) && !ctx.input(|i| i.modifiers.alt) {
                    let img = &self.view(mon).image;
                    let (y0, y1) = (rect.y.max(0) as u32, rect.bottom().max(0) as u32);
                    let (x0, x1) = (rect.x.max(0) as u32, rect.right().max(0) as u32);
                    let l = measure::snap_x(img, rect.x, y0, y1, 5);
                    let r = measure::snap_x(img, rect.right() as i32, y0, y1, 5);
                    let t = measure::snap_y(img, rect.y, x0, x1, 5);
                    let b = measure::snap_y(img, rect.bottom() as i32, x0, x1, 5);
                    Rect::from_corners(Point { x: l, y: t }, Point { x: r, y: b })
                } else {
                    rect
                };
                match d.kind {
                    DragKind::Measure => self.measure_rect = Some((mon, rect)),
                    _ => {
                        if let Some(s) = &mut self.sel {
                            s.rect = rect;
                        } else {
                            // Temporary selection while drawing (no analysis yet).
                            self.sel = Some(Sel { mon, rect, selection: Selection::new(rect, RgbaImage::new(1, 1)) });
                        }
                    }
                }
                if bg.drag_stopped() {
                    self.drag = None;
                    if !matches!(d.kind, DragKind::Measure) {
                        if rect.width >= 3 && rect.height >= 3 {
                            self.set_selection(env, mon, rect);
                        } else {
                            self.sel = None;
                        }
                    }
                }
            }
        }
        if bg.clicked() && self.drag.is_none() && !self.measure {
            if let Some((x, y)) = hover_px {
                if !self.inside_selection(mon, hover_px) {
                    let m = self.view(mon).monitor.rect;
                    if let Some(w) = self.window_at(mon, x, y).cloned() {
                        let local = Rect::new(w.rect.x - m.x, w.rect.y - m.y, w.rect.width, w.rect.height);
                        self.set_selection(env, mon, local);
                    } else if self.sel.is_some() {
                        self.clear_selection();
                    }
                }
            }
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context, env: &Env) {
        let events = ctx.input(|i| i.events.clone());
        let in_text_panel = matches!(self.panel, Panel::Expanded { .. });
        for e in events {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { continue };
            if self.job.is_some() && key != Key::Escape {
                continue;
            }
            match key {
                Key::Escape => {
                    match self.panel {
                        Panel::Primary if self.measure && self.sel.is_some() => self.measure = false,
                        Panel::Primary => self.close_requested = true,
                        _ => self.panel = Panel::Primary,
                    }
                    continue;
                }
                _ if in_text_panel => continue, // handled by the panel
                _ => {}
            }
            if self.measure {
                match key {
                    Key::M => {
                        self.measure = false;
                        self.measure_rect = None;
                    }
                    Key::C => {
                        if let Some((_, r)) = self.measure_rect {
                            let _ = env.host.set_clipboard_text(&format!("{} × {}", r.width, r.height));
                            self.toast(format!("Copied {} × {}", r.width, r.height), ToastKind::Success, false);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            match &mut self.panel {
                Panel::Primary => {
                    if self.sel.is_none() {
                        if key == Key::M {
                            self.measure = true;
                        }
                        continue;
                    }
                    let arrows = [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown];
                    if arrows.contains(&key) {
                        self.nudge(key, modifiers.shift, if modifiers.ctrl || modifiers.command { 10 } else { 1 });
                        continue;
                    }
                    match key {
                        Key::Enter => {
                            if let Some(e) = self.palette.default_entry().cloned() {
                                self.run_entry(env, e, Params::new(), false);
                            }
                        }
                        Key::Space | Key::Tab => self.panel = Panel::Expanded { filter: String::new(), cursor: 0 },
                        Key::C if modifiers.ctrl || modifiers.command => {
                            if let Some(e) = self.palette.all.iter().find(|e| e.target == Target::Action("core.region.copy".into())).cloned() {
                                self.run_entry(env, e, Params::new(), false);
                            }
                        }
                        k => {
                            if let Some(c) = key_name(k) {
                                if let Some(e) = self.palette.by_key(c).cloned() {
                                    self.run_entry(env, e, Params::new(), false);
                                }
                            }
                        }
                    }
                }
                Panel::Choice { choices, cursor, .. } => match key {
                    Key::ArrowDown => *cursor = (*cursor + 1).min(choices.len().saturating_sub(1)),
                    Key::ArrowUp => *cursor = cursor.saturating_sub(1),
                    Key::Enter => {
                        let c = *cursor;
                        self.pick_choice(env, c);
                    }
                    k => {
                        if let Some(d) = key_name(k).and_then(|c| c.to_digit(10)) {
                            if d >= 1 {
                                self.pick_choice(env, d as usize - 1);
                            }
                        }
                    }
                },
                Panel::Confirm { entry, .. } => {
                    // Dangerous actions need an explicit Ctrl+Enter (or a click), never a reflexive Enter.
                    let dangerous = entry.safety == lens_core::SafetyClass::Dangerous;
                    if key == Key::Enter && (!dangerous || modifiers.ctrl || modifiers.command) {
                        self.confirm(env);
                    }
                }
                Panel::Result { body, .. } => {
                    if key == Key::C && (modifiers.ctrl || modifiers.command) || key == Key::Enter {
                        let b = body.clone();
                        let _ = env.host.set_clipboard_text(&b);
                        self.toast("Copied", ToastKind::Success, true);
                    }
                }
                Panel::Expanded { .. } => {}
            }
        }
    }

    pub fn confirm(&mut self, env: &Env) {
        if let Panel::Confirm { entry, params, .. } = std::mem::replace(&mut self.panel, Panel::Primary) {
            self.run_entry(env, entry, params, true);
        }
    }

    pub fn pick_choice(&mut self, env: &Env, index: usize) {
        if let Panel::Choice { entry, choices, mut params, .. } = std::mem::replace(&mut self.panel, Panel::Primary) {
            match choices.get(index) {
                Some(c) => {
                    params.insert("choice".into(), c.value.clone());
                    self.run_entry(env, entry, params, false);
                }
                None => self.panel = Panel::Choice { entry, choices, cursor: 0, params },
            }
        }
    }

    fn nudge(&mut self, key: Key, resize_mode: bool, step: i32) {
        let Some(s) = &mut self.sel else { return };
        let (w, h) = (self.views[s.mon].image.width() as i32, self.views[s.mon].image.height() as i32);
        let r = &mut s.rect;
        let (dx, dy) = match key {
            Key::ArrowLeft => (-step, 0),
            Key::ArrowRight => (step, 0),
            Key::ArrowUp => (0, -step),
            _ => (0, step),
        };
        if resize_mode {
            r.width = (r.width as i32 + dx).clamp(1, w - r.x) as u32;
            r.height = (r.height as i32 + dy).clamp(1, h - r.y) as u32;
        } else {
            r.x = (r.x + dx).clamp(0, w - r.width as i32);
            r.y = (r.y + dy).clamp(0, h - r.height as i32);
        }
        self.reanalyze_at = Some(Instant::now());
    }

    fn paint_loupe(&self, painter: &egui::Painter, screen: egui::Rect, p: Pos2, (x, y): (i32, i32), mon: usize) {
        const N: i32 = 7; // pixels on each side of the center
        const BOX: f32 = 120.0;
        let v = self.view(mon);
        let (w, h) = (v.image.width() as f32, v.image.height() as f32);
        let uv = egui::Rect::from_min_max(Pos2::new((x - N) as f32 / w, (y - N) as f32 / h), Pos2::new((x + N + 1) as f32 / w, (y + N + 1) as f32 / h));
        let mut origin = p + Vec2::new(22.0, 22.0);
        if origin.x + BOX > screen.max.x {
            origin.x = p.x - 22.0 - BOX;
        }
        if origin.y + BOX + 26.0 > screen.max.y {
            origin.y = p.y - 22.0 - BOX - 26.0;
        }
        let r = egui::Rect::from_min_size(origin, Vec2::splat(BOX));
        painter.rect_filled(r.expand(1.0), CornerRadius::same(8), Color32::BLACK);
        painter.image(v.texture.id(), r, uv, Color32::WHITE);
        let cell = BOX / (2 * N + 1) as f32;
        let c = egui::Rect::from_min_size(r.min + Vec2::splat(N as f32 * cell), Vec2::splat(cell));
        painter.rect_stroke(c, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        painter.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0, BORDER), StrokeKind::Outside);
        let px = v.image.get_pixel(x.clamp(0, v.image.width() as i32 - 1) as u32, y.clamp(0, v.image.height() as i32 - 1) as u32);
        let hex = format!("#{:02X}{:02X}{:02X}", px[0], px[1], px[2]);
        let lr = egui::Rect::from_min_size(Pos2::new(r.min.x, r.max.y + 4.0), Vec2::new(BOX, 22.0));
        painter.rect(lr, CornerRadius::same(6), theme::SURFACE, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        let sw = egui::Rect::from_min_size(lr.min + Vec2::new(6.0, 6.0), Vec2::splat(10.0));
        painter.rect(sw, CornerRadius::same(2), Color32::from_rgb(px[0], px[1], px[2]), Stroke::new(1.0, Color32::from_white_alpha(60)), StrokeKind::Inside);
        let m = &v.monitor.rect;
        painter.text(Pos2::new(sw.max.x + 6.0, lr.center().y), Align2::LEFT_CENTER, format!("{hex}  {}, {}", m.x + x, m.y + y), theme::mono(10.5), TEXT);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_measure(
        &self,
        painter: &egui::Painter,
        mon: usize,
        hover_px: Option<(i32, i32)>,
        rect_pt: &dyn Fn(&Rect) -> egui::Rect,
        to_pt: &dyn Fn(f32, f32) -> Pos2,
        _ppp: f32,
    ) {
        let accent = Stroke::new(1.5, Color32::from_rgb(244, 114, 182));
        let halo = Stroke::new(3.5, Color32::from_black_alpha(150));
        if let Some((m, r)) = self.measure_rect {
            if m == mon {
                let rr = rect_pt(&r);
                painter.rect_filled(rr, 0.0, Color32::from_rgba_unmultiplied(244, 114, 182, 28));
                painter.rect_stroke(rr, 0.0, accent, StrokeKind::Middle);
                theme::pill(painter, Pos2::new(rr.center().x, rr.max.y + 6.0), Align2::CENTER_TOP, &format!("{} × {}", r.width, r.height), TEXT, 12.0);
            }
        }
        if self.drag.is_some() {
            return;
        }
        let Some((x, y)) = hover_px else { return };
        let img = &self.view(mon).image;
        let (l, r, u, d) = measure::extents(img, x as u32, y as u32);
        let (x0, x1) = ((x - l as i32) as f32, (x + r as i32 + 1) as f32);
        let (y0, y1) = ((y - u as i32) as f32, (y + d as i32 + 1) as f32);
        let cy = y as f32 + 0.5;
        let cx = x as f32 + 0.5;
        let segs = [
            ((x0, cy), (x1, cy)),
            ((cx, y0), (cx, y1)),
            ((x0, cy - 5.0), (x0, cy + 5.0)),
            ((x1, cy - 5.0), (x1, cy + 5.0)),
            ((cx - 5.0, y0), (cx + 5.0, y0)),
            ((cx - 5.0, y1), (cx + 5.0, y1)),
        ];
        // A dark halo keeps the guides readable on any background.
        for stroke in [halo, accent] {
            for (a, b) in segs {
                painter.line_segment([to_pt(a.0, a.1), to_pt(b.0, b.1)], stroke);
            }
        }
        let w = l + r + 1;
        let h = u + d + 1;
        theme::pill(painter, to_pt(cx, cy) + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, &format!("W {w}  H {h}"), TEXT, 11.5);
    }

    /// The top finding to describe in the palette header.
    pub fn headline(&self) -> Option<&Finding> {
        let id = self.palette.default_entry().map(|e| e.finding)?;
        self.report.findings.iter().find(|f| f.id == id)
    }

    pub fn job_running(&self) -> Option<(&str, Duration)> {
        self.job.as_ref().map(|j| (j.entry.label.as_str(), j.started.elapsed()))
    }

    pub fn analyzing(&self) -> bool {
        !self.analysis_done
    }

    pub fn palette_size_mut(&mut self) -> &mut Vec2 {
        &mut self.palette_size
    }

    pub fn palette_size(&self) -> Vec2 {
        self.palette_size
    }
}

fn handle_points(r: egui::Rect) -> [(Handle, Pos2); 8] {
    let c = r.center();
    [
        (Handle::NW, r.left_top()),
        (Handle::N, Pos2::new(c.x, r.min.y)),
        (Handle::NE, r.right_top()),
        (Handle::E, Pos2::new(r.max.x, c.y)),
        (Handle::SE, r.right_bottom()),
        (Handle::S, Pos2::new(c.x, r.max.y)),
        (Handle::SW, r.left_bottom()),
        (Handle::W, Pos2::new(r.min.x, c.y)),
    ]
}

fn resize(o: Rect, h: Handle, dx: i32, dy: i32) -> Rect {
    let (mut l, mut t, mut r, mut b) = (o.x, o.y, o.right() as i32, o.bottom() as i32);
    if matches!(h, Handle::W | Handle::NW | Handle::SW) {
        l += dx;
    }
    if matches!(h, Handle::E | Handle::NE | Handle::SE) {
        r += dx;
    }
    if matches!(h, Handle::N | Handle::NW | Handle::NE) {
        t += dy;
    }
    if matches!(h, Handle::S | Handle::SW | Handle::SE) {
        b += dy;
    }
    Rect::from_corners(Point { x: l, y: t }, Point { x: r, y: b })
}

pub fn effects_text(e: lens_core::Effects) -> String {
    use lens_core::Effects as E;
    let mut v = Vec::new();
    for (flag, s) in [
        (E::EXECUTES_COMMAND, "run commands"),
        (E::DELETES_FILES, "delete files"),
        (E::OVERWRITES_FILES, "overwrite files"),
        (E::PRIVILEGED, "use elevated privileges"),
        (E::UPLOADS_CONTENT, "send content to an online service"),
        (E::NETWORK, "use the network"),
        (E::SENDS_TO_DEVICE, "send to another device"),
        (E::WRITES_FILES, "write files"),
        (E::CLIPBOARD, "change the clipboard"),
    ] {
        if e.contains(flag) {
            v.push(s);
        }
    }
    v.join(", ")
}

fn chain_summary(c: &lens_core::chain::Chain) -> String {
    c.steps
        .iter()
        .map(|s| match s {
            lens_core::chain::ChainStep::Take { capability } => format!("take {capability}"),
            lens_core::chain::ChainStep::Run { action, .. } => action.clone(),
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// Short description of a finding for the palette header.
pub fn describe(f: &Finding) -> (String, String) {
    let title = cap_label(&f.capability);
    let summary = match &f.value {
        Value::Image(_) => f.details.iter().find(|(k, _)| k == "Size").map(|(_, v)| v.clone()).unwrap_or_default(),
        _ => f.summary(),
    };
    (title, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lens_core::caps;

    #[test]
    fn resizing_from_handles() {
        let o = Rect::new(10, 10, 100, 50);
        assert_eq!(resize(o, Handle::SE, 5, 5), Rect::new(10, 10, 105, 55));
        assert_eq!(resize(o, Handle::NW, -5, 20), Rect::new(5, 30, 105, 30));
        // Dragging past the opposite edge flips instead of producing negative sizes.
        assert_eq!(resize(o, Handle::E, -120, 0), Rect::new(-10, 10, 20, 50));
    }

    #[test]
    fn capability_labels() {
        assert_eq!(cap_label(&caps::URL), "Link");
        assert_eq!(cap_label(&Capability::custom("com.example.isbn")), "isbn");
    }
}
