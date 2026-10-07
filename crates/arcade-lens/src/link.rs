//! Arcade Link: Lens's presence among the other Arcade apps.
//!
//! The background instance writes Lens's manifest and listens on its
//! endpoint, on a thread started after the IPC listener, so startup never
//! waits for it. With "Connect with other Arcade apps" off, the manifest has
//! no actions and nothing listens.
//!
//! Exposed actions: `lens.capture` (select a region, get the pixels back),
//! `lens.capture_and_act` (the full Lens flow), `lens.analyze` (the palette
//! over an image), `lens.recognize` (headless recognition, also one-shot)
//! and `lens.pin`. Interactive actions are handed to the GUI through
//! [`set_gui`]; each display mode (one process on X11/Windows/macOS, window
//! processes on Wayland) installs its own way of showing them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::{ids, Action, Content, Handoff, InvokeRequest, InvokeResult, LinkError, Locations, Manifest, Presence};
use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::{Finding, Selection, Settings};
use serde_json::{json, Value};

use crate::config::Paths;
use crate::gui::runtime::Runtime;

/// The manifest for these settings (also printed by `--arcade-manifest`).
pub fn manifest(settings: &Settings) -> Manifest {
    let mut m = Manifest::new(ids::LENS, env!("CARGO_PKG_VERSION"), &arcade_link::manifest::current_executable());
    m.launch.background = vec!["--background".into()];
    m.launch.invoke = Some(vec![arcade_link::oneshot::FLAG.into()]);
    if let Some(s) = &settings.activation_shortcut {
        m.shortcuts.push(arcade_link::manifest::Shortcut { id: "capture".into(), accelerator: s.clone() });
    }
    m.settings.link_enabled = settings.link.enabled;
    m.actions = actions();
    m
}

/// The actions Lens exposes.
pub fn actions() -> Vec<Action> {
    vec![
        Action::new("lens.capture", "Select a region on screen", "capture").produces(&["file/image", "screen/region"]).effects(&["opens-ui"]).interactive(true),
        Action::new("lens.capture_and_act", "Arcade Lens", "capture").effects(&["opens-ui"]).interactive(true),
        Action::new("lens.analyze", "Analyze with Lens", "analyze").accepts(&["file/image"]).effects(&["opens-ui"]).interactive(true),
        Action::new("lens.recognize", "Recognize text and codes", "recognize")
            .accepts(&["file/image", "text/plain"])
            .produces(&["structured/findings", "text/plain"]),
        Action::new("lens.pin", "Pin", "pin").accepts(&["file/image"]).effects(&["opens-ui"]).interactive(true),
    ]
}

/// What the user selected for `lens.capture`.
pub struct Captured {
    pub image: RgbaImage,
    /// Physical pixels, virtual-desktop space.
    pub rect: Rect,
    pub monitor: Option<String>,
}

type CaptureSender = Mutex<Option<Sender<Captured>>>;

/// The GUI owns the selection sender; a job cancellation can release it
/// even before the GUI processes its queued request.
#[derive(Clone, Debug)]
pub struct CaptureReply(Arc<CaptureSender>);

impl CaptureReply {
    fn new(sender: Sender<Captured>) -> Self {
        Self(Arc::new(Mutex::new(Some(sender))))
    }

    fn cancellation(&self) -> CaptureCancel {
        CaptureCancel(Arc::downgrade(&self.0))
    }

    pub fn is_pending(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn deliver(self, captured: Captured) {
        if let Some(sender) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = sender.send(captured);
        }
    }
}

/// A weak reference keeps cancellation from retaining the selection sender
/// after the user closes the picker. Its identity scopes GUI dismissal to
/// this capture, so a late cancellation cannot close a newer overlay.
#[derive(Clone, Debug)]
pub struct CaptureCancel(Weak<CaptureSender>);

impl CaptureCancel {
    fn release(&self) {
        if let Some(sender) = self.0.upgrade() {
            sender.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
    }

    pub fn matches(&self, target: &PickTarget) -> bool {
        matches!(target, PickTarget::Reply(reply) if self.0.ptr_eq(&Arc::downgrade(&reply.0)))
    }
}

/// Where a capture's result goes: back to the waiting job (one process), or
/// a file (a Wayland window process, which then exits).
#[derive(Clone, Debug)]
pub enum PickTarget {
    Reply(CaptureReply),
    File(PathBuf),
}

impl PickTarget {
    pub fn is_pending(&self) -> bool {
        match self {
            Self::Reply(reply) => reply.is_pending(),
            Self::File(_) => true,
        }
    }

    /// Delivers the selection. For a file target, writes `<file>` (PNG) and
    /// `<file>.json` (`{rect, monitor}`).
    pub fn deliver(self, captured: Captured) {
        match self {
            PickTarget::Reply(reply) => reply.deliver(captured),
            PickTarget::File(path) => {
                let meta = json!({ "rect": captured.rect, "monitor": captured.monitor });
                if captured.image.save(&path).is_ok() {
                    let _ = std::fs::write(path.with_extension("png.json"), meta.to_string());
                }
            }
        }
    }
}

/// What `lens.capture_and_act` opens: the palette, or one tool directly
/// (`options.mode`: `palette`, `measure`, `pin`, `color`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureMode {
    #[default]
    Palette,
    Measure,
    Pin,
    Color,
}

impl CaptureMode {
    pub fn parse(hint: Option<&str>) -> Result<Self, LinkError> {
        match hint {
            None | Some("palette") => Ok(Self::Palette),
            Some("measure") => Ok(Self::Measure),
            Some("pin") => Ok(Self::Pin),
            Some("color") => Ok(Self::Color),
            Some(_) => Err(LinkError::unsupported("Unknown Lens capture mode")),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::Measure => "measure",
            Self::Pin => "pin",
            Self::Color => "color",
        }
    }
}

/// An interactive request for the GUI.
#[derive(Clone, Debug)]
pub enum GuiRequest {
    Capture { target: PickTarget, mode: CaptureMode, act: bool },
    CancelCapture(CaptureCancel),
    // Own decoded pixels before acknowledging the caller: its handoff can
    // disappear as soon as our job finishes, before the GUI drains its queue.
    Analyze(Arc<RgbaImage>),
    Pin(Arc<RgbaImage>),
}

type Gui = Box<dyn Fn(GuiRequest) -> bool + Send + Sync>;

fn gui() -> &'static Mutex<Option<Gui>> {
    static GUI: OnceLock<Mutex<Option<Gui>>> = OnceLock::new();
    GUI.get_or_init(|| Mutex::new(None))
}

/// Installs how this process shows interactive requests.
pub fn set_gui(f: impl Fn(GuiRequest) -> bool + Send + Sync + 'static) {
    *gui().lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
}

fn show(request: GuiRequest) -> Result<(), LinkError> {
    let g = gui().lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(f) if f(request) => Ok(()),
        Some(_) => Err(LinkError::unavailable("Arcade Lens couldn't open its window")),
        None => Err(LinkError::new(arcade_link::ErrorCode::NotRunning, "one-shot mode has no window")),
    }
}

fn image_path(request: &InvokeRequest) -> Result<PathBuf, LinkError> {
    let input = request
        .inputs
        .iter()
        .find(|c| arcade_link::content::type_matches("file/image", &c.kind))
        .ok_or_else(|| LinkError::unsupported("Arcade Lens needs an image"))?;
    let path = PathBuf::from(input.path.clone().unwrap_or_default());
    if !path.is_file() {
        return Err(LinkError::unsupported(format!("{} isn't a readable image", path.display())));
    }
    Ok(path)
}

fn gui_image(request: &InvokeRequest) -> Result<Arc<RgbaImage>, LinkError> {
    let path = image_path(request)?;
    image::open(&path).map(|image| Arc::new(image.to_rgba8())).map_err(|e| LinkError::unsupported(format!("{}: {e}", path.display())))
}

/// Text form of a finding for other apps (no image payloads).
fn finding_json(f: &Finding) -> Value {
    json!({
        "capability": f.capability.as_str(),
        "text": f.value.as_text(),
        "summary": f.summary(),
        "confidence": f.confidence,
        "recognizer": f.recognizer,
        "details": f.details.iter().map(|(k, v)| json!({ "label": k, "value": v })).collect::<Vec<_>>(),
    })
}

/// Recognition completes on independent workers. Wire order follows confidence,
/// then stable content/provenance, never the order workers finished or their IDs.
fn ordered_findings(findings: &[Finding]) -> Vec<&Finding> {
    let mut result: Vec<_> = findings.iter().filter(|f| f.recognizer != "core.input" && !matches!(f.capability.as_str(), "region" | "image")).collect();
    result.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.capability.cmp(&b.capability))
            .then_with(|| a.recognizer.cmp(&b.recognizer))
            .then_with(|| a.value.as_text().cmp(&b.value.as_text()))
            .then_with(|| a.summary().cmp(&b.summary()))
            .then_with(|| a.details.cmp(&b.details))
    });
    result
}

/// Lens's runtime for headless recognition, loaded on first use (the OCR
/// model load is paid only when someone asks).
fn runtime(paths: &Paths) -> Result<Arc<Runtime>, LinkError> {
    static RT: OnceLock<Mutex<Option<Arc<Runtime>>>> = OnceLock::new();
    let slot = RT.get_or_init(|| Mutex::new(None));
    let mut rt = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = rt.as_ref() {
        return Ok(r.clone());
    }
    let r = Arc::new(Runtime::headless(paths.clone()).map_err(LinkError::internal)?);
    *rt = Some(r.clone());
    Ok(r)
}

/// `lens.recognize`: runs Lens's recognizers on an image or text. With
/// `options.ocrOnly`, only OCR runs (what Arcade Box's OCR provider needs).
pub fn recognize(paths: &Paths, request: &InvokeRequest, cancel: CancelToken) -> Result<InvokeResult, LinkError> {
    let rt = runtime(paths)?;
    let ocr_only = request.options.get("ocrOnly").and_then(Value::as_bool).unwrap_or(false);
    let mut settings = (*rt.settings).clone();
    if ocr_only {
        settings.disabled_recognizers = rt.registry.recognizers().iter().map(|r| r.descriptor().id).filter(|id| id != "core.ocr").collect();
    }
    let engine = lens_core::Engine::new(rt.registry.clone(), Arc::new(lens_core::recognizer::LocalEnvironment), Arc::new(settings));
    let text_input = request.inputs.iter().find(|c| arcade_link::content::family(&c.kind) == "text");
    let report = match text_input {
        Some(c) => {
            let text = arcade_link::handoff::read_text(c).map_err(|e| LinkError::unsupported(e.to_string()))?;
            engine.analyze_text_blocking(&text, cancel)
        }
        None => {
            let path = image_path(request)?;
            let img = image::open(&path).map_err(|e| LinkError::unsupported(format!("{}: {e}", path.display())))?.to_rgba8();
            engine.analyze_blocking(Selection::from_image(img), cancel)
        }
    };
    // The caller's own input (the image, or its text) isn't a finding.
    let findings = ordered_findings(&report.findings);
    let ocr_text: Vec<String> = findings
        .iter()
        .filter(|f| f.recognizer == "core.ocr" && f.capability.as_str() == "text")
        .filter_map(|f| f.value.as_text().map(|t| t.into_owned()))
        .collect();
    let mut outputs = vec![Content::structured("findings", Value::Array(findings.iter().map(|f| finding_json(f)).collect()))];
    if !ocr_text.is_empty() {
        outputs.insert(0, Content::plain(ocr_text.join("\n")));
    }
    let message = match findings.len() {
        0 => "Nothing recognized".to_string(),
        1 => "1 finding".to_string(),
        n => format!("{n} findings"),
    };
    Ok(InvokeResult { outputs, message: Some(message), data: Some(json!({ "ocrEngine": rt.ocr_name })) })
}

struct LensHandler {
    paths: Paths,
    background: bool,
}

impl Handler for LensHandler {
    fn status(&self) -> Value {
        json!({ "mode": if self.background { "background" } else { "foreground" } })
    }

    fn describe(&self) -> Vec<Action> {
        actions()
    }

    fn invoke(&self, request: InvokeRequest, ctx: &InvokeContext) -> Result<Reply, LinkError> {
        match request.action.as_str() {
            "lens.recognize" => {
                let job = ctx.start_job();
                let ticket = job.ticket();
                let paths = self.paths.clone();
                let cancel = CancelToken::new();
                let token = cancel.clone();
                job.on_cancel(move || token.cancel());
                std::thread::spawn(move || {
                    job.progress(None, "Recognizing");
                    job.finish(recognize(&paths, &request, cancel));
                });
                Ok(Reply::Job(ticket))
            }
            "lens.capture" | "lens.capture_and_act" => {
                let act = request.action == "lens.capture_and_act";
                if act {
                    let mode = CaptureMode::parse(request.options.get("mode").and_then(Value::as_str))?;
                    show(GuiRequest::Capture { target: PickTarget::Reply(CaptureReply::new(mpsc::channel().0)), mode, act: true })?;
                    return Ok(Reply::Done(InvokeResult::message("Arcade Lens is open")));
                }
                let (tx, rx) = mpsc::channel();
                let reply = CaptureReply::new(tx);
                let cancel = reply.cancellation();
                show(GuiRequest::Capture { target: PickTarget::Reply(reply), mode: CaptureMode::Palette, act: false })?;
                let job = ctx.start_job();
                let ticket = job.ticket();
                job.on_cancel(move || {
                    // Wake the worker immediately, independently of GUI frame
                    // timing. Job::finish maps this to Cancelled on the wire.
                    cancel.release();
                    let _ = show(GuiRequest::CancelCapture(cancel));
                });
                std::thread::spawn(move || {
                    // The overlay drops the sender if the user closes it.
                    let result = match rx.recv() {
                        Ok(c) => captured_to_link(c),
                        Err(_) => Err(LinkError::denied(arcade_link::error::reason::USER_CANCELLED)),
                    };
                    job.finish(result);
                });
                Ok(Reply::Job(ticket))
            }
            "lens.analyze" => {
                show(GuiRequest::Analyze(gui_image(&request)?))?;
                Ok(Reply::Done(InvokeResult::message("Opened in Arcade Lens")))
            }
            "lens.pin" => {
                show(GuiRequest::Pin(gui_image(&request)?))?;
                Ok(Reply::Done(InvokeResult::message("Pinned")))
            }
            other => Err(LinkError::unavailable(format!("Arcade Lens has no action {other}"))),
        }
    }

    fn activate(&self) -> Result<(), LinkError> {
        let _ = std::process::Command::new(lens_platform::autostart::current_exe().map_err(|e| LinkError::internal(e.to_string()))?).arg("--settings").spawn();
        Ok(())
    }

    fn quit(&self) -> Result<(), LinkError> {
        let _ = lens_platform::ipc::send(&self.paths.endpoint(), "quit");
        Ok(())
    }
}

/// The selection as Link values: a PNG handoff file and its screen rectangle.
pub fn captured_to_link(c: Captured) -> Result<InvokeResult, LinkError> {
    let mut png = Vec::new();
    c.image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| LinkError::internal(e.to_string()))?;
    let handoff = Handoff::create(&Locations::discover(), ids::LENS).map_err(|e| LinkError::internal(e.to_string()))?;
    let file = handoff.file("region.png", &png).map_err(|e| LinkError::internal(e.to_string()))?;
    // The caller reads the file after this job ends; the 24-hour cleanup removes it.
    handoff.keep();
    let region = Content::structured("region", json!({ "rect": c.rect, "monitor": c.monitor }));
    let mut region = region;
    region.kind = "screen/region".into();
    Ok(InvokeResult::outputs(vec![file, region], format!("{} × {} px", c.rect.width, c.rect.height)))
}

/// Reads a capture written by a Wayland window process (`--link-out`).
pub fn read_captured_file(path: &Path) -> Option<Captured> {
    let image = image::open(path).ok()?.to_rgba8();
    let meta: Value = std::fs::read_to_string(path.with_extension("png.json")).ok().and_then(|t| serde_json::from_str(&t).ok())?;
    Some(Captured {
        image,
        rect: serde_json::from_value(meta.get("rect")?.clone()).ok()?,
        monitor: meta.get("monitor").and_then(Value::as_str).map(String::from),
    })
}

/// `--arcade-invoke`: serves one request from stdin without any window.
pub fn serve_oneshot() -> i32 {
    arcade_link::oneshot::serve(&LensHandler { paths: Paths::discover(), background: false })
}

static PRESENCE: OnceLock<Mutex<Option<Arc<Presence>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<Presence>>> {
    PRESENCE.get_or_init(|| Mutex::new(None))
}

/// Starts Lens's presence on a background thread.
pub fn start(settings: &Settings, background: bool) {
    let m = manifest(settings);
    std::thread::Builder::new()
        .name("lens-link".into())
        .spawn(move || {
            let p = Presence::start(Locations::discover(), m, Arc::new(LensHandler { paths: Paths::discover(), background }));
            if let Some(e) = p.last_error() {
                crate::lens_debug!("arcade link: {e}");
            }
            *slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(p));
        })
        .ok();
}

/// Rewrites the manifest after a settings change (shortcut, the Link switch).
pub fn refresh(settings: &Settings) {
    let m = manifest(settings);
    let p = slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(p) = p {
        std::thread::spawn(move || p.update(m));
    }
}

/// Stops listening and removes the endpoint file (the manifest stays).
pub fn stop() {
    if let Some(p) = slot().lock().unwrap_or_else(|e| e.into_inner()).take() {
        p.stop();
    }
}

/// An image shown as Lens's frozen overlay (`lens.analyze`): `image`
/// fitted and centered on a dark canvas the size of `monitor`, plus the
/// image's rectangle on that canvas (selected for the user).
pub fn analyze_canvas(image: &RgbaImage, monitor: &lens_core::geometry::MonitorInfo) -> (RgbaImage, Rect) {
    let (mw, mh) = (monitor.rect.width.max(1), monitor.rect.height.max(1));
    let scale = (f64::from(mw) * 0.9 / f64::from(image.width().max(1))).min(f64::from(mh) * 0.9 / f64::from(image.height().max(1))).min(1.0);
    let (w, h) = (((f64::from(image.width()) * scale) as u32).max(1), ((f64::from(image.height()) * scale) as u32).max(1));
    let fitted = if scale < 1.0 { image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle) } else { image.clone() };
    let mut canvas = RgbaImage::from_pixel(mw, mh, image::Rgba([18, 18, 22, 255]));
    let (x, y) = ((mw - w) / 2, (mh - h) / 2);
    image::imageops::overlay(&mut canvas, &fitted, i64::from(x), i64::from(y));
    (canvas, Rect::new(x as i32, y as i32, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_job_cancel_releases_the_picker_before_the_gui_drains_its_queue() {
        use arcade_link::client::Client;
        use arcade_link::server::{Server, ServerConfig};
        use arcade_link::wire::{JobDone, PeerInfo};
        use std::time::Duration;

        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                *gui().lock().unwrap_or_else(|e| e.into_inner()) = None;
                std::fs::remove_dir_all(&self.0).ok();
            }
        }
        let dir = std::env::temp_dir().join(format!("lens-capture-cancel-{}", std::process::id()));
        let _cleanup = Cleanup(dir.clone());
        let locations = Locations::under(&dir);
        let handler = LensHandler { paths: Paths { config: dir.join("config"), data: dir.join("data") }, background: true };
        let (tx, requests) = mpsc::channel();
        set_gui(move |request| tx.send(request).is_ok());
        let server =
            Server::start(ServerConfig { app: PeerInfo { id: ids::LENS.into(), version: "test".into() }, locations: locations.clone() }, Arc::new(handler))
                .unwrap();
        let mut client = Client::connect(&locations, ids::LENS, &PeerInfo { id: ids::BOX.into(), version: "test".into() }).unwrap();
        let start = |client: &mut Client| {
            client.call("invoke", serde_json::to_value(InvokeRequest::new("lens.capture", ids::BOX)).unwrap()).unwrap()["job"].as_str().unwrap().to_owned()
        };
        let done = |client: &mut Client| -> JobDone {
            let event = client.next_notification(Some(Duration::from_secs(1))).unwrap();
            assert_eq!(event.method.as_deref(), Some("job.done"));
            serde_json::from_value(event.params().clone()).unwrap()
        };

        let job = start(&mut client);
        client.call("job.cancel", json!({ "job": job })).unwrap();
        let cancelled = done(&mut client);
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(cancelled.error.unwrap().code, arcade_link::ErrorCode::Cancelled);
        // No GUI processing is needed for job.done. The queued capture is
        // already inert, and a targeted dismissal follows it.
        let GuiRequest::Capture { target, .. } = requests.recv_timeout(Duration::from_secs(1)).unwrap() else { panic!("capture request") };
        assert!(!target.is_pending());
        let GuiRequest::CancelCapture(cancel) = requests.recv_timeout(Duration::from_secs(1)).unwrap() else { panic!("capture dismissal") };
        assert!(cancel.matches(&target));

        let next = start(&mut client);
        let GuiRequest::Capture { target: next_target, .. } = requests.recv_timeout(Duration::from_secs(1)).unwrap() else { panic!("next capture") };
        assert!(next_target.is_pending());
        assert!(!cancel.matches(&next_target), "a late cancellation must not dismiss a newer picker");
        // User Escape drops the GUI's sender. A registered cancellation
        // callback must not keep that sender alive and hang the next job.
        drop(next_target);
        let dismissed = done(&mut client);
        assert_eq!(dismissed.job, next);
        assert_eq!(dismissed.status, "error");
        let error = dismissed.error.unwrap();
        assert_eq!(error.code, arcade_link::ErrorCode::Denied);
        assert_eq!(error.reason.as_deref(), Some(arcade_link::error::reason::USER_CANCELLED));
        drop(client);
        drop(server);
    }

    #[test]
    fn queued_images_survive_the_creators_handoff_cleanup() {
        let dir = std::env::temp_dir().join(format!("lens-gui-image-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.png");
        let pixels = RgbaImage::from_pixel(32, 24, image::Rgba([201, 40, 70, 255]));
        for action in ["lens.pin", "lens.analyze"] {
            pixels.save(&path).unwrap();
            let request = InvokeRequest::new(action, "arcade.test").input(Content::file(&path));
            let image = gui_image(&request).unwrap();
            let queued = if action == "lens.pin" { GuiRequest::Pin(image) } else { GuiRequest::Analyze(image) };
            std::fs::remove_file(&path).unwrap();
            match queued {
                GuiRequest::Pin(image) | GuiRequest::Analyze(image) => assert_eq!(*image, pixels),
                _ => unreachable!(),
            }
        }
        std::fs::write(&path, b"not an image").unwrap();
        let request = InvokeRequest::new("lens.pin", "arcade.test").input(Content::file(&path));
        assert!(gui_image(&request).is_err(), "invalid pixels must fail before acknowledging success");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn capture_mode_hints_are_explicit() {
        assert_eq!(CaptureMode::parse(None).unwrap(), CaptureMode::Palette);
        for mode in [CaptureMode::Palette, CaptureMode::Measure, CaptureMode::Pin, CaptureMode::Color] {
            assert_eq!(CaptureMode::parse(Some(mode.hint())).unwrap(), mode);
        }
        assert!(CaptureMode::parse(Some("unknown")).is_err());
    }

    #[test]
    fn status_reports_how_the_instance_was_started() {
        let paths = Paths { config: "unused-config".into(), data: "unused-data".into() };
        for (background, mode) in [(true, "background"), (false, "foreground")] {
            let handler = LensHandler { paths: paths.clone(), background };
            assert_eq!(handler.status(), json!({ "mode": mode }));
        }
    }

    #[test]
    fn wire_findings_are_independent_of_worker_completion_order() {
        let finding = |id, capability, text, confidence, recognizer: &str| Finding {
            id: lens_core::finding::FindingId(id),
            capability,
            value: lens_core::Value::text(text),
            confidence,
            recognizer: recognizer.into(),
            derived_from: None,
            span: None,
            details: vec![],
        };
        let mut findings = vec![
            finding(1, lens_core::caps::COLOR, "red", 0.8, "core.color"),
            finding(2, lens_core::caps::UI_ELEMENT, "panel", 0.95, "core.inspect"),
            finding(3, lens_core::caps::TEXT, "z", 0.8, "core.ocr"),
            finding(4, lens_core::caps::TEXT, "a", 0.8, "core.ocr"),
            finding(5, lens_core::caps::TEXT, "input", 1.0, "core.input"),
        ];
        let expected: Vec<_> = ordered_findings(&findings).into_iter().map(finding_json).collect();
        assert_eq!(expected.iter().map(|f| f["text"].as_str().unwrap()).collect::<Vec<_>>(), ["panel", "red", "a", "z"]);
        for _ in 0..findings.len() {
            findings.rotate_left(1);
            // IDs also depend on scheduling, and must never be tiebreakers.
            for (i, f) in findings.iter_mut().enumerate() {
                f.id = lens_core::finding::FindingId(i as u32);
            }
            assert_eq!(ordered_findings(&findings).into_iter().map(finding_json).collect::<Vec<_>>(), expected);
        }
        findings.reverse();
        assert_eq!(ordered_findings(&findings).into_iter().map(finding_json).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn manifest_lists_the_five_actions() {
        let m = manifest(&Settings::default());
        let ids: Vec<&str> = m.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["lens.capture", "lens.capture_and_act", "lens.analyze", "lens.recognize", "lens.pin"]);
        assert!(m.action("lens.recognize").is_some_and(|a| !a.interactive));
        assert_eq!(m.launch.invoke.as_deref(), Some(&["--arcade-invoke".to_string()][..]));
    }

    #[test]
    fn recognize_text_finds_urls() {
        let dir = std::env::temp_dir().join(format!("lens-link-test-{}", std::process::id()));
        let paths = Paths { config: dir.join("c"), data: dir.join("d") };
        let req = InvokeRequest::new("lens.recognize", "t").input(Content::plain("see https://example.com/docs or mail me@example.com"));
        let r = recognize(&paths, &req, CancelToken::new()).unwrap();
        let findings = r.outputs.iter().find(|c| c.kind == "structured/findings").unwrap().data.clone().unwrap();
        let caps: Vec<&str> = findings.as_array().unwrap().iter().filter_map(|f| f["capability"].as_str()).collect();
        assert!(caps.contains(&"url"), "{caps:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn analyze_canvas_fits_large_images() {
        let monitor = lens_core::geometry::MonitorInfo {
            id: "m".into(),
            name: "m".into(),
            rect: Rect::new(0, 0, 1000, 800),
            scale_factor: 1.0,
            refresh_rate_hz: None,
            is_primary: true,
        };
        let (canvas, rect) = analyze_canvas(&RgbaImage::new(4000, 1000), &monitor);
        assert_eq!((canvas.width(), canvas.height()), (1000, 800));
        assert!(rect.width <= 900 && rect.x >= 0 && rect.y > 0);
    }
}
