//! Registration of recognizers, actions and capabilities.
//!
//! Everything — including first-party features — registers through
//! [`Registry::register_plugin`], so the core has no hard-coded knowledge of
//! any recognizer or action. Each plugin declares the side effects it is
//! allowed to perform; actions declaring effects outside that set are
//! rejected at registration time.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::action::{Action, Effects};
use crate::capability::{Capability, CapabilityGraph};
use crate::error::{LensError, Result};
use crate::recognizer::Recognizer;

/// Version of the plugin interface. Plugins built for a different major
/// version are refused rather than loaded in a half-working state.
pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Reverse-DNS-ish id; every action/recognizer id must start with `<id>.`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    /// The maximum set of side effects this plugin's actions may declare.
    pub permissions: Effects,
}

impl PluginManifest {
    pub fn first_party(id: &str, name: &str) -> Self {
        Self { id: id.into(), name: name.into(), version: env!("CARGO_PKG_VERSION").into(), api_version: PLUGIN_API_VERSION, permissions: Effects::all() }
    }
}

#[derive(Default)]
pub struct Registry {
    graph: CapabilityGraph,
    recognizers: Vec<Arc<dyn Recognizer>>,
    actions: Vec<Arc<dyn Action>>,
    action_index: HashMap<String, usize>,
    plugins: Vec<PluginManifest>,
}

impl Registry {
    pub fn new() -> Self {
        Self { graph: CapabilityGraph::with_builtins(), ..Default::default() }
    }

    pub fn register_plugin(&mut self, manifest: PluginManifest, register: impl FnOnce(&mut PluginRegistrar)) -> Result<()> {
        if manifest.api_version != PLUGIN_API_VERSION {
            return Err(LensError::InvalidInput(format!(
                "plugin {} targets API v{}, this build provides v{}",
                manifest.id, manifest.api_version, PLUGIN_API_VERSION
            )));
        }
        if self.plugins.iter().any(|p| p.id == manifest.id) {
            return Err(LensError::InvalidInput(format!("plugin {} is already registered", manifest.id)));
        }
        let mut r = PluginRegistrar { manifest: &manifest, recognizers: Vec::new(), actions: Vec::new(), edges: Vec::new(), errors: Vec::new() };
        register(&mut r);
        let PluginRegistrar { recognizers, actions, edges, errors, .. } = r;
        if !errors.is_empty() {
            return Err(LensError::InvalidInput(errors.join("; ")));
        }
        for a in &actions {
            let id = &a.descriptor().id;
            if self.action_index.contains_key(id) {
                return Err(LensError::InvalidInput(format!("duplicate action id {id}")));
            }
        }
        // Validated as a whole: a rejected plugin registers nothing.
        for (child, parent) in edges {
            self.graph.add(child, parent);
        }
        self.recognizers.extend(recognizers);
        for a in actions {
            self.action_index.insert(a.descriptor().id.clone(), self.actions.len());
            self.actions.push(a);
        }
        self.plugins.push(manifest);
        Ok(())
    }

    pub fn graph(&self) -> &CapabilityGraph {
        &self.graph
    }

    pub fn recognizers(&self) -> &[Arc<dyn Recognizer>] {
        &self.recognizers
    }

    pub fn actions(&self) -> &[Arc<dyn Action>] {
        &self.actions
    }

    pub fn action(&self, id: &str) -> Option<&Arc<dyn Action>> {
        self.action_index.get(id).map(|&i| &self.actions[i])
    }

    pub fn plugins(&self) -> &[PluginManifest] {
        &self.plugins
    }
}

pub struct PluginRegistrar<'m> {
    manifest: &'m PluginManifest,
    recognizers: Vec<Arc<dyn Recognizer>>,
    actions: Vec<Arc<dyn Action>>,
    edges: Vec<(Capability, Capability)>,
    errors: Vec<String>,
}

impl PluginRegistrar<'_> {
    fn check_namespace(&mut self, kind: &str, id: &str) -> bool {
        let ok = id.strip_prefix(&self.manifest.id).is_some_and(|rest| rest.starts_with('.'));
        if !ok {
            self.errors.push(format!("{kind} id {id} must be namespaced under {}.", self.manifest.id));
        }
        ok
    }

    pub fn recognizer(&mut self, r: impl Recognizer + 'static) {
        let id = r.descriptor().id;
        if self.check_namespace("recognizer", &id) {
            self.recognizers.push(Arc::new(r));
        }
    }

    pub fn action(&mut self, a: impl Action + 'static) {
        self.action_arc(Arc::new(a));
    }

    pub fn action_arc(&mut self, a: Arc<dyn Action>) {
        let d = a.descriptor();
        if !self.check_namespace("action", &d.id) {
            return;
        }
        let excess = d.effects - self.manifest.permissions;
        if !excess.is_empty() {
            self.errors.push(format!("action {} needs {:?}, which plugin {} did not declare", d.id, excess, self.manifest.id));
            return;
        }
        self.actions.push(a);
    }

    /// Declares that `child` is a kind of `parent` (e.g. `isbn` is a `barcode`).
    pub fn capability(&mut self, child: Capability, parent: Capability) {
        self.edges.push((child, parent));
    }
}
