//! Arcade Lens: select anything on screen and act on it.
//!
//! A background app with a tray icon. Launching it again (from the
//! applications menu, or with one of the options below) talks to the running
//! instance instead of starting a second one.

// No console window on Windows: Lens is a desktop app.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
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
  arcade-lens --install [--silent] [--no-launch] [--keep-download] [--channel C]
                            Install this AppImage / macOS bundle
  arcade-lens --uninstall [--silent] [--remove-data]
                            Remove this installation via its receipt
  arcade-lens --repair      Rewrite the launcher, command and login entries
  arcade-lens --integration-status
                            Print the integration as JSON (no side effects)
  arcade-lens --arcade-manifest
                            Print Lens's Arcade Link manifest (no side effects)

Once running, press the activation shortcut (Ctrl+Alt+Shift+L by default)
or use the tray icon.";

fn main() -> ExitCode {
    // glibc gives each allocating thread its own malloc arena. With rayon's
    // workers and a thread per Link connection, freed memory scattered across
    // them and RSS crept by ~25 MiB over a few hundred OCR calls; two arenas
    // keep it flat. Must run before any thread starts.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    arcade_link::childenv::on_startup();
    lens_platform::autostart::init_app_identity();
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
        Some("install-dialog") => return app::installer_ui::serve_dialog(),
        Some("install" | "uninstall" | "repair" | "integration-status") => return integration_command(args),
        _ => {}
    }
    if first_run_install(args)? {
        return Ok(());
    }
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
                "settings" => gui::wayland::Solo::Settings(args.get(2).and_then(|l| gui::settings_view::Section::from_label(l))),
                "history" => gui::wayland::Solo::History,
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
            println!("{}", link::document(&settings, &Paths::discover()).to_json());
            Ok(())
        }
        Some("version" | "V") => {
            println!("Arcade Lens {}", crate::link::VERSION);
            Ok(())
        }
        Some(other) => Err(format!("unknown option {other:?}; see arcade-lens --help")),
    }
}

/// A real AppImage (or a macOS bundle outside Applications) offers to install
/// itself before the first window. `true` when this process is done.
fn first_run_install(args: &[String]) -> Result<bool, String> {
    let paths = Paths::discover();
    let (Ok(environment), Ok(runtime)) = (arcade_link::install::Environment::discover(), arcade_link::install::Runtime::discover()) else {
        return Ok(false);
    };
    let (detection, state) = match app::installer::automatic_prompt(&paths, args, &environment, &runtime) {
        Ok(Some(prompt)) => prompt,
        Ok(None) => return Ok(false),
        Err(e) => {
            eprintln!("arcade-lens: install detection: {e}");
            return Ok(false);
        }
    };
    let options = app::installer::Options::defaults(&environment);
    match app::installer_ui::run_install(environment, runtime, paths, detection, state, options)? {
        app::installer_ui::Outcome::Launch(path) => {
            arcade_link::install::reexec(&path, &args.iter().map(std::ffi::OsString::from).collect::<Vec<_>>()).map_err(|e| format!("relaunch: {e}"))?;
            Ok(true)
        }
        app::installer_ui::Outcome::Exit => Ok(true),
        app::installer_ui::Outcome::Continue => Ok(false),
    }
}

/// `--install`, `--uninstall`, `--repair` and `--integration-status` (SPEC §8.1).
fn integration_command(args: &[String]) -> Result<(), String> {
    use app::installer;
    let action = args[0].trim_start_matches('-');
    let mut silent = false;
    let mut no_launch = false;
    let mut keep_download = false;
    let mut remove_data = false;
    let mut channel = None;
    let mut it = args[1..].iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--silent" => silent = true,
            "--no-launch" => no_launch = true,
            "--keep-download" => keep_download = true,
            "--remove-data" => remove_data = true,
            "--channel" => channel = Some(it.next().filter(|c| !c.is_empty() && !c.starts_with('-')).ok_or("--channel needs a channel name")?.clone()),
            other => return Err(format!("unexpected argument {other:?} for --{action}")),
        }
    }
    if (no_launch || keep_download || channel.is_some()) && action != "install" {
        return Err("--no-launch, --keep-download and --channel require --install".into());
    }
    if remove_data && action != "uninstall" {
        return Err("--remove-data requires --uninstall".into());
    }
    if silent && !matches!(action, "install" | "uninstall") {
        return Err("--silent requires --install or --uninstall".into());
    }
    let paths = Paths::discover();
    let environment = arcade_link::install::Environment::discover().map_err(|e| e.to_string())?;
    let runtime = arcade_link::install::Runtime::discover().map_err(|e| e.to_string())?;
    let json = |v: &dyn erased::Json| println!("{}", v.pretty());
    match action {
        "integration-status" => json(&installer::integration_status(&environment).map_err(|e| e.to_string())?),
        "repair" => json(&installer::repair(&environment, &runtime).map_err(|e| e.to_string())?),
        "uninstall" if silent => {
            let removed = installer::uninstall(&environment, &paths, remove_data).map_err(|e| e.to_string())?;
            println!("{}", serde_json::json!({ "uninstalled": removed }));
        }
        "uninstall" => {
            app::installer_ui::run_uninstall(environment, paths, remove_data)?;
        }
        _ => {
            let mut options = installer::Options::defaults(&environment);
            if let Some(channel) = channel {
                options.channel = channel;
            }
            if keep_download {
                options.remove_download = false;
            }
            let launch = if silent {
                let receipt = installer::install_copy(&environment, &runtime, &paths, &options).map_err(|e| e.to_string())?;
                if let Some(hint) = installer::integration_status(&environment).map_err(|e| e.to_string())?.path_hint {
                    eprintln!("arcade-lens: {hint}");
                }
                json(&receipt);
                Some(installer::launch_path(&receipt))
            } else {
                let detection = arcade_link::install::detect(&environment, link::ME, &runtime).map_err(|e| e.to_string())?;
                if detection.state == arcade_link::install::State::DevBuild {
                    return Err("Development builds cannot be installed".into());
                }
                let state = installer::dialog_state(&detection, crate::link::VERSION).unwrap_or(installer::DialogState::Install);
                match app::installer_ui::run_install(environment, runtime, paths.clone(), detection, state, options)? {
                    app::installer_ui::Outcome::Launch(path) => Some(path),
                    _ => None,
                }
            };
            if let Some(path) = launch.filter(|_| !no_launch) {
                // The installed copy replaces an older one still running.
                installer::stop_running_copy(&paths);
                arcade_link::install::reexec(&path, &installer::launch_args(args)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

mod erased {
    /// Pretty JSON for the integration commands' different result types.
    pub trait Json {
        fn pretty(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn pretty(&self) -> String {
            serde_json::to_string_pretty(self).unwrap_or_default()
        }
    }
}
