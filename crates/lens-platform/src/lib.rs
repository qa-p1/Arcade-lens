//! Platform adapters for Arcade Lens.
//!
//! Shared logic lives in `lens-core`; this crate implements only what the
//! operating system must provide, with a native implementation per platform:
//!
//! | | Linux X11 | Linux Wayland | Windows | macOS |
//! |---|---|---|---|---|
//! | monitors / capture | RandR + GetImage | screenshot portal | xcap (WGC/GDI) | xcap (ScreenCaptureKit) |
//! | windows | EWMH | — (not exposed) | xcap + Win32 | xcap |
//! | window control | EWMH messages | — | Win32 | — (needs Accessibility) |
//! | OCR | ocrs | ocrs | Windows.Media.Ocr | Vision |

use std::sync::Arc;

use image::RgbaImage;
use lens_core::geometry::{MonitorInfo, Point, Rect};
use lens_core::host::{HostFeatures, WindowCommand};
use lens_core::selection::WindowInfo;
use lens_recognizers::ocr::OcrEngine;

pub mod autostart;
pub mod ipc;
pub mod shortcut;

#[cfg(target_os = "linux")]
mod linux_wayland;
#[cfg(target_os = "linux")]
mod linux_x11;
#[cfg(target_os = "macos")]
mod macos_native;
#[cfg(target_os = "macos")]
mod ocr_macos;
#[cfg(windows)]
mod ocr_windows;
#[cfg(windows)]
mod windows_native;
#[cfg(any(windows, target_os = "macos"))]
mod xcap_backend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError(pub String);

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlatformError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Windows,
    MacOS,
    Unknown,
}

pub fn display_server() -> DisplayServer {
    #[cfg(target_os = "linux")]
    {
        // XWayland exposes DISPLAY too; prefer native X11 capture when it works,
        // because it is instant and needs no portal permission.
        if linux_x11::available() && !(linux_wayland::active() && std::env::var_os("LENS_FORCE_PORTAL").is_some()) {
            if linux_wayland::active() {
                return DisplayServer::Wayland;
            }
            return DisplayServer::X11;
        }
        if linux_wayland::active() {
            return DisplayServer::Wayland;
        }
        return DisplayServer::Unknown;
    }
    #[cfg(windows)]
    return DisplayServer::Windows;
    #[cfg(target_os = "macos")]
    return DisplayServer::MacOS;
    #[allow(unreachable_code)]
    DisplayServer::Unknown
}

/// One monitor's frozen pixels, in physical resolution.
pub struct Capture {
    pub monitor: MonitorInfo,
    pub image: Arc<RgbaImage>,
}

pub fn monitors() -> Result<Vec<MonitorInfo>, PlatformError> {
    #[cfg(target_os = "linux")]
    return match display_server() {
        DisplayServer::X11 => linux_x11::monitors(),
        _ => Err(PlatformError("monitor geometry is not exposed on Wayland".into())),
    };
    #[cfg(any(windows, target_os = "macos"))]
    return xcap_backend::monitors();
    #[allow(unreachable_code)]
    Err(PlatformError("unsupported platform".into()))
}

/// Captures every monitor. On X11 this is a direct framebuffer read; on
/// Wayland it goes through the screenshot portal (which may ask the user).
pub fn capture_all() -> Result<Vec<Capture>, PlatformError> {
    #[cfg(target_os = "linux")]
    let raw = match display_server() {
        DisplayServer::X11 => linux_x11::capture_all(),
        DisplayServer::Wayland => {
            // Under XWayland, X11 capture only sees X clients; use the portal.
            if std::env::var_os("LENS_X11_CAPTURE").is_some() {
                linux_x11::capture_all()
            } else {
                linux_wayland::capture_all()
            }
        }
        _ => Err(PlatformError("no display server".into())),
    };
    #[cfg(any(windows, target_os = "macos"))]
    let raw = xcap_backend::capture_all();
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    let raw: Result<Vec<(MonitorInfo, RgbaImage)>, PlatformError> = Err(PlatformError("unsupported platform".into()));
    Ok(raw?.into_iter().map(|(monitor, image)| Capture { monitor, image: Arc::new(image) }).collect())
}

/// Re-captures a screen rectangle (used by live pins).
pub fn capture_rect(r: Rect) -> Result<RgbaImage, PlatformError> {
    #[cfg(target_os = "linux")]
    return match display_server() {
        DisplayServer::X11 => linux_x11::capture_rect(r),
        _ => Err(PlatformError("live capture is not available on Wayland".into())),
    };
    #[cfg(any(windows, target_os = "macos"))]
    return xcap_backend::capture_rect(r);
    #[allow(unreachable_code)]
    {
        let _ = r;
        Err(PlatformError("unsupported platform".into()))
    }
}

pub fn live_capture_supported() -> bool {
    matches!(display_server(), DisplayServer::X11 | DisplayServer::Windows | DisplayServer::MacOS)
}

/// Pointer position in physical virtual-desktop pixels.
pub fn cursor_position() -> Option<Point> {
    #[cfg(target_os = "linux")]
    return if display_server() == DisplayServer::X11 { linux_x11::cursor() } else { None };
    #[cfg(windows)]
    return windows_native::cursor();
    #[cfg(target_os = "macos")]
    return macos_native::cursor();
    #[allow(unreachable_code)]
    None
}

/// Visible top-level windows, front-most first, excluding `exclude_pid`.
pub fn windows(exclude_pid: Option<u32>) -> Vec<WindowInfo> {
    #[cfg(target_os = "linux")]
    return if display_server() == DisplayServer::X11 { linux_x11::windows(exclude_pid) } else { vec![] };
    #[cfg(any(windows, target_os = "macos"))]
    return xcap_backend::windows(exclude_pid);
    #[allow(unreachable_code)]
    {
        let _ = exclude_pid;
        vec![]
    }
}

/// Number of virtual desktops, when the platform exposes it.
pub fn workspace_count() -> u32 {
    #[cfg(target_os = "linux")]
    return if display_server() == DisplayServer::X11 { linux_x11::workspaces() } else { 0 };
    #[allow(unreachable_code)]
    0
}

/// Window operations this platform can perform.
pub fn window_features() -> HostFeatures {
    match display_server() {
        DisplayServer::X11 | DisplayServer::Windows => HostFeatures::WINDOW_TOPMOST | HostFeatures::WINDOW_MOVE | HostFeatures::WINDOW_CLOSE,
        _ => HostFeatures::empty(),
    }
}

pub fn window_command(w: &WindowInfo, cmd: &WindowCommand) -> Result<(), PlatformError> {
    #[cfg(target_os = "linux")]
    return linux_x11::window_command(w, cmd);
    #[cfg(windows)]
    return windows_native::window_command(w, cmd, &monitors().unwrap_or_default());
    #[allow(unreachable_code)]
    {
        let _ = (w, cmd);
        Err(PlatformError("window control requires Accessibility permission on this platform".into()))
    }
}

/// Gives keyboard focus to one of our own native windows.
pub fn focus_native(raw: raw_window_handle_shim::Raw) {
    match raw {
        #[cfg(target_os = "linux")]
        raw_window_handle_shim::Raw::X11(w) => linux_x11::focus(w),
        #[cfg(windows)]
        raw_window_handle_shim::Raw::Win32(h) => windows_native::focus(h),
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

/// Minimal handle type so callers don't need to share our raw-window-handle version.
pub mod raw_window_handle_shim {
    #[derive(Debug, Clone, Copy)]
    pub enum Raw {
        X11(u32),
        Win32(isize),
        Other,
    }
}

/// The best local OCR engine built into the OS, if any.
pub fn native_ocr() -> Option<Arc<dyn OcrEngine>> {
    #[cfg(windows)]
    if ocr_windows::WindowsOcr::available() {
        return Some(Arc::new(ocr_windows::WindowsOcr));
    }
    #[cfg(target_os = "macos")]
    return Some(Arc::new(ocr_macos::VisionOcr));
    #[allow(unreachable_code)]
    None
}
