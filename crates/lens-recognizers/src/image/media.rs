//! Regions inside a video player become media frames.

use lens_core::recognizer::RecognizerDescriptor;
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result};

use crate::text::context::{window_under, MEDIA_APPS};

pub struct MediaFrameRecognizer;

impl Recognizer for MediaFrameRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.media-frame".into(), consumes: vec![caps::REGION], produces: vec![caps::MEDIA_FRAME], cost: Cost::Trivial }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        window_under(cx).is_some_and(|w| {
            let hay = format!("{} {}", w.app_name.as_deref().unwrap_or_default(), w.title).to_lowercase();
            MEDIA_APPS.iter().any(|m| hay.contains(m))
        })
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let player = window_under(cx).map(|w| w.app_name.clone().unwrap_or_else(|| w.title.clone())).unwrap_or_default();
        Ok(vec![Detection::new(caps::MEDIA_FRAME, input.value.clone()).confidence(0.8).detail("Player", player)])
    }
}
