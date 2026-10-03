//! Win32 window control, cursor and focus.

use lens_core::geometry::{MonitorInfo, Point};
use lens_core::host::WindowCommand;
use lens_core::selection::WindowInfo;
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, PostMessageW, SetForegroundWindow, SetWindowPos, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, WM_CLOSE, WS_EX_TOPMOST,
};

use crate::PlatformError;

pub fn cursor() -> Option<Point> {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes into the provided POINT.
    (unsafe { GetCursorPos(&mut p) } != 0).then_some(Point { x: p.x, y: p.y })
}

fn hwnd(w: &WindowInfo) -> Result<HWND, PlatformError> {
    w.id.strip_prefix("native:").and_then(|n| n.parse::<isize>().ok()).map(|n| n as HWND).ok_or_else(|| PlatformError(format!("not a native window: {}", w.id)))
}

pub fn window_command(w: &WindowInfo, cmd: &WindowCommand, monitors: &[MonitorInfo]) -> Result<(), PlatformError> {
    let h = hwnd(w)?;
    // SAFETY: plain Win32 calls on a window handle we enumerated; failures are reported, not UB.
    let ok = unsafe {
        match cmd {
            WindowCommand::ToggleAlwaysOnTop => {
                let topmost = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
                SetWindowPos(h, if topmost { HWND_NOTOPMOST } else { HWND_TOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) != 0
            }
            WindowCommand::Close => PostMessageW(h, WM_CLOSE, 0, 0) != 0,
            WindowCommand::MoveToNextMonitor => {
                if monitors.len() < 2 {
                    return Err(PlatformError("only one monitor".into()));
                }
                let c = Point { x: w.rect.x + w.rect.width as i32 / 2, y: w.rect.y + w.rect.height as i32 / 2 };
                let cur = monitors.iter().position(|m| m.rect.contains(c)).unwrap_or(0);
                let (from, to) = (&monitors[cur].rect, &monitors[(cur + 1) % monitors.len()].rect);
                let nx = to.x + (w.rect.x - from.x).clamp(0, (to.width as i32 - w.rect.width as i32).max(0));
                let ny = to.y + (w.rect.y - from.y).clamp(0, (to.height as i32 - w.rect.height as i32).max(0));
                SetWindowPos(h, std::ptr::null_mut(), nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) != 0
            }
            WindowCommand::MoveToWorkspace(_) => return Err(PlatformError("virtual desktops are not scriptable on Windows".into())),
            WindowCommand::Record => return Err(PlatformError("recording is handled by the application".into())),
        }
    };
    ok.then_some(()).ok_or_else(|| PlatformError("the window rejected the request".into()))
}

pub fn focus(h: isize) {
    // SAFETY: focusing our own window handle.
    unsafe {
        SetForegroundWindow(h as HWND);
    }
}
