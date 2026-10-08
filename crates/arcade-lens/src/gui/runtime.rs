//! Everything the GUI needs to analyze and act, rebuilt when settings change.

use std::sync::Arc;

use lens_core::chain::Chain;
use lens_core::recognizer::LocalEnvironment;
use lens_core::{Engine, Registry, Settings};
use lens_recognizers::ocr::OcrEngine;

use crate::config::{self, Paths};

pub struct Runtime {
    pub paths: Paths,
    pub settings: Arc<Settings>,
    pub registry: Arc<Registry>,
    pub arcade: lens_actions::arcade::Arcade,
    pub engine: Engine,
    pub chains: Vec<Chain>,
    pub ocr_name: String,
    pub plugins: Vec<lens_plugins::PluginStatus>,
}

/// Picks the local OCR engine: the OS engine on Windows and macOS, otherwise
/// the user's Tesseract (a system install, or the copy any Arcade app downloaded).
pub fn ocr_engine() -> (Option<Arc<dyn OcrEngine>>, String) {
    if let Some(e) = lens_platform::native_ocr() {
        let name = e.name().to_string();
        return (Some(e), name);
    }
    match arcade_link::engines::find_tesseract() {
        Some(exe) => match lens_recognizers::ocr::tesseract::TesseractEngine::new(exe) {
            Ok(e) => (Some(Arc::new(e)), "Tesseract".into()),
            Err(e) => (None, format!("unavailable: {e}")),
        },
        None => (None, OCR_MISSING.into()),
    }
}

/// `ocr_name` when there is no engine; Settings then offers the download.
pub const OCR_MISSING: &str = "Tesseract not installed";

impl Runtime {
    pub fn load(paths: Paths) -> Result<Runtime, String> {
        Self::load_inner(paths, true)
    }

    pub fn headless(paths: Paths) -> Result<Runtime, String> {
        Self::load_inner(paths, false)
    }

    fn load_inner(paths: Paths, connected: bool) -> Result<Runtime, String> {
        let settings = config::load_settings(&paths)?;
        let chains = config::load_chains(&paths);
        let (ocr, ocr_name) = ocr_engine();
        let mut registry = lens_actions::standard_registry(ocr).map_err(|e| e.to_string())?;
        let plugins = lens_plugins::load_enabled(&mut registry, &paths.plugins(), &settings.enabled_plugins);
        let registry = Arc::new(registry);
        let settings = Arc::new(settings);
        let locations = arcade_link::Locations::discover();
        let arcade = if connected {
            lens_actions::arcade::Arcade::start(registry.clone(), settings.clone(), locations)
        } else {
            lens_actions::arcade::Arcade::offline(registry.clone(), locations)
        };
        let engine = Engine::new(registry.clone(), Arc::new(LocalEnvironment), settings.clone());
        Ok(Runtime { paths, settings, registry, arcade, engine, chains, ocr_name, plugins })
    }
}
