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
    /// Settings, optionally opened on a section.
    Settings(Option<super::settings_view::Section>),
    History,
    /// A pin of the image, and the screen area it came from (for Live refresh).
    Pin(PathBuf, Option<lens_core::geometry::Rect>),
    Annotate(PathBuf),
    /// Arcade Link `lens.capture`: select once, write the region to the path, exit.
    Pick(PathBuf),
    /// Arcade Link `lens.analyze`: an image as the frozen overlay.
    Analyze(PathBuf),
}

/// Keeps Lens windows out of the tiling layout and the overlay unanimated.
/// The overlay maps floating and full-screen at once: as a tiled window it
/// would first take a tile, push the other windows aside and animate.
/// Every Lens window has the app id `dev.arcade.Lens`; titles tell them apart.
const WINDOW_RULES: &[&str] = &[
    r#"match = { class = "^(dev\\.arcade\\.Lens)$", title = "^(Arcade Lens)$" }, float = true, fullscreen = true, no_anim = true, opaque = true, no_blur = true, no_shadow = true, no_dim = true"#,
    r#"match = { class = "^(dev\\.arcade\\.Lens)$", title = "^(Pin — Arcade Lens)$" }, float = true, pin = true, no_anim = true, no_blur = true, no_shadow = true, border_size = 0"#,
    r#"match = { class = "^(dev\\.arcade\\.Lens)$", title = "^((Annotate|Settings|History) — Arcade Lens)$" }, float = true, center = true, opaque = true"#,
];

use lens_platform::autostart::current_exe as exe;

fn spawn(solo: &Solo) -> Option<Child> {
    let exe = match exe() {
        Ok(e) => e,
        Err(e) => {
            notify(&format!("Couldn't find the Arcade Lens program. {e}"));
            return None;
        }
    };
    let mut cmd = Command::new(exe);
    cmd.arg("--window");
    match solo {
        Solo::Capture(mode) => cmd.arg("capture").arg(mode.hint()),
        Solo::Settings(section) => {
            cmd.arg("settings");
            if let Some(s) = section {
                cmd.arg(s.label());
            }
            &mut cmd
        }
        Solo::History => cmd.arg("history"),
        Solo::Pin(p, origin) => {
            cmd.arg("pin").arg(p);
            if let Some(r) = origin {
                cmd.arg(rect_arg(r));
            }
            &mut cmd
        }
        Solo::Annotate(p) => cmd.arg("annotate").arg(p),
        Solo::Pick(p) => cmd.arg("pick").arg(p),
        Solo::Analyze(p) => cmd.arg("analyze").arg(p),
    };
    cmd.spawn().map_err(|e| notify(&format!("Couldn't open the window. {e}"))).ok()
}

/// Opens a window process and reaps it when it exits.
/// Capture overlays opened for a Link request, which run outside the loop's
/// `capture` slot; `--status` counts them too (LN-11).
static LINK_OVERLAYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts one Link-opened overlay for as long as it lives.
struct LinkOverlay;

impl LinkOverlay {
    fn open() -> LinkOverlay {
        LINK_OVERLAYS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        LinkOverlay
    }
}

impl Drop for LinkOverlay {
    fn drop(&mut self) {
        LINK_OVERLAYS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn link_overlays_open() -> bool {
    LINK_OVERLAYS.load(std::sync::atomic::Ordering::SeqCst) > 0
}

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
/// `x,y,width,height`, the pin window's origin argument.
pub fn rect_arg(r: &lens_core::geometry::Rect) -> String {
    format!("{},{},{},{}", r.x, r.y, r.width, r.height)
}

pub fn parse_rect_arg(s: &str) -> Option<lens_core::geometry::Rect> {
    let mut it = s.split(',');
    let r = lens_core::geometry::Rect {
        x: it.next()?.parse().ok()?,
        y: it.next()?.parse().ok()?,
        width: it.next()?.parse().ok()?,
        height: it.next()?.parse().ok()?,
    };
    it.next().is_none().then_some(r)
}

pub fn open_image(kind: impl FnOnce(PathBuf) -> Solo, image: &RgbaImage) -> bool {
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
            notify(&format!("Couldn't pass the image to its window. {e}"));
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

/// The activation shortcut: a compositor binding where we can add one, a
/// native X11 grab outside Wayland.
struct Shortcut {
    bound: Option<(String, hyprland::Binding)>,
    native: Option<(lens_platform::shortcut::Shortcuts, Option<String>)>,
    warned: bool,
}

fn x11() -> bool {
    lens_platform::display_server() != lens_platform::DisplayServer::Wayland
}

impl Shortcut {
    fn apply(&mut self, paths: &Paths) {
        let wanted = config::load_settings(paths).ok().and_then(|s| s.activation_shortcut);
        if let Some((native, current)) = &mut self.native {
            match native.set(wanted.as_deref()) {
                Ok(()) => *current = wanted,
                Err(e) => notify(&format!("{e}. Change it in Settings › Shortcuts.")),
            }
            return;
        }
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
            Err(e) => notify(&format!("Couldn't set the shortcut {s}. {e}")),
        }
    }

    fn clear(&mut self) {
        if let Some((_, b)) = self.bound.take() {
            b.remove();
        }
    }

    fn current(&self) -> Option<String> {
        if let Some((_, current)) = &self.native {
            return current.clone();
        }
        self.bound.as_ref().map(|(s, _)| s.clone())
    }

    /// Binds exactly `accelerator` now, for `app.shortcuts.set` and imports.
    /// Desktops without runtime binds get the manual hint instead.
    fn bind(&mut self, accelerator: Option<&str>) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
        use arcade_link::app::ShortcutVia;
        if let Some((native, current)) = &mut self.native {
            native.set(accelerator).map_err(arcade_link::LinkError::unavailable)?;
            *current = accelerator.map(String::from);
            return Ok(crate::app::hub::apply_result(ShortcutVia::Native, None));
        }
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
        GuiRequest::Capture { act: true, mode, .. } => {
            let Some(mut child) = spawn(&Solo::Capture(mode)) else { return false };
            let open = LinkOverlay::open();
            std::thread::spawn(move || {
                let _open = open;
                child.wait()
            });
            return true;
        }
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
            let open = LinkOverlay::open();
            std::thread::spawn(move || {
                let _open = open;
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
        GuiRequest::Pin(image) => return open_image(|p| Solo::Pin(p, None), &image),
    }
    true
}

/// Starts `solo`, or brings the previous one to the front if it is still open.
fn open_once(slot: &mut Option<Child>, solo: &Solo) {
    open_once_with(slot, || spawn(solo));
}

fn open_once_with(slot: &mut Option<Child>, start: impl FnOnce() -> Option<Child>) {
    if let Some(c) = slot {
        if matches!(c.try_wait(), Ok(None)) {
            if hyprland::active() {
                hyprland::focus_pid(c.id());
            }
            return;
        }
    }
    *slot = start();
}

/// Forgets a window process that has exited; `try_wait` reaps it, so a
/// closed Settings or History window leaves no zombie (LN-3).
fn reap_exited(slot: &mut Option<Child>) {
    if slot.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None))) {
        *slot = None;
    }
}

fn close_child(slot: &mut Option<Child>) {
    if let Some(mut c) = slot.take() {
        if matches!(c.try_wait(), Ok(None)) {
            let _ = c.kill();
        }
        let _ = c.wait();
    }
}

/// Commands for the background instance's loop.
enum Cmd {
    Named(&'static str),
    /// Settings on a section (`settings:<label>`).
    SettingsAt(super::settings_view::Section),
    /// `--status`: answered from the loop, which knows whether a capture is open.
    Status(mpsc::SyncSender<String>),
    /// `detach:<pid>`: that capture closed its window but still waits for a
    /// peer to finish a handed-off action; it no longer counts as the capture.
    Detach(u32),
    Hub(Trigger),
    /// Every 2 s: reap window processes that have exited.
    Tick,
}

/// The background instance: IPC, the shortcut and the tray icon, no window.
pub fn daemon(paths: &Paths, initial: Option<Trigger>) -> Result<(), String> {
    use arcade_link::trayhost::TrayState;
    let background = initial.is_none();
    let (tx, rx) = mpsc::channel::<Cmd>();
    {
        let tick = tx.clone();
        std::thread::spawn(move || {
            while tick.send(Cmd::Tick).is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }
    let t = std::sync::Mutex::new(tx.clone());
    lens_platform::ipc::serve(&paths.endpoint(), move |cmd| {
        let send = |c: Cmd| t.lock().unwrap_or_else(|e| e.into_inner()).send(c).is_ok();
        if let Some(reply) = super::hub_command(cmd, |trigger| send(Cmd::Hub(trigger))) {
            return reply;
        }
        if let Some(label) = cmd.strip_prefix("settings:") {
            return match super::settings_view::Section::from_label(label) {
                Some(section) if send(Cmd::SettingsAt(section)) => "ok".into(),
                Some(_) => "busy".into(),
                None => "unknown section".into(),
            };
        }
        if let Some(pid) = cmd.strip_prefix("detach:").and_then(|p| p.parse().ok()) {
            return if send(Cmd::Detach(pid)) { "ok".into() } else { "busy".into() };
        }
        if cmd == "status" {
            let (reply, answer) = mpsc::sync_channel(1);
            return if send(Cmd::Status(reply)) { answer.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default() } else { String::new() };
        }
        let cmd = match cmd {
            "ping" => return "pong".into(),
            "history" => "history",
            "show" => "capture",
            "hide" => "hide",
            "toggle" => "toggle",
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
    let mut shortcut = Shortcut { bound: None, native: None, warned: false };
    if x11() {
        match lens_platform::shortcut::Shortcuts::new() {
            Ok(native) => {
                shortcut.native = Some((native, None));
                let t = std::sync::Mutex::new(tx.clone());
                lens_platform::shortcut::on_pressed(move |_| {
                    let _ = t.lock().unwrap_or_else(|e| e.into_inner()).send(Cmd::Named("capture"));
                });
            }
            Err(e) => notify(&format!("Couldn't register the global shortcut. {e}")),
        }
    }
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
    let mut capture: Option<Child> = None;
    let mut settings_window: Option<Child> = None;
    let mut history_window: Option<Child> = None;
    // Captures that handed an action to a peer and wait, windowless, for it.
    let mut finishing: Vec<Child> = Vec::new();
    for cmd in rx {
        finishing.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        for slot in [&mut capture, &mut settings_window, &mut history_window] {
            reap_exited(slot);
        }
        match cmd {
            Cmd::Tick => {}
            Cmd::Detach(pid) => {
                if capture.as_ref().is_some_and(|c| c.id() == pid) {
                    finishing.extend(capture.take());
                }
            }
            Cmd::Status(reply) => {
                let open = capture.as_mut().is_some_and(|c: &mut Child| matches!(c.try_wait(), Ok(None))) || link_overlays_open();
                let _ = reply.send(super::status_json(paths, true, open));
            }
            Cmd::SettingsAt(section) => {
                // A running Settings process can't be told to switch pages;
                // settings apply as they change, so reopening loses nothing.
                if let Some(mut c) = settings_window.take() {
                    if matches!(c.try_wait(), Ok(None)) {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                }
                open_once(&mut settings_window, &Solo::Settings(Some(section)));
            }
            Cmd::Named(cmd) => {
                crate::lens_debug!("command {cmd}");
                match cmd {
                    "capture" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Palette)),
                    "capture-text" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Text)),
                    "pick-color" => open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Color)),
                    "settings" => open_once(&mut settings_window, &Solo::Settings(None)),
                    "history" => open_once(&mut history_window, &Solo::History),
                    // The overlay is its own process with nothing to save.
                    "hide" => close_child(&mut capture),
                    "toggle" => {
                        if capture.as_mut().is_some_and(|c: &mut Child| matches!(c.try_wait(), Ok(None))) {
                            close_child(&mut capture);
                        } else {
                            open_once(&mut capture, &Solo::Capture(crate::link::CaptureMode::Palette));
                        }
                    }
                    "reload" => {
                        let settings = config::load_settings(paths).unwrap_or_default();
                        crate::link::refresh(&settings);
                        crate::app::trayhost::set_link_enabled(settings.link.enabled);
                        shortcut.apply(paths);
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
                        tray = background::tray(move |e| {
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
    // Quit (or Restart) closes Lens's windows too; they are separate
    // processes and would otherwise outlive the app.
    for slot in [&mut capture, &mut settings_window, &mut history_window] {
        close_child(slot);
    }
    shortcut.clear();
    crate::app::trayhost::stop();
    crate::link::stop();
    drop(tray);
    Ok(())
}

#[cfg(test)]
mod tests {

    /// LN-11: an overlay opened for a Link request counts while it is open.
    #[test]
    fn link_overlays_count_while_open() {
        let before = LINK_OVERLAYS.load(std::sync::atomic::Ordering::SeqCst);
        let open = LinkOverlay::open();
        assert!(link_overlays_open());
        drop(open);
        assert_eq!(LINK_OVERLAYS.load(std::sync::atomic::Ordering::SeqCst), before);
    }

    /// LN-7: a pin's origin survives the trip to its own process.
    #[test]
    fn a_pin_origin_round_trips_through_its_argument() {
        let r = lens_core::geometry::Rect { x: -1920, y: 40, width: 300, height: 200 };
        assert_eq!(parse_rect_arg(&rect_arg(&r)), Some(r));
        assert_eq!(parse_rect_arg("1,2,3"), None);
        assert_eq!(parse_rect_arg("1,2,3,4,5"), None);
    }

    use super::*;

    #[test]
    fn every_window_title_has_a_hyprland_rule() {
        let rules = WINDOW_RULES.join("\n");
        assert!(rules.contains("^(Arcade Lens)$"), "overlay");
        assert!(rules.contains("^(Pin — Arcade Lens)$"), "pin");
        for title in ["Annotate", "Settings", "History"] {
            assert!(rules.contains(title), "{title}");
        }
        // Every rule is scoped to Lens's own app id (Lua sees `\.`).
        assert_eq!(rules.matches(r#"class = "^(dev\\.arcade\\.Lens)$""#).count(), WINDOW_RULES.len());
    }

    #[cfg(target_os = "linux")]
    fn zombie(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|rest| rest.starts_with('Z')))
    }

    #[cfg(target_os = "linux")]
    // LN-3 / BP-6
    #[test]
    fn closed_windows_leave_no_zombies_after_20_cycles() {
        let mut slot: Option<Child> = None;
        let mut pids = Vec::new();
        for _ in 0..20 {
            open_once_with(&mut slot, || Command::new("sh").args(["-c", "exit 0"]).spawn().ok());
            pids.push(slot.as_ref().map(Child::id).unwrap());
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        reap_exited(&mut slot); // the daemon's 2 s tick
        let zombies: Vec<u32> = pids.into_iter().filter(|p| zombie(*p)).collect();
        assert!(zombies.is_empty(), "zombie children left behind: {zombies:?}");
    }
}
