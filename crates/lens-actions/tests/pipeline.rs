//! End-to-end: real recognizers → engine → palette → actions, against a
//! recording host. OCR is simulated by an engine that returns fixed text.

use std::sync::Arc;

use image::{Rgba, RgbaImage};
use lens_core::cancel::CancelToken;
use lens_core::chain::{Chain, ChainRunContext, ChainStep};
use lens_core::geometry::Rect;
use lens_core::host::{HostCall, HostFeatures, RecordingHost};
use lens_core::palette::{build_palette, invoke, Invocation, InvokeContext, Palette, PaletteInput, Target};
use lens_core::recognizer::NullEnvironment;
use lens_core::usage::UsageStore;
use lens_core::value::{TextLayout, TextLine};
use lens_core::{caps, AnalysisReport, Engine, Selection, Settings};
use lens_recognizers::ocr::OcrEngine;

struct FixedOcr(&'static str);

impl OcrEngine for FixedOcr {
    fn name(&self) -> &str {
        "fixed"
    }
    fn recognize(&self, _: &RgbaImage, _: &[String], _: &CancelToken) -> lens_core::Result<TextLayout> {
        Ok(TextLayout { lines: self.0.lines().map(|l| TextLine { text: l.into(), bbox: Rect::new(0, 0, 10, 10), words: vec![] }).collect() })
    }
}

/// A busy, text-like image so OCR is scheduled.
fn texty() -> RgbaImage {
    RgbaImage::from_fn(300, 60, |x, y| if (x / 3 + y / 5) % 3 == 0 { Rgba([20, 20, 20, 255]) } else { Rgba([250, 250, 250, 255]) })
}

struct Setup {
    engine: Engine,
    settings: Settings,
}

fn setup(ocr: Option<&'static str>, settings: Settings) -> Setup {
    let registry = lens_actions::standard_registry(ocr.map(|t| Arc::new(FixedOcr(t)) as Arc<dyn OcrEngine>)).unwrap();
    let engine = Engine::new(Arc::new(registry), Arc::new(NullEnvironment), Arc::new(settings.clone()));
    Setup { engine, settings }
}

impl Setup {
    fn analyze(&self, img: RgbaImage) -> (Selection, AnalysisReport) {
        let sel = Selection::from_image(img);
        let report = self.engine.analyze_blocking(sel.clone(), CancelToken::new());
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        (sel, report)
    }

    fn palette(&self, report: &AnalysisReport, usage: &UsageStore, chains: &[Chain]) -> Palette {
        build_palette(&PaletteInput {
            registry: self.engine.registry(),
            findings: &report.findings,
            settings: &self.settings,
            usage,
            host: HostFeatures::all(),
            chains,
            now: 0,
        })
    }
}

fn primary_ids(p: &Palette) -> Vec<String> {
    p.primary.iter().map(|e| e.target.usage_key()).collect()
}

#[test]
fn qr_with_url_combines_interpretations() {
    let s = setup(None, Settings::default());
    let code = qrcode::QrCode::new(b"https://arcade.example/lens").unwrap();
    let img = image::DynamicImage::ImageLuma8(code.render::<image::Luma<u8>>().module_dimensions(4, 4).build()).to_rgba8();
    let (_, report) = s.analyze(img);
    for cap in [caps::REGION, caps::QR_CODE, caps::URL] {
        assert!(report.has(&cap), "missing {cap}: {:?}", report.findings);
    }
    let p = s.palette(&report, &UsageStore::default(), &[]);
    let ids = primary_ids(&p);
    assert_eq!(ids[0], "core.url.open", "{ids:?}");
    assert!(ids.contains(&"core.url.copy".to_string()), "{ids:?}");
    assert!(p.primary.len() <= 5);
    assert_eq!(p.by_key('o').unwrap().target, Target::Action("core.url.open".into()));
    // Baseline region actions are still available.
    assert!(p.all.iter().any(|e| e.target == Target::Action("core.region.pin".into())));
}

#[test]
fn unknown_region_still_offers_screenshot_actions() {
    let s = setup(None, Settings::default());
    let img = RgbaImage::from_fn(200, 120, |x, y| Rgba([(x * 255 / 200) as u8, (y * 255 / 120) as u8, 128, 255]));
    let (_, report) = s.analyze(img);
    let p = s.palette(&report, &UsageStore::default(), &[]);
    let ids = primary_ids(&p);
    for id in ["core.region.copy", "core.region.save", "core.region.pin"] {
        assert!(ids.contains(&id.to_string()), "{ids:?}");
    }
}

#[test]
fn flat_color_selection() {
    let s = setup(None, Settings::default());
    let (_, report) = s.analyze(RgbaImage::from_pixel(12, 12, Rgba([0x18, 0x18, 0x1B, 255])));
    let color = report.first(&caps::COLOR).unwrap();
    assert!(color.details.contains(&("HEX".into(), "#18181B".into())));
    let p = s.palette(&report, &UsageStore::default(), &[]);
    assert_eq!(p.primary[0].target, Target::Action("core.color.copy-hex".into()));
}

#[test]
fn ocr_text_flows_into_structured_recognizers_and_actions_run() {
    let s = setup(Some("Error at /home/alice/app/src/db.rs:42\nsudo pacman -S package\nContact ops@example.com or +1 415-555-0100"), Settings::default());
    let (sel, report) = s.analyze(texty());
    for cap in [caps::TEXT, caps::PATH, caps::COMMAND, caps::EMAIL, caps::PHONE] {
        assert!(report.has(&cap), "missing {cap}");
    }
    let host = RecordingHost::all();
    let params = serde_json::Map::new();
    let cx = |confirmed| InvokeContext {
        registry: s.engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &s.settings,
        selection: Some(&sel),
        params: &params,
        confirmed,
    };

    // Running an OCR'd command always requires confirmation, listing its risks.
    let cmd = report.first(&caps::COMMAND).unwrap();
    match invoke("core.command.run", cmd.id, &cx(false)).unwrap() {
        Invocation::NeedsConfirmation(req) => {
            assert_eq!(req.subject, "sudo pacman -S package");
            assert!(req.reasons.iter().any(|r| r.contains("elevated privileges")), "{:?}", req.reasons);
        }
        other => panic!("expected confirmation, got {other:?}"),
    }
    assert!(host.calls().is_empty(), "nothing may run before confirmation");
    invoke("core.command.run", cmd.id, &cx(true)).unwrap();
    assert_eq!(host.take_calls(), vec![HostCall::Terminal { cwd: None, command: Some("sudo pacman -S package".into()), execute: true }]);

    // Pure transforms invoked from the palette copy their result.
    let text = report.first(&caps::TEXT).unwrap();
    let Invocation::Done(out) = invoke("core.text.upper", text.id, &cx(false)).unwrap() else { panic!() };
    assert!(out.message.unwrap().contains("copied"));
    assert!(matches!(&host.take_calls()[0], HostCall::ClipboardText(t) if t.starts_with("ERROR AT")));

    let email = report.first(&caps::EMAIL).unwrap();
    invoke("core.email.compose", email.id, &cx(false)).unwrap();
    assert_eq!(host.take_calls(), vec![HostCall::OpenUri { uri: "mailto:ops@example.com".into(), private: false }]);
}

#[test]
fn secrets_suppress_outbound_actions() {
    let s = setup(Some("export GITHUB_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\ncurl https://api.github.com/user"), Settings::default());
    let (sel, report) = s.analyze(texty());
    assert!(report.has(&caps::SECRET));
    let p = s.palette(&report, &UsageStore::default(), &[]);
    let text = report.first(&caps::TEXT).unwrap();
    // Searching the text would send the token to a search engine: not offered.
    assert!(!p.all.iter().any(|e| e.finding == text.id && e.target == Target::Action("core.text.search".into())));
    // The URL itself is fine to open.
    let url = report.first(&caps::URL).unwrap();
    assert!(p.all.iter().any(|e| e.finding == url.id && e.target == Target::Action("core.url.open".into())));
    // Even direct invocation is blocked.
    let host = RecordingHost::all();
    let params = serde_json::Map::new();
    let cx = InvokeContext {
        registry: s.engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &s.settings,
        selection: Some(&sel),
        params: &params,
        confirmed: true,
    };
    assert!(invoke("core.text.search", text.id, &cx).is_err());
    assert!(host.calls().is_empty());
    // Redacted copy is offered for the secret.
    let secret = report.first(&caps::SECRET).unwrap();
    assert!(p.all.iter().any(|e| e.finding == secret.id && e.target == Target::Action("core.secret.copy-redacted".into())));
}

#[test]
fn ambiguous_dates_require_a_choice() {
    let s = setup(Some("Launch on 05/03/2027"), Settings::default());
    let (sel, report) = s.analyze(texty());
    let date = report.first(&caps::DATE_TIME).unwrap();
    let host = RecordingHost::all();
    let mut params = serde_json::Map::new();
    let res = invoke(
        "core.date.event",
        date.id,
        &InvokeContext {
            registry: s.engine.registry(),
            findings: &report.findings,
            host: &host,
            settings: &s.settings,
            selection: Some(&sel),
            params: &params,
            confirmed: false,
        },
    )
    .unwrap();
    let Invocation::NeedsChoice(choices) = res else { panic!("expected a choice, got {res:?}") };
    assert_eq!(choices.len(), 2);
    assert!(choices[0].label.contains("5 March 2027"), "{:?}", choices);
    params.insert("choice".into(), serde_json::json!(1));
    invoke(
        "core.date.event",
        date.id,
        &InvokeContext {
            registry: s.engine.registry(),
            findings: &report.findings,
            host: &host,
            settings: &s.settings,
            selection: Some(&sel),
            params: &params,
            confirmed: false,
        },
    )
    .unwrap();
    assert!(matches!(&host.calls()[0], HostCall::SaveFile { name, mime, .. } if mime == "text/calendar" && name.contains("May")));
}

#[test]
fn usage_learning_reorders_actions() {
    let s = setup(Some("https://example.com/docs"), Settings::default());
    let (_, report) = s.analyze(texty());
    let before = s.palette(&report, &UsageStore::default(), &[]);
    let qr_rank = |p: &Palette| p.all.iter().position(|e| e.target == Target::Action("core.url.qr".into())).unwrap();
    let mut usage = UsageStore::default();
    for _ in 0..10 {
        usage.record("url", "core.url.qr", 0);
    }
    let after = s.palette(&report, &usage, &[]);
    assert!(qr_rank(&after) < qr_rank(&before));
}

#[test]
fn default_chains_validate_and_run() {
    let s = setup(Some("The quick brown\nfox jumps."), Settings::default());
    let chains = lens_actions::default_chains();
    for c in &chains {
        c.validate(s.engine.registry()).unwrap_or_else(|e| panic!("{}: {e}", c.name));
    }
    let (sel, report) = s.analyze(texty());
    let p = s.palette(&report, &UsageStore::default(), &chains);
    assert!(p.all.iter().any(|e| e.target == Target::Chain("clean-copy".into())));
    assert!(!p.all.iter().any(|e| e.target == Target::Chain("extract-table".into())), "no table in selection");

    let host = RecordingHost::all();
    let cancel = CancelToken::new();
    let run = |chain: &Chain, confirmed| {
        chain.run(ChainRunContext {
            registry: s.engine.registry(),
            findings: &report.findings,
            host: &host,
            settings: &s.settings,
            selection: Some(&sel),
            cancel: &cancel,
            confirmed,
        })
    };
    run(&chains[0], false).unwrap();
    assert_eq!(host.take_calls(), vec![HostCall::ClipboardText("The quick brown fox jumps.".into())]);
    run(&chains[1], false).unwrap();
    assert!(matches!(&host.take_calls()[0], HostCall::SaveFile { name, mime, .. } if name.ends_with(".webp") && mime == "image/webp"));
}

#[test]
fn chain_type_errors_are_caught() {
    let s = setup(None, Settings::default());
    let bad = Chain {
        id: "bad".into(),
        name: "Bad".into(),
        steps: vec![ChainStep::Take { capability: caps::URL }, ChainStep::Run { action: "core.image.resize".into(), params: Default::default() }],
    };
    let err = bad.validate(s.engine.registry()).unwrap_err();
    assert_eq!(err.step, 1);
    let dangerous = Chain {
        id: "d".into(),
        name: "Danger".into(),
        steps: vec![ChainStep::Take { capability: caps::COMMAND }, ChainStep::Run { action: "core.command.run".into(), params: Default::default() }],
    };
    assert!(dangerous.validate(s.engine.registry()).unwrap().needs_confirmation);
}
