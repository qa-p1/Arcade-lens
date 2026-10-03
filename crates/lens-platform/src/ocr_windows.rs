//! Windows.Media.Ocr: the OCR engine built into Windows 10/11.

use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::value::{TextLayout, TextLine, Word};
use lens_core::{LensError, Result};
use lens_recognizers::ocr::OcrEngine;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine as WinOcr;
use windows::Storage::Streams::DataWriter;

pub struct WindowsOcr;

fn e(err: windows::core::Error) -> LensError {
    LensError::Failed(format!("Windows OCR: {err}"))
}

impl WindowsOcr {
    pub fn available() -> bool {
        WinOcr::TryCreateFromUserProfileLanguages().is_ok()
    }
}

impl OcrEngine for WindowsOcr {
    fn name(&self) -> &str {
        "Windows OCR"
    }

    fn recognize(&self, image: &RgbaImage, languages: &[String], cancel: &CancelToken) -> Result<TextLayout> {
        let engine = languages
            .iter()
            .find_map(|l| Language::CreateLanguage(&HSTRING::from(l.as_str())).ok().and_then(|lang| WinOcr::TryCreateFromLanguage(&lang).ok()))
            .map_or_else(|| WinOcr::TryCreateFromUserProfileLanguages().map_err(e), Ok)?;
        let max = WinOcr::MaxImageDimension().map_err(e)?;
        if image.width() > max || image.height() > max {
            return Err(LensError::InvalidInput(format!("selection exceeds {max}px, the Windows OCR limit")));
        }
        let mut bgra = image.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new().map_err(e)?;
        writer.WriteBytes(&bgra).map_err(e)?;
        let buffer = writer.DetachBuffer().map_err(e)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, image.width() as i32, image.height() as i32).map_err(e)?;
        cancel.check()?;
        let result = engine.RecognizeAsync(&bitmap).map_err(e)?.join().map_err(e)?;
        let mut layout = TextLayout::default();
        let lines = result.Lines().map_err(e)?;
        for i in 0..lines.Size().map_err(e)? {
            let line = lines.GetAt(i).map_err(e)?;
            let words_view = line.Words().map_err(e)?;
            let mut words = Vec::new();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for j in 0..words_view.Size().map_err(e)? {
                let w = words_view.GetAt(j).map_err(e)?;
                let r = w.BoundingRect().map_err(e)?;
                x0 = x0.min(r.X);
                y0 = y0.min(r.Y);
                x1 = x1.max(r.X + r.Width);
                y1 = y1.max(r.Y + r.Height);
                // Windows OCR does not expose per-word confidence.
                words.push(Word {
                    text: w.Text().map_err(e)?.to_string(),
                    bbox: Rect::new(r.X as i32, r.Y as i32, r.Width as u32, r.Height as u32),
                    confidence: 0.9,
                });
            }
            let bbox = if words.is_empty() { Rect::default() } else { Rect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32) };
            layout.lines.push(TextLine { text: line.Text().map_err(e)?.to_string(), bbox, words });
        }
        Ok(layout)
    }
}
