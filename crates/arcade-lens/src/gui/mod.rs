//! The Arcade Lens desktop application.
//!
//! One process, one hidden root window. The root window *is* the overlay:
//! idle, it is hidden and costs nothing; on the shortcut, Lens captures the
//! screens first and only then shows the frozen frame, so the overlay never
//! sees itself. Pins, annotation editors, the settings window, extra-monitor
//! overlays and the recording indicator are independent child windows.

pub mod annotate;
pub mod ghost;
pub mod history;
pub mod measure;
pub mod overlay;
pub mod panels;
pub mod pins;
pub mod runtime;
pub mod settings_view;
pub mod theme;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId};
use image::RgbaImage;
use lens_core::host::{Host, SaveRequest};
use lens_core::usage::UsageStore;

use crate::config::{self, Paths};
use crate::host::DesktopHost;
use ghost::{GuiHost, UiCommand};
use overlay::{Env, MonitorView, Overlay};
use runtime::Runtime;

#[derive(Debug, Clone)]
pub enum Trigger {
    Capture,
    Settings,
    Pin(PathBuf),
    Quit,
}

pub struct Launch {
    /// Exit once nothing is on screen (used when no background instance runs).
    pub one_shot: bool,
    pub initial: Option<Trigger>,
    /// Listen for the global shortcut and IPC (the background instance).
    pub daemon: bool,
}

struct AppState {
    env: Env,
    overlay: Option<Overlay>,
    pins: Vec<pins::Pin>,
    editors: Vec<annotate::Editor>,
    settings: Option<settings_view::SettingsView>,
    recording: Option<pins::Recording>,
    ui_tx: Sender<UiCommand>,
    ui_rx: Receiver<UiCommand>,
    settings_requests: Vec<settings_view::SettingsRequest>,
    notices: Vec<String>,
    next_id: u64,
    models_rx: Option<Receiver<Result<(), String>>>,
}

impl AppState {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn add_pin(&mut self, image: Arc<RgbaImage>, origin: Option<lens_core::geometry::Rect>) {
        let scale = origin
            .and_then(|o| {
                self.overlay.as_ref().and_then(|ov| {
                    ov.views.iter().find(|v| v.monitor.rect.contains(lens_core::geometry::Point { x: o.x, y: o.y })).map(|v| v.monitor.scale_factor)
                })
            })
            .unwrap_or(1.0) as f32;
        let id = self.id();
        crate::lens_debug!("pin {id} created at {origin:?} scale {scale}");
        self.pins.push(pins::Pin::new(id, image, origin, scale));
    }

    fn save_image(&mut self, img: &RgbaImage) {
        let format = self.env.rt.settings.image_format;
        let r = lens_actions::util::encode(img, format).and_then(|bytes| {
            self.env.host.save_file(SaveRequest {
                suggested_name: lens_actions::util::timestamp_name("Lens", format.extension()),
                bytes,
                directory: None,
                mime: lens_actions::util::mime(format).into(),
            })
        });
        match r {
            Ok(p) => self.notices.push(format!("Saved to {}", p.display())),
            Err(e) => self.notices.push(format!("Save failed: {e}")),
        }
    }
}

fn notify(msg: &str) {
    eprintln!("arcade-lens: {msg}");
    // Best effort desktop notification; never required.
    let _ = std::process::Command::new(if cfg!(target_os = "macos") { "osascript" } else { "notify-send" })
        .args(if cfg!(target_os = "macos") {
            vec!["-e".to_string(), format!("display notification \"{}\" with title \"Arcade Lens\"", msg.replace('"', "'"))]
        } else {
            vec!["Arcade Lens".to_string(), msg.to_string()]
        })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

struct LensApp {
    state: Arc<Mutex<AppState>>,
    paths: Paths,
    triggers: Receiver<Trigger>,
    shortcuts: Option<lens_platform::shortcut::Shortcuts>,
    one_shot: bool,
    focus_pending: bool,
    focus_attempts: u32,
    hidden_once: bool,
    waking: bool,
    registered: std::collections::HashSet<ViewportId>,
    root_visible: bool,
    activity: bool,
}

fn build_env(rt: Arc<Runtime>, ui_tx: Sender<UiCommand>, ctx: &egui::Context, usage: Arc<Mutex<UsageStore>>) -> Env {
    let desktop = DesktopHost::new((*rt.settings).clone(), rt.paths.collections()).long_lived();
    let host = Arc::new(GuiHost::new(desktop, ui_tx, ctx.clone()));
    Env { rt, host, usage }
}

impl LensApp {
    fn begin_capture(&mut self, ctx: &egui::Context) {
        {
            let st = self.state.lock().unwrap();
            if st.overlay.is_some() {
                return;
            }
        }
        // Capture before anything of ours is visible.
        let captures = match lens_platform::capture_all() {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => return notify("no monitors to capture"),
            Err(e) => return notify(&format!("screen capture failed: {e}")),
        };
        let windows = lens_platform::windows(Some(std::process::id()));
        let cursor = lens_platform::cursor_position();
        let views: Vec<MonitorView> = captures
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let ci = egui::ColorImage::from_rgba_unmultiplied([c.image.width() as usize, c.image.height() as usize], c.image.as_raw());
                let texture = ctx.load_texture(format!("lens-capture-{i}"), ci, egui::TextureOptions::NEAREST);
                MonitorView { monitor: c.monitor, image: c.image, texture }
            })
            .collect();
        let ov = Overlay::new(views, windows, cursor);
        let m = ov.views[ov.root_monitor].monitor.clone();
        let s = m.scale_factor as f32;
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(m.rect.x as f32 / s, m.rect.y as f32 / s)));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(m.rect.width as f32 / s, m.rect.height as f32 / s)));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        if lens_platform::display_server() == lens_platform::DisplayServer::Wayland {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        }
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.state.lock().unwrap().overlay = Some(ov);
        self.root_visible = true;
        self.waking = false;
        self.focus_pending = true;
        self.focus_attempts = 0;
        self.activity = true;
        crate::lens_debug!("overlay shown on {} ({:?})", m.name, m.rect);
    }

    fn hide_overlay(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        if lens_platform::display_server() == lens_platform::DisplayServer::Wayland {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
        self.root_visible = false;
    }

    fn apply_settings_requests(&mut self, ctx: &egui::Context) {
        let requests = std::mem::take(&mut self.state.lock().unwrap().settings_requests);
        for r in requests {
            match r {
                settings_view::SettingsRequest::Save { settings, chains } => {
                    let shortcut_result = match &mut self.shortcuts {
                        Some(s) => s.set(settings.activation_shortcut.as_deref()),
                        None => Ok(()),
                    };
                    let (msg, ok) = match shortcut_result {
                        Err(e) => (e, false),
                        Ok(()) => match config::save_settings(&self.paths, &settings).and_then(|_| config::save_chains(&self.paths, &chains)) {
                            Ok(()) => ("Saved".to_string(), true),
                            Err(e) => (e.to_string(), false),
                        },
                    };
                    if ok {
                        self.reload_runtime(ctx);
                    }
                    if let Some(v) = &mut self.state.lock().unwrap().settings {
                        v.status = Some((msg, ok));
                    }
                }
                settings_view::SettingsRequest::DownloadModels => {
                    let (tx, rx) = mpsc::channel();
                    let dir = self.paths.models();
                    let c = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::net::download_models(&dir));
                        c.request_repaint();
                    });
                    let mut st = self.state.lock().unwrap();
                    st.models_rx = Some(rx);
                    if let Some(v) = &mut st.settings {
                        v.models_busy = true;
                    }
                }
                settings_view::SettingsRequest::ResetUsage => {
                    let st = self.state.lock().unwrap();
                    st.env.usage.lock().unwrap().reset();
                    let _ = config::save_usage(&self.paths, &UsageStore::default());
                }
                settings_view::SettingsRequest::ClearHistory => history::clear(&self.paths.history()),
                settings_view::SettingsRequest::Autostart(on) => {
                    let r = match std::env::current_exe() {
                        Ok(exe) if on => lens_platform::autostart::enable(&exe),
                        Ok(_) => lens_platform::autostart::disable(),
                        Err(e) => Err(e),
                    };
                    let mut st = self.state.lock().unwrap();
                    if let Some(v) = &mut st.settings {
                        v.autostart = lens_platform::autostart::is_enabled();
                        v.status = Some(match r {
                            Ok(()) => (if on { "Lens will start at login" } else { "Lens will not start at login" }.into(), true),
                            Err(e) => (e.to_string(), false),
                        });
                    }
                }
                settings_view::SettingsRequest::InstallLauncher => {
                    let r = std::env::current_exe().and_then(|e| lens_platform::autostart::install_launcher(&e));
                    if let Some(v) = &mut self.state.lock().unwrap().settings {
                        v.status = Some(match r {
                            Ok(_) => ("Added to the applications menu".into(), true),
                            Err(e) => (e.to_string(), false),
                        });
                    }
                }
                settings_view::SettingsRequest::OpenFolder(p) => {
                    let _ = std::fs::create_dir_all(&p);
                    let st = self.state.lock().unwrap();
                    let _ = st.env.host.open_path(&p, lens_core::host::OpenPathMode::Default);
                }
            }
        }
    }

    fn reload_runtime(&mut self, ctx: &egui::Context) {
        match Runtime::load(Paths::discover()) {
            Ok(rt) => {
                let rt = Arc::new(rt);
                let mut st = self.state.lock().unwrap();
                let usage = st.env.usage.clone();
                st.env = build_env(rt.clone(), st.ui_tx.clone(), ctx, usage);
                if let Some(v) = &mut st.settings {
                    v.refresh_runtime(rt);
                }
            }
            Err(e) => notify(&format!("could not reload settings: {e}")),
        }
    }

    fn handle_ui_commands(&mut self, ctx: &egui::Context) {
        let mut st = self.state.lock().unwrap();
        while let Ok(c) = st.ui_rx.try_recv() {
            self.activity = true;
            match c {
                UiCommand::Pin { image, origin } => st.add_pin(image, origin),
                UiCommand::Annotate { image } => {
                    let id = st.id();
                    st.editors.push(annotate::Editor::new(id, image));
                    if let Some(ov) = &mut st.overlay {
                        ov.close_requested = true;
                    }
                }
                UiCommand::Measure { rect } => {
                    if let Some(ov) = &mut st.overlay {
                        ov.enter_measure(rect);
                    }
                }
                UiCommand::Record { window } => {
                    let dir = directories::UserDirs::new()
                        .and_then(|u| u.video_dir().map(|p| p.join("Arcade Lens")))
                        .unwrap_or_else(|| self.paths.data.join("recordings"));
                    match pins::Recording::start(&window, &dir) {
                        Ok(r) => st.recording = Some(r),
                        Err(e) => st.notices.push(e),
                    }
                }
                UiCommand::Toast(t) => st.notices.push(t),
            }
        }
        if let Some(rx) = &st.models_rx {
            if let Ok(r) = rx.try_recv() {
                st.models_rx = None;
                drop(st);
                match r {
                    Ok(()) => self.reload_runtime(ctx),
                    Err(e) => notify(&format!("OCR model download failed: {e}")),
                }
                let mut st = self.state.lock().unwrap();
                if let Some(v) = &mut st.settings {
                    v.models_busy = false;
                }
                return;
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        for n in std::mem::take(&mut st.notices) {
            if let Some(ov) = &mut st.overlay {
                ov.toast = Some(overlay::Toast {
                    text: n,
                    kind: overlay::ToastKind::Info,
                    until: std::time::Instant::now() + Duration::from_millis(1600),
                    then_close: false,
                });
            } else {
                notify(&n);
            }
        }
    }

    /// Children that exist in our state but have not been created as windows yet.
    fn has_unregistered_children(&self) -> bool {
        let st = self.state.lock().unwrap();
        let mut ids: Vec<ViewportId> = st.pins.iter().map(|p| p.viewport_id()).collect();
        ids.extend(st.editors.iter().map(|e| e.viewport_id()));
        if st.settings.is_some() {
            ids.push(settings_view::SettingsView::viewport_id());
        }
        if st.recording.is_some() {
            ids.push(ViewportId::from_hash_of("lens-recording"));
        }
        ids.iter().any(|id| !self.registered.contains(id))
    }

    fn anything_open(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.overlay.is_some() || !st.pins.is_empty() || !st.editors.is_empty() || st.settings.is_some() || st.recording.is_some()
    }

    fn open_settings(&mut self) {
        let mut st = self.state.lock().unwrap();
        if st.settings.is_none() {
            st.settings = Some(settings_view::SettingsView::new(st.env.rt.clone()));
        }
        self.activity = true;
    }
}

impl eframe::App for LensApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Some window systems map the root window at creation even when asked
        // not to (X11 override-redirect windows). Make sure it is hidden.
        if !self.root_visible && ctx.input(|i| i.viewport().visible()).unwrap_or(true) && !self.hidden_once {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            self.hidden_once = true;
        }
        while let Ok(t) = self.triggers.try_recv() {
            match t {
                Trigger::Capture => self.begin_capture(ctx),
                Trigger::Settings => self.open_settings(),
                Trigger::Pin(path) => match image::open(&path) {
                    Ok(img) => {
                        self.state.lock().unwrap().add_pin(Arc::new(img.to_rgba8()), None);
                        self.activity = true;
                    }
                    Err(e) => notify(&format!("{}: {e}", path.display())),
                },
                Trigger::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
        self.handle_ui_commands(ctx);
        self.apply_settings_requests(ctx);
        // eframe only runs UI passes while some window is visible, and child
        // windows can only be created from a UI pass. If something must appear
        // while everything is hidden, map the root as a 1×1 window
        // (top-left corner; off-screen windows never repaint on X11) for a frame or
        // two; it hides again as soon as the child is up.
        if !self.root_visible && !self.waking && self.has_unregistered_children() {
            crate::lens_debug!("waking root to create child windows");
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.request_repaint();
            self.waking = true;
        }
        if self.one_shot && self.activity && !self.anything_open() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.waking {
            if child_visible(&ctx) && !self.root_visible {
                ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                self.waking = false;
            } else {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if self.root_visible && !focused && self.focus_attempts < 60 {
            self.focus_pending = true;
            self.focus_attempts += 1;
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let evs = ctx.input(|i| i.events.len());
        if evs > 0 {
            crate::lens_debug!("root frame: {evs} events, focused={focused}, visible={}", self.root_visible);
        }
        if self.focus_pending && self.root_visible {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = frame.window_handle() {
                // `c_ulong` is 64-bit on Linux but 32-bit on Windows.
                #[allow(clippy::unnecessary_cast)]
                let raw = match h.as_raw() {
                    RawWindowHandle::Xlib(x) => lens_platform::raw_window_handle_shim::Raw::X11(x.window as u32),
                    RawWindowHandle::Xcb(x) => lens_platform::raw_window_handle_shim::Raw::X11(x.window.get()),
                    RawWindowHandle::Win32(w) => lens_platform::raw_window_handle_shim::Raw::Win32(w.hwnd.get()),
                    _ => lens_platform::raw_window_handle_shim::Raw::Other,
                };
                lens_platform::focus_native(raw);
            }
            self.focus_pending = false;
        }

        // Root window: the overlay for the monitor under the pointer.
        let mut close_overlay = false;
        {
            let mut guard = self.state.lock().unwrap();
            let st = &mut *guard;
            if let Some(ov) = &mut st.overlay {
                let root = ov.root_monitor;
                egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ui, |ui| {
                    ov.ui(ui, &st.env, root, true);
                });
                if ov.analysis_done && !ov.history_saved {
                    ov.history_saved = true;
                    if st.env.rt.settings.privacy.history_enabled {
                        if let Some(sel) = &ov.sel {
                            history::save_async(st.env.rt.paths.history(), sel.selection.image.clone(), &ov.report.findings);
                        }
                    }
                }
                close_overlay = ov.close_requested;
            }
            if close_overlay {
                st.overlay = None;
            }
        }
        if close_overlay {
            self.hide_overlay(&ctx);
        }

        // Child windows.
        let mut ids: Vec<(ViewportId, ViewportBuilder, Child)> = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            st.pins.retain(|p| !p.closed);
            st.editors.retain(|e| !e.closed);
            if st.settings.as_ref().is_some_and(|s| s.closed) {
                st.settings = None;
            }
            if let Some(ov) = &st.overlay {
                for (i, v) in ov.views.iter().enumerate() {
                    if i == ov.root_monitor {
                        continue;
                    }
                    let s = v.monitor.scale_factor as f32;
                    let r = v.monitor.rect;
                    let b = ViewportBuilder::default()
                        .with_title("Arcade Lens")
                        .with_position([r.x as f32 / s, r.y as f32 / s])
                        .with_inner_size([r.width as f32 / s, r.height as f32 / s])
                        .with_decorations(false)
                        .with_taskbar(false)
                        .with_always_on_top()
                        .with_override_redirect(lens_platform::display_server() == lens_platform::DisplayServer::X11);
                    ids.push((ViewportId::from_hash_of(("lens-overlay", i)), b, Child::Overlay(i)));
                }
            }
            for p in &st.pins {
                ids.push((p.viewport_id(), p.builder(), Child::Pin(p.id)));
            }
            for e in &st.editors {
                ids.push((e.viewport_id(), e.builder(), Child::Editor(e.id)));
            }
            if st.settings.is_some() {
                ids.push((settings_view::SettingsView::viewport_id(), settings_view::SettingsView::builder(), Child::Settings));
            }
            if let Some(r) = &st.recording {
                let b = ViewportBuilder::default()
                    .with_title("Recording — Arcade Lens")
                    .with_inner_size([220.0, 44.0])
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_taskbar(false);
                let _ = r;
                ids.push((ViewportId::from_hash_of("lens-recording"), b, Child::Recording));
            }
        }
        self.registered = ids.iter().map(|(id, ..)| *id).collect();
        for (id, builder, child) in ids {
            let state = self.state.clone();
            ctx.show_viewport_deferred(id, builder, move |ui, _class| child.ui(ui, &state));
        }
        if self.anything_open() {
            self.activity = true;
        }
    }
}

fn child_visible(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.raw.viewports.iter().any(|(id, v)| *id != ViewportId::ROOT && v.visible() == Some(true)))
}

#[derive(Clone, Copy)]
enum Child {
    Overlay(usize),
    Pin(u64),
    Editor(u64),
    Settings,
    Recording,
}

impl Child {
    fn ui(self, ui: &mut egui::Ui, state: &Arc<Mutex<AppState>>) {
        let ctx = ui.ctx().clone();
        let mut guard = state.lock().unwrap();
        let st = &mut *guard;
        match self {
            Child::Overlay(i) => {
                if let Some(ov) = &mut st.overlay {
                    if i < ov.views.len() {
                        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ui, |ui| ov.ui(ui, &st.env, i, false));
                    }
                }
                // Keyboard focus lives in the root overlay; repaint it so key handling stays live.
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            Child::Pin(id) => {
                crate::lens_debug!("pin {id} frame");
                let Some(pin) = st.pins.iter_mut().find(|p| p.id == id) else { return };
                let reqs = pin.ui(ui);
                for r in reqs {
                    match r {
                        pins::PinRequest::Copy(img) => {
                            let _ = st.env.host.set_clipboard_image(&img);
                        }
                        pins::PinRequest::Save(img) => st.save_image(&img),
                        pins::PinRequest::Annotate(img) => {
                            let id = st.id();
                            st.editors.push(annotate::Editor::new(id, img));
                        }
                        pins::PinRequest::CloseAll => st.pins.iter_mut().for_each(|p| p.closed = true),
                    }
                }
                if st.pins.iter().any(|p| p.closed) {
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
            }
            Child::Editor(id) => {
                let Some(ed) = st.editors.iter_mut().find(|e| e.id == id) else { return };
                let reqs = ed.ui(ui);
                let closed = ed.closed;
                for r in reqs {
                    match r {
                        annotate::EditorRequest::Copy(img) => {
                            let msg = match st.env.host.set_clipboard_image(&img) {
                                Ok(()) => "Copied annotated image".to_string(),
                                Err(e) => e.to_string(),
                            };
                            notify(&msg);
                        }
                        annotate::EditorRequest::Save(img) => st.save_image(&img),
                        annotate::EditorRequest::Pin(img) => st.add_pin(img, None),
                    }
                }
                if closed {
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
            }
            Child::Settings => {
                if let Some(v) = &mut st.settings {
                    let reqs = v.ui(ui);
                    let closed = v.closed;
                    st.settings_requests.extend(reqs);
                    if closed || !st.settings_requests.is_empty() {
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                }
            }
            Child::Recording => {
                if let Some(r) = &st.recording {
                    if r.ui(ui) {
                        let rec = st.recording.take().unwrap();
                        let path = rec.stop();
                        st.notices.push(format!("Recording saved to {}", path.display()));
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                }
            }
        }
    }
}

static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Diagnostics on stderr when `LENS_DEBUG` is set. Never logs selection content.
pub fn debug_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LENS_DEBUG").is_some())
}

#[macro_export]
macro_rules! lens_debug {
    ($($t:tt)*) => {
        if $crate::gui::debug_enabled() {
            eprintln!("[lens {:>8.3}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() % 1000.0), format!($($t)*));
        }
    };
}

/// Runs the GUI until quit (daemon) or until nothing is on screen (one-shot).
pub fn run(launch: Launch) -> Result<(), String> {
    let paths = Paths::discover();
    let (tx, rx) = mpsc::channel::<Trigger>();
    if launch.daemon {
        let t = tx.clone();
        lens_platform::ipc::serve(&paths.endpoint(), move |cmd| {
            let trig = match cmd.split_once(' ').map_or((cmd, ""), |(a, b)| (a, b)) {
                ("ping", _) => return "pong".into(),
                ("capture", _) => Trigger::Capture,
                ("settings", _) => Trigger::Settings,
                ("quit", _) => Trigger::Quit,
                ("pin", p) if !p.is_empty() => Trigger::Pin(p.into()),
                _ => return "unknown command".into(),
            };
            let _ = t.send(trig);
            if let Some(c) = CONTEXT.get() {
                c.request_repaint();
            }
            "ok".into()
        })
        .map_err(|e| e.to_string())?;
    }
    if let Some(t) = launch.initial.clone() {
        let _ = tx.send(t);
    }
    let rt = Arc::new(Runtime::load(paths.clone())?);
    let x11 = lens_platform::display_server() == lens_platform::DisplayServer::X11;
    let root = ViewportBuilder::default()
        .with_title("Arcade Lens")
        .with_app_id("arcade-lens")
        .with_visible(false)
        .with_decorations(false)
        .with_taskbar(false)
        .with_inner_size([1.0, 1.0])
        .with_position([-100.0, -100.0])
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_override_redirect(x11);
    let options = eframe::NativeOptions { viewport: root, ..Default::default() };
    let daemon = launch.daemon;
    let one_shot = launch.one_shot;
    eframe::run_native(
        "Arcade Lens",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let _ = CONTEXT.set(ctx.clone());
            theme::apply_style(&ctx);
            let mut shortcuts = None;
            if daemon {
                match lens_platform::shortcut::Shortcuts::new() {
                    Ok(mut s) => {
                        if let Err(e) = s.set(rt.settings.activation_shortcut.as_deref()) {
                            notify(&format!("{e}. Change it in Settings → Shortcut."));
                        }
                        let t = tx.clone();
                        let c = ctx.clone();
                        lens_platform::shortcut::on_pressed(move |_| {
                            let _ = t.send(Trigger::Capture);
                            c.request_repaint();
                        });
                        shortcuts = Some(s);
                    }
                    Err(e) => notify(&format!("global shortcut unavailable: {e}")),
                }
            }
            // Wake the event loop for queued initial triggers.
            ctx.request_repaint();
            let (ui_tx, ui_rx) = mpsc::channel();
            let usage = Arc::new(Mutex::new(config::load_usage(&rt.paths)));
            let env = build_env(rt.clone(), ui_tx.clone(), &ctx, usage);
            let state = AppState {
                env,
                overlay: None,
                pins: Vec::new(),
                editors: Vec::new(),
                settings: None,
                recording: None,
                ui_tx,
                ui_rx,
                settings_requests: Vec::new(),
                notices: Vec::new(),
                next_id: 0,
                models_rx: None,
            };
            Ok(Box::new(LensApp {
                state: Arc::new(Mutex::new(state)),
                paths: rt.paths.clone(),
                triggers: rx,
                shortcuts,
                one_shot,
                focus_pending: false,
                focus_attempts: 0,
                hidden_once: false,
                waking: false,
                registered: Default::default(),
                root_visible: false,
                activity: false,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}
