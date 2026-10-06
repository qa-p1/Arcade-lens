//! Arcade Lens: select anything on screen and act on it.
//!
//! A background app with a tray icon. Launching it again (from the
//! applications menu, or with one of the options below) talks to the running
//! instance instead of starting a second one.

// No console window on Windows: Lens is a desktop app.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod config;
mod gui;
mod host;
mod icon;
mod link;
mod net;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::config::Paths;

const HELP: &str = "Arcade Lens: select anything on screen and act on it.

  arcade-lens               Open Settings; Lens keeps running in the tray
  arcade-lens --background  Start in the background (used at login)
  arcade-lens --capture     Select something on screen now
  arcade-lens --restart     Restart the background instance
  arcade-lens --quit        Quit the background instance
  arcade-lens --version     Print the version
  arcade-lens --arcade-manifest
                            Print Lens's Arcade Link manifest (no side effects)

Once running, press the activation shortcut (Ctrl+Alt+Shift+L by default)
or use the tray icon.";

fn main() -> ExitCode {
    // Older macOS passes `-psn_…` to apps opened from Finder.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            gui::notify(&e);
            ExitCode::FAILURE
        }
    }
}

/// Sends `command` to the running instance; `false` if none is running.
fn tell_running(command: &str) -> bool {
    lens_platform::ipc::send(&Paths::discover().endpoint(), command).is_ok()
}

fn run(args: &[String]) -> Result<(), String> {
    // `capture` and `--capture` both work, so older shortcut bindings keep working.
    let option = args.first().map(|a| a.trim_start_matches('-'));
    match option {
        None | Some("settings") => {
            if !tell_running("settings") {
                gui::run(Some(gui::Trigger::Settings))?;
            }
            Ok(())
        }
        Some("background" | "start") => {
            if !tell_running("ping") {
                gui::run(None)?;
            }
            Ok(())
        }
        Some("restarting") => {
            // Started by a restart: wait for the previous instance to exit.
            let deadline = Instant::now() + Duration::from_secs(5);
            while tell_running("ping") && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            gui::run(None)
        }
        Some("capture") => {
            if !tell_running("capture") {
                gui::run(Some(gui::Trigger::Capture))?;
            }
            Ok(())
        }
        Some("restart") => {
            if !tell_running("restart") {
                gui::run(None)?;
            }
            Ok(())
        }
        Some("quit") => {
            tell_running("quit");
            Ok(())
        }
        // Internal: one window in its own process (Wayland).
        Some("window") => {
            let kind = args.get(1).map(String::as_str).unwrap_or_default();
            let image = || args.get(2).map(PathBuf::from).ok_or_else(|| format!("{kind} needs an image path"));
            let solo = match kind {
                "capture" => gui::wayland::Solo::Capture(link::CaptureMode::parse(args.get(2).map(String::as_str)).map_err(|e| e.to_string())?),
                "settings" => gui::wayland::Solo::Settings,
                "pin" => gui::wayland::Solo::Pin(image()?),
                "annotate" => gui::wayland::Solo::Annotate(image()?),
                "pick" => gui::wayland::Solo::Pick(image()?),
                "analyze" => gui::wayland::Solo::Analyze(image()?),
                _ => return Err(format!("unknown window {kind:?}")),
            };
            gui::run_solo(Paths::discover(), solo)
        }
        Some("help" | "h") => {
            println!("{HELP}");
            Ok(())
        }
        Some("arcade-invoke") => std::process::exit(link::serve_oneshot()),
        Some("arcade-manifest") => {
            let settings = config::load_settings(&Paths::discover()).unwrap_or_default();
            println!("{}", link::manifest(&settings).to_json());
            Ok(())
        }
        Some("version" | "V") => {
            println!("Arcade Lens {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!("unknown option {other:?}; see arcade-lens --help")),
    }
}
