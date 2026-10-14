//! Lens's small native install/confirmation window. The model is pure;
//! install, dismissal and Tools handoff work runs on a worker. It wakes egui
//! when finished, with no polling and no installer work in update().
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use super::installer::{self, DialogState, Options};
use crate::config::Paths as AppPaths;
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
}

fn format(s: &str, version: &str, from: &str) -> String {
    strings::format(s, crate::link::NAME, version, from, crate::link::VERSION)
}

/// Verbatim contract strings with only shared placeholders substituted.
pub fn text(state: &DialogState) -> (String, Option<String>, Vec<String>) {
    let all = strings::get();
    let (dialog, version, from) = match state {
        DialogState::Install => (&all.not_installed, crate::link::VERSION, ""),
        DialogState::Update { from } => (&all.older, crate::link::VERSION, from.as_str()),
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Arcade Lens — Installation")
            .with_app_id("arcade-lens-settings")
            .with_inner_size([480.0, 300.0])
            .with_resizable(false)
            .with_icon(crate::icon::window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Arcade Lens — Installation",
        options,
        Box::new(move |_cc| {
            let (tx, rx) = mpsc::channel();
            Ok(Box::new(Dialog { paths, mode, result, tx, rx, busy: false, error: None, completed: None }))
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
        if self.busy && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);
            if let Some(done) = &self.completed {
                ui.heading("Arcade Lens is installed");
                if let Some(hint) = &done.hint {
                    ui.add_space(12.0);
                    ui.label(hint);
                }
                ui.add_space(12.0);
                if ui.button("Open Arcade Lens").clicked() {
                    self.finish(ctx, done.outcome.clone());
                }
                return;
            }
            let mut clicked = None;
            ui.add_enabled_ui(!self.busy, |ui| match &mut self.mode {
                Mode::Install { state, options, environment, managed, .. } => {
                    let (title, body, buttons) = text(state);
                    ui.heading(title);
                    if let Some(body) = body {
                        ui.add_space(10.0);
                        ui.label(body);
                    }
                    if *managed {
                        ui.label(format(&strings::get().managed_by_tools, crate::link::VERSION, ""));
                    }
                    ui.add_space(10.0);
                    if matches!(state, DialogState::Install | DialogState::Update { .. } | DialogState::MoveToApplications) {
                        ui.checkbox(&mut options.start_at_login, &strings::get().not_installed.options[0]);
                        #[cfg(target_os = "linux")]
                        ui.checkbox(&mut options.remove_download, &strings::get().not_installed.options[1]);
                        #[cfg(target_os = "macos")]
                        {
                            ui.checkbox(&mut options.install_cli, "Add the arcade-lens terminal command");
                            if !options.install_cli {
                                let exe = environment.home.join("Applications/Arcade Lens.app/Contents/MacOS/arcade-lens");
                                ui.small(install::platform::macos::cli_symlink_offer(environment, installer::CLI_NAME, &exe));
                            }
                        }
                    }
                    let _ = environment;
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        for (i, label) in buttons.iter().enumerate() {
                            if ui.button(label).clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
                }
                Mode::Uninstall { remove_data, .. } => {
                    ui.heading("Uninstall Arcade Lens?");
                    ui.label("Remove this installation, launcher, command and start-at-login entry.");
                    ui.checkbox(remove_data, "Remove settings and data (history, presets and plugins)");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Uninstall").clicked() {
                            clicked = Some(0);
                        }
                        if ui.button("Cancel").clicked() {
                            clicked = Some(1);
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
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Working…");
                });
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
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
        assert_eq!(buttons, strings::get().not_installed.buttons);
        assert!(text(&DialogState::Update { from: "0.0.1".into() }).0.contains(&format!("0.0.1 → {}", crate::link::VERSION)));
    }
}
