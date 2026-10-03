//! Built-in Arcade Lens actions and default chains.
//!
//! Every action is an independent provider registered through the plugin
//! API: it declares what it accepts, what it produces, its side effects and
//! required platform features. Recognizers know nothing about them.

pub mod data;
pub mod dev;
pub mod extra;
pub mod text;
pub mod util;
pub mod visual;
pub mod web;

use std::sync::Arc;

use lens_core::chain::{Chain, ChainStep};
use lens_core::registry::{PluginManifest, PluginRegistrar};
use lens_core::{caps, Registry, Result};
use lens_recognizers::ocr::OcrEngine;

/// Adds every built-in action to the `core` plugin.
pub fn register_builtins(r: &mut PluginRegistrar) {
    visual::register(r);
    text::register(r);
    web::register(r);
    data::register(r);
    dev::register(r);
    extra::register(r);
}

/// A registry with every built-in recognizer and action, registered as the
/// first-party `core` plugin.
pub fn standard_registry(ocr: Option<Arc<dyn OcrEngine>>) -> Result<Registry> {
    let mut registry = Registry::new();
    registry.register_plugin(PluginManifest::first_party("core", "Arcade Lens built-ins"), |r| {
        lens_recognizers::register_builtins(r, ocr);
        register_builtins(r);
    })?;
    Ok(registry)
}

fn run(action: &str, params: serde_json::Value) -> ChainStep {
    ChainStep::Run { action: action.into(), params: params.as_object().cloned().unwrap_or_default() }
}

/// Starter chains from the product spec; users can edit or delete them.
pub fn default_chains() -> Vec<Chain> {
    vec![
        Chain {
            id: "clean-copy".into(),
            name: "Clean Copy".into(),
            steps: vec![
                ChainStep::Take { capability: caps::TEXT },
                run("core.text.remove-line-breaks", serde_json::json!({})),
                run("core.copy", serde_json::json!({})),
            ],
        },
        Chain {
            id: "web-image".into(),
            name: "Web Image".into(),
            steps: vec![
                ChainStep::Take { capability: caps::REGION },
                run("core.image.resize", serde_json::json!({ "percent": 50 })),
                run("core.save", serde_json::json!({ "format": "webp", "directory": "~/Downloads" })),
            ],
        },
        Chain {
            id: "extract-table".into(),
            name: "Extract Table".into(),
            steps: vec![
                ChainStep::Take { capability: caps::TABLE },
                run("core.table.to-csv", serde_json::json!({})),
                run("core.save", serde_json::json!({ "format": "csv" })),
            ],
        },
    ]
}
