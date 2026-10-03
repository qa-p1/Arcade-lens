//! Single colors and representative palettes.

use std::collections::HashMap;

use image::RgbaImage;
use lens_core::color::{Lab, Rgb};
use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::PaletteColor;
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

/// Colors closer than this (CIE76 ΔE) are reported as one palette entry.
const MERGE_DELTA_E: f64 = 12.0;
const MIN_COVERAGE: f32 = 0.015;
const MAX_COLORS: usize = 6;

pub struct ColorRecognizer;

impl Recognizer for ColorRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.color".into(), consumes: vec![caps::REGION], produces: vec![caps::COLOR], cost: Cost::Trivial }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        cx.signals.is_tiny() || cx.signals.is_uniform()
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Some(img) = input.value.as_image() else { return Ok(vec![]) };
        let Some((color, share)) = most_common_exact(img) else { return Ok(vec![]) };
        let confidence = if cx.signals.is_uniform() { 0.95 } else if share > 0.5 { 0.8 } else { 0.6 };
        Ok(vec![color_detection(color, confidence)])
    }
}

pub fn color_detection(c: Rgb, confidence: f32) -> Detection {
    Detection::new(caps::COLOR, Value::Color(c))
        .confidence(confidence)
        .detail("HEX", c.hex())
        .detail("RGB", format!("{}, {}, {}", c.r, c.g, c.b))
        .detail("HSL", c.hsl_string().trim_start_matches("hsl(").trim_end_matches(')').to_string())
}

/// The exact most frequent opaque pixel. Exact (not averaged) so a 3×3
/// selection on an anti-aliased edge still reports a color that exists.
fn most_common_exact(img: &RgbaImage) -> Option<(Rgb, f32)> {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    let mut total = 0;
    for p in img.pixels().filter(|p| p[3] > 0) {
        *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
        total += 1;
    }
    let (c, n) = counts.into_iter().max_by_key(|(c, n)| (*n, std::cmp::Reverse(*c)))?;
    Some((Rgb::new(c[0], c[1], c[2]), n as f32 / total as f32))
}

pub struct PaletteRecognizer;

impl Recognizer for PaletteRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.palette".into(), consumes: vec![caps::REGION], produces: vec![caps::PALETTE], cost: Cost::Cheap }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        !cx.signals.is_uniform() && cx.signals.pixel_count() >= 32 * 32 && cx.signals.distinct_colors >= 2
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Some(img) = input.value.as_image() else { return Ok(vec![]) };
        cx.cancel.check()?;
        let palette = extract_palette(img);
        if palette.len() < 2 {
            return Ok(vec![]);
        }
        let mut d = Detection::new(caps::PALETTE, Value::Palette(palette.clone())).confidence(0.6);
        for p in &palette {
            d = d.detail(p.color.hex(), format!("{:.0}%", p.coverage * 100.0));
        }
        Ok(vec![d])
    }
}

/// Dominant colors via a quantized histogram merged greedily in Lab space,
/// so near-identical shades collapse into one entry.
pub fn extract_palette(img: &RgbaImage) -> Vec<PaletteColor> {
    let (w, h) = img.dimensions();
    let step = (((w as u64 * h as u64) as f64 / 20_000.0).sqrt().ceil() as u32).max(1);
    let mut hist: HashMap<u16, (u32, [u64; 3])> = HashMap::new();
    let mut total = 0u32;
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let p = img.get_pixel(x, y);
            if p[3] < 128 {
                continue;
            }
            let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
            let e = hist.entry(key).or_insert((0, [0; 3]));
            e.0 += 1;
            for i in 0..3 {
                e.1[i] += p[i] as u64;
            }
            total += 1;
        }
    }
    if total == 0 {
        return vec![];
    }
    let mut bins: Vec<(u32, Rgb)> = hist
        .into_values()
        .map(|(n, s)| (n, Rgb::new((s[0] / n as u64) as u8, (s[1] / n as u64) as u8, (s[2] / n as u64) as u8)))
        .collect();
    bins.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.hex().cmp(&b.1.hex())));

    struct Cluster {
        n: u32,
        sum: [f64; 3],
        lab: Lab,
    }
    let mut clusters: Vec<Cluster> = Vec::new();
    for (n, c) in bins {
        let lab = c.to_lab();
        let nearest = clusters.iter_mut().map(|k| (k.lab.delta_e(&lab), k)).filter(|(d, _)| *d < MERGE_DELTA_E).min_by(|a, b| a.0.total_cmp(&b.0));
        match nearest {
            Some((_, k)) => {
                // The seed (most common shade) keeps defining the cluster color.
                k.n += n;
                k.sum[0] += c.r as f64 * n as f64;
                k.sum[1] += c.g as f64 * n as f64;
                k.sum[2] += c.b as f64 * n as f64;
            }
            None => clusters.push(Cluster { n, sum: [c.r as f64 * n as f64, c.g as f64 * n as f64, c.b as f64 * n as f64], lab }),
        }
    }
    let mut out: Vec<PaletteColor> = clusters
        .into_iter()
        .map(|k| {
            let n = k.n as f64;
            PaletteColor { color: Rgb::new((k.sum[0] / n).round() as u8, (k.sum[1] / n).round() as u8, (k.sum[2] / n).round() as u8), coverage: k.n as f32 / total as f32 }
        })
        .filter(|p| p.coverage >= MIN_COVERAGE)
        .collect();
    out.sort_by(|a, b| b.coverage.total_cmp(&a.coverage));
    out.truncate(MAX_COLORS);
    out
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn exact_color_on_antialiased_edge() {
        let mut img = RgbaImage::from_pixel(3, 3, Rgba([0x7C, 0x3A, 0xED, 255]));
        img.put_pixel(0, 0, Rgba([0x90, 0x60, 0xF0, 255]));
        assert_eq!(most_common_exact(&img).unwrap().0, Rgb::new(0x7C, 0x3A, 0xED));
    }

    #[test]
    fn palette_merges_near_duplicates() {
        // Four bands: two nearly identical darks, a white, and violet.
        let img = RgbaImage::from_fn(200, 100, |x, _| match x {
            0..=59 => Rgba([0x18, 0x18, 0x1B, 255]),
            60..=99 => Rgba([0x1A, 0x1A, 0x1D, 255]),
            100..=159 => Rgba([0xFA, 0xFA, 0xFA, 255]),
            _ => Rgba([0x7C, 0x3A, 0xED, 255]),
        });
        let p = extract_palette(&img);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!((p[0].coverage - 0.5).abs() < 0.02);
        assert!(p[0].color.to_lab().delta_e(&Rgb::new(0x18, 0x18, 0x1B).to_lab()) < 2.0);
        assert!(p.iter().any(|c| c.color == Rgb::new(0x7C, 0x3A, 0xED)));
    }

    #[test]
    fn noise_does_not_explode_palette() {
        let img = RgbaImage::from_fn(256, 256, |x, y| Rgba([(x ^ y) as u8, (x * 7 % 256) as u8, (y * 13 % 256) as u8, 255]));
        assert!(extract_palette(&img).len() <= MAX_COLORS);
    }
}
