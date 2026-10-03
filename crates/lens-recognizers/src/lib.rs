//! Built-in Arcade Lens recognizers.
//!
//! Recognizers are registered through the same plugin API third parties use
//! (see [`register`]); nothing here is special-cased by the core.

pub mod image;
pub mod ocr;
pub mod text;

use std::sync::Arc;

use lens_core::registry::{PluginManifest, PluginRegistrar};
use lens_core::{Registry, Result};

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

/// Registers only the built-in recognizers (no actions) as the `core` plugin.
pub fn register(registry: &mut Registry, ocr: Option<Arc<dyn ocr::OcrEngine>>) -> Result<()> {
    registry.register_plugin(PluginManifest::first_party("core", "Arcade Lens built-ins"), |r: &mut PluginRegistrar| register_builtins(r, ocr))
}
