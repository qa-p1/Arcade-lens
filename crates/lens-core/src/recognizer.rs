//! Recognizer interface.
//!
//! Recognizers form a dataflow graph: each declares which capabilities it
//! consumes, and the engine feeds it every finding with one of those
//! capabilities. OCR consumes `region` and produces `text`; the URL
//! recognizer consumes `text` (and `qr-code`, so decoded QR payloads flow
//! back into text recognition). No recognizer ever branches on "the" type
//! of a selection.

use std::path::PathBuf;
use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::capability::Capability;
use crate::error::Result;
use crate::finding::{Detection, Finding};
use crate::selection::{Selection, Signals};
use crate::settings::Settings;
use crate::value::PathKind;

/// Rough cost class; cheaper recognizers are scheduled first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cost {
    /// Microseconds: regex over short text, arithmetic on signals.
    Trivial,
    /// Low milliseconds: full-image passes, small decoders.
    Cheap,
    /// Tens to hundreds of milliseconds: OCR, multi-format barcode scans.
    Expensive,
}

#[derive(Debug, Clone)]
pub struct RecognizerDescriptor {
    pub id: String,
    pub consumes: Vec<Capability>,
    pub produces: Vec<Capability>,
    pub cost: Cost,
}

pub trait Recognizer: Send + Sync {
    fn descriptor(&self) -> RecognizerDescriptor;

    /// Cheap gate evaluated before scheduling (e.g. skip OCR on a flat color).
    fn should_run(&self, _input: &Finding, _cx: &RecognizeContext) -> bool {
        true
    }

    /// Must check `cx.cancel` periodically if it can take more than a few ms.
    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>>;
}

/// Local, read-only facts about the machine a recognizer may consult.
pub trait Environment: Send + Sync {
    fn path_kind(&self, path: &str) -> Option<PathKind>;
    fn home_dir(&self) -> Option<PathBuf>;
}

/// Environment that knows nothing; used in tests.
pub struct NullEnvironment;

impl Environment for NullEnvironment {
    fn path_kind(&self, _path: &str) -> Option<PathKind> {
        None
    }
    fn home_dir(&self) -> Option<PathBuf> {
        None
    }
}

/// The real local filesystem.
pub struct LocalEnvironment;

impl Environment for LocalEnvironment {
    fn path_kind(&self, path: &str) -> Option<PathKind> {
        Some(match std::fs::metadata(path) {
            Ok(m) if m.is_dir() => PathKind::Directory,
            Ok(_) => PathKind::File,
            Err(_) => PathKind::Missing,
        })
    }
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
    }
}

#[derive(Clone)]
pub struct RecognizeContext {
    pub selection: Arc<Selection>,
    pub signals: Arc<Signals>,
    pub cancel: CancelToken,
    pub env: Arc<dyn Environment>,
    pub settings: Arc<Settings>,
}
