//! End-to-end plugin tests with real plugin processes (Python 3).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::host::{HostCall, HostFeatures, RecordingHost};
use lens_core::palette::{build_palette, invoke, Invocation, InvokeContext, PaletteInput, Target};
use lens_core::recognizer::{NullEnvironment, RecognizerDescriptor};
use lens_core::registry::PluginManifest;
use lens_core::usage::UsageStore;
use lens_core::{caps, Capability, Cost, Detection, Engine, Finding, LensError, RecognizeContext, Recognizer, Registry, Selection, Settings, Value};

fn python() -> bool {
    std::process::Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Stands in for OCR: turns the region into fixed text.
struct FakeOcr(&'static str);
impl Recognizer for FakeOcr {
    fn descriptor(&self) -> RecognizerDescriptor {
        RecognizerDescriptor { id: "test.ocr".into(), consumes: vec![caps::REGION], produces: vec![caps::TEXT], cost: Cost::Cheap }
    }
    fn recognize(&self, _: &Finding, _: &RecognizeContext) -> lens_core::Result<Vec<Detection>> {
        Ok(vec![Detection::new(caps::TEXT, Value::text(self.0))])
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

fn plugins_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lens-plugins-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    copy_dir(&examples.join("isbn"), &dir.join("isbn"));
    dir
}

fn registry(text: &'static str, dir: &Path, enabled: &[String]) -> (Registry, Vec<lens_plugins::PluginStatus>) {
    let mut r = Registry::new();
    r.register_plugin(PluginManifest::first_party("test", "Test"), |p| p.recognizer(FakeOcr(text))).unwrap();
    let status = lens_plugins::load_enabled(&mut r, dir, enabled);
    (r, status)
}

#[test]
fn isbn_plugin_recognizes_and_acts() {
    if !python() {
        eprintln!("skipping: python3 not available");
        return;
    }
    let dir = plugins_dir("isbn");
    // Discovered but disabled by default: nothing registered.
    let (r, status) = registry("ISBN 978-0-306-40615-7", &dir, &[]);
    assert_eq!(status.len(), 1);
    assert!(!status[0].enabled);
    assert!(r.action("dev.example.isbn.lookup").is_none());

    let (r, status) = registry("Book: ISBN 978-0-306-40615-7 and 0-306-40615-X (bad checksum)", &dir, &["dev.example.isbn".into()]);
    assert!(status[0].enabled && status[0].error.is_none(), "{status:?}");
    let engine = Engine::new(Arc::new(r), Arc::new(NullEnvironment), Arc::new(Settings::default()));
    let sel = Selection::from_image(RgbaImage::new(40, 20));
    let report = engine.analyze_blocking(sel.clone(), CancelToken::new());
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let isbn: Vec<&Finding> = report.with(&Capability::custom("dev.example.isbn")).collect();
    assert_eq!(isbn.len(), 1, "{:?}", report.findings);
    assert_eq!(isbn[0].value.as_text().unwrap(), "9780306406157");

    let settings = Settings::default();
    let palette = build_palette(&PaletteInput {
        registry: engine.registry(),
        findings: &report.findings,
        settings: &settings,
        usage: &UsageStore::default(),
        host: HostFeatures::all(),
        chains: &[],
        now: 0,
    });
    assert_eq!(palette.default_entry().unwrap().target, Target::Action("dev.example.isbn.lookup".into()));

    let host = RecordingHost::all();
    let params = serde_json::Map::new();
    let cx = InvokeContext {
        registry: engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &settings,
        selection: Some(&sel),
        params: &params,
        confirmed: false,
    };
    let Invocation::Done(out) = invoke("dev.example.isbn.lookup", isbn[0].id, &cx).unwrap() else { panic!() };
    assert_eq!(out.message.as_deref(), Some("Looking up 9780306406157"));
    assert_eq!(host.take_calls(), vec![HostCall::OpenUri { uri: "https://openlibrary.org/isbn/9780306406157".into(), private: false }]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn undeclared_effects_are_refused() {
    if !python() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("lens-plugins-sneaky-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sneaky")).unwrap();
    std::fs::write(
        dir.join("sneaky/plugin.toml"),
        "[plugin]\nid='dev.example.sneaky'\nname='Sneaky'\nversion='1'\napi_version=1\nexecutable='p.py'\npermissions=['read-text']\n\n[[actions]]\nid='dev.example.sneaky.go'\nlabel='Go'\naccepts=['text']\n",
    )
    .unwrap();
    // The action declares no effects but asks Lens to put something on the clipboard.
    std::fs::write(
        dir.join("sneaky/p.py"),
        "import json,sys\nfor l in sys.stdin:\n r=json.loads(l)\n print(json.dumps({'id':r['id'],'result':{'effects':[{'type':'copy-text','text':'pwned'}]}}),flush=True)\n",
    )
    .unwrap();
    let (r, status) = registry("hello", &dir, &["dev.example.sneaky".into()]);
    assert!(status[0].error.is_none(), "{status:?}");
    let engine = Engine::new(Arc::new(r), Arc::new(NullEnvironment), Arc::new(Settings::default()));
    let sel = Selection::from_image(RgbaImage::new(10, 10));
    let report = engine.analyze_blocking(sel.clone(), CancelToken::new());
    let text = report.first(&caps::TEXT).unwrap();
    let host = RecordingHost::all();
    let settings = Settings::default();
    let params = serde_json::Map::new();
    let cx = InvokeContext {
        registry: engine.registry(),
        findings: &report.findings,
        host: &host,
        settings: &settings,
        selection: Some(&sel),
        params: &params,
        confirmed: false,
    };
    let err = invoke("dev.example.sneaky.go", text.id, &cx).unwrap_err();
    assert!(matches!(err, LensError::Blocked(_)), "{err:?}");
    assert!(host.calls().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn manifest_cannot_exceed_its_permissions() {
    let dir = std::env::temp_dir().join(format!("lens-plugins-greedy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("greedy")).unwrap();
    std::fs::write(
        dir.join("greedy/plugin.toml"),
        "[plugin]\nid='dev.example.greedy'\nname='Greedy'\nversion='1'\napi_version=1\nexecutable='p.py'\npermissions=['read-text']\n\n[[actions]]\nid='dev.example.greedy.run'\nlabel='Run'\naccepts=['text']\neffects=['executes-commands']\n",
    )
    .unwrap();
    let (r, status) = registry("x", &dir, &["dev.example.greedy".into()]);
    assert!(status[0].error.as_deref().is_some_and(|e| e.contains("did not declare")), "{status:?}");
    assert!(r.action("dev.example.greedy.run").is_none());
    let _ = std::fs::remove_dir_all(dir);
}
