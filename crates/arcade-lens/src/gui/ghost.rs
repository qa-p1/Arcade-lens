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
    crate::host::which(cmd).is_some()
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
        // x11grab records X11 (and Windows' gdigrab); a Wayland session has no
        // window list or grab for it, so the action is not offered there (LN-6).
        if ffmpeg_available() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
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
                let path = write_share(&std::env::temp_dir(), img, std::time::SystemTime::now()).map_err(|e| LensError::Failed(e.to_string()))?;
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

/// Shares older than this are deleted when a new one is written. KDE Connect
/// reads the file after the command returns, so it cannot be deleted at once.
const SHARE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

fn share_file_name(pid: u32, nanos: u128, seq: u64) -> String {
    let _ = (nanos, seq);
    format!("arcade-lens-share-{pid}.png")
}

/// Writes `img` for KDE Connect into `dir` and returns its path.
fn write_share(dir: &Path, img: &RgbaImage, now: std::time::SystemTime) -> std::io::Result<PathBuf> {
    let _ = now;
    let path = dir.join(share_file_name(std::process::id(), 0, 0));
    img.save(&path).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, SystemTime};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lens-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn img() -> RgbaImage {
        RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
    }

    #[test]
    fn share_names_are_unique_per_share_and_per_process() {
        let a = share_file_name(7, 100, 0);
        assert_ne!(a, share_file_name(7, 100, 1), "same instant, next share");
        assert_ne!(a, share_file_name(7, 101, 0));
        assert_ne!(a, share_file_name(8, 100, 0));
        assert!(a.starts_with("arcade-lens-share-") && a.ends_with(".png"), "{a}");
    }

    #[test]
    fn a_share_is_written_private_and_two_shares_do_not_collide() {
        let d = dir("mode");
        let now = SystemTime::now();
        let one = write_share(&d, &img(), now).unwrap();
        let two = write_share(&d, &img(), now).unwrap();
        assert_ne!(one, two);
        for p in [&one, &two] {
            assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600, "{}", p.display());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writing_a_share_deletes_old_shares_only() {
        let d = dir("sweep");
        let now = SystemTime::now();
        let touch = |name: &str, age: Duration| {
            let p = d.join(name);
            std::fs::write(&p, b"x").unwrap();
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(now - age).unwrap();
            p
        };
        let old = touch("arcade-lens-share-1-1-0.png", SHARE_MAX_AGE + Duration::from_secs(60));
        let recent = touch("arcade-lens-share-1-2-0.png", SHARE_MAX_AGE - Duration::from_secs(60));
        let other = touch("not-a-share.png", SHARE_MAX_AGE * 5);
        let new = write_share(&d, &img(), now).unwrap();
        assert!(!old.exists(), "a share older than an hour is deleted");
        assert!(recent.exists(), "KDE Connect may still be reading a recent one");
        assert!(other.exists(), "only Lens's own shares are touched");
        assert!(new.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
