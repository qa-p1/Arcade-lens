//! OCR integration.
//!
//! OCR engines are pluggable: the engine built into the OS (Windows.Media.Ocr,
//! Apple Vision) where there is one, otherwise the user's Tesseract. Nothing
//! is bundled and everything runs locally.

use std::sync::Arc;

use image::imageops::FilterType;
use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::{TextLayout, TextValue};
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

pub trait OcrEngine: Send + Sync {
    fn name(&self) -> &str;
    /// Recognizes text; bounding boxes are in `image` pixel coordinates.
    fn recognize(&self, image: &RgbaImage, languages: &[String], cancel: &CancelToken) -> Result<TextLayout>;
}

pub struct OcrRecognizer {
    engine: Arc<dyn OcrEngine>,
}

impl OcrRecognizer {
    pub fn new(engine: Arc<dyn OcrEngine>) -> Self {
        Self { engine }
    }
}

/// Small screen text (a 12px label) OCRs far better when upscaled first.
const MIN_HEIGHT_FOR_OCR: u32 = 64;
const MAX_UPSCALE: u32 = 4;

impl Recognizer for OcrRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.ocr".into(), consumes: vec![caps::REGION], produces: vec![caps::TEXT], cost: Cost::Expensive }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        let s = &cx.signals;
        !s.is_uniform() && s.width >= 6 && s.height >= 6 && s.edge_density > 0.002
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Some(image) = input.value.as_image() else { return Ok(vec![]) };
        let scale = if image.height() < MIN_HEIGHT_FOR_OCR { MIN_HEIGHT_FOR_OCR.div_ceil(image.height().max(1)).min(MAX_UPSCALE) } else { 1 };
        let scaled;
        let source: &RgbaImage = if scale > 1 {
            scaled = image::imageops::resize(&**image, image.width() * scale, image.height() * scale, FilterType::CatmullRom);
            &scaled
        } else {
            image
        };
        cx.cancel.check()?;
        let mut layout = self.engine.recognize(source, &cx.settings.ocr_languages, &cx.cancel)?;
        cx.cancel.check()?;
        if scale > 1 {
            let down = |r: &mut Rect| {
                let s = scale as i32;
                *r = Rect::new(r.x / s, r.y / s, r.width / scale, r.height / scale);
            };
            for line in &mut layout.lines {
                down(&mut line.bbox);
                for w in &mut line.words {
                    down(&mut w.bbox);
                }
            }
        }
        layout.lines.retain(|l| !l.text.trim().is_empty());
        if layout.lines.is_empty() {
            return Ok(vec![]);
        }
        let words: Vec<f32> = layout.lines.iter().flat_map(|l| l.words.iter().map(|w| w.confidence)).collect();
        let confidence = if words.is_empty() { 0.8 } else { words.iter().sum::<f32>() / words.len() as f32 };
        let text = layout.text();
        let lines = layout.lines.len();
        Ok(vec![Detection::new(caps::TEXT, Value::Text(TextValue { text, layout: Some(Arc::new(layout)) }))
            .confidence(confidence)
            .detail("Lines", lines.to_string())
            .detail("Engine", self.engine.name())])
    }
}

pub mod tesseract;
