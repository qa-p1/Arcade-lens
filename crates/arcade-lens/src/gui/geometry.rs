//! Where the Settings and History windows open (STANDARD §5.1, window
//! behaviour): on the monitor under the pointer, at the size and position
//! they last had on that monitor, or centred at their default size.
//!
//! Positions are kept per monitor as an offset from its top-left corner, in
//! points (physical pixels / the monitor's scale, as the overlay places its
//! windows). Where the platform doesn't expose monitors or the pointer
//! (Wayland), the compositor places windows and only the size is restored.
//!
//! The placement is worked out once per opening and cached: egui re-applies a
//! changed builder to an open window, which would make it follow the pointer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use eframe::egui::{self, Pos2, Rect, Vec2, ViewportBuilder};
use lens_core::geometry::MonitorInfo;
use serde::{Deserialize, Serialize};

const FILE: &str = "window-geometry.json";
/// Key for windows whose monitor can't be known.
const ANY_MONITOR: &str = "*";
/// A moved or resized window is saved once it has been still this long.
const SETTLE: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Saved {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// window kind → monitor key → geometry
type Store = BTreeMap<String, BTreeMap<String, Saved>>;

struct Open {
    /// The monitor it opened on: its key and origin in points (None: unknown).
    monitor: Option<(String, Pos2, Vec2)>,
    builder: ViewportBuilder,
    last: Option<(Rect, Instant)>,
    saved: Option<Rect>,
}

static OPEN: Mutex<BTreeMap<&'static str, Open>> = Mutex::new(BTreeMap::new());

fn key(m: &MonitorInfo) -> String {
    format!("{}@{}x{}", m.name, m.rect.width, m.rect.height)
}

fn origin_and_size(m: &MonitorInfo) -> (Pos2, Vec2) {
    let s = m.scale_factor.max(0.1) as f32;
    (egui::pos2(m.rect.x as f32 / s, m.rect.y as f32 / s), egui::vec2(m.rect.width as f32 / s, m.rect.height as f32 / s))
}

fn load(data: &Path) -> Store {
    std::fs::read(data.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(data: &Path, kind: &str, monitor: &str, g: Saved) {
    let mut store = load(data);
    store.entry(kind.to_string()).or_default().insert(monitor.to_string(), g);
    if let Ok(json) = serde_json::to_vec_pretty(&store) {
        let path: PathBuf = data.join(FILE);
        let _ = std::fs::create_dir_all(data);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// The monitor under `cursor` (physical), else the primary, else the first.
fn pick(monitors: &[MonitorInfo], cursor: Option<lens_core::geometry::Point>) -> Option<&MonitorInfo> {
    cursor.and_then(|c| monitors.iter().find(|m| m.rect.contains(c))).or_else(|| monitors.iter().find(|m| m.is_primary)).or_else(|| monitors.first())
}

/// Where a window of `default` size opens on a monitor at `origin`/`area`
/// (points), given what was saved for it there. Always fully on the monitor
/// when it fits.
fn place(origin: Pos2, area: Vec2, default: Vec2, min: Vec2, saved: Option<Saved>) -> Rect {
    let size = saved.map_or(default, |g| egui::vec2(g.w, g.h)).max(min).min(area.max(min));
    let offset = match saved {
        Some(g) => egui::vec2(g.x, g.y),
        None => (area - size) / 2.0,
    };
    let max = (area - size).max(Vec2::ZERO);
    Rect::from_min_size(origin + offset.clamp(Vec2::ZERO, max), size)
}

/// The builder for window `kind`: `base` with the size and, where known, the
/// position it should open with. Call it every frame; it is stable while the
/// window stays open.
pub fn builder(kind: &'static str, base: ViewportBuilder, data: &Path) -> ViewportBuilder {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = open.get(kind) {
        return o.builder.clone();
    }
    let default = base.inner_size.unwrap_or(egui::vec2(760.0, 560.0));
    let min = base.min_inner_size.unwrap_or(Vec2::ZERO);
    let store = load(data);
    let saved = store.get(kind);
    let monitors = lens_platform::monitors().unwrap_or_default();
    let (builder, monitor) = match pick(&monitors, lens_platform::cursor_position()) {
        Some(m) => {
            let k = key(m);
            let (origin, area) = origin_and_size(m);
            let r = place(origin, area, default, min, saved.and_then(|s| s.get(&k)).copied());
            (base.with_position(r.min).with_inner_size(r.size()), Some((k, origin, area)))
        }
        None => {
            let size = saved.and_then(|s| s.get(ANY_MONITOR)).map_or(default, |g| egui::vec2(g.w, g.h)).max(min);
            (base.with_inner_size(size), None)
        }
    };
    open.insert(kind, Open { monitor, builder: builder.clone(), last: None, saved: None });
    builder
}

/// Call every frame from inside window `kind`: saves its geometry once it
/// has settled after a move or resize.
pub fn track(ctx: &egui::Context, kind: &'static str, data: &Path) {
    let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
    let Some(inner) = inner else { return };
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(o) = open.get_mut(kind) else { return };
    // Position from the outer frame (what with_position sets), size from the inside.
    let rect = Rect::from_min_size(outer.unwrap_or(inner).min, inner.size());
    let now = Instant::now();
    match o.last {
        Some((r, _)) if r == rect => {}
        _ => o.last = Some((rect, now)),
    }
    let Some((r, since)) = o.last else { return };
    if o.saved == Some(r) {
        return;
    }
    if now.duration_since(since) < SETTLE {
        ctx.request_repaint_after(SETTLE);
        return;
    }
    let (monitor, g) = match &o.monitor {
        Some((k, origin, _)) => (k.clone(), Saved { x: r.min.x - origin.x, y: r.min.y - origin.y, w: r.width(), h: r.height() }),
        None => (ANY_MONITOR.to_string(), Saved { x: 0.0, y: 0.0, w: r.width(), h: r.height() }),
    };
    o.saved = Some(r);
    drop(open);
    save(data, kind, &monitor, g);
}

/// The window closed: the next opening is placed afresh.
pub fn forget(kind: &'static str) {
    OPEN.lock().unwrap_or_else(|e| e.into_inner()).remove(kind);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lens_core::geometry::{Point, Rect as PRect};

    fn monitor(name: &str, x: i32, w: u32, scale: f64, primary: bool) -> MonitorInfo {
        MonitorInfo {
            id: name.into(),
            name: name.into(),
            rect: PRect { x, y: 0, width: w, height: 1080 },
            scale_factor: scale,
            refresh_rate_hz: None,
            is_primary: primary,
        }
    }

    #[test]
    fn opens_on_the_monitor_under_the_pointer() {
        let ms = [monitor("A", 0, 1920, 1.0, true), monitor("B", 1920, 2560, 1.0, false)];
        assert_eq!(pick(&ms, Some(Point { x: 2000, y: 10 })).unwrap().name, "B");
        assert_eq!(pick(&ms, Some(Point { x: 10, y: 10 })).unwrap().name, "A");
        assert_eq!(pick(&ms, None).unwrap().name, "A");
        assert_eq!(pick(&ms, Some(Point { x: -50, y: 10 })).unwrap().name, "A");
    }

    #[test]
    fn centred_by_default_and_restored_per_monitor_inside_it() {
        let (origin, area) = origin_and_size(&monitor("B", 1920, 2560, 1.25, false));
        assert_eq!(origin, egui::pos2(1536.0, 0.0));
        let min = egui::vec2(720.0, 520.0);
        let centred = place(origin, area, egui::vec2(760.0, 560.0), min, None);
        assert_eq!(centred.center(), origin + area / 2.0);
        let saved = Saved { x: 40.0, y: 30.0, w: 900.0, h: 600.0 };
        assert_eq!(
            place(origin, area, egui::vec2(760.0, 560.0), min, Some(saved)),
            Rect::from_min_size(origin + egui::vec2(40.0, 30.0), egui::vec2(900.0, 600.0))
        );
        // A position saved further right than the monitor now allows is pulled back on.
        let off = Saved { x: 5000.0, y: -20.0, w: 800.0, h: 600.0 };
        let r = place(origin, area, egui::vec2(760.0, 560.0), min, Some(off));
        assert_eq!((r.min.y, r.max.x), (0.0, origin.x + area.x));
        // Never below the minimum size.
        let tiny = Saved { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert_eq!(place(origin, area, egui::vec2(760.0, 560.0), min, Some(tiny)).size(), min);
    }

    #[test]
    fn store_round_trips_per_kind_and_monitor() {
        let dir = std::env::temp_dir().join(format!("lens-geometry-{}", std::process::id()));
        let g = Saved { x: 1.0, y: 2.0, w: 800.0, h: 600.0 };
        save(&dir, "settings", "A@1920x1080", g);
        save(&dir, "history", "*", g);
        let s = load(&dir);
        assert_eq!(s["settings"]["A@1920x1080"], g);
        assert_eq!(s["history"]["*"], g);
        let _ = std::fs::remove_file(dir.join(FILE));
        let _ = std::fs::remove_dir(&dir);
    }
}
