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

/// One saved history entry, as the History window lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Unix milliseconds (local wall-clock stamp).
    pub time_ms: i64,
    pub summary: String,
    pub kinds: Vec<String>,
    pub thumbnail: PathBuf,
}

/// Newest first. Unreadable entries are skipped; an unreadable folder is an
/// error unless it simply doesn't exist yet (nothing saved).
pub fn load(dir: &Path) -> Result<Vec<Entry>, String> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Couldn't read the history folder. {e}")),
    };
    let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    paths.reverse();
    let mut out = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let stamp = doc["time"].as_str().unwrap_or_default();
        let Some(time_ms) = chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S%3f")
            .ok()
            .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
            .map(|t| t.timestamp_millis())
        else {
            continue;
        };
        let findings = doc["findings"].as_array().cloned().unwrap_or_default();
        let summary = findings
            .iter()
            .filter_map(|f| f["summary"].as_str())
            .find(|s| !s.trim().is_empty())
            .unwrap_or("A screen selection")
            .to_string();
        let mut kinds: Vec<String> = Vec::new();
        for f in &findings {
            if let Some(c) = f["capability"].as_str() {
                let label = super::overlay::cap_label(&lens_core::Capability::custom(c));
                if !kinds.contains(&label) {
                    kinds.push(label);
                }
            }
        }
        out.push(Entry { time_ms, summary, kinds, thumbnail: path.with_extension("png") });
    }
    Ok(out)
}

pub enum HistoryRequest {
    /// "Turn on in Privacy": open Settings › Privacy.
    OpenPrivacy,
    OpenFolder(PathBuf),
}

/// History — Arcade Lens: a Template A window listing saved selections.
pub struct HistoryView {
    dir: PathBuf,
    entries: Option<Result<Vec<Entry>, String>>,
    loading: Option<std::sync::mpsc::Receiver<Result<Vec<Entry>, String>>>,
    thumbs: std::collections::HashMap<PathBuf, Option<eframe::egui::TextureHandle>>,
    pub closed: bool,
}

impl HistoryView {
    pub fn new(dir: PathBuf) -> Self {
        let mut view = Self { dir, entries: None, loading: None, thumbs: Default::default(), closed: false };
        view.reload();
        view
    }

    pub fn viewport_id() -> eframe::egui::ViewportId {
        eframe::egui::ViewportId::from_hash_of("lens-history")
    }

    pub fn builder() -> eframe::egui::ViewportBuilder {
        eframe::egui::ViewportBuilder::default()
            .with_title("History — Arcade Lens")
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([720.0, 520.0])
            .with_app_id(crate::link::OS_ID)
            .with_icon(crate::icon::window_icon())
    }

    pub fn reload(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = self.dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(&dir));
        });
        self.loading = Some(rx);
        self.thumbs.clear();
    }

    fn thumbnail(&mut self, ctx: &eframe::egui::Context, path: &Path, budget: &mut u32) -> Option<eframe::egui::TextureId> {
        if let Some(t) = self.thumbs.get(path) {
            return t.as_ref().map(|t| t.id());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let texture = image::open(path).ok().map(|img| {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            ctx.load_texture(path.display().to_string(), eframe::egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), Default::default())
        });
        let id = texture.as_ref().map(|t| t.id());
        self.thumbs.insert(path.to_path_buf(), texture);
        id
    }

    pub fn ui(&mut self, ctx: &eframe::egui::Context, enabled: bool) -> Vec<HistoryRequest> {
        use arcade_link::design::egui::components::{ButtonKind, Tone};
        use eframe::egui::{self, Rect, Sense, Vec2};
        super::theme::sync(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.closed = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::W)) {
            self.closed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(result) => {
                    self.entries = Some(result);
                    self.loading = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
            }
        }
        let kit = super::theme::kit();
        let c = super::theme::c();
        let mut out = Vec::new();
        let count = self.entries.as_ref().and_then(|e| e.as_ref().ok()).map_or(0, Vec::len);
        let description = if count == 0 {
            "What Lens recognized in your selections, newest first.".to_string()
        } else {
            format!("{} item{}, newest first. Secrets are masked.", arcade_link::design::format::format_count(count as i64), if count == 1 { "" } else { "s" })
        };
        let mut reload = false;
        let mut budget = 4;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let offset = chrono::Local::now().offset().local_minus_utc() / 60;
        let dir = self.dir.clone();
        let entries = self.entries.clone();
        let loading = self.loading.is_some();
        kit.page_with_head_actions(
            ctx,
            "History",
            &description,
            |ui| {
                if kit.button(ui, "Open folder", ButtonKind::Default).clicked() {
                    out.push(HistoryRequest::OpenFolder(dir.clone()));
                }
                if kit.button_state(ui, "Refresh", ButtonKind::Default, !loading, loading, false).clicked() {
                    reload = true;
                }
            },
            |ui| match entries {
                None => {
                    kit.list(ui, |ui| {
                        for _ in 0..4 {
                            kit.skeleton_row(ui);
                        }
                    });
                }
                Some(Err(error)) => {
                    kit.notice_action(ui, &error, Tone::Danger, |ui| {
                        if kit.button_state(ui, "Retry", ButtonKind::Default, true, false, true).clicked() {
                            reload = true;
                        }
                    });
                }
                Some(Ok(list)) if list.is_empty() && !enabled => {
                    kit.empty_state(ui, "clock", "History is off", "Turn it on to keep thumbnails and what Lens recognized, on this computer only.", |ui| {
                        if kit.button(ui, "Turn on in Privacy", ButtonKind::Primary).clicked() {
                            out.push(HistoryRequest::OpenPrivacy);
                        }
                    });
                }
                Some(Ok(list)) if list.is_empty() => {
                    kit.empty_state(ui, "clock", "Nothing yet", "Selections you make with Lens appear here.", |_| ());
                }
                Some(Ok(list)) => {
                    if !enabled {
                        kit.notice_action(ui, "History is off. These items stay until you clear them.", Tone::Neutral, |ui| {
                            if kit.button_state(ui, "Turn on in Privacy", ButtonKind::Default, true, false, true).clicked() {
                                out.push(HistoryRequest::OpenPrivacy);
                            }
                        });
                        ui.add_space(16.0);
                    }
                    kit.list(ui, |ui| {
                        for entry in &list {
                            let thumb = self.thumbnail(ui.ctx(), &entry.thumbnail, &mut budget);
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                let (rect, _) = ui.allocate_exact_size(Vec2::new(52.0, 52.0), Sense::hover());
                                ui.painter().rect_filled(rect, arcade_link::design::radius::SM, c.sunken);
                                if let Some(t) = thumb {
                                    let size = self.thumbs.get(&entry.thumbnail).and_then(|t| t.as_ref()).map_or(Vec2::splat(52.0), |t| t.size_vec2());
                                    let fit = (52.0 / size.x.max(size.y)).min(1.0);
                                    let img = Rect::from_center_size(rect.center(), size * fit);
                                    ui.painter().image(t, img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                }
                                ui.add_space(8.0);
                                let width = ui.available_width();
                                kit.row(ui, &entry.summary, &entry.kinds.join(", "), |ui| {
                                    ui.set_max_width(width * 0.3);
                                    ui.label(
                                        egui::RichText::new(arcade_link::design::format::format_relative(entry.time_ms, now, offset))
                                            .font(super::theme::font(arcade_link::design::text::SMALL))
                                            .color(c.muted),
                                    );
                                });
                            });
                        }
                    });
                }
            },
        );
        if reload {
            self.reload();
        }
        out
    }
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
