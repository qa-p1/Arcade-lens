//! Windows and macOS capture, monitors and windows via `xcap`
//! (Windows Graphics Capture/GDI, ScreenCaptureKit/CoreGraphics).

use image::RgbaImage;
use lens_core::geometry::{MonitorInfo, Point, Rect};
use lens_core::selection::WindowInfo;

use crate::PlatformError;

fn err(e: impl std::fmt::Display) -> PlatformError {
    PlatformError(format!("capture: {e}"))
}

fn info(m: &xcap::Monitor) -> Result<MonitorInfo, PlatformError> {
    let scale = m.scale_factor().map_err(err)? as f64;
    // xcap reports logical sizes on macOS and physical on Windows; normalize to physical.
    let (w, h) = (m.width().map_err(err)?, m.height().map_err(err)?);
    let (x, y) = (m.x().map_err(err)?, m.y().map_err(err)?);
    let (pw, ph, px, py) = if cfg!(target_os = "macos") {
        ((w as f64 * scale) as u32, (h as f64 * scale) as u32, (x as f64 * scale) as i32, (y as f64 * scale) as i32)
    } else {
        (w, h, x, y)
    };
    Ok(MonitorInfo {
        id: m.id().map_err(err)?.to_string(),
        name: m.friendly_name().or_else(|_| m.name()).map_err(err)?,
        rect: Rect::new(px, py, pw, ph),
        scale_factor: scale,
        refresh_rate_hz: m.frequency().ok().map(|f| f as f64).filter(|f| *f > 0.0),
        is_primary: m.is_primary().unwrap_or(false),
    })
}

pub fn monitors() -> Result<Vec<MonitorInfo>, PlatformError> {
    xcap::Monitor::all().map_err(err)?.iter().map(info).collect()
}

pub fn capture_all() -> Result<Vec<(MonitorInfo, RgbaImage)>, PlatformError> {
    xcap::Monitor::all()
        .map_err(err)?
        .iter()
        .map(|m| {
            let i = info(m)?;
            let img = m.capture_image().map_err(err)?;
            Ok((i, img))
        })
        .collect()
}

pub fn capture_rect(r: Rect) -> Result<RgbaImage, PlatformError> {
    let center = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    for m in xcap::Monitor::all().map_err(err)? {
        let i = info(&m)?;
        if i.rect.contains(Point { x: center.0, y: center.1 }) {
            let img = m.capture_image().map_err(err)?;
            let local = Rect::new(r.x - i.rect.x, r.y - i.rect.y, r.width, r.height);
            let clip = local.intersection(&Rect::new(0, 0, img.width(), img.height())).ok_or_else(|| PlatformError("region off-screen".into()))?;
            return Ok(image::imageops::crop_imm(&img, clip.x as u32, clip.y as u32, clip.width, clip.height).to_image());
        }
    }
    Err(PlatformError("region is not on any monitor".into()))
}

pub fn windows(exclude_pid: Option<u32>) -> Vec<WindowInfo> {
    let Ok(all) = xcap::Window::all() else { return vec![] };
    let mut v: Vec<(i32, WindowInfo)> = all
        .iter()
        .filter(|w| !w.is_minimized().unwrap_or(false))
        .filter_map(|w| {
            let pid = w.pid().ok();
            if exclude_pid.is_some() && pid == exclude_pid {
                return None;
            }
            let scale = if cfg!(target_os = "macos") { w.current_monitor().ok().and_then(|m| m.scale_factor().ok()).unwrap_or(1.0) as f64 } else { 1.0 };
            let s = |v: i32| (v as f64 * scale) as i32;
            let rect = Rect::new(s(w.x().ok()?), s(w.y().ok()?), s(w.width().ok()? as i32) as u32, s(w.height().ok()? as i32) as u32);
            if rect.is_empty() {
                return None;
            }
            Some((
                w.z().unwrap_or(0),
                WindowInfo { id: format!("native:{}", w.id().ok()?), title: w.title().unwrap_or_default(), app_name: w.app_name().ok(), rect, pid },
            ))
        })
        .collect();
    // Higher z is closer to the viewer.
    v.sort_by_key(|(z, _)| std::cmp::Reverse(*z));
    v.into_iter().map(|(_, w)| w).collect()
}
