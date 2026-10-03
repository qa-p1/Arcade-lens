//! The user's selection and the cheap signals computed from it.

use std::collections::HashMap;
use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::geometry::{MonitorInfo, Rect};

/// A captured screen region plus whatever the platform could tell us about
/// what was under it. Selections are ephemeral and are never persisted
/// unless an action explicitly does so.
#[derive(Clone)]
pub struct Selection {
    /// Physical pixels in virtual-desktop space.
    pub rect: Rect,
    /// Exactly the pixels inside `rect`.
    pub image: Arc<RgbaImage>,
    pub context: SelectionContext,
}

impl Selection {
    pub fn new(rect: Rect, image: RgbaImage) -> Self {
        Self { rect, image: Arc::new(image), context: SelectionContext::default() }
    }

    /// Convenience for tests and tools operating on a standalone image.
    pub fn from_image(image: RgbaImage) -> Self {
        let rect = Rect::new(0, 0, image.width(), image.height());
        Self::new(rect, image)
    }

    pub fn scale_factor(&self) -> f64 {
        self.context.monitor.as_ref().map_or(1.0, |m| m.scale_factor)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SelectionContext {
    pub monitor: Option<MonitorInfo>,
    /// Top-level windows visible at capture time, front-most first.
    pub windows: Vec<WindowInfo>,
    /// Application that had focus before Lens activated.
    pub focused_app: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: Option<String>,
    pub rect: Rect,
    pub pid: Option<u32>,
}

/// Inexpensive whole-region statistics used to schedule recognizers before
/// any expensive analysis runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Signals {
    pub width: u32,
    pub height: u32,
    /// Distinct colors among sampled pixels after 5-bit-per-channel quantization.
    pub distinct_colors: usize,
    /// Most common quantized color and the fraction of samples it covers.
    pub dominant: Rgb,
    pub dominant_fraction: f32,
    /// Fraction of sampled neighbor pairs with a strong luminance step.
    /// Text and UI chrome produce many sharp edges; photos produce few.
    pub edge_density: f32,
    /// Mean per-sample saturation in `[0, 1]`.
    pub mean_saturation: f32,
}

impl Signals {
    pub fn compute(image: &RgbaImage) -> Signals {
        let (w, h) = image.dimensions();
        if w == 0 || h == 0 {
            return Signals { width: w, height: h, distinct_colors: 0, dominant: Rgb::new(0, 0, 0), dominant_fraction: 0.0, edge_density: 0.0, mean_saturation: 0.0 };
        }
        // Sample at most ~40k pixels on a regular grid.
        let step = (((w as u64 * h as u64) as f64 / 40_000.0).sqrt().floor() as u32).max(1);
        let mut hist: HashMap<u16, (u32, [u64; 3])> = HashMap::new();
        let mut samples = 0u32;
        let mut edges = 0u32;
        let mut pairs = 0u32;
        let mut sat_sum = 0f64;
        let lum = |p: &image::Rgba<u8>| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let mut y = 0;
        while y < h {
            let mut x = 0;
            while x < w {
                let p = image.get_pixel(x, y);
                let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
                let e = hist.entry(key).or_insert((0, [0; 3]));
                e.0 += 1;
                e.1[0] += p[0] as u64;
                e.1[1] += p[1] as u64;
                e.1[2] += p[2] as u64;
                samples += 1;
                let max = p[0].max(p[1]).max(p[2]) as f64;
                let min = p[0].min(p[1]).min(p[2]) as f64;
                if max > 0.0 {
                    sat_sum += (max - min) / max;
                }
                if x + 1 < w {
                    pairs += 1;
                    if (lum(p) - lum(image.get_pixel(x + 1, y))).abs() > 48.0 {
                        edges += 1;
                    }
                }
                x += step;
            }
            y += step;
        }
        let (_, (count, sums)) = hist.iter().max_by_key(|(k, (c, _))| (*c, std::cmp::Reverse(**k))).expect("non-empty");
        let avg = |s: u64| (s / *count as u64) as u8;
        Signals {
            width: w,
            height: h,
            distinct_colors: hist.len(),
            dominant: Rgb::new(avg(sums[0]), avg(sums[1]), avg(sums[2])),
            dominant_fraction: *count as f32 / samples as f32,
            edge_density: if pairs == 0 { 0.0 } else { edges as f32 / pairs as f32 },
            mean_saturation: (sat_sum / samples as f64) as f32,
        }
    }

    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Nearly a single flat color.
    pub fn is_uniform(&self) -> bool {
        self.dominant_fraction >= 0.9
    }

    /// Small enough that the user is probably pointing at a color or icon.
    pub fn is_tiny(&self) -> bool {
        self.width <= 24 && self.height <= 24
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn uniform_region() {
        let img = RgbaImage::from_pixel(40, 30, Rgba([0x18, 0x18, 0x1B, 255]));
        let s = Signals::compute(&img);
        assert!(s.is_uniform());
        assert_eq!(s.distinct_colors, 1);
        assert_eq!(s.dominant, Rgb::new(0x18, 0x18, 0x1B));
        assert_eq!(s.edge_density, 0.0);
    }

    #[test]
    fn striped_region_has_edges() {
        let img = RgbaImage::from_fn(100, 20, |x, _| if x % 2 == 0 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let s = Signals::compute(&img);
        assert!(!s.is_uniform());
        assert!(s.edge_density > 0.9);
    }
}
