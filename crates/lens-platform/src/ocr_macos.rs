//! Apple Vision text recognition (VNRecognizeTextRequest).

use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::value::{TextLayout, TextLine, Word};
use lens_core::{LensError, Result};
use lens_recognizers::ocr::OcrEngine;
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_core_foundation::CFData;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo};
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

pub struct VisionOcr;

fn cg_image(img: &RgbaImage) -> Option<objc2_core_foundation::CFRetained<CGImage>> {
    let data = CFData::from_bytes(img.as_raw());
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    let space = CGColorSpace::new_device_rgb()?;
    let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
    // SAFETY: decode is null (allowed); all other arguments describe `data` exactly.
    unsafe {
        CGImage::new(
            img.width() as usize,
            img.height() as usize,
            8,
            32,
            img.width() as usize * 4,
            Some(&space),
            info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}

impl OcrEngine for VisionOcr {
    fn name(&self) -> &str {
        "Apple Vision"
    }

    fn recognize(&self, image: &RgbaImage, languages: &[String], cancel: &CancelToken) -> Result<TextLayout> {
        let cg = cg_image(image).ok_or_else(|| LensError::Failed("could not create CGImage".into()))?;
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        if !languages.is_empty() {
            let langs: Vec<Retained<NSString>> = languages.iter().map(|l| NSString::from_str(l)).collect();
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&langs));
        }
        cancel.check()?;
        // SAFETY: an empty options dictionary is valid for initWithCGImage:options:.
        let handler = unsafe { VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &cg, &NSDictionary::new()) };
        let as_request: &VNRequest = &request;
        handler.performRequests_error(&NSArray::from_slice(&[as_request])).map_err(|e| LensError::Failed(format!("Vision: {}", e.localizedDescription())))?;
        let (w, h) = (image.width() as f64, image.height() as f64);
        let mut layout = TextLayout::default();
        for obs in request.results().map(|r| r.to_vec()).unwrap_or_default() {
            let Some(best) = obs.topCandidates(1).firstObject() else { continue };
            // SAFETY: boundingBox is a plain property read; it is normalized with a bottom-left origin.
            let bb = unsafe { obs.boundingBox() };
            let rect =
                Rect::new((bb.origin.x * w) as i32, ((1.0 - bb.origin.y - bb.size.height) * h) as i32, (bb.size.width * w) as u32, (bb.size.height * h) as u32);
            let text = best.string().to_string();
            let words = text.split_whitespace().map(|t| Word { text: t.to_string(), bbox: rect, confidence: best.confidence() }).collect();
            layout.lines.push(TextLine { text, bbox: rect, words });
        }
        Ok(layout)
    }
}
