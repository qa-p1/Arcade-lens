//! Screen geometry.
//!
//! All rectangles in Arcade Lens are expressed in **physical pixels in the
//! virtual desktop coordinate space** (the union of all monitors, which may
//! contain negative coordinates). Logical (scaled) coordinates are derived
//! only for display, using the scale factor of the monitor the selection is
//! on. Keeping one canonical space is what guarantees that the selection
//! rectangle corresponds exactly to captured pixels on mixed-DPI setups.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Builds a normalized rectangle from two arbitrary drag corners.
    pub fn from_corners(a: Point, b: Point) -> Self {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        Self::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    pub fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    pub fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }

    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, p: Point) -> bool {
        (p.x as i64) >= self.x as i64
            && (p.x as i64) < self.right()
            && (p.y as i64) >= self.y as i64
            && (p.y as i64) < self.bottom()
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x0 = (self.x as i64).max(other.x as i64);
        let y0 = (self.y as i64).max(other.y as i64);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// Intersection over union, in `[0, 1]`.
    pub fn iou(&self, other: &Rect) -> f64 {
        let inter = self.intersection(other).map_or(0, |r| r.area()) as f64;
        let union = self.area() as f64 + other.area() as f64 - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }

    /// Converts to logical coordinates relative to a monitor.
    pub fn to_logical(&self, monitor: &MonitorInfo) -> LogicalRect {
        let s = monitor.scale_factor.max(f64::EPSILON);
        LogicalRect {
            x: (self.x - monitor.rect.x) as f64 / s,
            y: (self.y - monitor.rect.y) as f64 / s,
            width: self.width as f64 / s,
            height: self.height as f64 / s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    /// Physical rectangle in virtual-desktop space.
    pub rect: Rect,
    pub scale_factor: f64,
    pub refresh_rate_hz: Option<f64>,
    pub is_primary: bool,
}

/// Picks the monitor containing most of `rect`.
pub fn monitor_for<'a>(monitors: &'a [MonitorInfo], rect: &Rect) -> Option<&'a MonitorInfo> {
    monitors
        .iter()
        .map(|m| (m, m.rect.intersection(rect).map_or(0, |r| r.area())))
        .filter(|(_, a)| *a > 0)
        .max_by_key(|(_, a)| *a)
        .map(|(m, _)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_normalize() {
        let r = Rect::from_corners(Point { x: 10, y: -5 }, Point { x: -20, y: 15 });
        assert_eq!(r, Rect::new(-20, -5, 30, 20));
    }

    #[test]
    fn iou_and_intersection() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(a.iou(&a), 1.0);
        let b = Rect::new(50, 0, 100, 100);
        assert_eq!(a.intersection(&b), Some(Rect::new(50, 0, 50, 100)));
        assert!((a.iou(&b) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(a.intersection(&Rect::new(100, 0, 10, 10)), None);
    }

    #[test]
    fn negative_monitor_coordinates_and_scaling() {
        let monitors = vec![
            MonitorInfo { id: "a".into(), name: "left".into(), rect: Rect::new(-2560, 0, 2560, 1440), scale_factor: 1.0, refresh_rate_hz: None, is_primary: false },
            MonitorInfo { id: "b".into(), name: "main".into(), rect: Rect::new(0, 0, 3840, 2160), scale_factor: 1.5, refresh_rate_hz: Some(144.0), is_primary: true },
        ];
        let sel = Rect::new(-100, 10, 300, 50);
        assert_eq!(monitor_for(&monitors, &sel).unwrap().id, "b");
        let sel = Rect::new(-500, 10, 300, 50);
        let m = monitor_for(&monitors, &sel).unwrap();
        assert_eq!(m.id, "a");
        let l = Rect::new(300, 300, 300, 150).to_logical(&monitors[1]);
        assert_eq!((l.x, l.y, l.width, l.height), (200.0, 200.0, 200.0, 100.0));
    }
}
