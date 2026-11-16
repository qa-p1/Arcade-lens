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
        cancel: None,
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
        cancel: None,
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
            cancel: None,
        },
    )
    .unwrap();
    let Invocation::NeedsChoice(choices) = res else { panic!("expected a choice, got {res:?}") };
    assert_eq!(choices.len(), 2);
    assert!(choices[0].label.contains("5 March 2027"), "{choices:?}");
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
            cancel: None,
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

#[test]
fn download_is_offered_only_for_http_and_https_addresses() {
    let s = setup(Some("https://example.com/a.zip\nftp://example.com/b.zip\nwss://example.com/socket"), Settings::default());
    let (_, report) = s.analyze(texty());
    let p = s.palette(&report, &UsageStore::default(), &[]);
    let download = |scheme: &str| {
        let finding =
            report.findings.iter().find(|f| matches!(&f.value, lens_core::Value::Url(u) if u.scheme == scheme)).unwrap_or_else(|| panic!("no {scheme} URL"));
        p.all.iter().any(|e| e.finding == finding.id && e.target == Target::Action("core.url.download".into()))
    };
    assert!(download("https"));
    assert!(!download("ftp"));
    assert!(!download("wss"));
}

/// The commands Lens starts in a terminal without asking are built from a validated domain or IP address, never from
/// the raw text around it.
#[test]
fn terminal_commands_for_hosts_carry_nothing_but_the_host() {
    let s = setup(Some("see evil.com$(reboot) and `id`.example.org then 10.0.0.1;ls"), Settings::default());
    let (sel, report) = s.analyze(texty());
    let host = RecordingHost::all();
    let params = serde_json::Map::new();
    let cx = InvokeContext {
        registry: s.engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &s.settings,
        selection: Some(&sel),
        params: &params,
        confirmed: false,
        cancel: None,
    };
    let mut commands = Vec::new();
    for (capability, actions) in
        [(caps::DOMAIN, &["core.domain.ping", "core.domain.dns"][..]), (caps::IP_ADDRESS, &["core.ip.ping", "core.ip.ssh", "core.ip.traceroute"][..])]
    {
        for finding in report.findings.iter().filter(|f| f.capability == capability) {
            for action in actions {
                invoke(action, finding.id, &cx).unwrap();
                commands.extend(host.take_calls());
            }
        }
    }
    assert!(commands.len() >= 4, "{commands:?}");
    for call in &commands {
        let HostCall::Terminal { command: Some(command), .. } = call else { panic!("{call:?}") };
        assert!(command.chars().all(|c| c.is_ascii_alphanumeric() || " .:+-".contains(c)), "{command}");
    }
}

#[test]
fn copying_a_secret_asks_first_and_the_redacted_copy_does_not() {
    let s = setup(Some("export GITHUB_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"), Settings::default());
    let (sel, report) = s.analyze(texty());
    let secret = report.first(&caps::SECRET).unwrap();
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
        cancel: None,
    };
    assert!(matches!(invoke("core.secret.copy", secret.id, &cx(false)).unwrap(), Invocation::NeedsConfirmation(_)));
    assert!(host.calls().is_empty());
    let Invocation::Done(_) = invoke("core.secret.copy-redacted", secret.id, &cx(false)).unwrap() else { panic!("the redacted copy needs no confirmation") };
    let calls = host.take_calls();
    assert!(matches!(&calls[..], [HostCall::ClipboardText(t)] if !t.contains("aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789")), "{calls:?}");
}

/// Reverse image search hands the pixels to the user's clipboard and opens the configured page; Lens sends nothing.
#[test]
fn reverse_image_search_is_off_until_configured_and_never_uploads() {
    let img = || RgbaImage::from_pixel(40, 30, Rgba([200, 30, 30, 255]));
    let offered = |s: &Setup, report: &AnalysisReport| {
        s.palette(report, &UsageStore::default(), &[]).all.iter().any(|e| e.target == Target::Action("core.image.reverse-search".into()))
    };
    let off = setup(None, Settings::default());
    let (_, report) = off.analyze(img());
    assert!(!offered(&off, &report));

    let mut settings = Settings::default();
    settings.providers.reverse_image_search = Some("https://images.example/search".into());
    let on = setup(None, settings);
    let (sel, report) = on.analyze(img());
    assert!(offered(&on, &report));
    let region = report.first(&caps::REGION).unwrap();
    let host = RecordingHost::all();
    let params = serde_json::Map::new();
    let cx = InvokeContext {
        registry: on.engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &on.settings,
        selection: Some(&sel),
        params: &params,
        confirmed: false,
        cancel: None,
    };
    invoke("core.image.reverse-search", region.id, &cx).unwrap();
    assert_eq!(
        host.take_calls(),
        vec![HostCall::ClipboardImage { width: 40, height: 30 }, HostCall::OpenUri { uri: "https://images.example/search".into(), private: false }]
    );
}

#[test]
fn a_chain_with_a_dangerous_step_does_not_run_until_confirmed() {
    let s = setup(Some("$ sudo pacman -S package"), Settings::default());
    let (sel, report) = s.analyze(texty());
    let chain = Chain {
        id: "run".into(),
        name: "Run it".into(),
        steps: vec![ChainStep::Take { capability: caps::COMMAND }, ChainStep::Run { action: "core.command.run".into(), params: Default::default() }],
    };
    let host = RecordingHost::all();
    let cancel = CancelToken::new();
    let run = |confirmed| {
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
    assert!(matches!(run(false), Err(lens_core::LensError::Blocked(_))));
    assert!(host.calls().is_empty(), "nothing may run before confirmation");
    run(true).unwrap();
    assert_eq!(host.take_calls(), vec![HostCall::Terminal { cwd: None, command: Some("sudo pacman -S package".into()), execute: true }]);
}

#[test]
fn a_confirmed_outbound_chain_is_refused_when_the_selection_holds_a_secret() {
    let secret_text = "export GITHUB_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\ncurl https://api.github.com/user";
    let chain = Chain {
        id: "search".into(),
        name: "Search it".into(),
        steps: vec![ChainStep::Take { capability: caps::TEXT }, ChainStep::Run { action: "core.text.search".into(), params: Default::default() }],
    };
    let host = RecordingHost::all();
    let cancel = CancelToken::new();
    let run = |s: &Setup, sel: &Selection, report: &AnalysisReport, confirmed| {
        chain.run(ChainRunContext {
            registry: s.engine.registry(),
            findings: &report.findings,
            host: &host,
            settings: &s.settings,
            selection: Some(sel),
            cancel: &cancel,
            confirmed,
        })
    };

    // Guard on (the default): refused, confirmed or not, and not offered.
    let s = setup(Some(secret_text), Settings::default());
    let (sel, report) = s.analyze(texty());
    assert!(report.has(&caps::SECRET));
    assert!(matches!(run(&s, &sel, &report, true), Err(lens_core::LensError::Blocked(_))), "a confirmation must not override the guard");
    assert!(matches!(run(&s, &sel, &report, false), Err(lens_core::LensError::Blocked(_))));
    assert!(host.calls().is_empty(), "nothing may be sent");
    let p = s.palette(&report, &UsageStore::default(), std::slice::from_ref(&chain));
    assert!(!p.all.iter().any(|e| e.target == Target::Chain("search".into())), "the chain is not offered");

    // A chain with no outbound step still runs with the guard on.
    let local = Chain {
        id: "copy".into(),
        name: "Copy".into(),
        steps: vec![ChainStep::Take { capability: caps::TEXT }, ChainStep::Run { action: "core.copy".into(), params: Default::default() }],
    };
    let p = s.palette(&report, &UsageStore::default(), std::slice::from_ref(&local));
    assert!(p.all.iter().any(|e| e.target == Target::Chain("copy".into())));
    local
        .run(ChainRunContext {
            registry: s.engine.registry(),
            findings: &report.findings,
            host: &host,
            settings: &s.settings,
            selection: Some(&sel),
            cancel: &cancel,
            confirmed: false,
        })
        .unwrap();

    // Guard off: the same chain runs once confirmed.
    let mut settings = Settings::default();
    settings.privacy.guard_secrets = false;
    let s = setup(Some(secret_text), settings);
    let (sel, report) = s.analyze(texty());
    host.take_calls();
    run(&s, &sel, &report, true).unwrap();
    assert!(!host.calls().is_empty(), "with the guard off the confirmed chain runs");
}
