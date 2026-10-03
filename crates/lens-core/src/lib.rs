//! Arcade Lens core.
//!
//! Platform-independent heart of Arcade Lens: the data model for selections
//! and findings, the progressive capability engine, the action/recognizer
//! plugin registry, ranking, safety policy and typed action chains.
//!
//! Nothing in this crate captures the screen or draws UI; platform shells
//! provide a [`host::Host`] and feed [`selection::Selection`]s in.

pub mod action;
pub mod builder;
pub mod cancel;
pub mod capability;
pub mod chain;
pub mod color;
pub mod engine;
pub mod error;
pub mod finding;
pub mod geometry;
pub mod host;
pub mod palette;
pub mod recognizer;
pub mod registry;
pub mod selection;
pub mod settings;
pub mod usage;
pub mod value;

pub use action::{Action, ActionContext, ActionDescriptor, ActionGroup, ActionOutcome, Choice, Effects, Item, Produces, SafetyClass};
pub use capability::{caps, Capability};
pub use engine::{Analysis, AnalysisEvent, AnalysisReport, Engine};
pub use error::{LensError, Result};
pub use finding::{Detection, Finding, FindingId};
pub use recognizer::{Cost, RecognizeContext, Recognizer, RecognizerDescriptor};
pub use registry::{PluginManifest, Registry};
pub use selection::{Selection, Signals};
pub use settings::Settings;
pub use value::Value;
