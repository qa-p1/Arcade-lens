//! UI inspection: dimensions, position and colors of whatever was selected.
//! Works on any application because it only reads pixels.

use std::collections::HashMap;

use lens_core::color::Rgb;
use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::GeometryValue;
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

use super::color::extract_palette;

pub struct InspectRecognizer;

impl Recognizer for InspectRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.inspect".into(), consumes: vec![caps::REGION], produces: vec![caps::UI_ELEMENT], cost: Cost::Cheap }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        cx.signals.width >= 4 && cx.signals.height >= 4
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Some(img) = input.value.as_image() else { return Ok(vec![]) };
        let (w, h) = img.dimensions();
        // Background: most common color along the selection's border.
        let mut border: HashMap<[u8; 3], u32> = HashMap::new();
        let mut n = 0u32;
        for x in 0..w {
            for y in [0, h - 1] {
                let p = img.get_pixel(x, y);
                *border.entry([p[0], p[1], p[2]]).or_default() += 1;
                n += 1;
            }
        }
        for y in 1..h.saturating_sub(1) {
            for x in [0, w - 1] {
                let p = img.get_pixel(x, y);
                *border.entry([p[0], p[1], p[2]]).or_default() += 1;
                n += 1;
            }
        }
        let (bg, bg_count) = border.into_iter().max_by_key(|(c, k)| (*k, std::cmp::Reverse(*c))).expect("border is non-empty");
        let background = Rgb::new(bg[0], bg[1], bg[2]);
        let bg_share = bg_count as f32 / n as f32;
        // Foreground: the palette color that stands out most against the background.
        let bg_lab = background.to_lab();
        let foreground = extract_palette(img)
            .into_iter()
            .filter(|p| p.color.to_lab().delta_e(&bg_lab) > 20.0)
            .max_by(|a, b| (a.coverage.sqrt() as f64 * a.color.contrast(&background)).total_cmp(&(b.coverage.sqrt() as f64 * b.color.contrast(&background))))
            .map(|p| p.color);

        let sel = &cx.selection;
        let scale = sel.scale_factor();
        let rect = sel.rect;
        let mut d = Detection::new(caps::UI_ELEMENT, Value::Geometry(GeometryValue { rect, scale_factor: scale, background: Some(background), foreground }))
            .confidence(if bg_share >= 0.6 { 0.6 } else { 0.35 });
        d = d.detail("Size", format!("{} × {} px", rect.width, rect.height));
        if (scale - 1.0).abs() > f64::EPSILON {
            d = d.detail("Logical size", format!("{} × {} pt @{scale}x", (rect.width as f64 / scale).round(), (rect.height as f64 / scale).round()));
        }
        match &sel.context.monitor {
            Some(m) => {
                let l = rect.to_logical(m);
                d = d.detail("Position", format!("X {} Y {} on {}", l.x.round(), l.y.round(), m.name));
            }
            None => d = d.detail("Position", format!("X {} Y {}", rect.x, rect.y)),
        }
        d = d.detail("Background", background.hex());
        if let Some(fg) = foreground {
            let ratio = fg.contrast(&background);
            let grade = if ratio >= 7.0 {
                "AAA"
            } else if ratio >= 4.5 {
                "AA"
            } else if ratio >= 3.0 {
                "AA large text"
            } else {
                "fails WCAG"
            };
            d = d.detail("Foreground", fg.hex()).detail("Contrast", format!("{ratio:.1}:1 ({grade})"));
        }
        Ok(vec![d])
    }
}
