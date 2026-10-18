//! The Arcade Lens desktop application.
//!
//! One process, one hidden root window. The root window *is* the overlay:
//! idle, it is hidden and costs nothing; on the shortcut, Lens captures the
//! screens first and only then shows the frozen frame, so the overlay never
//! sees itself. Pins, annotation editors, the settings window, extra-monitor
//! overlays and the recording indicator are independent child windows.

pub mod annotate;
pub mod background;
pub mod ghost;
pub mod glyphs;
pub mod history;
pub mod measure;
pub mod overlay;
pub mod panels;
pub mod pins;
pub mod runtime;
pub mod settings_view;
pub mod theme;
pub mod wayland;

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
    CaptureText,
    PickColor,
    Settings,
    Restart,
    Quit,
    TrayState(arcade_link::trayhost::TrayState),
    ShortcutSet {
        accelerator: String,
        reply: std::sync::mpsc::SyncSender<Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError>>,
    },
    SettingsImport {
        inputs: Vec<arcade_link::Content>,
        reply: std::sync::mpsc::SyncSender<Result<arcade_link::app::ImportResult, arcade_link::LinkError>>,
    },
    /// An interactive request from another Arcade app.
    Link(crate::link::GuiRequest),
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
    tesseract_rx: Option<Receiver<Result<(), String>>>,
    /// One window per process (Wayland): new pins and editors open in their own process.
    solo: bool,
}

impl AppState {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn add_pin(&mut self, image: Arc<RgbaImage>, origin: Option<lens_core::geometry::Rect>) {
        if self.solo {
            wayland::open_image(wayland::Solo::Pin, &image);
            return;
        }
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

    fn add_editor(&mut self, image: Arc<RgbaImage>) {
        if self.solo {
            wayland::open_image(wayland::Solo::Annotate, &image);
            return;
        }
        let id = self.id();
        self.editors.push(annotate::Editor::new(id, image));
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

/// Tells the user something outside of any Lens window.
pub fn notify(msg: &str) {
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

/// Sets up a process's first window (see [`LensApp::init`]).
type InitWindow = Box<dyn FnOnce(&mut AppState, &egui::Context)>;

struct LensApp {
    state: Arc<Mutex<AppState>>,
    paths: Paths,
    triggers: Receiver<Trigger>,
    trigger_sender: Sender<Trigger>,
    shortcuts: Option<lens_platform::shortcut::Shortcuts>,
    tray: Option<lens_platform::tray::Tray>,
    one_shot: bool,
    focus_pending: bool,
    focus_attempts: u32,
    hidden_once: bool,
    waking: bool,
    registered: std::collections::HashSet<ViewportId>,
    root_visible: bool,
    activity: bool,
    /// The root window is the only window: an overlay, pin, editor or settings (Wayland).
    solo: bool,
    /// Sets up the initial window; runs on the first frame, once egui knows the GPU's texture limits.
    init: Option<InitWindow>,
}

fn build_env(rt: Arc<Runtime>, ui_tx: Sender<UiCommand>, ctx: &egui::Context, usage: Arc<Mutex<UsageStore>>) -> Env {
    let desktop = DesktopHost::new((*rt.settings).clone(), rt.paths.collections()).long_lived();
    let repaint = ctx.clone();
    rt.arcade.on_change(move || repaint.request_repaint());
    let host = Arc::new(GuiHost::new(desktop, ui_tx, ctx.clone(), rt.arcade.clone(), rt.settings.clone()));
    Env { rt, host, usage }
}

/// The overlay for already captured screens.
fn make_overlay(ctx: &egui::Context, captures: Vec<lens_platform::Capture>) -> Overlay {
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
    Overlay::new(views, windows, cursor)
}

impl LensApp {
    fn apply_live_shortcut(&mut self, accelerator: Option<&str>) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
        use arcade_link::app::ShortcutVia;
        let shortcuts = self.shortcuts.as_mut().ok_or_else(|| arcade_link::LinkError::unavailable("native shortcuts are unavailable"))?;
        shortcuts.set(accelerator).map_err(arcade_link::LinkError::unavailable)?;
        Ok(crate::app::hub::apply_result(ShortcutVia::Native, None))
    }

    fn apply_tray_state(&mut self, state: arcade_link::trayhost::TrayState) {
        use arcade_link::trayhost::TrayState;
        match state {
            TrayState::Own if self.tray.is_none() => {
                let shortcut = self.state.lock().unwrap_or_else(|e| e.into_inner()).env.rt.settings.activation_shortcut.clone();
                let tx = self.trigger_sender.clone();
                self.tray = background::tray(shortcut, move |event| {
                    use lens_platform::tray::TrayEvent;
                    let trigger = match event {
                        TrayEvent::Capture => Trigger::Capture,
                        TrayEvent::CaptureText => Trigger::CaptureText,
                        TrayEvent::PickColor => Trigger::PickColor,
                        TrayEvent::Settings => Trigger::Settings,
                        TrayEvent::Restart => Trigger::Restart,
                        TrayEvent::Quit => Trigger::Quit,
                    };
                    // Wake the event loop too, or the click waits for the next repaint.
                    send_trigger(&tx, trigger);
                });
            }
            TrayState::Own => {
                if let Some(tray) = &self.tray {
                    tray.set_visible(true);
                }
            }
            // StatusNotifierItem hosts such as Caelestia and Waybar still show
            // Passive items, so Linux unregisters a hosted icon by dropping it.
            TrayState::Hosted if cfg!(target_os = "linux") => self.tray = None,
            TrayState::Hosted => {
                if let Some(tray) = &self.tray {
                    tray.set_visible(false);
                }
            }
            TrayState::None => self.tray = None,
        }
        // Report after the icon changed, so status.tray never runs ahead of it.
        crate::link::set_tray_state(if state == TrayState::Own && self.tray.is_none() { TrayState::None } else { state });
    }

    fn apply_hub_shortcut(&mut self, ctx: &egui::Context, accelerator: &str) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
        let paths = self.paths.clone();
        let result = crate::app::hub::set_shortcut(&paths, accelerator, |a| self.apply_live_shortcut(a))?;
        if let Some(tray) = &self.tray {
            let settings = config::load_settings(&self.paths).map_err(arcade_link::LinkError::unavailable)?;
            tray.set_shortcut(settings.activation_shortcut);
        }
        self.reload_runtime(ctx);
        Ok(result)
    }

    fn import_hub_settings(&mut self, ctx: &egui::Context, inputs: &[arcade_link::Content]) -> Result<arcade_link::app::ImportResult, arcade_link::LinkError> {
        let paths = self.paths.clone();
        let result = crate::app::hub::import_settings(&paths, inputs, |a| self.apply_live_shortcut(a))?;
        if result.imported {
            self.reload_runtime(ctx);
            if let Some(tray) = &self.tray {
                let settings = config::load_settings(&self.paths).map_err(arcade_link::LinkError::unavailable)?;
                tray.set_shortcut(settings.activation_shortcut);
            }
        }
        Ok(result)
    }

    fn begin_capture(&mut self, ctx: &egui::Context) {
        self.begin_capture_mode(ctx, crate::link::CaptureMode::Palette);
    }

    fn begin_capture_mode(&mut self, ctx: &egui::Context, mode: crate::link::CaptureMode) {
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
        self.show_overlay(ctx, captures);
        if let Some(overlay) = self.state.lock().unwrap().overlay.as_mut() {
            overlay.mode = mode;
            overlay.measure = mode == crate::link::CaptureMode::Measure;
        }
    }

    /// Requests from other Arcade apps (Arcade Link).
    fn link_request(&mut self, ctx: &egui::Context, request: crate::link::GuiRequest) {
        use crate::link::GuiRequest;
        self.activity = true;
        match request {
            GuiRequest::Capture { target, mode, act } => {
                if !act && !target.is_pending() {
                    return;
                }
                self.begin_capture(ctx);
                let mut st = self.state.lock().unwrap();
                if let Some(ov) = &mut st.overlay {
                    if act {
                        ov.mode = mode;
                        ov.measure = mode == crate::link::CaptureMode::Measure;
                    } else {
                        ov.pick = Some(target);
                    }
                }
            }
            GuiRequest::CancelCapture(cancel) => {
                let dismissed = {
                    let mut st = self.state.lock().unwrap();
                    if st.overlay.as_ref().and_then(|ov| ov.pick.as_ref()).is_some_and(|pick| cancel.matches(pick)) {
                        st.overlay = None;
                        true
                    } else {
                        false
                    }
                };
                if dismissed {
                    self.hide_overlay(ctx);
                }
            }
            GuiRequest::Analyze(image) => {
                if self.state.lock().unwrap().overlay.is_some() {
                    return;
                }
                let monitors = lens_platform::monitors().unwrap_or_default();
                let cursor = lens_platform::cursor_position();
                let Some(monitor) = cursor
                    .and_then(|c| monitors.iter().find(|m| m.rect.contains(c)))
                    .or_else(|| monitors.iter().find(|m| m.is_primary))
                    .or(monitors.first())
                    .cloned()
                else {
                    return notify("no monitor to show the image on");
                };
                let (canvas, rect) = crate::link::analyze_canvas(&image, &monitor);
                self.show_overlay(ctx, vec![lens_platform::Capture { monitor, image: Arc::new(canvas) }]);
                let mut guard = self.state.lock().unwrap();
                let st = &mut *guard;
                if let Some(ov) = &mut st.overlay {
                    ov.set_selection(&st.env, 0, rect);
                }
            }
            GuiRequest::Pin(image) => self.state.lock().unwrap().add_pin(image, None),
        }
    }

    /// Shows the overlay over already captured (or composed) screens.
    fn show_overlay(&mut self, ctx: &egui::Context, captures: Vec<lens_platform::Capture>) {
        let ov = make_overlay(ctx, captures);
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
                        None => settings.activation_shortcut.as_deref().map_or(Ok(()), |s| lens_platform::shortcut::parse(s).map(|_| ())),
                    };
                    let (msg, ok) = match shortcut_result {
                        Err(e) => (e, false),
                        Ok(()) => match config::save_settings(&self.paths, &settings).and_then(|_| config::save_chains(&self.paths, &chains)) {
                            Ok(()) => ("Saved".to_string(), true),
                            Err(e) => (e.to_string(), false),
                        },
                    };
                    if ok {
                        if let Some(t) = &self.tray {
                            t.set_shortcut(settings.activation_shortcut.clone());
                        }
                        self.reload_runtime(ctx);
                        if self.solo {
                            // The background instance owns the shortcut binding.
                            let _ = lens_platform::ipc::send(&self.paths.endpoint(), "reload");
                        }
                    }
                    if let Some(v) = &mut self.state.lock().unwrap().settings {
                        v.status = Some((msg, ok));
                    }
                }
                settings_view::SettingsRequest::DownloadTesseract => {
                    let (tx, rx) = mpsc::channel();
                    let c = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(arcade_link::engines::download_tesseract().map(|_| ()));
                        c.request_repaint();
                    });
                    let mut st = self.state.lock().unwrap();
                    st.tesseract_rx = Some(rx);
                    if let Some(v) = &mut st.settings {
                        v.tesseract_busy = true;
                    }
                }
                settings_view::SettingsRequest::ResetUsage => {
                    let st = self.state.lock().unwrap();
                    st.env.usage.lock().unwrap().reset();
                    let _ = config::save_usage(&self.paths, &UsageStore::default());
                }
                settings_view::SettingsRequest::ClearHistory => history::clear(&self.paths.history()),
                settings_view::SettingsRequest::Autostart(on) => {
                    let r = match lens_platform::autostart::launch_path() {
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
                    let icons: Vec<(u32, Vec<u8>)> = crate::icon::THEME_SIZES.iter().map(|&s| (s, crate::icon::png(s))).collect();
                    let r = lens_platform::autostart::launch_path().and_then(|e| lens_platform::autostart::install_launcher(&e, &icons));
                    if let Some(v) = &mut self.state.lock().unwrap().settings {
                        v.launcher = lens_platform::autostart::launcher_installed();
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
                settings_view::SettingsRequest::GetApp(app) => {
                    let st = self.state.lock().unwrap();
                    let arcade = st.env.rt.arcade.clone();
                    let host = st.env.host.clone();
                    let tx = st.ui_tx.clone();
                    let repaint = ctx.clone();
                    std::thread::spawn(move || {
                        let result = arcade.get(&app).unwrap_or_else(|| host.open_uri(arcade_link::manifest::releases_url(&app), false));
                        let (message, ok) = match result {
                            Ok(()) => ("Opened".into(), true),
                            Err(e) => (e.to_string(), false),
                        };
                        let _ = tx.send(UiCommand::SettingsStatus { message, ok });
                        repaint.request_repaint();
                    });
                }
            }
        }
    }

    fn reload_runtime(&mut self, ctx: &egui::Context) {
        match Runtime::load(Paths::discover()) {
            Ok(rt) => {
                let rt = Arc::new(rt);
                if !self.solo {
                    crate::link::refresh(&rt.settings);
                }
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
                    st.add_editor(image);
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
                UiCommand::SettingsStatus { message, ok } => {
                    if let Some(settings) = &mut st.settings {
                        settings.status = Some((message, ok));
                    }
                }
            }
        }
        if let Some(rx) = &st.tesseract_rx {
            if let Ok(r) = rx.try_recv() {
                st.tesseract_rx = None;
                drop(st);
                match r {
                    Ok(()) => self.reload_runtime(ctx),
                    Err(e) => notify(&format!("Tesseract download failed: {e}")),
                }
                let mut st = self.state.lock().unwrap();
                if let Some(v) = &mut st.settings {
                    v.tesseract_busy = false;
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

    fn open_settings(&mut self, ctx: &egui::Context) {
        let mut st = self.state.lock().unwrap();
        if st.settings.is_none() {
            st.settings = Some(settings_view::SettingsView::new(st.env.rt.clone()));
        } else {
            ctx.send_viewport_cmd_to(settings_view::SettingsView::viewport_id(), ViewportCommand::Focus);
        }
        self.activity = true;
    }
}

impl LensApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(init) = self.init.take() {
            init(&mut self.state.lock().unwrap(), ctx);
        }
        // Some window systems map the root window at creation even when asked
        // not to (X11 override-redirect windows). Make sure it is hidden.
        if !self.solo && !self.root_visible && !self.hidden_once {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            self.hidden_once = true;
        }
        while let Ok(t) = self.triggers.try_recv() {
            match t {
                Trigger::Capture => self.begin_capture(ctx),
                Trigger::CaptureText => self.begin_capture_mode(ctx, crate::link::CaptureMode::Text),
                Trigger::PickColor => self.begin_capture_mode(ctx, crate::link::CaptureMode::Color),
                Trigger::Settings => self.open_settings(ctx),
                Trigger::Restart => {
                    if background::restart() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
                Trigger::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
                Trigger::TrayState(state) => self.apply_tray_state(state),
                Trigger::ShortcutSet { accelerator, reply } => {
                    let result = self.apply_hub_shortcut(ctx, &accelerator);
                    let _ = reply.send(result);
                }
                Trigger::SettingsImport { inputs, reply } => {
                    let result = self.import_hub_settings(ctx, &inputs);
                    let _ = reply.send(result);
                }
                Trigger::Link(request) => self.link_request(ctx, request),
            }
        }
        self.handle_ui_commands(ctx);
        self.apply_settings_requests(ctx);
        // eframe only runs UI passes while some window is visible, and child
        // windows can only be created from a UI pass. If something must appear
        // while everything is hidden, map the root as a 1×1 window
        // (top-left corner; off-screen windows never repaint on X11) for a frame or
        // two; it hides again as soon as the child is up.
        if !self.solo && !self.root_visible && !self.waking && self.has_unregistered_children() {
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

    fn paint(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let ctx = ctx.clone();
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
                prepare_overlay(&ctx, "Arcade Lens");
                let root = ov.root_monitor;
                egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(&ctx, |ui| {
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
        if close_overlay && !self.solo {
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
                        .with_title(format!("Arcade Lens Capture {i}"))
                        .with_position([r.x as f32 / s, r.y as f32 / s])
                        .with_inner_size([r.width as f32 / s, r.height as f32 / s])
                        .with_decorations(false)
                        .with_taskbar(false)
                        .with_always_on_top()
                        .with_window_type(egui::X11WindowType::Dock);
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
        let mut ids = ids.into_iter();
        if self.solo && self.state.lock().unwrap().overlay.is_none() {
            // This process's one window is the root window itself.
            if let Some((_, _, child)) = ids.next() {
                child.ui(&ctx, &self.state);
            }
        }
        for (id, builder, child) in ids {
            let state = self.state.clone();
            ctx.show_viewport_deferred(id, builder, move |ctx, _class| child.ui(ctx, &state));
        }
        if self.anything_open() {
            self.activity = true;
        }
    }
}

impl eframe::App for LensApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.logic(ctx, frame);
        self.paint(ctx, frame);
    }
}

fn child_visible(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.raw.viewports.iter().any(|(id, v)| *id != ViewportId::ROOT && v.inner_rect.is_some() && v.minimized != Some(true)))
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
    fn ui(self, ctx: &egui::Context, state: &Arc<Mutex<AppState>>) {
        let ctx = ctx.clone();
        let mut guard = state.lock().unwrap();
        let st = &mut *guard;
        match self {
            Child::Overlay(i) => {
                prepare_overlay(&ctx, &format!("Arcade Lens Capture {i}"));
                if let Some(ov) = &mut st.overlay {
                    if i < ov.views.len() {
                        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(&ctx, |ui| ov.ui(ui, &st.env, i, false));
                    }
                }
                // Keyboard focus lives in the root overlay; repaint it so key handling stays live.
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            Child::Pin(id) => {
                crate::lens_debug!("pin {id} frame");
                let Some(pin) = st.pins.iter_mut().find(|p| p.id == id) else { return };
                let reqs = egui::CentralPanel::default().show(&ctx, |ui| pin.ui(ui)).inner;
                for r in reqs {
                    match r {
                        pins::PinRequest::Copy(img) => {
                            let _ = st.env.host.set_clipboard_image(&img);
                        }
                        pins::PinRequest::Save(img) => st.save_image(&img),
                        pins::PinRequest::Annotate(img) => st.add_editor(img),
                        pins::PinRequest::CloseAll => st.pins.iter_mut().for_each(|p| p.closed = true),
                    }
                }
                if st.pins.iter().any(|p| p.closed) {
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
            }
            Child::Editor(id) => {
                let Some(ed) = st.editors.iter_mut().find(|e| e.id == id) else { return };
                let reqs = ed.ui(&ctx);
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
                    let reqs = v.ui(&ctx);
                    let closed = v.closed;
                    st.settings_requests.extend(reqs);
                    if closed || !st.settings_requests.is_empty() {
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                }
            }
            Child::Recording => {
                if let Some(r) = &st.recording {
                    if egui::CentralPanel::default().show(&ctx, |ui| r.ui(ui)).inner {
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

/// Apply the X11 override-redirect flag once per capture window. egui 0.33
/// exposes EWMH window types; the platform adapter supplies the native flag.
fn prepare_overlay(ctx: &egui::Context, title: &str) {
    let key = egui::Id::new(("native-overlay", ctx.viewport_id()));
    if !ctx.data(|data| data.get_temp::<bool>(key).unwrap_or(false)) {
        lens_platform::prepare_overlay(title);
        ctx.data_mut(|data| data.insert_temp(key, true));
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

/// Runs the background instance until it quits: the global shortcut, the
/// tray icon and IPC, then `initial` if given.
pub fn run(initial: Option<Trigger>) -> Result<(), String> {
    let background = initial.is_none();
    let paths = Paths::discover();
    // The log folder the manifest advertises: without a terminal, warnings
    // and errors go there instead of nowhere.
    if let Err(e) = arcade_link::logs::capture_stderr(&paths.data.join("logs"), "arcade-lens") {
        eprintln!("arcade-lens: log file: {e}");
    }
    // The resident instance holds no window, GL context or egui state: each
    // capture, settings, pin or editor window is its own short-lived process
    // (a capture opens in about a quarter of a second). On X11 this saves
    // the ~50 MiB an in-process overlay keeps after its first use.
    // ARCADE_LENS_RESIDENT_UI=1 keeps the windows in this process instead.
    let resident_ui = std::env::var_os("ARCADE_LENS_RESIDENT_UI").is_some_and(|v| v == "1");
    if lens_platform::display_server() == lens_platform::DisplayServer::Wayland || !resident_ui {
        return wayland::daemon(&paths, initial);
    }
    let (tx, rx) = mpsc::channel::<Trigger>();
    let t = tx.clone();
    let hub_tx = Mutex::new(tx.clone());
    lens_platform::ipc::serve(&paths.endpoint(), move |cmd| {
        if let Some(reply) = hub_command(cmd, |trigger| {
            let tx = hub_tx.lock().unwrap_or_else(|e| e.into_inner()).clone();
            send_trigger(&tx, trigger);
            true
        }) {
            return reply;
        }
        let trig = match cmd {
            "ping" => return "pong".into(),
            "capture" => Trigger::Capture,
            "capture-text" => Trigger::CaptureText,
            "pick-color" => Trigger::PickColor,
            "settings" => Trigger::Settings,
            "restart" => Trigger::Restart,
            "quit" => Trigger::Quit,
            _ => return "unknown command".into(),
        };
        send_trigger(&t, trig);
        "ok".into()
    })
    .map_err(|e| e.to_string())?;
    if let Some(t) = initial {
        let _ = tx.send(t);
    }
    let link_tx = tx.clone();
    crate::link::set_gui(move |request| {
        send_trigger(&link_tx, Trigger::Link(request));
        true
    });
    let rt = Arc::new(Runtime::load(paths)?);
    crate::link::start(&rt.settings, background);
    background::integrate(&rt.paths, rt.settings.activation_shortcut.clone());
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
        .with_window_type(if x11 { egui::X11WindowType::Dock } else { egui::X11WindowType::Normal });
    let mode = Mode { daemon: true, one_shot: false, solo: false };
    let r = start_app(rt, root, (tx, rx), mode, |_, _| {});
    crate::app::trayhost::stop();
    crate::link::stop();
    r
}

/// `shortcut-set` and `settings-import` from Link workers: answered by the
/// UI thread, which owns the native shortcut, and returned as a Link result.
pub(crate) fn hub_command(cmd: &str, send: impl Fn(Trigger) -> bool) -> Option<String> {
    use crate::app::hub::{ask, encode_reply};
    if let Some(json) = cmd.strip_prefix("shortcut-set ") {
        let accelerator = serde_json::from_str::<serde_json::Value>(json).ok().and_then(|v| v["accelerator"].as_str().map(str::to_string));
        let result = match accelerator {
            Some(accelerator) => ask(|reply| send(Trigger::ShortcutSet { accelerator, reply })),
            None => Err(arcade_link::LinkError::denied("missing accelerator")),
        };
        return Some(encode_reply(result));
    }
    if let Some(json) = cmd.strip_prefix("settings-import ") {
        let result = match serde_json::from_str::<Vec<arcade_link::Content>>(json) {
            Ok(inputs) => ask(|reply| send(Trigger::SettingsImport { inputs, reply })),
            Err(e) => Err(arcade_link::LinkError::denied(&format!("invalid settings input: {e}"))),
        };
        return Some(encode_reply(result));
    }
    None
}

/// Queues `t` for the app and wakes its event loop.
fn send_trigger(tx: &Sender<Trigger>, t: Trigger) {
    let _ = tx.send(t);
    if let Some(c) = CONTEXT.get() {
        c.request_repaint();
    }
}

/// Runs one window as this process's root window (Wayland), until it closes.
pub fn run_solo(paths: Paths, solo: wayland::Solo) -> Result<(), String> {
    use wayland::Solo;
    // Settings, the OCR engine and plugins load while the screen is captured.
    let started = std::time::Instant::now();
    let loading = std::thread::spawn(move || Runtime::load(paths));
    // Capture before anything of ours is on screen.
    let mut analyze_rect = None;
    let captures = match &solo {
        Solo::Capture(_) | Solo::Pick(_) => match lens_platform::capture_all() {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => return Err("no monitors to capture".into()),
            Err(e) => return Err(format!("screen capture failed: {e}")),
        },
        Solo::Analyze(p) => {
            let image = wayland::load_image(p)?;
            let monitors = lens_platform::monitors().map_err(|e| e.to_string())?;
            let monitor = monitors.iter().find(|m| m.is_primary).or(monitors.first()).cloned().ok_or("no monitor")?;
            let (canvas, rect) = crate::link::analyze_canvas(&image, &monitor);
            analyze_rect = Some(rect);
            vec![lens_platform::Capture { monitor, image: Arc::new(canvas) }]
        }
        _ => Vec::new(),
    };
    crate::lens_debug!("captured after {:?}", started.elapsed());
    let image = match &solo {
        Solo::Pin(p) | Solo::Annotate(p) => Some(Arc::new(wayland::load_image(p)?)),
        _ => None,
    };
    let rt = Arc::new(loading.join().map_err(|_| "loading settings failed".to_string())??);
    crate::lens_debug!("ready after {:?}", started.elapsed());
    let root = match &solo {
        Solo::Capture(_) | Solo::Pick(_) | Solo::Analyze(_) => {
            ViewportBuilder::default().with_title("Arcade Lens").with_app_id("arcade-lens").with_decorations(false).with_fullscreen(true)
        }
        Solo::Settings => settings_view::SettingsView::builder(),
        Solo::Pin(_) => pins::Pin::new(0, image.clone().unwrap(), None, 1.0).builder(),
        Solo::Annotate(_) => annotate::Editor::new(0, image.clone().unwrap()).builder(),
    };
    let mode = Mode { daemon: false, one_shot: true, solo: true };
    start_app(rt, root, mpsc::channel(), mode, move |st, ctx| match solo {
        Solo::Capture(mode) => {
            let mut ov = make_overlay(ctx, captures);
            ov.mode = mode;
            ov.measure = mode == crate::link::CaptureMode::Measure;
            st.overlay = Some(ov);
        }
        Solo::Pick(out) => {
            let mut ov = make_overlay(ctx, captures);
            ov.pick = Some(crate::link::PickTarget::File(out));
            st.overlay = Some(ov);
        }
        Solo::Analyze(_) => {
            let mut ov = make_overlay(ctx, captures);
            if let Some(rect) = analyze_rect {
                ov.set_selection(&st.env, 0, rect);
            }
            st.overlay = Some(ov);
        }
        Solo::Settings => st.settings = Some(settings_view::SettingsView::new(st.env.rt.clone())),
        Solo::Pin(_) => {
            let id = st.id();
            st.pins.push(pins::Pin::new(id, image.unwrap(), None, 1.0));
        }
        Solo::Annotate(_) => {
            let id = st.id();
            st.editors.push(annotate::Editor::new(id, image.unwrap()));
        }
    })
}

struct Mode {
    daemon: bool,
    one_shot: bool,
    solo: bool,
}

fn start_app(
    rt: Arc<Runtime>,
    root: ViewportBuilder,
    (tx, rx): (Sender<Trigger>, Receiver<Trigger>),
    mode: Mode,
    init: impl FnOnce(&mut AppState, &egui::Context) + 'static,
) -> Result<(), String> {
    let options = eframe::NativeOptions { viewport: root.with_icon(crate::icon::window_icon()), ..Default::default() };
    let Mode { daemon, one_shot, solo } = mode;
    eframe::run_native(
        "Arcade Lens",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let _ = CONTEXT.set(ctx.clone());
            theme::apply_style(&ctx);
            let mut shortcuts = None;
            let tray = None;
            if daemon {
                // The icon appears on the watcher's first answer: at once
                // without Tools, never while Tools hosts it (300 ms cap).
                let t = tx.clone();
                if let Err(e) = crate::app::trayhost::start(&rt.settings, move |state| send_trigger(&t, Trigger::TrayState(state))) {
                    crate::lens_debug!("tray host watcher: {e}");
                    send_trigger(&tx, Trigger::TrayState(arcade_link::trayhost::TrayState::Own));
                }
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
                tesseract_rx: None,
                solo,
            };
            Ok(Box::new(LensApp {
                state: Arc::new(Mutex::new(state)),
                paths: rt.paths.clone(),
                triggers: rx,
                trigger_sender: tx,
                shortcuts,
                tray,
                one_shot,
                focus_pending: false,
                focus_attempts: 0,
                hidden_once: false,
                waking: false,
                registered: Default::default(),
                root_visible: solo,
                activity: solo,
                solo,
                init: Some(Box::new(init)),
            }))
        }),
    )
    .map_err(|e| e.to_string())
}
