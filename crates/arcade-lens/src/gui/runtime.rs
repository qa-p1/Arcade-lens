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
    pub engine: Engine,
    pub chains: Vec<Chain>,
    pub ocr_name: String,
    pub plugins: Vec<lens_plugins::PluginStatus>,
}

/// Picks the best local OCR engine: the OS engine when present (Windows,
/// macOS), otherwise the portable ocrs engine if its models are installed.
pub fn ocr_engine(paths: &Paths) -> (Option<Arc<dyn OcrEngine>>, String) {
    if let Some(e) = lens_platform::native_ocr() {
        let name = e.name().to_string();
        return (Some(e), name);
    }
    #[cfg(feature = "ocrs")]
    {
        use lens_recognizers::ocr::ocrs_engine::OcrsEngine;
        let dir = paths.models();
        if OcrsEngine::models_present(&dir) {
            return match OcrsEngine::load(&dir) {
                Ok(e) => (Some(Arc::new(e)), "ocrs".into()),
                Err(e) => (None, format!("unavailable: {e}")),
            };
        }
        return (None, "models not installed".into());
    }
    #[allow(unreachable_code)]
    {
        let _ = paths;
        (None, "not available".into())
    }
}

impl Runtime {
    pub fn load(paths: Paths) -> Result<Runtime, String> {
        let settings = config::load_settings(&paths)?;
        let chains = config::load_chains(&paths);
        let (ocr, ocr_name) = ocr_engine(&paths);
        let mut registry = lens_actions::standard_registry(ocr).map_err(|e| e.to_string())?;
        let plugins = lens_plugins::load_enabled(&mut registry, &paths.plugins(), &settings.enabled_plugins);
        let registry = Arc::new(registry);
        let settings = Arc::new(settings);
        let engine = Engine::new(registry.clone(), Arc::new(LocalEnvironment), settings.clone());
        Ok(Runtime { paths, settings, registry, engine, chains, ocr_name, plugins })
    }
}
