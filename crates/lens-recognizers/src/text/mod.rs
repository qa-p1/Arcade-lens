//! Structured-text recognizers.
//!
//! Every recognizer here consumes `text` (from OCR) and `qr-code` (decoded
//! payloads), so content inside a QR code gets exactly the same treatment as
//! visible text. Each is a pure function over the text, which keeps them
//! deterministic and easy to test.

pub mod address;
pub mod code;
pub mod command;
pub mod contact;
pub mod context;
pub mod datetime;
pub mod error;
pub mod geo;
pub mod ids;
pub mod money;
pub mod network;
pub mod path;
pub mod secret;
pub mod table;
pub mod units;
pub mod url;

use std::ops::Range;

use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::TextLayout;
use lens_core::{caps, Capability, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

pub struct TextInput<'a> {
    pub text: &'a str,
    pub layout: Option<&'a TextLayout>,
}

type TextFn = fn(&TextInput, &RecognizeContext) -> Vec<Detection>;

/// Adapts a pure text function into a [`Recognizer`].
pub struct TextRecognizer {
    id: &'static str,
    produces: Vec<Capability>,
    cost: Cost,
    detect: TextFn,
}

impl TextRecognizer {
    pub fn new(id: &'static str, produces: Vec<Capability>, cost: Cost, detect: TextFn) -> Self {
        Self { id, produces, cost, detect }
    }
}

impl Recognizer for TextRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: self.id.into(), consumes: vec![caps::TEXT, caps::QR_CODE], produces: self.produces.clone(), cost: self.cost }
    }

    fn should_run(&self, input: &Finding, _cx: &RecognizeContext) -> bool {
        input.value.as_text().is_some_and(|t| !t.trim().is_empty())
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let text = input.value.as_text().unwrap_or_default();
        let layout = match &input.value {
            Value::Text(t) => t.layout.as_deref(),
            _ => None,
        };
        cx.cancel.check()?;
        Ok((self.detect)(&TextInput { text: &text, layout }, cx))
    }
}

/// All built-in text recognizers.
pub fn recognizers() -> Vec<TextRecognizer> {
    use caps::*;
    use Cost::*;
    vec![
        TextRecognizer::new("core.text.url", vec![URL], Trivial, url::detect),
        TextRecognizer::new("core.text.contact", vec![EMAIL, PHONE], Trivial, contact::detect),
        TextRecognizer::new("core.text.network", vec![IP_ADDRESS, DOMAIN], Trivial, network::detect),
        TextRecognizer::new("core.text.path", vec![PATH], Trivial, path::detect),
        TextRecognizer::new("core.text.ids", vec![HASH, UUID], Trivial, ids::detect),
        TextRecognizer::new("core.text.datetime", vec![DATE_TIME, TIMECODE], Trivial, datetime::detect),
        TextRecognizer::new("core.text.geo", vec![COORDINATES], Trivial, geo::detect),
        TextRecognizer::new("core.text.money", vec![CURRENCY], Trivial, money::detect),
        TextRecognizer::new("core.text.units", vec![QUANTITY], Trivial, units::detect),
        TextRecognizer::new("core.text.address", vec![ADDRESS], Trivial, address::detect),
        TextRecognizer::new("core.text.secret", vec![SECRET], Trivial, secret::detect),
        TextRecognizer::new("core.text.command", vec![COMMAND], Trivial, command::detect),
        TextRecognizer::new("core.text.code", vec![CODE], Cheap, code::detect),
        TextRecognizer::new("core.text.error", vec![ERROR], Cheap, error::detect),
        TextRecognizer::new("core.text.table", vec![TABLE], Cheap, table::detect),
        TextRecognizer::new("core.text.git-commit", vec![GIT_COMMIT], Trivial, context::git_commits),
        TextRecognizer::new("core.text.document", vec![DOCUMENT], Trivial, context::document),
        TextRecognizer::new("core.text.subtitle", vec![SUBTITLE], Trivial, context::subtitle),
    ]
}

pub(crate) fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Strips trailing sentence punctuation and unbalanced closing brackets from
/// a token found in prose: `(see https://x.com/a_(b)).` → `https://x.com/a_(b)`.
pub(crate) fn trim_token(s: &str) -> &str {
    let mut s = s;
    loop {
        let before = s.len();
        s = s.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"', '`', '>', '*']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            if s.ends_with(close) && s.matches(close).count() > s.matches(open).count() {
                s = &s[..s.len() - 1];
            }
        }
        if s.len() == before {
            return s;
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;

    use image::RgbaImage;
    use lens_core::cancel::CancelToken;
    use lens_core::recognizer::{Environment, NullEnvironment};
    use lens_core::{RecognizeContext, Selection, Settings, Signals};

    pub fn cx() -> RecognizeContext {
        cx_with(Settings::default(), Arc::new(NullEnvironment))
    }

    pub fn cx_with(settings: Settings, env: Arc<dyn Environment>) -> RecognizeContext {
        let img = RgbaImage::new(4, 4);
        RecognizeContext {
            signals: Arc::new(Signals::compute(&img)),
            selection: Arc::new(Selection::from_image(img)),
            cancel: CancelToken::new(),
            env,
            settings: Arc::new(settings),
        }
    }

    pub fn texts(f: super::TextFn, text: &str) -> Vec<String> {
        f(&super::TextInput { text, layout: None }, &cx()).into_iter().map(|d| d.value.as_text().unwrap().into_owned()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming() {
        assert_eq!(trim_token("https://x.com/a_(b))."), "https://x.com/a_(b)");
        assert_eq!(trim_token("x.com/a\"),"), "x.com/a");
        assert_eq!(trim_token("foo"), "foo");
    }
}
