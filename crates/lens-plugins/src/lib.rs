//! Out-of-process plugins for Arcade Lens.
//!
//! A plugin is a directory with a `plugin.toml` manifest and an executable
//! that speaks newline-delimited JSON over stdin/stdout (see PROTOCOL.md).
//!
//! Security model:
//! * Plugins are discovered but **disabled until the user enables them**.
//! * The manifest declares permissions. Data access (`read-text`,
//!   `read-pixels`) bounds what Lens sends to the plugin; effect
//!   permissions bound what its actions may declare (enforced by the core
//!   registry) and which host effects Lens will perform on its behalf.
//! * Plugins never get Lens' host directly: they return requested effects
//!   (copy text, open a URI, save a file, notify) and Lens performs only
//!   those the action declared.
//! * The plugin process itself runs with the user's privileges, like any
//!   program they install; enabling a plugin is an explicit trust decision.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lens_core::action::{Action, ActionContext, ActionDescriptor, ActionGroup, ActionOutcome, Effects, Item, Produces};
use lens_core::host::{HostFeatures, SaveRequest};
use lens_core::recognizer::{Cost, RecognizeContext, Recognizer, RecognizerDescriptor};
use lens_core::registry::{PluginManifest, PLUGIN_API_VERSION};
use lens_core::value::UrlValue;
use lens_core::{caps, Capability, Detection, Finding, LensError, Registry, Result, Value};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub plugin: PluginInfo,
    #[serde(default)]
    pub capabilities: Vec<CapabilityDecl>,
    #[serde(default)]
    pub recognizers: Vec<RecognizerDecl>,
    #[serde(default)]
    pub actions: Vec<ActionDecl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub description: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapabilityDecl {
    pub name: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecognizerDecl {
    pub id: String,
    pub consumes: Vec<String>,
    pub produces: Vec<String>,
    #[serde(default)]
    pub cost: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionDecl {
    pub id: String,
    pub label: String,
    pub accepts: Vec<String>,
    #[serde(default)]
    pub produces: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub key: Option<char>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub error: Option<String>,
}

pub fn effect(name: &str) -> Option<Effects> {
    Some(match name {
        "clipboard" => Effects::CLIPBOARD,
        "writes-files" => Effects::WRITES_FILES,
        "overwrites-files" => Effects::OVERWRITES_FILES,
        "deletes-files" => Effects::DELETES_FILES,
        "network" => Effects::NETWORK,
        "uploads-content" => Effects::UPLOADS_CONTENT,
        "sends-to-device" => Effects::SENDS_TO_DEVICE,
        "launch-apps" => Effects::LAUNCHES_APP,
        "executes-commands" => Effects::EXECUTES_COMMAND,
        "privileged" => Effects::PRIVILEGED,
        "window-control" => Effects::WINDOW_CONTROL,
        "persists" => Effects::PERSISTS,
        _ => return None,
    })
}

fn feature(name: &str) -> Option<HostFeatures> {
    Some(match name {
        "clipboard" => HostFeatures::CLIPBOARD_TEXT,
        "clipboard-image" => HostFeatures::CLIPBOARD_IMAGE,
        "open-uri" => HostFeatures::OPEN_URI,
        "open-path" => HostFeatures::OPEN_PATH,
        "save-file" => HostFeatures::SAVE_FILE,
        "terminal" => HostFeatures::TERMINAL,
        _ => return None,
    })
}

const DATA_PERMISSIONS: &[&str] = &["read-text", "read-pixels"];

impl Manifest {
    pub fn load(dir: &Path) -> std::result::Result<Manifest, String> {
        let text = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| e.to_string())?;
        let m: Manifest = toml::from_str(&text).map_err(|e| e.to_string())?;
        if m.plugin.api_version != PLUGIN_API_VERSION {
            return Err(format!("targets plugin API v{}, this Lens provides v{PLUGIN_API_VERSION}", m.plugin.api_version));
        }
        for p in &m.plugin.permissions {
            if effect(p).is_none() && !DATA_PERMISSIONS.contains(&p.as_str()) {
                return Err(format!("unknown permission \"{p}\""));
            }
        }
        for a in &m.actions {
            for e in &a.effects {
                effect(e).ok_or_else(|| format!("action {}: unknown effect \"{e}\"", a.id))?;
            }
        }
        Ok(m)
    }

    pub fn effects(&self) -> Effects {
        self.plugin.permissions.iter().filter_map(|p| effect(p)).fold(Effects::empty(), |a, b| a | b)
    }

    pub fn can(&self, data: &str) -> bool {
        self.plugin.permissions.iter().any(|p| p == data)
    }
}

/// Every plugin directory under `dir`, with its manifest or the reason it is invalid.
pub fn discover(dir: &Path) -> Vec<(PathBuf, std::result::Result<Manifest, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.join("plugin.toml").is_file()).map(|p| (p.clone(), Manifest::load(&p))).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// ---------------------------------------------------------------- protocol

/// Minimal base64 (standard alphabet) so pixels can travel over JSON.
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

/// A lazily started plugin process. Crashes and timeouts kill it; the next
/// call starts a fresh one.
pub struct PluginProcess {
    dir: PathBuf,
    manifest: Manifest,
    proc: Mutex<Option<Proc>>,
    next: Mutex<u64>,
}

impl PluginProcess {
    pub fn new(dir: PathBuf, manifest: Manifest) -> Self {
        Self { dir, manifest, proc: Mutex::new(None), next: Mutex::new(0) }
    }

    fn spawn(&self) -> Result<Proc> {
        let exe = self.dir.join(&self.manifest.plugin.executable);
        let mut cmd = if exe.extension().is_some_and(|e| e == "py") {
            let mut c = Command::new(if cfg!(windows) { "python" } else { "python3" });
            c.arg(&exe);
            c
        } else {
            Command::new(&exe)
        };
        let mut child = cmd
            .args(&self.manifest.plugin.args)
            .current_dir(&self.dir)
            .env("ARCADE_LENS_PLUGIN_API", PLUGIN_API_VERSION.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| LensError::Failed(format!("plugin {}: {e}", self.manifest.plugin.id)))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Proc { child, stdin, lines: rx })
    }

    pub fn call(&self, method: &str, params: serde_json::Value, timeout: Duration) -> Result<serde_json::Value> {
        let mut guard = self.proc.lock().unwrap();
        if guard.as_mut().is_none_or(|p| p.child.try_wait().ok().flatten().is_some()) {
            *guard = Some(self.spawn()?);
        }
        let id = {
            let mut n = self.next.lock().unwrap();
            *n += 1;
            *n
        };
        let p = guard.as_mut().unwrap();
        let req = serde_json::json!({ "id": id, "method": method, "params": params });
        let sent = writeln!(p.stdin, "{req}").and_then(|_| p.stdin.flush());
        let fail = |guard: &mut Option<Proc>, msg: String| {
            if let Some(mut p) = guard.take() {
                let _ = p.child.kill();
            }
            Err(LensError::Failed(format!("plugin {}: {msg}", self.manifest.plugin.id)))
        };
        if let Err(e) = sent {
            return fail(&mut guard, e.to_string());
        }
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match guard.as_ref().unwrap().lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                    if v.get("id").and_then(|i| i.as_u64()) != Some(id) {
                        continue;
                    }
                    if let Some(e) = v.get("error") {
                        return Err(LensError::Failed(format!("plugin {}: {}", self.manifest.plugin.id, e.as_str().unwrap_or("error"))));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
                }
                Err(RecvTimeoutError::Timeout) => return fail(&mut guard, "timed out".into()),
                Err(RecvTimeoutError::Disconnected) => return fail(&mut guard, "exited".into()),
            }
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Some(mut p) = self.proc.lock().unwrap().take() {
            let _ = p.child.kill();
        }
    }
}

fn png_base64(img: &image::RgbaImage) -> Option<String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(base64_encode(buf.get_ref()))
}

/// The input object sent to a plugin, limited by its data permissions.
fn input_json(manifest: &Manifest, capability: &Capability, value: &Value) -> serde_json::Value {
    let mut obj = serde_json::json!({ "capability": capability.as_str() });
    if manifest.can("read-text") {
        if let Some(t) = value.as_text() {
            obj["text"] = serde_json::Value::String(t.into_owned());
        }
    }
    if manifest.can("read-pixels") {
        if let Some(img) = value.as_image() {
            if let Some(b) = png_base64(img) {
                obj["image_png"] = serde_json::Value::String(b);
            }
        }
    }
    obj
}

/// Builds a typed value for well-known capabilities, a custom one otherwise.
fn value_for(capability: &Capability, text: Option<String>, data: serde_json::Value) -> Value {
    match (capability.as_str(), text) {
        ("url", Some(t)) => {
            let scheme = t.split("://").next().unwrap_or("https").to_ascii_lowercase();
            let host = t.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).map(|h| h.to_ascii_lowercase());
            Value::Url(UrlValue { url: t, scheme, host })
        }
        ("email", Some(t)) => Value::Email(t),
        ("domain", Some(t)) => Value::Domain(t),
        ("address", Some(t)) => Value::Address(t),
        ("text", Some(t)) => Value::text(t),
        (_, text) => Value::Custom { type_name: capability.as_str().to_string(), data, text },
    }
}

struct ProxyRecognizer {
    decl: RecognizerDecl,
    proc: Arc<PluginProcess>,
}

impl Recognizer for ProxyRecognizer {
    fn descriptor(&self) -> RecognizerDescriptor {
        let cost = match self.decl.cost.as_deref() {
            Some("trivial") => Cost::Trivial,
            Some("expensive") => Cost::Expensive,
            _ => Cost::Cheap,
        };
        RecognizerDescriptor {
            id: self.decl.id.clone(),
            consumes: self.decl.consumes.iter().map(|c| Capability::custom(c.clone())).collect(),
            produces: self.decl.produces.iter().map(|c| Capability::custom(c.clone())).collect(),
            cost,
        }
    }

    fn should_run(&self, input: &Finding, _cx: &RecognizeContext) -> bool {
        let m = &self.proc.manifest;
        (m.can("read-text") && input.value.as_text().is_some()) || (m.can("read-pixels") && input.value.as_image().is_some())
    }

    fn recognize(&self, input: &Finding, cx: &RecognizeContext) -> Result<Vec<Detection>> {
        cx.cancel.check()?;
        let params = serde_json::json!({ "recognizer": self.decl.id, "input": input_json(&self.proc.manifest, &input.capability, &input.value) });
        let result = self.proc.call("recognize", params, Duration::from_secs(3))?;
        let mut out = Vec::new();
        for d in result.get("detections").and_then(|d| d.as_array()).cloned().unwrap_or_default() {
            let Some(cap) = d.get("capability").and_then(|c| c.as_str()) else { continue };
            // A plugin may only produce what it declared.
            if !self.decl.produces.iter().any(|p| p == cap) {
                continue;
            }
            let cap = Capability::custom(cap);
            let text = d.get("text").and_then(|t| t.as_str()).map(String::from);
            let mut det = Detection::new(cap.clone(), value_for(&cap, text, d.get("data").cloned().unwrap_or_default()))
                .confidence(d.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.7) as f32);
            for kv in d.get("details").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                if let (Some(k), Some(v)) = (kv.get(0).and_then(|k| k.as_str()), kv.get(1).and_then(|v| v.as_str())) {
                    det = det.detail(k, v);
                }
            }
            out.push(det);
        }
        Ok(out)
    }
}

struct ProxyAction {
    descriptor: ActionDescriptor,
    proc: Arc<PluginProcess>,
}

impl Action for ProxyAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn preview(&self, input: &Item, _s: &lens_core::Settings) -> Option<String> {
        // Whatever the plugin may upload is shown, and checked by the secret guard.
        (self.descriptor.effects.intersects(Effects::OUTBOUND) && self.proc.manifest.can("read-text"))
            .then(|| input.value.as_text().map(|t| t.into_owned()))
            .flatten()
    }

    fn execute(&self, input: &Item, cx: &ActionContext) -> Result<ActionOutcome> {
        let params =
            serde_json::json!({ "action": self.descriptor.id, "input": input_json(&self.proc.manifest, &input.capability, &input.value), "params": cx.params });
        let r = self.proc.call("execute", params, Duration::from_secs(30))?;
        let allowed = self.descriptor.effects;
        for e in r.get("effects").and_then(|e| e.as_array()).cloned().unwrap_or_default() {
            let ty = e.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            let s = |k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let denied = |what: &str| LensError::Blocked(format!("plugin action {} did not declare permission to {what}", self.descriptor.id));
            match ty {
                "copy-text" if allowed.contains(Effects::CLIPBOARD) => cx.host.set_clipboard_text(&s("text"))?,
                "copy-text" => return Err(denied("use the clipboard")),
                "open-uri" if allowed.intersects(Effects::LAUNCHES_APP | Effects::NETWORK) => cx.host.open_uri(&s("uri"), false)?,
                "open-uri" => return Err(denied("open links")),
                "save-file" if allowed.contains(Effects::WRITES_FILES) => {
                    let bytes = base64_decode(&s("base64")).ok_or_else(|| LensError::InvalidInput("bad base64 from plugin".into()))?;
                    cx.host.save_file(SaveRequest { suggested_name: s("name"), bytes, directory: None, mime: s("mime") })?;
                }
                "save-file" => return Err(denied("write files")),
                "notify" => cx.host.notify(&s("text")),
                other => return Err(LensError::InvalidInput(format!("unknown plugin effect \"{other}\""))),
            }
        }
        let mut outcome = ActionOutcome { message: r.get("message").and_then(|m| m.as_str()).map(String::from), ..Default::default() };
        if let Some(o) = r.get("output") {
            if let Some(cap) = o.get("capability").and_then(|c| c.as_str()) {
                let cap = Capability::custom(cap);
                let text = o.get("text").and_then(|t| t.as_str()).map(String::from);
                outcome.output = Some(Item::new(cap.clone(), value_for(&cap, text, o.get("data").cloned().unwrap_or_default())));
            }
        }
        Ok(outcome)
    }
}

fn group(name: Option<&str>) -> ActionGroup {
    match name.unwrap_or("system") {
        "copy" => ActionGroup::Copy,
        "open" => ActionGroup::Open,
        "transform" => ActionGroup::Transform,
        "save" => ActionGroup::Save,
        "share" => ActionGroup::Share,
        "search" => ActionGroup::Search,
        "inspect" => ActionGroup::Inspect,
        "edit" => ActionGroup::Edit,
        _ => ActionGroup::System,
    }
}

/// Registers one plugin. All-or-nothing: an invalid plugin registers nothing.
pub fn register(registry: &mut Registry, dir: &Path, m: &Manifest) -> Result<()> {
    let proc = Arc::new(PluginProcess::new(dir.to_path_buf(), m.clone()));
    let manifest = PluginManifest {
        id: m.plugin.id.clone(),
        name: m.plugin.name.clone(),
        version: m.plugin.version.clone(),
        api_version: m.plugin.api_version,
        permissions: m.effects(),
    };
    registry.register_plugin(manifest, |r| {
        for c in &m.capabilities {
            let parent = c.parent.clone().map(Capability::custom).unwrap_or(caps::TEXT);
            r.capability(Capability::custom(c.name.clone()), parent);
        }
        for d in &m.recognizers {
            r.recognizer(ProxyRecognizer { decl: d.clone(), proc: proc.clone() });
        }
        for a in &m.actions {
            let descriptor = ActionDescriptor {
                id: a.id.clone(),
                label: a.label.clone(),
                icon: a.icon.clone().unwrap_or_else(|| "puzzle".into()),
                group: group(a.group.as_deref()),
                accepts: a.accepts.iter().map(|c| Capability::custom(c.clone())).collect(),
                produces: match a.produces.as_deref() {
                    None | Some("nothing") => Produces::Nothing,
                    Some("same") => Produces::Same,
                    Some(c) => Produces::Capability(Capability::custom(c)),
                },
                priority: a.priority.unwrap_or(50).clamp(0, 90),
                key: a.key,
                effects: a.effects.iter().filter_map(|e| effect(e)).fold(Effects::empty(), |x, y| x | y),
                requires: a.requires.iter().filter_map(|f| feature(f)).fold(HostFeatures::empty(), |x, y| x | y),
                in_palette: true,
                params: Vec::new(),
            };
            r.action(ProxyAction { descriptor, proc: proc.clone() });
        }
    })
}

/// Loads every enabled plugin found under `dir`.
pub fn load_enabled(registry: &mut Registry, dir: &Path, enabled: &[String]) -> Vec<PluginStatus> {
    let mut seen = HashMap::new();
    discover(dir)
        .into_iter()
        .map(|(path, m)| match m {
            Ok(m) => {
                let on = enabled.contains(&m.plugin.id) && seen.insert(m.plugin.id.clone(), ()).is_none();
                let error = if on { register(registry, &path, &m).err().map(|e| e.to_string()) } else { None };
                PluginStatus { id: m.plugin.id.clone(), name: m.plugin.name.clone(), enabled: on, error }
            }
            Err(e) => PluginStatus {
                id: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                name: String::new(),
                enabled: false,
                error: Some(e),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            assert_eq!(base64_decode(&base64_encode(s)).unwrap(), s);
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn manifest_validation() {
        let dir = std::env::temp_dir().join(format!("lens-plugin-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            "[plugin]\nid='x.y'\nname='X'\nversion='1'\napi_version=1\nexecutable='run'\npermissions=['launch-apps','teleport']\n",
        )
        .unwrap();
        assert!(Manifest::load(&dir).unwrap_err().contains("teleport"));
        std::fs::write(dir.join("plugin.toml"), "[plugin]\nid='x.y'\nname='X'\nversion='1'\napi_version=99\nexecutable='run'\n").unwrap();
        assert!(Manifest::load(&dir).unwrap_err().contains("API v99"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
