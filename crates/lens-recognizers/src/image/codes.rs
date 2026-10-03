//! QR codes and barcodes (local decoding via rxing).
//!
//! QR payloads are emitted as `qr-code` findings whose text flows back into
//! the text recognizers, so a QR containing a URL also yields URL actions.

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use lens_core::recognizer::RecognizerDescriptor;
use lens_core::value::BarcodeValue;
use lens_core::{caps, Cost, Detection, Finding, LensError, RecognizeContext, Recognizer, Result, Value};
use rxing::BarcodeFormat;

pub struct CodeRecognizer;

fn label(f: &BarcodeFormat) -> &'static str {
    match f {
        BarcodeFormat::QR_CODE => "QR Code",
        BarcodeFormat::MICRO_QR_CODE => "Micro QR",
        BarcodeFormat::RECTANGULAR_MICRO_QR_CODE => "rMQR",
        BarcodeFormat::AZTEC => "Aztec",
        BarcodeFormat::DATA_MATRIX => "Data Matrix",
        BarcodeFormat::PDF_417 => "PDF417",
        BarcodeFormat::MAXICODE => "MaxiCode",
        BarcodeFormat::EAN_8 => "EAN-8",
        BarcodeFormat::EAN_13 => "EAN-13",
        BarcodeFormat::UPC_A => "UPC-A",
        BarcodeFormat::UPC_E => "UPC-E",
        BarcodeFormat::CODE_39 => "Code 39",
        BarcodeFormat::CODE_93 => "Code 93",
        BarcodeFormat::CODE_128 => "Code 128",
        BarcodeFormat::CODABAR => "Codabar",
        BarcodeFormat::ITF => "ITF",
        BarcodeFormat::RSS_14 => "GS1 DataBar",
        BarcodeFormat::RSS_EXPANDED => "GS1 DataBar Expanded",
        _ => "Barcode",
    }
}

fn is_qr(f: &BarcodeFormat) -> bool {
    matches!(f, BarcodeFormat::QR_CODE | BarcodeFormat::MICRO_QR_CODE | BarcodeFormat::RECTANGULAR_MICRO_QR_CODE)
}

/// Adds a white quiet zone (users often select tightly around a code) and
/// upscales tiny captures so modules span several pixels.
fn prepare(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let pad = (w.max(h) / 8).max(8);
    let mut out = RgbaImage::from_pixel(w + 2 * pad, h + 2 * pad, Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut out, img, pad as i64, pad as i64);
    if w.max(h) < 240 {
        let s = (240 / w.max(h).max(1)).clamp(2, 4);
        out = image::imageops::resize(&out, out.width() * s, out.height() * s, FilterType::Nearest);
    }
    out
}

impl Recognizer for CodeRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "core.codes".into(), consumes: vec![caps::REGION], produces: vec![caps::QR_CODE, caps::BARCODE], cost: Cost::Expensive }
    }

    fn should_run(&self, _input: &Finding, cx: &RecognizeContext) -> bool {
        let s = &cx.signals;
        !s.is_uniform() && s.width.min(s.height) >= 16 && s.edge_density > 0.01
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        let Some(img) = input.value.as_image() else { return Ok(vec![]) };
        let prepared = DynamicImage::ImageRgba8(prepare(img));
        cx.cancel.check()?;
        let results = match rxing::helpers::detect_multiple_in_image(prepared) {
            Ok(r) => r,
            // "Not found" is the normal outcome for most selections.
            Err(rxing::Exceptions::NotFoundException(_)) => return Ok(vec![]),
            Err(e) => return Err(LensError::Failed(format!("barcode decoder: {e}"))),
        };
        let mut seen = std::collections::HashSet::new();
        Ok(results
            .into_iter()
            .filter(|r| seen.insert((r.getBarcodeFormat().to_string(), r.getText().to_string())))
            .map(|r| {
                let f = r.getBarcodeFormat();
                let cap = if is_qr(f) { caps::QR_CODE } else { caps::BARCODE };
                let payload = r.getText().to_string();
                let kind = if payload.starts_with("WIFI:") {
                    "Wi-Fi network"
                } else if payload.starts_with("BEGIN:VCARD") {
                    "Contact card"
                } else {
                    ""
                };
                let mut d = Detection::new(cap, Value::Barcode(BarcodeValue { format: label(f).into(), payload })).confidence(0.99).detail("Format", label(f));
                if !kind.is_empty() {
                    d = d.detail("Contains", kind);
                }
                d
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lens_core::cancel::CancelToken;
    use lens_core::recognizer::NullEnvironment;
    use lens_core::value::ImageValue;
    use lens_core::{FindingId, Selection, Settings, Signals};

    use super::*;

    fn run(img: RgbaImage) -> Vec<Detection> {
        let sel = Selection::from_image(img.clone());
        let cx = RecognizeContext {
            signals: Arc::new(Signals::compute(&img)),
            selection: Arc::new(sel),
            cancel: CancelToken::new(),
            env: Arc::new(NullEnvironment),
            settings: Arc::new(Settings::default()),
        };
        let f = Finding {
            id: FindingId(0),
            capability: caps::REGION,
            value: Value::Image(ImageValue { image: Arc::new(img), origin: None }),
            confidence: 1.0,
            recognizer: "test".into(),
            derived_from: None,
            span: None,
            details: vec![],
        };
        assert!(CodeRecognizer.should_run(&f, &cx));
        CodeRecognizer.recognize(&f, &cx).unwrap()
    }

    pub fn qr_image(data: &str, module_px: u32, quiet: bool) -> RgbaImage {
        let code = qrcode::QrCode::new(data.as_bytes()).unwrap();
        let luma = code.render::<image::Luma<u8>>().quiet_zone(quiet).module_dimensions(module_px, module_px).build();
        DynamicImage::ImageLuma8(luma).to_rgba8()
    }

    #[test]
    fn decodes_tightly_cropped_small_qr() {
        let found = run(qr_image("https://example.com/lens", 3, false));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].capability, caps::QR_CODE);
        assert_eq!(found[0].value.as_text().unwrap(), "https://example.com/lens");
    }

    #[test]
    fn nothing_in_noise() {
        let img = RgbaImage::from_fn(120, 120, |x, y| Rgba([((x * 31) ^ (y * 17)) as u8, (x * y) as u8, 90, 255]));
        assert!(run(img).is_empty());
    }
}
