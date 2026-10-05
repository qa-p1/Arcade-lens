//! The capability engine: progressive, parallel, cancellable recognition.
//!
//! 1. Cheap [`Signals`] are computed synchronously.
//! 2. A `region` finding is emitted immediately, so baseline actions (Copy,
//!    Save, Pin) are available before any recognizer runs.
//! 3. Every finding is offered to each recognizer that consumes its
//!    capability. Jobs run on a shared thread pool, cheapest first, and every
//!    result is streamed back as soon as it is ready.
//! 4. New findings are fed back in (QR → decoded text → URL) up to a fixed
//!    derivation depth.
//!
//! A failing or panicking recognizer only loses its own results.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;
use crate::capability::{caps, Capability};
use crate::error::LensError;
use crate::finding::{Detection, Finding, FindingId};
use crate::recognizer::{Environment, RecognizeContext, Recognizer};
use crate::registry::Registry;
use crate::selection::{Selection, Signals};
use crate::settings::Settings;
use crate::value::{ImageValue, Value};

/// Maximum derivation chain length (region → text → url is depth 2).
const MAX_DEPTH: u8 = 4;

#[derive(Debug, Clone)]
pub enum AnalysisEvent {
    Findings(Vec<Finding>),
    RecognizerFailed { recognizer: String, error: String },
    Finished { cancelled: bool, elapsed: Duration },
}

#[derive(Clone)]
pub struct Engine {
    registry: Arc<Registry>,
    env: Arc<dyn Environment>,
    settings: Arc<Settings>,
}

impl Engine {
    pub fn new(registry: Arc<Registry>, env: Arc<dyn Environment>, settings: Arc<Settings>) -> Self {
        Self { registry, env, settings }
    }

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn settings(&self) -> &Arc<Settings> {
        &self.settings
    }

    /// Starts analysis in the background and returns a handle streaming
    /// [`AnalysisEvent`]s. Dropping the handle cancels outstanding work.
    pub fn analyze(&self, selection: Selection) -> Analysis {
        let cancel = CancelToken::new();
        let (events_tx, events_rx) = mpsc::channel();
        let (msg_tx, msg_rx) = mpsc::channel();
        let engine = self.clone();
        let token = cancel.clone();
        let jobs_tx = msg_tx.clone();
        std::thread::Builder::new()
            .name("lens-analysis".into())
            .spawn(move || {
                engine.coordinate(Arc::new(selection), None, token, jobs_tx, msg_rx, &mut |e| {
                    let _ = events_tx.send(e);
                })
            })
            .expect("spawn analysis thread");
        Analysis { events: events_rx, cancel, wake: msg_tx }
    }

    /// Runs analysis to completion on the calling thread.
    pub fn analyze_blocking(&self, selection: Selection, cancel: CancelToken) -> AnalysisReport {
        let (msg_tx, msg_rx) = mpsc::channel();
        let mut report = AnalysisReport::default();
        self.coordinate(Arc::new(selection), None, cancel, msg_tx, msg_rx, &mut |e| report.apply(e));
        report
    }

    /// Runs the text recognizers over `text` to completion (no image), for
    /// callers that already have text, such as Arcade Link's `lens.recognize`.
    pub fn analyze_text_blocking(&self, text: &str, cancel: CancelToken) -> AnalysisReport {
        let (msg_tx, msg_rx) = mpsc::channel();
        let mut report = AnalysisReport::default();
        let root = Detection::new(crate::caps::TEXT, Value::text(text));
        let selection = Selection::from_image(image::RgbaImage::new(1, 1));
        self.coordinate(Arc::new(selection), Some(root), cancel, msg_tx, msg_rx, &mut |e| report.apply(e));
        report
    }

    fn coordinate(
        &self,
        selection: Arc<Selection>,
        root_override: Option<Detection>,
        cancel: CancelToken,
        tx: Sender<Msg>,
        rx: Receiver<Msg>,
        emit: &mut dyn FnMut(AnalysisEvent),
    ) {
        let started = Instant::now();
        let signals = Arc::new(Signals::compute(&selection.image));
        let cx = RecognizeContext {
            selection: Arc::clone(&selection),
            signals,
            cancel: cancel.clone(),
            env: Arc::clone(&self.env),
            settings: Arc::clone(&self.settings),
        };
        let mut state = State { next_id: 0, seen: HashSet::new(), pending: 0 };

        let root = match root_override {
            Some(d) => state.make(None, 0, "core.input", d),
            None => state.make(
                None,
                0,
                "core.region",
                Detection::new(caps::REGION, Value::Image(ImageValue { image: Arc::clone(&selection.image), origin: Some(selection.rect) }))
                    .detail("Size", format!("{} × {} px", selection.rect.width, selection.rect.height)),
            ),
        };
        if let Some(root) = root {
            emit(AnalysisEvent::Findings(vec![root.0.clone()]));
            self.schedule(&root.0, root.1, &cx, &tx, &mut state);
        }

        while state.pending > 0 && !cancel.is_cancelled() {
            let msg = match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let Msg::Done { recognizer, input, depth, result } = msg else {
                break; // Msg::Wake: cancellation requested.
            };
            state.pending -= 1;
            match result {
                Ok(detections) => {
                    let mut batch = Vec::new();
                    for d in detections {
                        if let Some((f, depth)) = state.make(Some(input), depth, &recognizer, d) {
                            batch.push((f, depth));
                        }
                    }
                    if !batch.is_empty() {
                        emit(AnalysisEvent::Findings(batch.iter().map(|(f, _)| f.clone()).collect()));
                        for (f, depth) in &batch {
                            self.schedule(f, *depth, &cx, &tx, &mut state);
                        }
                    }
                }
                Err(LensError::Cancelled) => {}
                Err(e) => emit(AnalysisEvent::RecognizerFailed { recognizer, error: e.to_string() }),
            }
        }
        emit(AnalysisEvent::Finished { cancelled: cancel.is_cancelled(), elapsed: started.elapsed() });
    }

    fn schedule(&self, finding: &Finding, depth: u8, cx: &RecognizeContext, tx: &Sender<Msg>, state: &mut State) {
        if depth >= MAX_DEPTH {
            return;
        }
        let mut jobs: Vec<(&Arc<dyn Recognizer>, crate::recognizer::RecognizerDescriptor)> = self
            .registry
            .recognizers()
            .iter()
            .map(|r| (r, r.descriptor()))
            .filter(|(_, d)| d.consumes.contains(&finding.capability))
            .filter(|(_, d)| d.id != finding.recognizer)
            .filter(|(_, d)| !self.settings.disabled_recognizers.contains(&d.id))
            .filter(|(r, _)| r.should_run(finding, cx))
            .collect();
        jobs.sort_by_key(|(_, d)| d.cost);
        for (r, d) in jobs {
            state.pending += 1;
            let (r, input, cx, tx) = (Arc::clone(r), finding.clone(), cx.clone(), tx.clone());
            rayon::spawn(move || {
                let result = if cx.cancel.is_cancelled() {
                    Err(LensError::Cancelled)
                } else {
                    catch_unwind(AssertUnwindSafe(|| r.recognize(&input, &cx))).unwrap_or_else(|panic| {
                        let msg = panic
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| panic.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "panicked".into());
                        Err(LensError::Failed(format!("recognizer panicked: {msg}")))
                    })
                };
                let _ = tx.send(Msg::Done { recognizer: d.id, input: input.id, depth: depth + 1, result });
            });
        }
    }
}

enum Msg {
    Done { recognizer: String, input: FindingId, depth: u8, result: crate::error::Result<Vec<Detection>> },
    Wake,
}

struct State {
    next_id: u32,
    seen: HashSet<(Capability, String)>,
    pending: usize,
}

impl State {
    fn make(&mut self, parent: Option<FindingId>, depth: u8, recognizer: &str, d: Detection) -> Option<(Finding, u8)> {
        if let Some(key) = d.value.dedup_key() {
            if !self.seen.insert((d.capability.clone(), key)) {
                return None;
            }
        }
        let id = FindingId(self.next_id);
        self.next_id += 1;
        Some((
            Finding {
                id,
                capability: d.capability,
                value: d.value,
                confidence: d.confidence,
                recognizer: recognizer.to_string(),
                derived_from: parent,
                span: d.span,
                details: d.details,
            },
            depth,
        ))
    }
}

/// Handle to a running analysis.
pub struct Analysis {
    events: Receiver<AnalysisEvent>,
    cancel: CancelToken,
    wake: Sender<Msg>,
}

impl Analysis {
    pub fn events(&self) -> &Receiver<AnalysisEvent> {
        &self.events
    }

    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Stops scheduling and abandons outstanding recognizers immediately.
    pub fn cancel(&self) {
        self.cancel.cancel();
        let _ = self.wake.send(Msg::Wake);
    }

    /// Blocks until analysis finishes and collects everything.
    pub fn wait(self) -> AnalysisReport {
        let mut report = AnalysisReport::default();
        while let Ok(e) = self.events.recv() {
            let done = matches!(e, AnalysisEvent::Finished { .. });
            report.apply(e);
            if done {
                break;
            }
        }
        report
    }
}

impl Drop for Analysis {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnalysisReport {
    pub findings: Vec<Finding>,
    pub failures: Vec<(String, String)>,
    pub cancelled: bool,
    pub elapsed: Duration,
}

impl AnalysisReport {
    pub fn apply(&mut self, e: AnalysisEvent) {
        match e {
            AnalysisEvent::Findings(f) => self.findings.extend(f),
            AnalysisEvent::RecognizerFailed { recognizer, error } => self.failures.push((recognizer, error)),
            AnalysisEvent::Finished { cancelled, elapsed } => {
                self.cancelled = cancelled;
                self.elapsed = elapsed;
            }
        }
    }

    pub fn with(&self, cap: &Capability) -> impl Iterator<Item = &Finding> {
        let cap = cap.clone();
        self.findings.iter().filter(move |f| f.capability == cap)
    }

    pub fn first(&self, cap: &Capability) -> Option<&Finding> {
        self.with(cap).max_by(|a, b| a.confidence.total_cmp(&b.confidence))
    }

    pub fn has(&self, cap: &Capability) -> bool {
        self.with(cap).next().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::recognizer::{Cost, NullEnvironment, RecognizerDescriptor};
    use crate::registry::PluginManifest;
    use image::{Rgba, RgbaImage};

    struct FakeOcr;
    impl Recognizer for FakeOcr {
        fn descriptor(&self) -> RecognizerDescriptor {
            RecognizerDescriptor { id: "test.ocr".into(), consumes: vec![caps::REGION], produces: vec![caps::TEXT], cost: Cost::Expensive }
        }
        fn recognize(&self, _: &Finding, _: &RecognizeContext) -> Result<Vec<Detection>> {
            Ok(vec![Detection::new(caps::TEXT, Value::text("see https://example.com"))])
        }
    }

    struct FakeQr;
    impl Recognizer for FakeQr {
        fn descriptor(&self) -> RecognizerDescriptor {
            RecognizerDescriptor { id: "test.qr".into(), consumes: vec![caps::REGION], produces: vec![caps::QR_CODE], cost: Cost::Cheap }
        }
        fn recognize(&self, _: &Finding, _: &RecognizeContext) -> Result<Vec<Detection>> {
            Ok(vec![Detection::new(caps::QR_CODE, Value::text("https://example.com"))])
        }
    }

    struct FakeUrl;
    impl Recognizer for FakeUrl {
        fn descriptor(&self) -> RecognizerDescriptor {
            RecognizerDescriptor { id: "test.url".into(), consumes: vec![caps::TEXT, caps::QR_CODE], produces: vec![caps::URL], cost: Cost::Trivial }
        }
        fn recognize(&self, input: &Finding, _: &RecognizeContext) -> Result<Vec<Detection>> {
            let t = input.value.as_text().unwrap_or_default();
            Ok(t.split_whitespace().filter(|w| w.starts_with("https://")).map(|w| Detection::new(caps::URL, Value::text(w))).collect())
        }
    }

    struct Panics;
    impl Recognizer for Panics {
        fn descriptor(&self) -> RecognizerDescriptor {
            RecognizerDescriptor { id: "test.panics".into(), consumes: vec![caps::REGION], produces: vec![], cost: Cost::Cheap }
        }
        fn recognize(&self, _: &Finding, _: &RecognizeContext) -> Result<Vec<Detection>> {
            panic!("boom")
        }
    }

    struct Slow;
    impl Recognizer for Slow {
        fn descriptor(&self) -> RecognizerDescriptor {
            RecognizerDescriptor { id: "test.slow".into(), consumes: vec![caps::REGION], produces: vec![], cost: Cost::Expensive }
        }
        fn recognize(&self, _: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
            for _ in 0..500 {
                cx.cancel.check()?;
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(vec![])
        }
    }

    fn engine(register: impl FnOnce(&mut crate::registry::PluginRegistrar)) -> Engine {
        let mut reg = Registry::new();
        reg.register_plugin(PluginManifest::first_party("test", "Test"), register).unwrap();
        Engine::new(Arc::new(reg), Arc::new(NullEnvironment), Arc::new(Settings::default()))
    }

    fn selection() -> Selection {
        Selection::from_image(RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255])))
    }

    #[test]
    fn findings_flow_through_and_deduplicate() {
        let e = engine(|r| {
            r.recognizer(FakeOcr);
            r.recognizer(FakeQr);
            r.recognizer(FakeUrl);
            r.recognizer(Panics);
        });
        let report = e.analyze_blocking(selection(), CancelToken::new());
        let caps: Vec<_> = report.findings.iter().map(|f| f.capability.as_str()).collect();
        assert!(caps.contains(&"region") && caps.contains(&"text") && caps.contains(&"qr-code"));
        // The URL is found via both OCR and QR, but reported once.
        assert_eq!(report.with(&caps::URL).count(), 1);
        let url = report.first(&caps::URL).unwrap();
        assert!(url.derived_from.is_some());
        // The panicking recognizer is isolated.
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].1.contains("boom"));
        assert!(!report.cancelled);
    }

    #[test]
    fn region_arrives_first_and_cancel_is_prompt() {
        let e = engine(|r| r.recognizer(Slow));
        let analysis = e.analyze(selection());
        match analysis.events().recv().unwrap() {
            AnalysisEvent::Findings(f) => assert_eq!(f[0].capability, caps::REGION),
            other => panic!("unexpected {other:?}"),
        }
        let t = Instant::now();
        analysis.cancel();
        let report = analysis.wait();
        assert!(report.cancelled);
        assert!(t.elapsed() < Duration::from_millis(500), "cancel took {:?}", t.elapsed());
    }

    #[test]
    fn disabled_recognizers_do_not_run() {
        let mut reg = Registry::new();
        reg.register_plugin(PluginManifest::first_party("test", "Test"), |r| r.recognizer(FakeOcr)).unwrap();
        let settings = Settings { disabled_recognizers: vec!["test.ocr".into()], ..Default::default() };
        let e = Engine::new(Arc::new(reg), Arc::new(NullEnvironment), Arc::new(settings));
        let report = e.analyze_blocking(selection(), CancelToken::new());
        assert!(!report.has(&caps::TEXT));
    }
}
