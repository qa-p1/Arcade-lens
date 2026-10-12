//! Lens-specific policy around Arcade Link's receipt and install APIs.
//!
//! Filesystem paths and runtime identity are explicit in these operations;
//! callers and tests can provide an isolated Link environment.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use arcade_link::install::AppInfo;
use arcade_link::install::{self, Detection, Environment, Runtime, State};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use arcade_link::receipt::ManagedBy;
use arcade_link::receipt::{InstallMethod, Receipt};
use serde::{Deserialize, Serialize};

use crate::config::Paths;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub const CLI_NAME: &str = "arcade-lens";
#[cfg(target_os = "linux")]
pub const DESKTOP_ID: &str = "arcade-lens";

#[cfg(target_os = "linux")]
pub fn app_info() -> AppInfo {
    AppInfo {
        id: crate::link::ME.into(),
        name: crate::link::NAME.into(),
        filename: "Arcade-Lens.AppImage".into(),
        desktop_id: DESKTOP_ID.into(),
        cli_name: CLI_NAME.into(),
        background_args: vec!["--background".into()],
        legacy_desktop_ids: vec![DESKTOP_ID.into()],
        autostart_id: DESKTOP_ID.into(),
    }
}

pub fn method() -> InstallMethod {
    match Environment::discover().and_then(|e| Runtime::discover().and_then(|r| install::detect(&e, crate::link::ME, &r))) {
        Ok(d) if d.state == State::DevBuild => InstallMethod::Dev,
        Ok(d) => d
            .receipt
            .as_ref()
            .filter(|_| d.state == State::InstalledHere)
            .map_or(if d.is_app_image { InstallMethod::Appimage } else { InstallMethod::Manual }, |r| r.method),
        Err(_) => InstallMethod::Manual,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialogState {
    Install,
    Update { from: String },
    OpenInstalled { version: String },
    MoveToApplications,
}

pub fn dialog_state(d: &Detection, version: &str) -> Option<DialogState> {
    match &d.state {
        State::DevBuild | State::InstalledHere => None,
        State::NotInstalled => Some(DialogState::Install),
        State::InstalledElsewhere { version: installed } => {
            let older = semver::Version::parse(installed).ok().zip(semver::Version::parse(version).ok()).is_some_and(|(a, b)| a < b);
            if older {
                Some(DialogState::Update { from: installed.clone() })
            } else {
                Some(DialogState::OpenInstalled { version: installed.clone() })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub channel: String,
    pub start_at_login: bool,
    pub remove_download: bool,
    pub install_cli: bool,
}

impl Options {
    pub fn defaults(environment: &Environment) -> Self {
        let receipt = environment.receipts().read(crate::link::ME).ok().flatten();
        let start_at_login = receipt.as_ref().is_none_or(|r| r.integration.autostart.as_ref().is_some_and(|p| p.exists()));
        let install_cli = environment.path.as_ref().is_some_and(|p| std::env::split_paths(p).any(|p| p == environment.bin));
        Self { channel: "stable".into(), start_at_login, remove_download: true, install_cli }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileIdentity {
    path: PathBuf,
    size: u64,
    modified: u128,
}

fn identity(path: &Path) -> io::Result<FileIdentity> {
    let meta = fs::metadata(path)?;
    Ok(FileIdentity { path: path.into(), size: meta.len(), modified: meta.modified()?.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() })
}

pub fn dismissed(paths: &Paths, path: &Path) -> bool {
    identity(path).ok().is_some_and(|id| {
        fs::read(paths.config.join("install-dismissed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<FileIdentity>>(&b).ok())
            .is_some_and(|v| v.contains(&id))
    })
}

pub fn dismiss(paths: &Paths, path: &Path) -> io::Result<()> {
    let id = identity(path)?;
    let file = paths.config.join("install-dismissed.json");
    let mut ids: Vec<FileIdentity> = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    ids.retain(|old| old.path != id.path);
    if ids.len() >= 128 {
        ids.remove(0);
    }
    ids.push(id);
    arcade_link::paths::write_atomic(&file, &serde_json::to_vec(&ids).map_err(io::Error::other)?, true)
}

/// Only AppImages and first-run macOS bundles prompt. The checks before the
/// platform-specific discovery are cheap and keep normal startup untouched.
pub fn automatic_prompt(paths: &Paths, args: &[String], environment: &Environment, runtime: &Runtime) -> io::Result<Option<(Detection, DialogState)>> {
    const SKIP: &[&str] = &[
        "--background",
        "--arcade-invoke",
        "--arcade-manifest",
        "--version",
        "-V",
        "--install",
        "--uninstall",
        "--repair",
        "--integration-status",
        "--settings-window",
        "--window",
        "--quit",
        "--restart",
        "--help",
        "-h",
    ];
    if std::env::var("ARCADE_NO_INSTALL_PROMPT").as_deref() == Ok("1") || args.iter().any(|a| SKIP.contains(&a.split('=').next().unwrap_or(a))) {
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    {
        if !runtime.is_appimage() {
            return Ok(None);
        }
        let detection = install::detect(environment, crate::link::ME, runtime)?;
        if !install::should_prompt(&detection, args, false, dismissed(paths, &detection.running_from)) {
            return Ok(None);
        }
        Ok(dialog_state(&detection, env!("CARGO_PKG_VERSION")).map(|s| (detection, s)))
    }
    #[cfg(target_os = "macos")]
    {
        use arcade_link::install::platform::macos;
        let Some(bundle) = macos::bundle_for(&runtime.executable) else { return Ok(None) };
        let mut detection = install::detect(environment, crate::link::ME, runtime)?;
        if macos::in_applications(environment, &bundle) {
            if detection.receipt.is_none() {
                let receipt = macos_receipt(environment, &bundle, None, None, "stable")?;
                environment.receipts().write(&receipt)?;
            }
            return Ok(None);
        }
        let metadata = fs::metadata(&bundle)?;
        let needs_move = macos::is_translocated(&bundle)
            || bundle.starts_with(environment.home.join("Downloads"))
            || bundle.starts_with("/Volumes")
            || metadata.permissions().readonly();
        if !needs_move || dismissed(paths, &bundle) {
            return Ok(None);
        }
        let Some(state) = dialog_state(&detection, env!("CARGO_PKG_VERSION")) else { return Ok(None) };
        let state = if state == DialogState::Install { DialogState::MoveToApplications } else { state };
        detection.running_from = bundle;
        Ok(Some((detection, state)))
    }
    #[cfg(windows)]
    {
        let _ = (paths, environment, runtime);
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
pub fn install_copy(environment: &Environment, runtime: &Runtime, paths: &Paths, options: &Options) -> io::Result<Receipt> {
    if !runtime.is_appimage() {
        return Err(io::Error::other("--install requires a real AppImage, not an ordinary or development binary"));
    }
    struct Stage(PathBuf);
    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let stage = Stage(paths.data.join("install-icons").join(arcade_link::endpoint::new_token()?));
    arcade_link::paths::ensure_private_dir(&stage.0)?;
    let mut icons = Vec::new();
    for size in [16, 22, 24, 32, 48, 64, 128, 256, 512] {
        let source = stage.0.join(format!("{size}.png"));
        fs::write(&source, crate::icon::png(size))?;
        icons.push(install::Icon { source, size: install::IconSize::Pixels(size as u16) });
    }
    let source = stage.0.join("icon.svg");
    fs::write(&source, icon_svg())?;
    icons.push(install::Icon { source, size: install::IconSize::Scalable });
    let existing = environment.receipts().read(crate::link::ME)?;
    let mut receipt = install::install(
        environment,
        install::InstallOptions {
            app: &app_info(),
            runtime,
            version: env!("CARGO_PKG_VERSION"),
            channel: &options.channel,
            managed_by: existing.as_ref().map_or(ManagedBy::SelfManaged, |r| r.managed_by),
            icons: &icons,
            start_at_login: options.start_at_login,
            remove_download: options.remove_download,
            refresh_caches: true,
        },
    )?;
    if !options.start_at_login {
        if let Some(path) = receipt.integration.autostart.take() {
            remove_file(&path)?;
            environment.receipts().write(&receipt)?;
        }
    }
    Ok(receipt)
}

#[cfg(target_os = "macos")]
pub fn install_copy(environment: &Environment, runtime: &Runtime, _paths: &Paths, options: &Options) -> io::Result<Receipt> {
    use arcade_link::install::platform::macos;
    let detection = install::detect(environment, crate::link::ME, runtime)?;
    if detection.state == State::DevBuild {
        return Err(io::Error::other("development builds cannot be installed"));
    }
    let source = macos::bundle_for(&runtime.executable).ok_or_else(|| io::Error::other("--install requires an app bundle"))?;
    let root = environment.home.join("Applications");
    fs::create_dir_all(&root)?;
    let target = root.join("Arcade Lens.app");
    if fs::canonicalize(&source).ok() != fs::canonicalize(&target).ok() {
        let stage = root.join(format!(".arcade-lens-{}", arcade_link::endpoint::new_token()?));
        let staged = macos::move_to_applications(environment, &source, Some(&stage))?;
        if target.exists() {
            let owned = detection.receipt.as_ref().is_some_and(|r| r.path == target);
            if !owned {
                return Err(io::Error::other("Applications already contains an Arcade Lens bundle without its receipt"));
            }
            fs::remove_dir_all(&target)?;
        }
        fs::rename(staged, &target)?;
    }
    let executable = target.join("Contents/MacOS/arcade-lens");
    let cli = if options.install_cli { Some(macos::install_cli(environment, CLI_NAME, &executable)?) } else { None };
    let autostart = detection.receipt.as_ref().and_then(|r| r.integration.autostart.clone());
    let receipt = macos_receipt(environment, &target, cli, autostart, &options.channel)?;
    environment.receipts().write(&receipt)?;
    Ok(receipt)
}

#[cfg(windows)]
pub fn install_copy(_environment: &Environment, _runtime: &Runtime, _paths: &Paths, _options: &Options) -> io::Result<Receipt> {
    Err(io::Error::other("Use the Arcade Lens per-user installer on Windows"))
}

#[cfg(target_os = "macos")]
fn macos_receipt(environment: &Environment, bundle: &Path, cli: Option<PathBuf>, autostart: Option<PathBuf>, channel: &str) -> io::Result<Receipt> {
    let old = environment.receipts().read(crate::link::ME)?;
    let now = arcade_link::manifest::now_rfc3339();
    Ok(Receipt {
        schema: 1,
        id: crate::link::ME.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        channel: channel.into(),
        method: InstallMethod::MacosBundle,
        managed_by: old.as_ref().map_or(ManagedBy::SelfManaged, |r| r.managed_by),
        path: bundle.into(),
        integration: arcade_link::receipt::Integration { cli, autostart, ..Default::default() },
        previous: None,
        installed_at: old.as_ref().map_or_else(|| now.clone(), |r| r.installed_at.clone()),
        updated_at: now,
    })
}

pub fn launch_path(receipt: &Receipt) -> PathBuf {
    if receipt.method == InstallMethod::MacosBundle {
        receipt.path.join("Contents/MacOS/arcade-lens")
    } else {
        receipt.path.clone()
    }
}

pub fn current_executable(paths: &Paths) -> io::Result<PathBuf> {
    if let Ok(environment) = Environment::discover() {
        if let Some(receipt) = environment.receipts().read(crate::link::ME)? {
            let executable = launch_path(&receipt);
            if executable.is_file() {
                return Ok(executable);
            }
        }
    }
    let _ = paths;
    lens_platform::autostart::launch_path()
}

pub fn repair(environment: &Environment, runtime: &Runtime) -> io::Result<Receipt> {
    let detection = install::detect(environment, crate::link::ME, runtime)?;
    #[cfg(target_os = "linux")]
    {
        let moved = (detection.state != State::DevBuild && detection.is_app_image).then_some(detection.running_from.as_path());
        install::repair(environment, &app_info(), moved)
    }
    #[cfg(target_os = "macos")]
    {
        use arcade_link::install::platform::macos;
        let mut receipt = detection.receipt.ok_or_else(|| io::Error::other("no Arcade Lens receipt"))?;
        if let Some(bundle) = macos::bundle_for(&runtime.executable) {
            if detection.state != State::DevBuild {
                receipt.path = bundle;
            }
        }
        let executable = launch_path(&receipt);
        if !executable.is_file() {
            return Err(io::Error::other("installed Arcade Lens executable is missing"));
        }
        if receipt.integration.cli.is_some() {
            receipt.integration.cli = Some(macos::install_cli(environment, CLI_NAME, &executable)?);
        }
        receipt.updated_at = arcade_link::manifest::now_rfc3339();
        environment.receipts().write(&receipt)?;
        Ok(receipt)
    }
    #[cfg(windows)]
    {
        let _ = detection;
        Err(io::Error::other("Repair the Arcade Lens integration with the Windows installer"))
    }
}

pub fn integration_status(environment: &Environment) -> io::Result<install::IntegrationStatus> {
    install::integration_status(environment, crate::link::ME, &std::env::var("SHELL").unwrap_or_default())
}

pub fn uninstall(environment: &Environment, paths: &Paths, remove_data: bool) -> io::Result<bool> {
    let endpoint = paths.endpoint();
    if lens_platform::ipc::send(&endpoint, "ping").is_ok() {
        let _ = lens_platform::ipc::send(&endpoint, "quit");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while lens_platform::ipc::send(&endpoint, "ping").is_ok() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    let data_folders = [paths.config.clone(), paths.data.clone()];
    install::uninstall(environment, crate::link::ME, install::UninstallOptions { remove_data, data_folders: &data_folders })
}

pub fn tools_installed(environment: &Environment) -> bool {
    arcade_link::Registry::load(&environment.locations).get(arcade_link::manifest::ids::TOOLS).is_some_and(|m| m.executable_exists() && m.settings.link_enabled)
}

pub fn handoff_to_tools(environment: &Environment) -> Result<(), arcade_link::LinkError> {
    let registry = arcade_link::Registry::load(&environment.locations);
    let tools = registry.get(arcade_link::manifest::ids::TOOLS).ok_or_else(|| arcade_link::LinkError::unavailable("Arcade Tools isn't installed"))?;
    let action = tools
        .action("tools.install")
        .filter(|a| a.available)
        .ok_or_else(|| arcade_link::LinkError::unavailable("This version of Arcade Tools cannot install Lens"))?;
    let request = arcade_link::InvokeRequest::new(action.id.split('#').next().unwrap_or(&action.id), crate::link::ME)
        .input(arcade_link::Content::text("text/plain", crate::link::ME))
        .options(serde_json::json!({ "app": crate::link::ME }));
    let me = arcade_link::PeerInfo { id: crate::link::ME.into(), version: env!("CARGO_PKG_VERSION").into() };
    arcade_link::client::invoke_action(&environment.locations, &me, tools, &request, Default::default()).map(|_| ())
}

pub fn launch_args(args: &[String]) -> Vec<std::ffi::OsString> {
    let mut result = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--channel" => {
                it.next();
            }
            "--install" | "--silent" | "--no-launch" => {}
            _ => result.push(arg.into()),
        }
    }
    result
}

#[cfg(target_os = "linux")]
fn icon_svg() -> &'static str {
    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 256 256\"><defs><linearGradient id=\"g\" x2=\"1\" y2=\"1\"><stop stop-color=\"#a78bfa\"/><stop offset=\"1\" stop-color=\"#6d28d9\"/></linearGradient></defs><rect width=\"256\" height=\"256\" rx=\"54\" fill=\"url(#g)\"/><g fill=\"none\" stroke=\"white\" stroke-linecap=\"round\" stroke-width=\"17\"><path d=\"M60 99V60h39m58 0h39v39m0 58v39h-39m-58 0H60v-39\"/><circle cx=\"116\" cy=\"116\" r=\"35\"/><path d=\"m142 142 44 44\"/></g></svg>"
}

#[cfg(target_os = "linux")]
fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection(state: State) -> Detection {
        Detection { state, running_from: PathBuf::from("/test/Arcade-Lens.AppImage"), is_app_image: true, receipt: None, installed_path: None }
    }

    #[test]
    fn install_dialog_state_covers_new_update_and_development_copies() {
        assert_eq!(dialog_state(&detection(State::NotInstalled), "0.1.1"), Some(DialogState::Install));
        assert_eq!(
            dialog_state(&detection(State::InstalledElsewhere { version: "0.1.0".into() }), "0.1.1"),
            Some(DialogState::Update { from: "0.1.0".into() })
        );
        assert_eq!(
            dialog_state(&detection(State::InstalledElsewhere { version: "0.1.1".into() }), "0.1.1"),
            Some(DialogState::OpenInstalled { version: "0.1.1".into() })
        );
        assert_eq!(dialog_state(&detection(State::InstalledHere), "0.1.1"), None);
        assert_eq!(dialog_state(&detection(State::DevBuild), "0.1.1"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_install_uninstall_and_repair_use_explicit_temp_environment() {
        let root = std::env::temp_dir().join(format!("lens-install-{}", arcade_link::endpoint::new_token().unwrap()));
        let env = Environment::under(&root.join("home"));
        let paths = Paths { config: root.join("app/config"), data: root.join("app/data") };
        let appdir = root.join("mount");
        let exe = appdir.join("usr/bin/arcade-lens");
        let image = root.join("Arcade-Lens.AppImage");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"test binary").unwrap();
        fs::write(&image, b"test appimage").unwrap();
        let runtime = Runtime { executable: exe, appimage: Some(image), appdir: Some(appdir), dev_build: false };
        let opts = Options { channel: "stable".into(), start_at_login: true, remove_download: false, install_cli: false };
        let receipt = install_copy(&env, &runtime, &paths, &opts).unwrap();
        assert!(receipt.path.is_file());
        assert!(receipt.integration.desktop_entry.as_ref().unwrap().is_file());
        assert!(receipt.integration.cli.as_ref().unwrap().exists());
        assert!(receipt.integration.autostart.as_ref().unwrap().is_file());
        assert!(receipt.integration.icons.iter().any(|p| p.ends_with("scalable/apps/arcade-lens.svg")));
        let repaired = install::repair(&env, &app_info(), None).unwrap();
        assert_eq!(repaired.path, receipt.path);
        assert!(uninstall(&env, &paths, false).unwrap());
        assert!(env.receipts().read(crate::link::ME).unwrap().is_none());
        assert!(!receipt.path.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
