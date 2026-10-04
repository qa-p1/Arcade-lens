//! Wayland backend. Wayland deliberately forbids global screen capture,
//! global shortcuts and window enumeration; the desktop portal provides a
//! user-mediated screenshot instead. The portal returns one image of the
//! whole desktop, which becomes the frozen frame.

use image::RgbaImage;
use lens_core::geometry::{MonitorInfo, Rect};

use crate::PlatformError;

pub fn active() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("XDG_SESSION_TYPE").map_or(true, |t| t == "wayland")
}

pub fn capture_all() -> Result<Vec<(MonitorInfo, RgbaImage)>, PlatformError> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| PlatformError(e.to_string()))?;
    // A request nobody answers (a dismissed permission prompt) would otherwise
    // block every later capture.
    let uri = rt.block_on(async {
        let request = async {
            let request = ashpd::desktop::screenshot::Screenshot::request().interactive(false).modal(false).send().await?;
            request.response().map(|s| s.uri().as_str().to_string())
        };
        tokio::time::timeout(std::time::Duration::from_secs(60), request).await
    });
    let uri = uri.map_err(|_| PlatformError("the screenshot portal did not answer".into()))?.map_err(|e| PlatformError(format!("screenshot portal: {e}")))?;
    let path = uri.strip_prefix("file://").ok_or_else(|| PlatformError(format!("unexpected screenshot URI {uri}")))?;
    let path = percent_decode(path);
    let img = image::open(&path).map_err(|e| PlatformError(format!("{path}: {e}")))?.to_rgba8();
    // The portal saves into the user's Pictures folder; Lens selections are ephemeral.
    let _ = std::fs::remove_file(&path);
    let monitor = MonitorInfo {
        id: "wayland".into(),
        name: "Desktop".into(),
        rect: Rect::new(0, 0, img.width(), img.height()),
        scale_factor: 1.0,
        refresh_rate_hz: None,
        is_primary: true,
    };
    Ok(vec![(monitor, img)])
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
