//! Standalone snapshots, registry updates and real mock-peer consumer checks.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcade_link::manifest::write_manifest;
use arcade_link::{ids, Content, InvokeRequest, Locations, Manifest};
use image::{Rgba, RgbaImage};
use lens_actions::arcade::{self, Arcade};
use lens_core::action::{Effects, Item, Params};
use lens_core::cancel::CancelToken;
use lens_core::finding::FindingId;
use lens_core::host::{HostCall, HostFeatures, RecordingHost};
use lens_core::palette::{self, Invocation, InvokeContext, Palette, PaletteInput};
use lens_core::usage::UsageStore;
use lens_core::value::{ImageValue, SecretValue, Sensitive};
use lens_core::{caps, Finding, Registry, Settings, Value};
use serde_json::json;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("lens-consumer-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn locations(&self) -> Locations {
        Locations::under(&self.0)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn fixture(name: &str) -> PathBuf {
    // Copies of Arcade-link/fixtures at the Link tag in Cargo.toml (see VENDORED).
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(format!("{name}.json"))
}

fn manifest(loc: &Locations, name: &str) -> Manifest {
    let f: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture(name)).unwrap()).unwrap();
    let mut m = Manifest::new(f["id"].as_str().unwrap(), "test", std::env::current_exe().unwrap().to_str().unwrap());
    m.actions = serde_json::from_value(f["actions"].clone()).unwrap();
    m.shortcuts = serde_json::from_value(f.get("shortcuts").cloned().unwrap_or(json!([]))).unwrap();
    write_manifest(loc, &m).unwrap();
    m
}

fn finding(id: u32, capability: lens_core::Capability, value: Value) -> Finding {
    Finding { id: FindingId(id), capability, value, confidence: 1.0, recognizer: "fixture".into(), derived_from: None, span: None, details: vec![] }
}

fn region() -> Finding {
    finding(1, caps::REGION, Value::Image(ImageValue { image: Arc::new(RgbaImage::from_pixel(80, 60, Rgba([20, 40, 60, 255]))), origin: None }))
}

fn build(registry: &Registry, findings: &[Finding], settings: &Settings) -> Palette {
    palette::build_palette(&PaletteInput { registry, findings, settings, usage: &UsageStore::default(), host: HostFeatures::all(), chains: &[], now: 0 })
}

fn snapshot(palette: &Palette) -> serde_json::Value {
    let rows = |rows: &[lens_core::palette::PaletteEntry]| {
        rows.iter()
            .map(|e| {
                json!({
                    "action": e.target.usage_key(), "finding": e.finding.0, "label": e.label, "key": e.key,
                    "preview": e.preview, "safety": format!("{:?}", e.safety), "asks": e.needs_confirmation, "disabled": e.disabled_reason
                })
            })
            .collect::<Vec<_>>()
    };
    json!({ "primary": rows(&palette.primary), "all": rows(&palette.all), "default": palette.default_index })
}

fn wait(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < end, "timed out waiting for registry event");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn without_peers_palette_matches_standalone_fixtures_exactly() {
    let temp = Temp::new();
    let loc = temp.locations();
    let settings = Settings::default();
    let base = lens_actions::standard_registry(None).unwrap();
    let peers = arcade_link::Registry::load(&loc);
    let registry = arcade::with_peers(&base, &settings, &peers, &loc).unwrap();
    for findings in [vec![region()], vec![finding(2, caps::TEXT, Value::text("hello world"))], vec![finding(3, caps::COMMAND, Value::text("cargo test"))]] {
        assert_eq!(snapshot(&build(&base, &findings, &settings)), snapshot(&build(&registry, &findings, &settings)));
    }
    let p = build(&registry, &[region()], &settings);
    assert_eq!(
        p.primary.iter().map(|e| e.target.usage_key()).collect::<Vec<_>>(),
        ["core.region.copy", "core.region.save", "core.region.pin", "core.region.annotate", "core.region.share"]
    );
}

#[test]
fn watcher_discovers_late_peers_and_removes_disabled_actions() {
    let temp = Temp::new();
    let loc = temp.locations();
    let base = Arc::new(lens_actions::standard_registry(None).unwrap());
    let settings = Arc::new(Settings::default());
    let service = Arcade::start(base.clone(), settings.clone(), loc.clone());
    wait(|| service.revision() > 0);
    assert!(service.actions().action("arcade.box.open").is_none());
    let mut box_app = manifest(&loc, "box");
    wait(|| service.actions().action("arcade.box.open").is_some());
    let visible = service.actions();
    let p = build(&visible, &[region()], &settings);
    assert!(p.all.iter().any(|e| e.target.usage_key() == "arcade.box:arcade.image.convert#webp"));
    assert!(visible.action("arcade.box:arcade.pdf.ocr").is_none());
    let before = p.primary.clone();
    manifest(&loc, "clipboard");
    wait(|| service.actions().action("arcade.clipboard.add").is_some());
    let current = service.actions();
    let after = palette::stabilize(&before, build(&current, &[region()], &settings), true, &settings, &current);
    for old in &before {
        if let Some(index) = after.primary.iter().position(|e| e.target == old.target) {
            assert_eq!(index, before.iter().position(|e| e.target == old.target).unwrap());
        }
    }
    box_app.settings.link_enabled = false;
    box_app.actions.clear();
    write_manifest(&loc, &box_app).unwrap();
    wait(|| service.actions().action("arcade.box.open").is_none());
    let mut settings = (*settings).clone();
    settings.link.disabled_peers.push(ids::CLIPBOARD.into());
    let installed = arcade_link::Registry::load(&loc);
    assert!(arcade::with_peers(&base, &settings, &installed, &loc).unwrap().action("arcade.clipboard.add").is_none());
    settings.link.enabled = false;
    assert_eq!(
        snapshot(&build(&base, &[region()], &settings)),
        snapshot(&build(&arcade::with_peers(&base, &settings, &installed, &loc).unwrap(), &[region()], &settings))
    );
    assert_eq!(installed.shortcut_owner(ids::LENS, "alt+control+Space"), Some("Arcade Box".into()));
}

#[test]
fn sends_have_real_effects_and_secret_findings_remove_even_image_sends() {
    let temp = Temp::new();
    let loc = temp.locations();
    manifest(&loc, "clipboard");
    let settings = Settings::default();
    let base = lens_actions::standard_registry(None).unwrap();
    let r = arcade::with_peers(&base, &settings, &arcade_link::Registry::load(&loc), &loc).unwrap();
    assert!(r.action("arcade.clipboard.add").unwrap().descriptor().effects.contains(Effects::SENDS_TO_DEVICE));
    let mut findings = vec![region()];
    assert_eq!(build(&r, &findings, &settings).by_key('m').unwrap().target.usage_key(), "arcade.clipboard.add");
    findings.push(finding(
        2,
        caps::SECRET,
        Value::Secret(SecretValue { raw: Sensitive::new("synthetic-secret"), kind: "password".into(), masked: "••••".into() }),
    ));
    assert!(!build(&r, &findings, &settings).all.iter().any(|e| e.target.usage_key() == "arcade.clipboard.add"));
    let host = RecordingHost::all();
    assert!(palette::invoke(
        "arcade.clipboard.add",
        findings[0].id,
        &InvokeContext {
            registry: &r,
            findings: &findings,
            host: &host,
            settings: &settings,
            selection: None,
            params: &Params::new(),
            confirmed: true,
            cancel: None
        }
    )
    .is_err());
    assert!(host.calls().is_empty());
}

#[test]
fn oversized_inputs_are_disabled_with_standard_reason_and_no_shortcut() {
    let temp = Temp::new();
    let loc = temp.locations();
    let mut m = manifest(&loc, "clipboard");
    m.actions[0].max_bytes = Some(4);
    write_manifest(&loc, &m).unwrap();
    let settings = Settings::default();
    let r = arcade::with_peers(&lens_actions::standard_registry(None).unwrap(), &settings, &arcade_link::Registry::load(&loc), &loc).unwrap();
    for f in [finding(1, caps::TEXT, Value::text("12345")), region()] {
        let p = build(&r, std::slice::from_ref(&f), &settings);
        let e = p.all.iter().find(|e| e.target.usage_key() == "arcade.clipboard.add").unwrap();
        assert_eq!(e.disabled_reason.as_deref(), Some("Too large to send to your devices (limit 4 bytes)."));
        assert!(e.key.is_none());
        assert!(p.primary.iter().all(|e| e.disabled_reason.is_none()));
        assert!(palette::invoke(
            "arcade.clipboard.add",
            f.id,
            &InvokeContext {
                registry: &r,
                findings: &[f],
                host: &RecordingHost::all(),
                settings: &settings,
                selection: None,
                params: &Params::new(),
                confirmed: true,
                cancel: None
            }
        )
        .is_err());
    }
    let command = arcade::content(&Item::new(caps::COMMAND, Value::text("cargo test"))).unwrap();
    assert_eq!(command.kind, "text/plain");
    assert_eq!(command.hints, ["command"]);
}

#[test]
fn cached_pipelines_match_first_input_hide_interactive_and_enforce_effects() {
    let temp = Temp::new();
    let loc = temp.locations();
    let mut box_app = manifest(&loc, "box");
    let base = lens_actions::standard_registry(None).unwrap();
    let settings = Settings::default();
    let pipeline = |id: &str, kind: &str, interactive, effects| arcade::Pipeline {
        id: id.into(),
        name: id.into(),
        version: 1,
        accepts: vec![kind.into()],
        produces: vec!["file/image".into()],
        effects,
        interactive,
    };
    let offers = vec![
        pipeline("image", "file/image", false, vec!["writes-files".into()]),
        pipeline("text", "text/plain", false, vec!["sends-to-device".into(), "executes-commands".into()]),
        pipeline("interactive", "file/image", true, vec!["opens-ui".into()]),
        pipeline("empty", "", false, vec![]),
    ];
    let r = arcade::with_cached_peers(&base, &settings, &arcade_link::Registry::load(&loc), &loc, &offers).unwrap();
    assert!(r.action("arcade.box.pipeline.interactive").is_none());
    let image = build(&r, &[region()], &settings);
    assert!(image.all.iter().any(|e| e.label == "▶ image"));
    assert!(!image.all.iter().any(|e| e.label.starts_with("▶ text")));
    let text = finding(2, caps::TEXT, Value::text("synthetic text"));
    let text_palette = build(&r, std::slice::from_ref(&text), &settings);
    let entry = text_palette.all.iter().find(|e| e.label == "▶ text ↗").unwrap();
    assert!(entry.needs_confirmation);
    assert_eq!(entry.preview.as_deref(), Some("synthetic text"));
    let host = RecordingHost::all();
    assert!(matches!(
        palette::invoke(
            "arcade.box.pipeline.text",
            text.id,
            &InvokeContext {
                registry: &r,
                findings: std::slice::from_ref(&text),
                host: &host,
                settings: &settings,
                selection: None,
                params: &Params::new(),
                confirmed: false,
                cancel: None
            }
        )
        .unwrap(),
        Invocation::NeedsConfirmation(_)
    ));
    assert!(host.calls().is_empty());
    box_app.actions.iter_mut().find(|a| a.id == "box.pipeline.run").unwrap().available = false;
    write_manifest(&loc, &box_app).unwrap();
    let unavailable = arcade::with_cached_peers(&base, &settings, &arcade_link::Registry::load(&loc), &loc, &offers).unwrap();
    assert!(unavailable.action("arcade.box.pipeline.image").is_none());
}

#[test]
fn disabled_peers_and_live_unavailability_remove_cached_entries() {
    let temp = Temp::new();
    let loc = temp.locations();
    let mut clipboard = manifest(&loc, "clipboard");
    let settings = Settings::default();
    let base = lens_actions::standard_registry(None).unwrap();
    let r = arcade::with_peers(&base, &settings, &arcade_link::Registry::load(&loc), &loc).unwrap();
    let before = build(&r, &[region()], &settings);
    clipboard.actions.iter_mut().find(|a| a.id == "clipboard.add").unwrap().available = false;
    write_manifest(&loc, &clipboard).unwrap();
    let r = arcade::with_peers(&base, &settings, &arcade_link::Registry::load(&loc), &loc).unwrap();
    let after = palette::stabilize(&before.primary, build(&r, &[region()], &settings), true, &settings, &r);
    assert!(after.all.iter().all(|e| e.target.usage_key() != "arcade.clipboard.add"));
    assert!(after.primary.iter().all(|e| e.target.usage_key() != "arcade.clipboard.add"));
    let mut disabled = settings.clone();
    disabled.link.disabled_peers.push(ids::CLIPBOARD.into());
    let r = arcade::with_peers(&base, &disabled, &arcade_link::Registry::load(&loc), &loc).unwrap();
    assert!(r.action("arcade.clipboard.add").is_none());
    assert!(r.action("core.region.send").is_some());
}

struct Mock(Child);
impl Drop for Mock {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

fn mock(loc: &Locations, name: &str, file: &Path, log: &Path) -> Mock {
    let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Arcade-link/target/debug/arcade-link");
    let child = Command::new(cli)
        .args(["mock", "--as", name, "--actions"])
        .arg(file)
        .env("ARCADE_HOME", loc.registry.parent().unwrap())
        .env("ARCADE_MOCK_LOG", log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait(|| loc.runtime.join(format!("arcade.{name}.endpoint")).exists());
    Mock(child)
}

#[test]
#[ignore = "Run through Arcade-link/tools/e2e.py run to isolate mock processes"]
fn real_mock_peers_invoke_outputs_wheel_payloads_private_cancel_crash_and_timeout() {
    assert!(std::env::var_os("ARCADE_E2E_INNER").is_some(), "use the isolated e2e runner");
    let temp = Temp::new();
    let loc = temp.locations();
    let log = temp.0.join("invokes.jsonl");
    // A receiver owns its outputs. The shared mock's default echoes inputs,
    // which are deleted when their handoff job finishes; never race that Drop
    // while testing consumption of a Box output.
    let output = temp.0.join("box-output.png");
    RgbaImage::from_pixel(80, 60, Rgba([20, 40, 60, 255])).save(&output).unwrap();
    let mut box_fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture("box")).unwrap()).unwrap();
    for action in box_fixture["actions"].as_array_mut().unwrap() {
        if matches!(action["id"].as_str(), Some("box:arcade.image.convert#webp" | "box.pipeline.run")) {
            action["mock"]["result"] = json!({"outputs": [{"type": "file/image", "path": output}], "message": "Converted"});
        }
    }
    let box_file = temp.0.join("box.json");
    std::fs::write(&box_file, box_fixture.to_string()).unwrap();
    let _box = mock(&loc, "box", &box_file, &log);
    let _wheel = mock(&loc, "wheel", &fixture("wheel"), &log);
    let _clipboard = mock(&loc, "clipboard", &fixture("clipboard"), &log);
    let _look = mock(&loc, "look", &fixture("look"), &log);
    let base = Arc::new(lens_actions::standard_registry(None).unwrap());
    let settings = Arc::new(Settings::default());
    let service = Arcade::start(base, settings.clone(), loc.clone());
    wait(|| service.revision() > 0);
    let r = service.actions();
    assert!(r.action("arcade.box.pipeline.p-web-image").is_some());
    assert!(r.action("arcade.box.pipeline.p-optimized-screenshot").is_none());
    let f = region();
    let host = RecordingHost::all();
    let run = |id: &str, findings: &[Finding]| {
        palette::invoke(
            id,
            findings[0].id,
            &InvokeContext { registry: &r, findings, host: &host, settings: &settings, selection: None, params: &Params::new(), confirmed: true, cancel: None },
        )
        .unwrap()
    };
    assert!(matches!(run("arcade.box:arcade.image.convert#webp", std::slice::from_ref(&f)), Invocation::Done(_)));
    assert!(host.calls().iter().any(|c| matches!(c, HostCall::ClipboardImage { width: 80, height: 60 })));
    run("arcade.box.open", std::slice::from_ref(&f));
    run("arcade.clipboard.add", std::slice::from_ref(&f));
    run("arcade.box.pipeline.p-web-image", std::slice::from_ref(&f));
    let cmd = finding(3, caps::COMMAND, Value::text("cargo test"));
    run("arcade.wheel.add_action", &[cmd]);
    run(&arcade::preset_wheel_id("arcade.box:arcade.image.convert#webp"), &[f]);
    let requests: Vec<InvokeRequest> = std::fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let command = requests.iter().find(|r| r.action == "wheel.add_action" && r.inputs[0].kind == "text/plain").unwrap();
    assert_eq!(requests.iter().find(|r| r.action == "box.pipeline.run").unwrap().options["pipeline"], "p-web-image");
    assert_eq!(command.inputs[0].hints, ["command"]);
    let preset = requests.iter().find(|r| r.inputs.first().is_some_and(|c| c.kind == "structured/arcade-action")).unwrap().inputs[0].data.as_ref().unwrap();
    assert_eq!(preset["app"], ids::BOX);
    assert_eq!(preset["action"], "box:arcade.image.convert#webp");
    assert_eq!(preset["input"], "lens-selection");
    let png = temp.0.join("look.png");
    RgbaImage::new(2, 2).save(&png).unwrap();
    assert!(service.quick_look(&settings, &png).unwrap().is_ok());
    assert!(service.send_to_devices(&settings, Some("hello"), None).unwrap().is_ok());

    // Script a real peer's Private mode, then a cooperative long job.
    drop(_clipboard);
    std::fs::remove_file(loc.runtime.join("arcade.clipboard.endpoint")).ok();
    let private = temp.0.join("private.json");
    std::fs::write(&private, json!({"actions":[{"id":"clipboard.add","title":"Send", "accepts":["text/*"],"effects":["sends-to-device"],"mock":{"error":"denied","reason":"private_mode"}}]}).to_string()).unwrap();
    let private_peer = mock(&loc, "clipboard", &private, &log);
    wait(|| service.snapshot().installed.get(ids::CLIPBOARD).is_some_and(|m| m.actions[0].title == "Send"));
    let failure = service.send_to_devices(&settings, Some("hello"), None).unwrap().unwrap_err().to_string();
    assert!(failure.contains("Arcade Clipboard is in Private mode."), "{failure}");
    drop(private_peer);
    std::fs::remove_file(loc.runtime.join("arcade.clipboard.endpoint")).ok();
    std::fs::write(&private, json!({"actions":[{"id":"clipboard.add","title":"Slow", "accepts":["text/*"],"mock":{"steps":50,"stepMs":20}}]}).to_string())
        .unwrap();
    let _slow = mock(&loc, "clipboard", &private, &log);
    let manifest = arcade_link::Registry::load(&loc).get(ids::CLIPBOARD).unwrap().clone();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        token.cancel();
    });
    let req = InvokeRequest::new("clipboard.add", ids::LENS).input(Content::plain("test"));
    assert!(matches!(arcade::invoke(&loc, &manifest, req.clone(), cancel, Duration::from_secs(3)), Err(lens_core::LensError::Cancelled)));
    assert!(arcade::invoke(&loc, &manifest, req, CancelToken::new(), Duration::from_millis(100)).unwrap_err().to_string().contains("didn't respond in time"));
    let manifest = arcade_link::Registry::load(&loc).get(ids::BOX).unwrap().clone();
    let req = InvokeRequest::new("box:arcade.media.inspect", ids::LENS).input(Content::file(&png));
    assert!(arcade::invoke(&loc, &manifest, req, CancelToken::new(), Duration::from_secs(3)).unwrap_err().to_string().contains("isn't running"));
    println!(
        "real mocks: image output copied, Box open, Clipboard send/Private mode, Wheel command/preset, Look, cancellation, timeout, mid-job crash verified"
    );
}
