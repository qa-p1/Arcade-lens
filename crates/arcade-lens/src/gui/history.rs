//! Opt-in selection history (off by default). Each entry is a thumbnail and
//! a JSON summary of what was recognized; secrets are stored masked, also
//! where they appear inside another finding (the Text that contains them).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::RgbaImage;
use lens_core::{Finding, Value};

const MAX_ENTRIES: usize = 200;

/// Serializes writes with Clear. The generation changes on every Clear, so a
/// save queued before it (its thumbnail still being made) writes nothing.
static WRITES: Mutex<u64> = Mutex::new(0);

/// Fits `image` in a `max`×`max` box, from the borrowed pixels (no full copy).
fn thumbnail(image: &RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    if w <= max && h <= max {
        return image.clone();
    }
    let scale = max as f64 / w.max(h) as f64;
    let (tw, th) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    image::imageops::thumbnail(image, tw, th)
}

pub fn count(dir: &Path) -> usize {
    fs::read_dir(dir).map(|d| d.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "json")).count()).unwrap_or(0)
}

pub fn clear(dir: &Path) {
    let mut generation = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    *generation += 1;
    if let Ok(d) = fs::read_dir(dir) {
        for e in d.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// Like `Finding::summary`, with every secret found in the same selection
/// masked before the text is shortened.
fn masked_summary(f: &Finding, secrets: &[(String, String)]) -> String {
    let text = match &f.value {
        Value::Secret(s) => s.masked.clone(),
        Value::Error(e) => e.headline.clone(),
        v => v.as_text().map(|t| t.into_owned()).unwrap_or_else(|| format!("{v:?}")),
    };
    let text = secrets.iter().fold(text, |text, (raw, masked)| text.replace(raw.as_str(), masked));
    let first_line = text.lines().next().unwrap_or_default();
    let mut s: String = first_line.chars().take(60).collect();
    if s.len() < first_line.len() || text.lines().nth(1).is_some() {
        s.push('…');
    }
    s
}

fn summarize(findings: &[Finding]) -> serde_json::Value {
    let mut secrets: Vec<(String, String)> = findings
        .iter()
        .filter_map(|f| match &f.value {
            Value::Secret(s) => Some((s.raw.expose().to_string(), s.masked.clone())),
            _ => None,
        })
        .filter(|(raw, _)| !raw.is_empty())
        .collect();
    // Longest first, so a secret containing another is masked whole.
    secrets.sort_by_key(|(raw, _)| std::cmp::Reverse(raw.len()));
    serde_json::Value::Array(
        findings
            .iter()
            .filter(|f| !matches!(f.value, Value::Image(_)))
            .map(|f| serde_json::json!({ "capability": f.capability.as_str(), "summary": masked_summary(f, &secrets), "confidence": f.confidence }))
            .collect(),
    )
}

#[cfg(test)]
pub fn save(dir: &Path, image: &RgbaImage, findings: &[Finding]) -> std::io::Result<PathBuf> {
    let generation = *WRITES.lock().unwrap_or_else(|e| e.into_inner());
    save_unless_cleared(dir, image, findings, generation)?.ok_or_else(|| std::io::Error::other("history was cleared"))
}

fn save_unless_cleared(dir: &Path, image: &RgbaImage, findings: &[Finding], queued: u64) -> std::io::Result<Option<PathBuf>> {
    let thumb = thumbnail(image, 480);
    let summary = summarize(findings);
    let generation = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    if *generation != queued {
        return Ok(None);
    }
    fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
    thumb.save(dir.join(format!("{stamp}.png"))).map_err(std::io::Error::other)?;
    let path = dir.join(format!("{stamp}.json"));
    fs::write(&path, serde_json::to_string_pretty(&serde_json::json!({ "time": stamp, "findings": summary }))?)?;
    // Keep the newest MAX_ENTRIES.
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    entries.sort();
    while entries.len() > MAX_ENTRIES {
        let old = entries.remove(0);
        let _ = fs::remove_file(old.with_extension("png"));
        let _ = fs::remove_file(old);
    }
    Ok(Some(path))
}

pub fn save_async(dir: PathBuf, image: Arc<RgbaImage>, findings: &[Finding]) {
    let findings = findings.to_vec();
    let queued = *WRITES.lock().unwrap_or_else(|e| e.into_inner());
    std::thread::spawn(move || {
        let _ = save_unless_cleared(&dir, &image, &findings, queued);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lens_core::value::{SecretValue, Sensitive};

    fn finding(capability: lens_core::Capability, value: Value) -> Finding {
        Finding { id: lens_core::FindingId(0), capability, value, confidence: 1.0, recognizer: "test".into(), derived_from: None, span: None, details: vec![] }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lens-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn saves_counts_and_clears() {
        let dir = temp("basic");
        let img = RgbaImage::new(10, 10);
        save(&dir, &img, &[]).unwrap();
        assert_eq!(count(&dir), 1);
        clear(&dir);
        assert_eq!(count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_save_queued_before_clear_writes_nothing() {
        let dir = temp("race");
        let queued = *WRITES.lock().unwrap();
        clear(&dir);
        assert!(save_unless_cleared(&dir, &RgbaImage::new(600, 400), &[], queued).unwrap().is_none());
        assert_eq!(count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn secrets_are_masked_inside_other_findings() {
        let password = "hunter2-Correct-Horse-Battery";
        let text = finding(lens_core::caps::TEXT, Value::text(format!("login: alice password: {password}")));
        let secret =
            finding(lens_core::caps::SECRET, Value::Secret(SecretValue { kind: "password".into(), masked: "hun…tery".into(), raw: Sensitive::new(password) }));
        let json = summarize(&[text, secret]).to_string();
        assert!(!json.contains(password), "{json}");
        assert!(json.contains("password: hun…tery"), "{json}");
    }
}
