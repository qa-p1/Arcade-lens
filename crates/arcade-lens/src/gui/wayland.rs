//! Wayland: one window per process.
//!
//! Wayland clients can't hide a window, so the hidden root window the other
//! platforms keep around would show up as an empty window (a tile, on tiling
//! compositors). Here the background instance has no window at all: each
//! overlay, pin, annotation editor and the settings window runs in its own
//! process (`arcade-lens --window …`) whose root window *is* that window,
//! and which exits when it closes.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;

use image::RgbaImage;
use lens_platform::hyprland;
use lens_platform::tray::TrayEvent;

use super::{background, notify, Trigger};
use crate::config::{self, Paths};

/// What a window process shows.
#[derive(Debug, Clone)]
pub enum Solo {
    Capture(crate::link::CaptureMode),
    Settings,
    Pin(PathBuf),
    Annotate(PathBuf),
    /// Arcade Link `lens.capture`: select once, write the region to the path, exit.
    Pick(PathBuf),
    /// Arcade Link `lens.analyze`: an image as the frozen overlay.
    Analyze(PathBuf),
}

/// Keeps Lens windows out of the tiling layout and the overlay unanimated.
const WINDOW_RULES: &[&str] = &[
    r#"match = { class = "^(arcade-lens)$" }, no_anim = true, opaque = true, no_blur = true, no_shadow = true, no_dim = true"#,
    r#"match = { class = "^(arcade-lens-pin)$" }, float = true, pin = true, opaque = true, no_anim = true, no_blur = true, no_shadow = true, border_size = 0"#,
    r#"match = { class = "^(arcade-lens-(annotate|settings))$" }, float = true, center = true, opaque = true"#,
];

use lens_platform::autostart::current_exe as exe;

fn spawn(solo: &Solo) -> Option<Child> {
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => {
            notify(&format!("cannot locate arcade-lens: {e}"));
            return None;
        }
    };
    let mut cmd = Command::new(exe);
    cmd.arg("--window");
    match solo {
        Solo::Capture(mode) => cmd.arg("capture").arg(mode.hint()),
        Solo::Settings => cmd.arg("settings"),
        Solo::Pin(p) => cmd.arg("pin").arg(p),
        Solo::Annotate(p) => cmd.arg("annotate").arg(p),
        Solo::Pick(p) => cmd.arg("pick").arg(p),
        Solo::Analyze(p) => cmd.arg("analyze").arg(p),
    };
    cmd.spawn().map_err(|e| notify(&format!("cannot open window: {e}"))).ok()
}

/// Opens a window process and reaps it when it exits.
fn spawn_detached(solo: &Solo) -> bool {
    if let Some(mut child) = spawn(solo) {
        std::thread::spawn(move || child.wait());
        true
    } else {
        false
    }
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("arcade-lens")
}

/// Opens `image` in a new pin or editor process (`kind` is `Solo::Pin` or `Solo::Annotate`).
pub fn open_image(kind: fn(PathBuf) -> Solo, image: &RgbaImage) -> bool {
    let dir = temp_dir();
    let path =
        dir.join(format!("{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    let r = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        image.save(&path).map_err(|e| e.to_string())
    });
    match r {
        Ok(()) => {
            let started = spawn_detached(&kind(path.clone()));
            if !started {
                let _ = std::fs::remove_file(path);
            }
            started
        }
        Err(e) => {
            notify(&format!("cannot hand over image: {e}"));
            false
        }
    }
}

/// Loads a window's image; hand-over files from [`open_image`] are deleted.
pub fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
    if path.starts_with(temp_dir()) {
        let _ = std::fs::remove_file(path);
    }
    Ok(img)
}

/// The activation shortcut, as a compositor binding where we can add one.
struct Shortcut {
    bound: Option<(String, hyprland::Binding)>,
    warned: bool,
}

impl Shortcut {
    fn apply(&mut self, paths: &Paths) {
        let wanted = config::load_settings(paths).ok().and_then(|s| s.activation_shortcut);
        if !hyprland::active() {
            if wanted.is_some() && !self.warned {
                notify("This Wayland desktop doesn't let apps register shortcuts. Bind `arcade-lens --capture` to a key in its keyboard settings.");
                self.warned = true;
            }
            return;
        }
        if self.bound.as_ref().map(|(s, _)| s) == wanted.as_ref() {
            return;
        }
        self.clear();
        let Some(s) = wanted else { return };
        let exe = exe().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|_| "arcade-lens".into());
        match hyprland::bind(&s, &format!("{} --capture", hyprland::shell_quote(&exe))) {
            Ok(b) => {
                crate::lens_debug!("bound {s} in Hyprland");
                self.bound = Some((s, b));
            }
            Err(e) => notify(&format!("could not bind {s}: {e}")),
        }
    }

    fn clear(&mut self) {
        if let Some((_, b)) = self.bound.take() {
            b.remove();
        }
    }

    fn current(&self) -> Option<String> {
        self.bound.as_ref().map(|(s, _)| s.clone())
    }

    /// Binds exactly `accelerator` now, for `app.shortcuts.set` and imports.
    /// Desktops without runtime binds get the manual hint instead.
    fn bind(&mut self, accelerator: Option<&str>) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
        use arcade_link::app::ShortcutVia;
        if !hyprland::active() {
            return Ok(crate::app::hub::apply_result(
                ShortcutVia::Manual,
                Some("Bind `arcade-lens --capture` to a key in your desktop's keyboard settings.".into()),
            ));
        }
        if self.bound.as_ref().map(|(s, _)| s.as_str()) == accelerator {
            return Ok(crate::app::hub::apply_result(ShortcutVia::HyprlandRuntime, None));
        }
        self.clear();
        if let Some(s) = accelerator {
            let exe = exe().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|_| "arcade-lens".into());
            let binding =
                hyprland::bind(s, &format!("{} --capture", hyprland::shell_quote(&exe))).map_err(|e| arcade_link::LinkError::unavailable(e.to_string()))?;
            self.bound = Some((s.to_string(), binding));
        }
        Ok(crate::app::hub::apply_result(ShortcutVia::HyprlandRuntime, None))
    }
}

/// Requests from other Arcade apps: each opens its own window process.
fn link_request(request: crate::link::GuiRequest) -> bool {
    use crate::link::{GuiRequest, PickTarget};
    match request {
        GuiRequest::Capture { act: true, mode, .. } => return spawn_detached(&Solo::Capture(mode)),
        GuiRequest::Capture { target: PickTarget::Reply(reply), .. } => {
            if !reply.is_pending() {
                return true;
            }
            let out = temp_dir().join(format!(
                "pick-{}-{}.png",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
            ));
            let _ = std::fs::create_dir_all(temp_dir());
            let Some(mut child) = spawn(&Solo::Pick(out.clone())) else { return false };
            std::thread::spawn(move || {
                // Only a pending picker needs this worker. Cancellation
                // releases its sender immediately and stops this child.
                while reply.is_pending() {
                    match child.try_wait() {
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                        Ok(Some(_)) | Err(_) => break,
                    }
                }
                if !reply.is_pending() {
                    let _ = child.kill();
                }
                let _ = child.wait();
                if let Some(c) = crate::link::read_captured_file(&out) {
                    reply.deliver(c);
                }
                let _ = std::fs::remove_file(&out);
                let _ = std::fs::remove_file(out.with_extension("png.json"));
            });
        }
        GuiRequest::CancelCapture(_) => {} // the picker worker observes the released sender
        GuiRequest::Capture { target: PickTarget::File(_), .. } => return false,
        GuiRequest::Analyze(image) => return open_image(Solo::Analyze, &image),
        GuiRequest::Pin(image) => return open_image(Solo::Pin, &image),
    }
    true
}

/// Starts `solo`, or brings the previous one to the front if it is still open.
fn open_once(slot: &mut Option<Child>, solo: &Solo) {
    if let Some(c) = slot {
        if matches!(c.try_wait(), Ok(None)) {
            if hyprland::active() {
                hyprland::focus_pid(c.id());
            }
            return;
        }
    }
    *slot = spawn(solo);
}

/// Commands for the background instance's loop.
enum Cmd {
    Named(&'static str),
    Hub(Trigger),
}

/// The background instance: IPC, the shortcut and the tray icon, no window.
pub fn daemon(paths: &Paths, initial: Option<Trigger>) -> Result<(), String> {
    use arcade_link::trayhost::TrayState;
    let background = initial.is_none();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let t = std::sync::Mutex::new(tx.clone());
    lens_platform::ipc::serve(&paths.endpoint(), move |cmd| {
        let send = |c: Cmd| t.lock().unwrap_or_else(|e| e.into_inner()).send(c).is_ok();
        if let Some(reply) = super::hub_command(cmd, |trigger| send(Cmd::Hub(trigger))) {
            return reply;
        }
        let cmd = match cmd {
            "ping" => return "pong".into(),
            "capture" => "capture",
            "capture-text" => "capture-text",
            "pick-color" => "pick-color",
            "settings" => "settings",
            "reload" => "reload",
            "restart" => "restart",
            "quit" => "quit",
            _ => return "unknown command".into(),
        };
        send(Cmd::Named(cmd));
        "ok".into()
    })
    .map_err(|e| e.to_string())?;
    match initial {
        Some(Trigger::Capture) => tx.send(Cmd::Named("capture")),
        Some(Trigger::CaptureText) => tx.send(Cmd::Named("capture-text")),
        Some(Trigger::PickColor) => tx.send(Cmd::Named("pick-color")),
        Some(Trigger::Settings) => tx.send(Cmd::Named("settings")),
        _ => Ok(()),
    }
    .ok();
    if hyprland::active() {
        hyprland::add_window_rules(WINDOW_RULES);
        let t = tx.clone();
        hyprland::on_config_reloaded(move || {
            let _ = t.send(Cmd::Named("rebind"));
        });
    }
    crate::link::set_gui(link_request);
    let settings = config::load_settings(paths).unwrap_or_default();
    crate::link::start(&settings, background);
    let mut shortcut = Shortcut { bound: None, warned: false };
    shortcut.apply(paths);
    background::integrate(paths, shortcut.current());
    let t = tx.clone();
    if let Err(e) = crate::app::trayhost::start(&settings, move |state| {
        let _ = t.send(Cmd::Hub(Trigger::TrayState(state)));
    }) {
        crate::lens_debug!("tray host watcher: {e}");
        let _ = tx.send(Cmd::Hub(Trigger::TrayState(TrayState::Own)));
    }
    let mut tray: Option<lens_platform::tray::Tray> = None;
    let mut capture = None;
    let mut settings_window = None;
    for cmd in rx {
        match cmd {
            Cmd::Named(cmd) => {
                crate::lens_debug!("command {cmd}");
                match cmd {
                    "capture" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Palette)),
                    "capture-text" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Text)),
                    "pick-color" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Color)),
                    "settings" => open_once(&mut settings_window, &Solo::Settings),
                    "reload" => {
                        let settings = config::load_settings(paths).unwrap_or_default();
                        crate::link::refresh(&settings);
                        crate::app::trayhost::set_link_enabled(settings.link.enabled);
                        shortcut.apply(paths);
                        if let Some(t) = &tray {
                            t.set_shortcut(shortcut.current());
                        }
                    }
                    "rebind" => {
                        // The reload dropped our binding and rules.
                        shortcut.bound = None;
                        hyprland::add_window_rules(WINDOW_RULES);
                        shortcut.apply(paths);
                    }
                    "restart" if background::restart() => break,
                    "quit" => break,
                    _ => {}
                }
            }
            Cmd::Hub(Trigger::TrayState(state)) => {
                match state {
                    TrayState::Own if tray.is_none() => {
                        let t = tx.clone();
                        tray = background::tray(shortcut.current(), move |e| {
                            let _ = t.send(Cmd::Named(match e {
                                TrayEvent::Capture => "capture",
                                TrayEvent::CaptureText => "capture-text",
                                TrayEvent::PickColor => "pick-color",
                                TrayEvent::Settings => "settings",
                                TrayEvent::Restart => "restart",
                                TrayEvent::Quit => "quit",
                            }));
                        });
                    }
                    TrayState::Own => {
                        if let Some(t) = &tray {
                            t.set_visible(true);
                        }
                    }
                    // SNI hosts still show Passive items: unregister by dropping.
                    TrayState::Hosted | TrayState::None => tray = None,
                }
                // Report after the icon changed, so status.tray never runs ahead of it.
                crate::link::set_tray_state(if state == TrayState::Own && tray.is_none() { TrayState::None } else { state });
            }
            Cmd::Hub(Trigger::ShortcutSet { accelerator, reply }) => {
                let result = crate::app::hub::set_shortcut(paths, &accelerator, |a| shortcut.bind(a));
                if let Some(t) = &tray {
                    t.set_shortcut(shortcut.current());
                }
                let _ = reply.send(result);
            }
            Cmd::Hub(Trigger::SettingsImport { inputs, reply }) => {
                let result = crate::app::hub::import_settings(paths, &inputs, |a| shortcut.bind(a));
                if let Ok(settings) = config::load_settings(paths) {
                    crate::app::trayhost::set_link_enabled(settings.link.enabled);
                }
                let _ = reply.send(result);
            }
            Cmd::Hub(_) => {}
        }
    }
    shortcut.clear();
    crate::app::trayhost::stop();
    crate::link::stop();
    drop(tray);
    Ok(())
}
