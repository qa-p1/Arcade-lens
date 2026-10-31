//! Lens's small native install/confirmation window. The model is pure;
//! install, dismissal and Tools handoff work runs on a worker. It wakes egui
//! when finished, with no polling and no installer work in update().
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use super::installer::{self, DialogState, Options};
use crate::config::Paths as AppPaths;
use arcade_link::design::egui::components::{ButtonKind, Toast, Tone};
use arcade_link::design::size;
use arcade_link::install::{self, strings, Detection, Environment, Runtime};
use arcade_link::receipt::ManagedBy;
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Outcome {
    Continue,
    Launch(PathBuf),
    Exit,
}

// Keep the dialog in a short-lived helper process, as Lens does for windows.
// winit permits one event loop per process; "run once" must still be able to
// create the resident instance after this window closes. Requests travel over
// a private pipe, never through generated startup/environment files.
#[derive(Serialize, Deserialize)]
struct PromptEnvironment {
    home: PathBuf,
    data: PathBuf,
    config: PathBuf,
    applications: PathBuf,
    bin: PathBuf,
    local_app_data: PathBuf,
    registry: PathBuf,
    runtime: PathBuf,
    handoff: PathBuf,
    path: Option<String>,
    windows_environment_key: String,
}
impl From<Environment> for PromptEnvironment {
    fn from(e: Environment) -> Self {
        Self {
            home: e.home,
            data: e.data,
            config: e.config,
            applications: e.applications,
            bin: e.bin,
            local_app_data: e.local_app_data,
            registry: e.locations.registry,
            runtime: e.locations.runtime,
            handoff: e.locations.handoff,
            path: e.path.map(|s| s.to_string_lossy().into()),
            windows_environment_key: e.windows_environment_key,
        }
    }
}
impl From<PromptEnvironment> for Environment {
    fn from(e: PromptEnvironment) -> Self {
        Self {
            home: e.home,
            data: e.data,
            config: e.config,
            applications: e.applications,
            bin: e.bin,
            local_app_data: e.local_app_data,
            locations: arcade_link::Locations { registry: e.registry, runtime: e.runtime, handoff: e.handoff },
            path: e.path.map(Into::into),
            windows_environment_key: e.windows_environment_key,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct PromptRuntime {
    executable: PathBuf,
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
    dev_build: bool,
}
impl From<Runtime> for PromptRuntime {
    fn from(r: Runtime) -> Self {
        Self { executable: r.executable, appimage: r.appimage, appdir: r.appdir, dev_build: r.dev_build }
    }
}
impl From<PromptRuntime> for Runtime {
    fn from(r: PromptRuntime) -> Self {
        Self { executable: r.executable, appimage: r.appimage, appdir: r.appdir, dev_build: r.dev_build }
    }
}
#[derive(Serialize, Deserialize)]
enum Prompt {
    Install { environment: Box<PromptEnvironment>, runtime: PromptRuntime, paths: AppPaths, detection: Box<Detection>, state: DialogState, options: Options },
    Uninstall { environment: Box<PromptEnvironment>, paths: AppPaths, remove_data: bool },
}

fn helper(prompt: Prompt) -> Result<Outcome, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--install-dialog")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&prompt).map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("dialog pipe is unavailable")?.write_all(&bytes).map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("The install dialog could not open".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid dialog reply: {e}"))
}

pub fn serve_dialog() -> Result<(), String> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    std::io::stdin().take(4 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let prompt: Prompt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let outcome = match prompt {
        Prompt::Install { environment, runtime, paths, detection, state, options } => {
            let environment: Environment = (*environment).into();
            let managed = detection.receipt.as_ref().is_some_and(|r| r.managed_by == ManagedBy::Tools) && installer::tools_installed(&environment);
            run(paths, Mode::Install { environment: Box::new(environment), runtime: runtime.into(), detection, state, options, managed })?
        }
        Prompt::Uninstall { environment, paths, remove_data } => run(paths, Mode::Uninstall { environment: Box::new((*environment).into()), remove_data })?,
    };
    let bytes = serde_json::to_vec(&outcome).map_err(|e| e.to_string())?;
    std::io::stdout().write_all(&bytes).map_err(|e| e.to_string())
}

#[derive(Clone)]
enum Mode {
    Install { environment: Box<Environment>, runtime: Runtime, detection: Box<Detection>, state: DialogState, options: Options, managed: bool },
    Uninstall { environment: Box<Environment>, remove_data: bool },
}

struct Completed {
    outcome: Outcome,
    hint: Option<String>,
}
type WorkerResult = Result<Completed, String>;

struct Dialog {
    paths: AppPaths,
    mode: Mode,
    result: Arc<Mutex<Option<Outcome>>>,
    tx: mpsc::Sender<WorkerResult>,
    rx: mpsc::Receiver<WorkerResult>,
    busy: bool,
    error: Option<String>,
    completed: Option<Completed>,
    /// Older pre-installer copies this install replaces (named in the prompt).
    legacy: Vec<PathBuf>,
    toast: Option<Toast>,
    /// The Appearance setting last applied, and when it was last read.
    appearance: Option<(lens_core::settings::Appearance, std::time::Instant)>,
}

fn format(s: &str, version: &str, from: &str) -> String {
    strings::format(s, crate::link::NAME, version, from, crate::link::VERSION)
}

/// Verbatim contract strings with only shared placeholders substituted.
pub fn text(state: &DialogState) -> (String, Option<String>, Vec<String>) {
    let all = strings::get();
    let newer = |installed: &str| {
        semver::Version::parse(installed).ok().zip(semver::Version::parse(crate::link::VERSION).ok()).is_some_and(|(installed, this)| installed > this)
    };
    let (dialog, version, from) = match state {
        DialogState::Install => (&all.not_installed, crate::link::VERSION, ""),
        DialogState::Update { from } => (&all.older, crate::link::VERSION, from.as_str()),
        // Say so when this file is the older one, not just "already installed".
        DialogState::OpenInstalled { version } if newer(version) => {
            (all.newer_installed.as_ref().unwrap_or(&all.same_or_newer), version.as_str(), crate::link::VERSION)
        }
        DialogState::OpenInstalled { version } => (&all.same_or_newer, version.as_str(), ""),
        DialogState::MoveToApplications => (&all.macos, crate::link::VERSION, ""),
    };
    (format(&dialog.title, version, from), dialog.body.as_ref().map(|s| format(s, version, from)), dialog.buttons.clone())
}

pub fn run_install(
    environment: Environment,
    runtime: Runtime,
    paths: AppPaths,
    detection: Detection,
    state: DialogState,
    options: Options,
) -> Result<Outcome, String> {
    helper(Prompt::Install { environment: Box::new(environment.into()), runtime: runtime.into(), paths, detection: Box::new(detection), state, options })
}

pub fn run_uninstall(environment: Environment, paths: AppPaths, remove_data: bool) -> Result<Outcome, String> {
    helper(Prompt::Uninstall { environment: Box::new(environment.into()), paths, remove_data })
}

fn run(paths: AppPaths, mode: Mode) -> Result<Outcome, String> {
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    #[cfg(target_os = "linux")]
    let legacy = match &mode {
        Mode::Install { environment, .. } => install::legacy_copies(environment, &installer::app_info()),
        _ => Vec::new(),
    };
    #[cfg(not(target_os = "linux"))]
    let legacy = Vec::new();
    let title = if matches!(mode, Mode::Uninstall { .. }) { "Uninstall — Arcade Lens" } else { "Install — Arcade Lens" };
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id(crate::link::OS_ID)
            .with_inner_size([size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT])
            .with_min_inner_size([size::WINDOW_MIN_WIDTH, size::WINDOW_MIN_HEIGHT])
            .with_icon(crate::icon::window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |_cc| {
            let (tx, rx) = mpsc::channel();
            Ok(Box::new(Dialog { paths, mode, result, tx, rx, busy: false, error: None, completed: None, legacy, toast: None, appearance: None }))
        }),
    )
    .map_err(|e| e.to_string())?;
    let outcome = output.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or(Outcome::Continue);
    Ok(outcome)
}

impl Dialog {
    fn finish(&self, ctx: &egui::Context, outcome: Outcome) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn start(&mut self, ctx: &egui::Context, action: usize) {
        self.busy = true;
        self.error = None;
        let mode = self.mode.clone();
        let paths = self.paths.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| match mode {
                Mode::Install { environment, runtime, detection, state, options, managed } => {
                    if action == 2 {
                        installer::dismiss(&paths, &detection.running_from).map_err(|e| e.to_string())?;
                        return Ok(Completed { outcome: Outcome::Continue, hint: None });
                    }
                    if matches!(state, DialogState::OpenInstalled { .. }) {
                        let r = detection.receipt.ok_or("No installed copy was found")?;
                        return Ok(Completed { outcome: Outcome::Launch(installer::launch_path(&r)), hint: None });
                    }
                    if managed {
                        installer::handoff_to_tools(&environment).map_err(|e| e.to_string())?;
                        return Ok(Completed { outcome: Outcome::Exit, hint: None });
                    }
                    let r = installer::install_copy(&environment, &runtime, &paths, &options).map_err(|e| e.to_string())?;
                    // The installed copy replaces an older one still running.
                    installer::stop_running_copy(&paths);
                    let status =
                        install::integration_status(&environment, crate::link::ME, &std::env::var("SHELL").unwrap_or_default()).map_err(|e| e.to_string())?;
                    let hint =
                        status.path_hint.map(|command| strings::format(&strings::get().path_hint, crate::link::NAME, crate::link::VERSION, "", &command));
                    Ok(Completed { outcome: Outcome::Launch(installer::launch_path(&r)), hint })
                }
                Mode::Uninstall { environment, remove_data } => {
                    installer::uninstall(&environment, &paths, remove_data).map_err(|e| e.to_string())?;
                    Ok(Completed { outcome: Outcome::Exit, hint: None })
                }
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for Dialog {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(result) = self.rx.try_recv() {
            self.busy = false;
            match result {
                Ok(done) if done.hint.is_none() => self.finish(ctx, done.outcome),
                Ok(done) => {
                    *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(done.outcome.clone());
                    self.completed = Some(done);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.busy {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.completed.is_none() {
                // Closing the question answers nothing: don't install, don't start.
                let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
                result.get_or_insert(Outcome::Exit);
            }
        }
        if !self.busy && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.finish(ctx, Outcome::Exit);
        }
        // The Appearance setting, re-read once a second so a change made in
        // Settings shows here too; System follows the desktop by itself.
        if self.appearance.is_none_or(|(_, read)| read.elapsed() >= std::time::Duration::from_secs(1)) {
            let appearance = crate::config::load_settings(&self.paths).unwrap_or_default().appearance;
            if self.appearance.is_none_or(|(applied, _)| applied != appearance) {
                crate::gui::theme::apply_style(ctx, appearance.as_str());
            }
            self.appearance = Some((appearance, std::time::Instant::now()));
        }
        crate::gui::theme::sync(ctx);
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let kit = crate::gui::theme::kit();
        let glyph = crate::gui::glyphs::texture(ctx, crate::link::ME, 28.0);
        kit.page(ctx, |ui| {
            kit.glyph_tile(ui, glyph, "search");
            ui.add_space(16.0);
            if let Some(done) = &self.completed {
                kit.dialog_head(ui, "Arcade Lens is installed", done.hint.as_deref().unwrap_or_default());
                kit.dialog_actions(ui, |ui| {
                    if kit.button(ui, "Open Arcade Lens", ButtonKind::Primary).clicked() {
                        self.finish(ctx, done.outcome.clone());
                    }
                });
                return;
            }
            let mut clicked = None;
            ui.add_enabled_ui(!self.busy, |ui| match &mut self.mode {
                Mode::Install { state, options, environment, managed, .. } => {
                    let (title, body, buttons) = text(state);
                    kit.dialog_head(ui, &title, body.as_deref().unwrap_or_default());
                    if *managed {
                        kit.notice(ui, &format(&strings::get().managed_by_tools, crate::link::VERSION, ""), Tone::Neutral);
                    }
                    if let (Some(note), Some(old), DialogState::Install | DialogState::Update { .. }) =
                        (&strings::get().replaces_legacy, self.legacy.first(), &state)
                    {
                        kit.notice(ui, &format(note, crate::link::VERSION, &old.display().to_string()), Tone::Neutral);
                    }
                    ui.add_space(10.0);
                    if !*managed && matches!(state, DialogState::Install | DialogState::Update { .. } | DialogState::MoveToApplications) {
                        kit.checkbox(ui, &mut options.start_at_login, "Start at login");
                        #[cfg(target_os = "linux")]
                        kit.checkbox(ui, &mut options.remove_download, "Remove the downloaded file");
                        #[cfg(target_os = "macos")]
                        {
                            kit.checkbox(ui, &mut options.install_cli, "Add the arcade-lens terminal command");
                            if !options.install_cli {
                                let exe = environment.home.join("Applications/Arcade Lens.app/Contents/MacOS/arcade-lens");
                                kit.notice(ui, &install::platform::macos::cli_symlink_offer(environment, installer::CLI_NAME, &exe), Tone::Neutral);
                            }
                        }
                    }
                    let _ = environment;
                    // Tools' order: the main action rightmost and primary.
                    kit.dialog_actions(ui, |ui| {
                        for (index, label) in buttons.iter().enumerate().rev() {
                            let kind = match index {
                                0 => ButtonKind::Primary,
                                1 => ButtonKind::Default,
                                _ => ButtonKind::Ghost,
                            };
                            let label = if index == 0 && *managed { "Open Tools" } else { label };
                            if kit.button_state(ui, label, kind, true, self.busy && index == 0, false).clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                }
                Mode::Uninstall { remove_data, .. } => {
                    kit.dialog_head(ui, "Uninstall Arcade Lens?", "Remove this installation, launcher, command and start-at-login entry.");
                    ui.add_space(10.0);
                    kit.checkbox(ui, remove_data, "Remove settings and data (history, presets and plugins)");
                    kit.dialog_actions(ui, |ui| {
                        if kit.button(ui, "Cancel", ButtonKind::Default).clicked() {
                            clicked = Some(1);
                        }
                        if kit.button_state(ui, "Uninstall", ButtonKind::Destructive, true, self.busy, false).clicked() {
                            clicked = Some(0);
                        }
                    });
                }
            });
            match clicked {
                Some(1) => self.finish(ctx, Outcome::Continue),
                Some(i) => self.start(ctx, i),
                None => {}
            }
            if self.busy {
                kit.notice(ui, "Working…", Tone::Neutral);
            }
            if let Some(error) = &self.error {
                let message = if matches!(self.mode, Mode::Uninstall { .. }) {
                    "Couldn't uninstall Arcade Lens. Check the app folder's permissions, then try again."
                } else {
                    "Couldn't install Arcade Lens. Check the app folder's permissions, then try again."
                };
                kit.notice_action(ui, message, Tone::Danger, |ui| {
                    if kit.button(ui, "Copy details", ButtonKind::Ghost).clicked() {
                        ctx.copy_text(error.clone());
                        self.toast = Some(Toast::copied());
                    }
                });
            }
        });
        if let Some(toast) = &self.toast {
            if !toast.show(ctx, &kit) {
                self.toast = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_verbatim_shared_strings() {
        let (title, body, buttons) = text(&DialogState::Install);
        assert_eq!(title, "Install Arcade Lens?");
        assert_eq!(body.as_deref(), Some("This adds Arcade Lens to the app launcher and your apps folder."));
        assert_eq!(buttons, ["Install", "Just run it", "Don't ask again for this file"]);
        assert!(text(&DialogState::Update { from: "0.0.1".into() }).0.contains(&format!("0.0.1 → {}", crate::link::VERSION)));
        let (title, body, _) = text(&DialogState::OpenInstalled { version: "99.0.0".into() });
        assert_eq!(title, "A newer Arcade Lens is already installed.");
        assert!(body.unwrap().contains(&format!("this file is the older {}", crate::link::VERSION)));
    }
}
