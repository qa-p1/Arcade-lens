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
    Capture,
    Settings,
    Pin(PathBuf),
    Annotate(PathBuf),
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
        Solo::Capture => cmd.arg("capture"),
        Solo::Settings => cmd.arg("settings"),
        Solo::Pin(p) => cmd.arg("pin").arg(p),
        Solo::Annotate(p) => cmd.arg("annotate").arg(p),
    };
    cmd.spawn().map_err(|e| notify(&format!("cannot open window: {e}"))).ok()
}

/// Opens a window process and reaps it when it exits.
fn spawn_detached(solo: &Solo) {
    if let Some(mut child) = spawn(solo) {
        std::thread::spawn(move || child.wait());
    }
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("arcade-lens")
}

/// Opens `image` in a new pin or editor process (`kind` is `Solo::Pin` or `Solo::Annotate`).
pub fn open_image(kind: fn(PathBuf) -> Solo, image: &RgbaImage) {
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
        Ok(()) => spawn_detached(&kind(path)),
        Err(e) => notify(&format!("cannot hand over image: {e}")),
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

/// The background instance: IPC, the shortcut and the tray icon, no window.
pub fn daemon(paths: &Paths, initial: Option<Trigger>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<&'static str>();
    let t = tx.clone();
    lens_platform::ipc::serve(&paths.endpoint(), move |cmd| {
        let cmd = match cmd {
            "ping" => return "pong".into(),
            "capture" => "capture",
            "settings" => "settings",
            "reload" => "reload",
            "restart" => "restart",
            "quit" => "quit",
            _ => return "unknown command".into(),
        };
        let _ = t.send(cmd);
        "ok".into()
    })
    .map_err(|e| e.to_string())?;
    match initial {
        Some(Trigger::Capture) => tx.send("capture"),
        Some(Trigger::Settings) => tx.send("settings"),
        Some(Trigger::Restart | Trigger::Quit) | None => Ok(()),
    }
    .ok();
    let t = tx.clone();
    if hyprland::active() {
        hyprland::add_window_rules(WINDOW_RULES);
        hyprland::on_config_reloaded(move || {
            let _ = tx.send("rebind");
        });
    }
    let mut shortcut = Shortcut { bound: None, warned: false };
    shortcut.apply(paths);
    let bound = || shortcut.bound.as_ref().map(|(s, _)| s.clone());
    background::integrate(paths, bound());
    let tray = background::tray(bound(), move |e| {
        let _ = t.send(match e {
            TrayEvent::Capture => "capture",
            TrayEvent::Settings => "settings",
            TrayEvent::Restart => "restart",
            TrayEvent::Quit => "quit",
        });
    });
    let mut capture = None;
    let mut settings = None;
    for cmd in rx {
        crate::lens_debug!("command {cmd}");
        match cmd {
            "capture" => open_once(&mut capture, &Solo::Capture),
            "settings" => open_once(&mut settings, &Solo::Settings),
            "reload" => {
                shortcut.apply(paths);
                if let Some(t) = &tray {
                    t.set_shortcut(shortcut.bound.as_ref().map(|(s, _)| s.clone()));
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
    shortcut.clear();
    drop(tray);
    Ok(())
}
