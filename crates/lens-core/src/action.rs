//! Action providers.
//!
//! Actions are registered independently of recognizers. Each declares the
//! capabilities it accepts, what it produces (for chaining), its side
//! effects (for safety), and an execution handler. The URL recognizer knows
//! nothing about "Open", "Generate QR" or "Send to Phone"; those are separate
//! providers that happen to accept `url`.

use std::borrow::Cow;
use std::sync::Arc;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::Result;
use crate::finding::Finding;
use crate::host::{Host, HostFeatures};
use crate::selection::Selection;
use crate::settings::Settings;
use crate::value::Value;

bitflags! {
    /// Side effects an action may have. Used for safety classification,
    /// plugin permission checks and chain confirmation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
    pub struct Effects: u32 {
        const CLIPBOARD        = 1 << 0;
        const WRITES_FILES     = 1 << 1;
        const OVERWRITES_FILES = 1 << 2;
        const DELETES_FILES    = 1 << 3;
        /// Contacts a remote host (opening a web page counts).
        const NETWORK          = 1 << 4;
        /// Sends selection content (text, pixels, a query) to a third party.
        const UPLOADS_CONTENT  = 1 << 5;
        const SENDS_TO_DEVICE  = 1 << 6;
        const LAUNCHES_APP     = 1 << 7;
        const EXECUTES_COMMAND = 1 << 8;
        const PRIVILEGED       = 1 << 9;
        const WINDOW_CONTROL   = 1 << 10;
        /// Keeps content around after Lens closes (pins, palettes).
        const PERSISTS         = 1 << 11;
    }
}

impl Effects {
    /// Effects that move content off this machine.
    pub const OUTBOUND: Effects = Effects::NETWORK.union(Effects::UPLOADS_CONTENT).union(Effects::SENDS_TO_DEVICE);
    /// Effects that can cause damage that is hard to undo.
    pub const DANGEROUS: Effects = Effects::EXECUTES_COMMAND
        .union(Effects::DELETES_FILES)
        .union(Effects::OVERWRITES_FILES)
        .union(Effects::PRIVILEGED);

    pub fn safety_class(&self) -> SafetyClass {
        if self.intersects(Effects::DANGEROUS) {
            SafetyClass::Dangerous
        } else if self.intersects(Effects::OUTBOUND) {
            SafetyClass::External
        } else if self.is_empty() {
            SafetyClass::Pure
        } else {
            SafetyClass::Local
        }
    }
}

/// How an action is presented and gated. Ordered from safest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SafetyClass {
    /// Transforms data in memory only.
    Pure,
    /// Touches the local machine (clipboard, files, apps) without network.
    Local,
    /// Contacts the network or another device. Visually marked.
    External,
    /// Executes, deletes, overwrites or escalates. Always confirmed.
    Dangerous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionGroup {
    Copy,
    Open,
    Transform,
    Save,
    Share,
    Search,
    Inspect,
    Edit,
    System,
}

/// What an action outputs, for chain type-checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Produces {
    /// A terminal action (Open, Pin): nothing to chain after it.
    Nothing,
    /// Passes its input through unchanged (Copy), so chains can continue.
    Same,
    /// A new value of the given capability.
    Capability(Capability),
}

#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: String,
    pub description: String,
    pub default: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ActionDescriptor {
    /// Namespaced, stable id: `<provider>.<name>`, e.g. `core.url.open`.
    pub id: String,
    pub label: String,
    pub icon: String,
    pub group: ActionGroup,
    /// Palette matching is exact on these; chain matching honors "is-a".
    pub accepts: Vec<Capability>,
    pub produces: Produces,
    /// Base relevance in `[0, 100]` relative to other actions on the same input.
    pub priority: i32,
    /// Default single-key shortcut while the palette is open.
    pub key: Option<char>,
    pub effects: Effects,
    /// Platform services that must be available for the action to show up.
    pub requires: HostFeatures,
    /// False for chain-only building blocks.
    pub in_palette: bool,
    pub params: Vec<ParamSpec>,
}

impl ActionDescriptor {
    pub fn safety(&self) -> SafetyClass {
        self.effects.safety_class()
    }
}

/// An action input or output: a typed value tagged with its capability.
#[derive(Debug, Clone)]
pub struct Item {
    pub capability: Capability,
    pub value: Value,
}

impl Item {
    pub fn new(capability: Capability, value: Value) -> Self {
        Self { capability, value }
    }
}

impl From<&Finding> for Item {
    fn from(f: &Finding) -> Self {
        Item { capability: f.capability.clone(), value: f.value.clone() }
    }
}

pub type Params = serde_json::Map<String, serde_json::Value>;

pub struct ActionContext<'a> {
    pub host: &'a dyn Host,
    pub settings: &'a Settings,
    pub selection: Option<&'a Selection>,
    pub params: &'a Params,
}

impl ActionContext<'_> {
    pub fn param_str(&self, name: &str) -> Option<&str> {
        self.params.get(name).and_then(|v| v.as_str())
    }
    pub fn param_f64(&self, name: &str) -> Option<f64> {
        self.params.get(name).and_then(|v| v.as_f64())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActionOutcome {
    /// Value for the next chain step (or to display, for Pure actions).
    pub output: Option<Item>,
    /// Short confirmation for the user ("Copied", "Saved to …").
    pub message: Option<String>,
}

impl ActionOutcome {
    pub fn done(message: impl Into<String>) -> Self {
        Self { output: None, message: Some(message.into()) }
    }
    pub fn output(item: Item) -> Self {
        Self { output: Some(item), message: None }
    }
    pub fn with_message(mut self, m: impl Into<String>) -> Self {
        self.message = Some(m.into());
        self
    }
}

/// Request shown to the user before a dangerous or sensitive action runs.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmRequest {
    pub title: String,
    /// The exact content that will be executed, uploaded or written.
    pub subject: String,
    pub reasons: Vec<String>,
}

pub trait Action: Send + Sync {
    fn descriptor(&self) -> &ActionDescriptor;

    /// Extra applicability checks beyond capability matching (e.g. "path
    /// exists", "window supports always-on-top").
    fn applies(&self, _input: &Finding, _host: HostFeatures) -> bool {
        true
    }

    /// What will leave the machine or be executed, shown before running
    /// External and Dangerous actions (e.g. the cleaned search query).
    fn preview(&self, _input: &Item, _settings: &Settings) -> Option<String> {
        None
    }

    /// Whether the user must confirm. Defaults to "always for Dangerous".
    fn confirmation(&self, input: &Item, settings: &Settings) -> Option<ConfirmRequest> {
        let d = self.descriptor();
        (d.safety() == SafetyClass::Dangerous).then(|| ConfirmRequest {
            title: d.label.clone(),
            subject: self
                .preview(input, settings)
                .or_else(|| input.value.as_text().map(Cow::into_owned))
                .unwrap_or_default(),
            reasons: vec!["This action can make changes that are hard to undo.".into()],
        })
    }

    fn execute(&self, input: &Item, cx: &ActionContext) -> Result<ActionOutcome>;
}

pub type ActionRef = Arc<dyn Action>;
