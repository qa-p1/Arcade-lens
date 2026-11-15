//! Built-in Arcade Lens recognizers.
//!
//! Recognizers are registered through the same plugin API third parties use
//! (see [`register_builtins`]); nothing here is special-cased by the core.

pub mod image;
pub mod ocr;
pub mod text;

use std::sync::Arc;

use lens_core::registry::PluginRegistrar;

/// Adds every built-in recognizer to the `core` plugin. `ocr` is optional
/// so the rest of Lens keeps working where no OCR engine is available.
pub fn register_builtins(r: &mut PluginRegistrar, ocr: Option<Arc<dyn ocr::OcrEngine>>) {
    for t in text::recognizers() {
        r.recognizer(t);
    }
    image::register(r);
    if let Some(engine) = ocr {
        r.recognizer(ocr::OcrRecognizer::new(engine));
    }
}
