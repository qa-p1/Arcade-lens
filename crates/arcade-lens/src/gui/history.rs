//! Opt-in selection history (off by default). Each entry is a thumbnail and
//! a JSON summary of what was recognized; secrets are stored masked.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use lens_core::{Finding, Value};

const MAX_ENTRIES: usize = 200;

pub fn count(dir: &Path) -> usize {
    fs::read_dir(dir).map(|d| d.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "json")).count()).unwrap_or(0)
}

pub fn clear(dir: &Path) {
    if let Ok(d) = fs::read_dir(dir) {
        for e in d.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn summarize(findings: &[Finding]) -> serde_json::Value {
    serde_json::Value::Array(
        findings
            .iter()
            .filter(|f| !matches!(f.value, Value::Image(_)))
            .map(|f| serde_json::json!({ "capability": f.capability.as_str(), "summary": f.summary(), "confidence": f.confidence }))
            .collect(),
    )
}

pub fn save(dir: &Path, image: &RgbaImage, findings: &[Finding]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
    let thumb = image::DynamicImage::ImageRgba8(image.clone()).thumbnail(480, 480);
    thumb.save(dir.join(format!("{stamp}.png"))).map_err(std::io::Error::other)?;
    let path = dir.join(format!("{stamp}.json"));
    fs::write(&path, serde_json::to_string_pretty(&serde_json::json!({ "time": stamp, "findings": summarize(findings) }))?)?;
    // Keep the newest MAX_ENTRIES.
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    entries.sort();
    while entries.len() > MAX_ENTRIES {
        let old = entries.remove(0);
        let _ = fs::remove_file(old.with_extension("png"));
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

pub fn save_async(dir: PathBuf, image: Arc<RgbaImage>, findings: &[Finding]) {
    let findings = findings.to_vec();
    std::thread::spawn(move || {
        let _ = save(&dir, &image, &findings);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_counts_and_clears() {
        let dir = std::env::temp_dir().join(format!("lens-history-{}", std::process::id()));
        let img = RgbaImage::new(10, 10);
        save(&dir, &img, &[]).unwrap();
        assert_eq!(count(&dir), 1);
        clear(&dir);
        assert_eq!(count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }
}
