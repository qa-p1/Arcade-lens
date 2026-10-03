//! Detects when the selection is (almost exactly) an OS window.

use lens_core::recognizer::RecognizerDescriptor;
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

/// Minimum overlap (intersection over union) to call it "the window".
const MIN_IOU: f64 = 0.9;

pub struct WindowRecognizer;

impl Recognizer for WindowRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.window".into(), consumes: vec![caps::REGION], produces: vec![caps::WINDOW], cost: Cost::Trivial }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        !cx.selection.context.windows.is_empty()
    }

    fn recognize(&self, _input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let sel = cx.selection.rect;
        // Front-most window wins ties: windows are ordered front to back.
        let best = cx.selection.context.windows.iter().map(|w| (w.rect.iou(&sel), w)).filter(|(iou, _)| *iou >= MIN_IOU).fold(
            None,
            |best: Option<(f64, _)>, (iou, w)| match best {
                Some((b, _)) if b >= iou => best,
                _ => Some((iou, w)),
            },
        );
        Ok(best
            .map(|(iou, w)| {
                let mut d = Detection::new(caps::WINDOW, Value::Window(w.clone())).confidence(iou as f32).detail("Window", w.title.clone());
                if let Some(app) = &w.app_name {
                    d = d.detail("Application", app.clone());
                }
                d
            })
            .into_iter()
            .collect())
    }
}
