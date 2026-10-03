//! Portable local OCR using the `ocrs` engine (pure Rust, ONNX models).
//! Build in release mode: debug builds of the inference runtime are very slow.

use std::path::Path;

use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::value::{TextLayout, TextLine, Word};
use lens_core::{LensError, Result};
use ocrs::{ImageSource, OcrEngineParams, TextItem};

pub const DETECTION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx";
pub const RECOGNITION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx";
pub const DETECTION_MODEL_FILE: &str = "text-detection.onnx";
pub const RECOGNITION_MODEL_FILE: &str = "text-recognition.onnx";

pub struct OcrsEngine {
    engine: ocrs::OcrEngine,
}

impl OcrsEngine {
    /// Loads models from `dir` (see the `*_MODEL_FILE` names).
    pub fn load(dir: &Path) -> Result<Self> {
        let load = |f: &str| rten::Model::load_file(dir.join(f)).map_err(|e| LensError::Io(format!("loading OCR model {f}: {e}")));
        let params = OcrEngineParams {
            detection_model: Some(load(DETECTION_MODEL_FILE)?),
            recognition_model: Some(load(RECOGNITION_MODEL_FILE)?),
            ..Default::default()
        };
        let engine = ocrs::OcrEngine::new(params).map_err(|e| LensError::Failed(e.to_string()))?;
        Ok(Self { engine })
    }

    pub fn models_present(dir: &Path) -> bool {
        dir.join(DETECTION_MODEL_FILE).is_file() && dir.join(RECOGNITION_MODEL_FILE).is_file()
    }
}

fn rect_of(item: &impl TextItem) -> Rect {
    let r = item.bounding_rect();
    Rect::new(r.left(), r.top(), r.width().max(0) as u32, r.height().max(0) as u32)
}

impl super::OcrEngine for OcrsEngine {
    fn name(&self) -> &str {
        "ocrs"
    }

    fn recognize(&self, image: &RgbaImage, _languages: &[String], cancel: &CancelToken) -> Result<TextLayout> {
        let fail = |e: String| LensError::Failed(format!("OCR: {e}"));
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
        let source = ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions()).map_err(|e| fail(e.to_string()))?;
        let input = self.engine.prepare_input(source).map_err(|e| fail(e.to_string()))?;
        cancel.check()?;
        let words = self.engine.detect_words(&input).map_err(|e| fail(e.to_string()))?;
        cancel.check()?;
        let lines = self.engine.find_text_lines(&input, &words);
        let texts = self.engine.recognize_text(&input, &lines).map_err(|e| fail(e.to_string()))?;
        let mut out = TextLayout::default();
        for line in texts.into_iter().flatten() {
            let text = line.to_string();
            if text.trim().is_empty() {
                continue;
            }
            // ocrs does not report per-word confidence.
            let words = line.words().map(|w| Word { text: w.to_string(), bbox: rect_of(&w), confidence: 0.9 }).collect();
            out.lines.push(TextLine { text: text.trim().to_string(), bbox: rect_of(&line), words });
        }
        Ok(out)
    }
}
