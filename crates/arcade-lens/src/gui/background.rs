//! The background instance's place on the desktop: the tray icon, the
//! login item, the applications-menu entry, and restarting.

use std::process::Command;

use lens_platform::autostart;
use lens_platform::tray::{Tray, TrayEvent};

use super::notify;
use crate::config::Paths;
use crate::icon;

/// Shows the tray icon, if the desktop has a tray.
pub fn tray(on_event: impl Fn(TrayEvent) + Send + Sync + 'static) -> Option<Tray> {
    match Tray::spawn(icon::tray_icons(), on_event) {
        Ok(t) => Some(t),
        Err(e) => {
            crate::lens_debug!("no tray icon: {e}");
            None
        }
    }
}

/// Keeps the launcher entry and the login item pointing at this executable.
/// The first time Lens runs it also turns on start at login. Runs in the
/// background, so it never delays startup.
pub fn integrate(paths: &Paths, shortcut: Option<String>) {
    let Ok(exe) = autostart::launch_path() else { return };
    if !permanent(&exe) {
        return;
    }
    let marker = paths.config.join("installed");
    std::thread::spawn(move || {
        let first_run = !marker.exists();
        // An installer (the self-installing AppImage, the Windows installer,
        // the macOS move) owns the entries of a copy with a receipt; an
        // AppImage without one asks first (see `first_run_install`).
        let installed = arcade_link::install::Environment::discover().ok().and_then(|e| e.receipts().read(crate::link::ME).ok().flatten()).is_some();
        if !installed && std::env::var_os("APPIMAGE").is_none() {
            if first_run || autostart::is_enabled() {
                if let Err(e) = autostart::enable(&exe) {
                    notify(&format!("Couldn't turn on Start at login. {e}"));
                }
            }
            let icons: Vec<(u32, Vec<u8>)> = icon::THEME_SIZES.iter().map(|&s| (s, icon::png(s))).collect();
            if let Err(e) = autostart::install_launcher(&exe, &icons) {
                notify(&format!("Couldn't add Arcade Lens to the applications menu. {e}"));
            }
        }
        if first_run {
            let _ = std::fs::create_dir_all(marker.parent().unwrap_or(&marker)).and_then(|_| std::fs::write(&marker, ""));
            notify(&match shortcut {
                Some(s) => format!("Arcade Lens is running in the tray. Press {s} to select anything on screen."),
                None => "Arcade Lens is running in the tray.".into(),
            });
        }
    });
}

/// Whether `exe` is an installed copy that the desktop may point at. Test
/// instances, cargo builds and copies run from a mounted disk image (or
/// relocated by macOS Gatekeeper) leave the desktop alone.
fn permanent(exe: &std::path::Path) -> bool {
    let s = exe.to_string_lossy();
    std::env::var_os("ARCADE_LENS_HOME").is_none()
        && !exe.components().any(|c| c.as_os_str() == "target")
        && !(cfg!(target_os = "macos") && (s.starts_with("/Volumes/") || s.contains("/AppTranslocation/")))
}

/// Starts a new background instance that takes over once this one exits.
/// `false` (and this instance should keep running) if it couldn't start.
pub fn restart() -> bool {
    // The successor runs from a new mount; it mustn't keep this one alive.
    let start = |exe| arcade_link::childenv::detach_from_image(&mut Command::new(exe)).arg("--restarting").spawn();
    match autostart::launch_path().and_then(start) {
        Ok(_) => true,
        Err(e) => {
            notify(&format!("Couldn't restart Arcade Lens. {e}"));
            false
        }
    }
}
