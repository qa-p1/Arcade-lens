//! Findings: one interpretation of (part of) a selection.

use std::ops::Range;

use serde::Serialize;

use crate::capability::Capability;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct FindingId(pub u32);

/// A recognized interpretation. A selection usually has many findings at
/// once (region + image + QR + URL); none of them excludes the others.
#[derive(Debug, Clone)]
pub struct Finding {
    pub id: FindingId,
    pub capability: Capability,
    pub value: Value,
    /// Recognizer confidence in `[0, 1]`. Used for ranking, not shown raw.
    pub confidence: f32,
    pub recognizer: String,
    /// The finding this one was derived from (e.g. URL ← OCR text ← region).
    pub derived_from: Option<FindingId>,
    /// Byte range inside the parent finding's text, when applicable.
    pub span: Option<Range<usize>>,
    /// Short human-readable facts shown in the palette header
    /// (e.g. `("HEX", "#18181B")`).
    pub details: Vec<(String, String)>,
}

impl Finding {
    /// Short label for list display.
    pub fn summary(&self) -> String {
        let text = match &self.value {
            Value::Secret(s) => s.masked.clone(),
            Value::Error(e) => e.headline.clone(),
            v => v.as_text().map(|t| t.into_owned()).unwrap_or_else(|| format!("{v:?}")),
        };
        let first_line = text.lines().next().unwrap_or_default();
        let mut s: String = first_line.chars().take(60).collect();
        if s.len() < first_line.len() || text.lines().nth(1).is_some() {
            s.push('…');
        }
        s
    }
}

/// What a recognizer returns; the engine assigns ids and provenance.
#[derive(Debug, Clone)]
pub struct Detection {
    pub capability: Capability,
    pub value: Value,
    pub confidence: f32,
    pub span: Option<Range<usize>>,
    pub details: Vec<(String, String)>,
}

impl Detection {
    pub fn new(capability: Capability, value: Value) -> Self {
        Self { capability, value, confidence: 1.0, span: None, details: Vec::new() }
    }

    pub fn confidence(mut self, c: f32) -> Self {
        self.confidence = c.clamp(0.0, 1.0);
        self
    }

    pub fn span(mut self, r: Range<usize>) -> Self {
        self.span = Some(r);
        self
    }

    pub fn detail(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.details.push((k.into(), v.into()));
        self
    }
}
