//! Classifies regions that are primarily a picture or an icon.

use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::ImageValue;
use lens_core::{caps, Cost, Detection, Finding, RecognizeContext, Recognizer, Result, Value};

pub struct ImageKindRecognizer;

impl Recognizer for ImageKindRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.image-kind".into(), consumes: vec![caps::REGION], produces: vec![caps::IMAGE, caps::ICON], cost: Cost::Trivial }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        !cx.signals.is_uniform() && cx.signals.width >= 8 && cx.signals.height >= 8
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Value::Image(img) = &input.value else { return Ok(vec![]) };
        let s = &cx.signals;
        let value = || Value::Image(ImageValue { image: img.image.clone(), origin: img.origin });
        let mut out = Vec::new();
        let aspect = s.width as f32 / s.height as f32;
        if (12..=256).contains(&s.width) && (12..=256).contains(&s.height) && (0.7..=1.43).contains(&aspect) && s.distinct_colors >= 3 {
            out.push(Detection::new(caps::ICON, value()).confidence(0.5).detail("Size", format!("{} × {} px", s.width, s.height)));
        }
        // Photographs and illustrations: many distinct colors, comparatively few hard edges.
        let sampled = (s.pixel_count()).min(40_000) as f32;
        let color_richness = s.distinct_colors as f32 / sampled.max(1.0);
        if s.pixel_count() >= 64 * 64 && s.distinct_colors >= 600 && s.edge_density < 0.3 {
            let confidence = (0.5 + color_richness * 2.0).min(0.85);
            out.push(Detection::new(caps::IMAGE, value()).confidence(confidence));
        }
        Ok(out)
    }
}
