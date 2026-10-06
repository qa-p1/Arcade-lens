//! The GUI's [`Host`]: the desktop host plus overlay-only services (pins,
//! annotation, measure mode, recording), window control and device sends.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use eframe::egui;
use image::RgbaImage;
use lens_core::geometry::Rect;
use lens_core::host::{Host, HostFeatures, OpenPathMode, SaveRequest, WindowCommand};
use lens_core::selection::WindowInfo;
use lens_core::{LensError, Result};

use crate::host::DesktopHost;

pub enum UiCommand {
    Pin { image: Arc<RgbaImage>, origin: Option<Rect> },
    Annotate { image: Arc<RgbaImage> },
    Measure { rect: Rect },
    Record { window: WindowInfo },
    Toast(String),
    SettingsStatus { message: String, ok: bool },
}

pub struct GuiHost {
    desktop: DesktopHost,
    tx: Sender<UiCommand>,
    ctx: egui::Context,
    features: HostFeatures,
    arcade: lens_actions::arcade::Arcade,
    settings: Arc<lens_core::Settings>,
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file() || d.join(format!("{cmd}.exe")).is_file()))
}

pub fn ffmpeg_available() -> bool {
    which("ffmpeg") && matches!(lens_platform::display_server(), lens_platform::DisplayServer::X11 | lens_platform::DisplayServer::Windows)
}

fn kdeconnect_device() -> Option<String> {
    if !which("kdeconnect-cli") {
        return None;
    }
    let out = Command::new("kdeconnect-cli").args(["-a", "--id-only"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).find(|l| !l.is_empty()).map(String::from)
}

impl GuiHost {
    pub fn new(
        desktop: DesktopHost,
        tx: Sender<UiCommand>,
        ctx: egui::Context,
        arcade: lens_actions::arcade::Arcade,
        settings: Arc<lens_core::Settings>,
    ) -> Self {
        let mut f = desktop.features() | HostFeatures::PIN | HostFeatures::ANNOTATE | HostFeatures::MEASURE | lens_platform::window_features();
        if ffmpeg_available() {
            f |= HostFeatures::RECORD_WINDOW;
        }
        if which("kdeconnect-cli") {
            f |= HostFeatures::SEND_TO_DEVICE;
        }
        if lens_platform::workspace_count() > 1 {
            f |= HostFeatures::WORKSPACES;
        }
        if cfg!(target_os = "linux") && which("gdbus") {
            f |= HostFeatures::QUICK_LOOK;
        }
        Self { desktop, tx, ctx, features: f, arcade, settings }
    }

    fn ui(&self, c: UiCommand) -> Result<()> {
        self.tx.send(c).map_err(|_| LensError::Failed("Lens is shutting down".into()))?;
        self.ctx.request_repaint();
        Ok(())
    }
}

impl Host for GuiHost {
    fn features(&self) -> HostFeatures {
        self.features
    }
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        self.desktop.set_clipboard_text(text)
    }
    fn set_clipboard_image(&self, image: &RgbaImage) -> Result<()> {
        self.desktop.set_clipboard_image(image)
    }
    fn clipboard_text(&self) -> Result<String> {
        self.desktop.clipboard_text()
    }
    fn open_uri(&self, uri: &str, private: bool) -> Result<()> {
        self.desktop.open_uri(uri, private)
    }
    fn open_path(&self, path: &Path, mode: OpenPathMode) -> Result<()> {
        if mode == OpenPathMode::QuickLook {
            if let Some(result) = self.arcade.quick_look(&self.settings, path) {
                if result.is_ok() || !cfg!(target_os = "linux") {
                    return result;
                }
            }
        }
        if mode == OpenPathMode::QuickLook && cfg!(target_os = "linux") {
            return self.quick_look_fallback(path);
        }
        self.desktop.open_path(path, mode)
    }
    fn quick_look_fallback(&self, path: &Path) -> Result<()> {
        if cfg!(target_os = "linux") {
            // GNOME Sushi previewer, the closest Linux equivalent of Quick Look.
            let uri = format!("file://{}", path.display());
            let ok = Command::new("gdbus")
                .args([
                    "call",
                    "--session",
                    "--dest",
                    "org.gnome.NautilusPreviewer",
                    "--object-path",
                    "/org/gnome/NautilusPreviewer",
                    "--method",
                    "org.gnome.NautilusPreviewer.ShowFile",
                    &uri,
                    "0",
                    "false",
                ])
                .status()
                .is_ok_and(|s| s.success());
            return if ok { Ok(()) } else { self.desktop.open_path(path, OpenPathMode::Default) };
        }
        self.desktop.open_path(path, OpenPathMode::QuickLook)
    }
    fn terminal(&self, cwd: Option<&Path>, command: Option<&str>, execute: bool) -> Result<()> {
        self.desktop.terminal(cwd, command, execute)
    }
    fn save_file(&self, request: SaveRequest) -> Result<PathBuf> {
        self.desktop.save_file(request)
    }
    fn download(&self, url: &str) -> Result<PathBuf> {
        self.desktop.download(url)
    }
    fn persist(&self, collection: &str, entry: serde_json::Value) -> Result<()> {
        self.desktop.persist(collection, entry)
    }
    fn print(&self, path: &Path) -> Result<()> {
        self.desktop.print(path)
    }
    fn pin(&self, image: Arc<RgbaImage>, origin: Option<Rect>) -> Result<()> {
        self.ui(UiCommand::Pin { image, origin })
    }
    fn annotate(&self, image: Arc<RgbaImage>) -> Result<()> {
        self.ui(UiCommand::Annotate { image })
    }
    fn measure(&self, rect: Rect) -> Result<()> {
        self.ui(UiCommand::Measure { rect })
    }
    fn window_command(&self, window: &WindowInfo, command: WindowCommand) -> Result<()> {
        if command == WindowCommand::Record {
            return self.ui(UiCommand::Record { window: window.clone() });
        }
        lens_platform::window_command(window, &command).map_err(|e| LensError::Failed(e.0))
    }
    fn send_to_device(&self, text: Option<&str>, image: Option<&RgbaImage>) -> Result<()> {
        if let Some(result) = self.arcade.send_to_devices(&self.settings, text, image) {
            return result;
        }
        let device = kdeconnect_device().ok_or_else(|| LensError::Unsupported("no paired device is reachable (KDE Connect)".into()))?;
        let status = match (text, image) {
            (Some(t), _) if t.starts_with("http") || t.starts_with("tel:") => Command::new("kdeconnect-cli").args(["-d", &device, "--share", t]).status(),
            (Some(t), _) => Command::new("kdeconnect-cli").args(["-d", &device, "--share-text", t]).status(),
            (None, Some(img)) => {
                let path = std::env::temp_dir().join(format!("arcade-lens-share-{}.png", std::process::id()));
                img.save(&path).map_err(|e| LensError::Failed(e.to_string()))?;
                Command::new("kdeconnect-cli").args(["-d", &device, "--share", &path.to_string_lossy()]).status()
            }
            (None, None) => return Err(LensError::InvalidInput("nothing to send".into())),
        };
        status.ok().filter(|s| s.success()).map(|_| ()).ok_or_else(|| LensError::Failed("KDE Connect could not send".into()))
    }
    fn notify(&self, message: &str) {
        let _ = self.ui(UiCommand::Toast(message.to_string()));
    }
}
