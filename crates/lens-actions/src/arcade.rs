//! First-party peer actions. Discovery and rebuilding happen on watcher workers;
//! palettes only borrow the cached action registry. Invocations run on Lens's
//! action worker, and use the shared Link lifecycle and standard error copy.

use std::path::Path;
use std::sync::{mpsc, Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use arcade_link::client::{self, AppState, CallOptions};
use arcade_link::registry::SharedRegistry;
use arcade_link::{ids, Content, Handoff, InvokeRequest, InvokeResult, LinkError, Locations, Manifest, PeerInfo};
use image::RgbaImage;
use lens_core::action::{Action, ActionContext, ActionDescriptor, ActionGroup, ActionOutcome, Effects, Item, Produces};
use lens_core::cancel::CancelToken;
use lens_core::host::{HostFeatures, OpenPathMode};
use lens_core::registry::PluginManifest;
use lens_core::value::{FileValue, PathKind};
use lens_core::{caps, Finding, LensError, Registry, Result, Settings, Value};
use serde_json::json;

const JOB_TIMEOUT: Duration = Duration::from_secs(60);
type Wake = Arc<dyn Fn() + Send + Sync>;
type PngCache = Arc<Mutex<Option<(Weak<RgbaImage>, Arc<Vec<u8>>)>>>;

#[derive(Clone)]
pub struct Peer {
    pub id: String,
    pub state: AppState,
    pub link_enabled: bool,
}

#[derive(Clone)]
pub struct Snapshot {
    pub registry: Arc<Registry>,
    pub installed: arcade_link::registry::Registry,
    pub peers: Vec<Peer>,
    pub revision: u64,
    pub last_error: Option<String>,
}

struct Inner {
    state: RwLock<Snapshot>,
    watch: Mutex<Option<SharedRegistry>>,
    wake: Mutex<Option<Wake>>,
    locations: Locations,
}

/// Shared between the GUI, its host and settings. No getter does disk or IPC.
#[derive(Clone)]
pub struct Arcade(Arc<Inner>);

impl Arcade {
    /// Recognition-only runtimes never discover or launch another application.
    pub fn offline(base: Arc<Registry>, locations: Locations) -> Self {
        let inner = Arc::new(Inner {
            state: RwLock::new(Snapshot {
                registry: base.clone(),
                installed: arcade_link::registry::Registry::new(&locations),
                peers: vec![],
                revision: 0,
                last_error: None,
            }),
            watch: Mutex::new(None),
            wake: Mutex::new(None),
            locations,
        });
        Self(inner)
    }

    pub fn start(base: Arc<Registry>, settings: Arc<Settings>, locations: Locations) -> Self {
        let service = Self::offline(base.clone(), locations);
        let inner = service.0.clone();
        let worker = inner.clone();
        std::thread::spawn(move || {
            let shared = SharedRegistry::load(&worker.locations);
            let weak = Arc::downgrade(&worker);
            let b = base.clone();
            let s = settings.clone();
            let watching = shared.watch(move |registry| {
                if let Some(inner) = weak.upgrade() {
                    rebuild(&inner, &b, &s, registry, None);
                }
            });
            // Read again after installing the watch, closing the startup race.
            shared.refresh();
            rebuild(&worker, &base, &settings, &shared.snapshot(), (!watching).then(|| "Registry watcher is unavailable".into()));
            *worker.watch.lock().unwrap_or_else(|e| e.into_inner()) = Some(shared);
        });
        service
    }

    pub fn snapshot(&self) -> Snapshot {
        self.0.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn actions(&self) -> Arc<Registry> {
        self.0.state.read().unwrap_or_else(|e| e.into_inner()).registry.clone()
    }

    pub fn revision(&self) -> u64 {
        self.0.state.read().unwrap_or_else(|e| e.into_inner()).revision
    }

    pub fn on_change(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.0.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(wake));
    }

    pub fn locations(&self) -> &Locations {
        &self.0.locations
    }

    /// Called only from an action worker. An installed Clipboard's refusal is
    /// returned to the user; Private mode never silently falls through to KDE.
    pub fn send_to_devices(&self, settings: &Settings, text: Option<&str>, image: Option<&RgbaImage>) -> Option<Result<()>> {
        if !settings.link.uses(ids::CLIPBOARD) {
            return None;
        }
        let snapshot = self.snapshot();
        let manifest = snapshot.installed.get(ids::CLIPBOARD)?.clone();
        if !manifest.settings.link_enabled {
            return None;
        }
        let input = match (text, image) {
            (Some(text), _) => Item::new(caps::TEXT, Value::text(text)),
            (_, Some(image)) => Item::new(caps::IMAGE, Value::Image(lens_core::value::ImageValue { image: Arc::new(image.clone()), origin: None })),
            _ => return Some(Err(LensError::InvalidInput("nothing to send".into()))),
        };
        let peer = manifest.action("clipboard.add").cloned();
        Some((|| {
            let peer = peer.ok_or_else(|| error(ids::CLIPBOARD, LinkError::unavailable("Sending is unavailable")))?;
            let action = PeerAction::new(&self.0.locations, &manifest, &peer);
            if let Some(reason) = action.unavailable_reason(&input, settings) {
                return Err(LensError::Blocked(reason));
            }
            let (content, handoff) = action.input(&input)?;
            let request = request(&peer).input(content);
            let result = invoke_with_handoff(&self.0.locations, &manifest, request, CancelToken::new(), JOB_TIMEOUT, handoff);
            result.map(|_| ())
        })())
    }

    /// Quick Look's legacy host path also prefers the installed Look peer.
    pub fn quick_look(&self, settings: &Settings, path: &Path) -> Option<Result<()>> {
        if !settings.link.uses(ids::LOOK) {
            return None;
        }
        let manifest = self.snapshot().installed.get(ids::LOOK)?.clone();
        let action = manifest.usable_actions().find(|a| a.id == "look.preview" && a.offer_for(&Content::file(path)))?;
        let req = request(action).input(Content::file(path));
        Some(invoke(&self.0.locations, &manifest, req, CancelToken::new(), JOB_TIMEOUT).map(|_| ()))
    }

    /// Settings' Get button: use the manager if it advertises tools.install.
    pub fn get(&self, app: &str) -> Option<Result<()>> {
        let snapshot = self.snapshot();
        if let Some(m) = snapshot.installed.get(ids::TOOLS) {
            if let Some(action) = m.usable_actions().find(|a| a.id == "tools.install") {
                let mut req = request(action);
                req.options = json!({ "app": app });
                return Some(invoke(&self.0.locations, m, req, CancelToken::new(), JOB_TIMEOUT).map(|_| ()));
            }
        }
        None
    }
}

fn me() -> PeerInfo {
    PeerInfo { id: ids::LENS.into(), version: env!("CARGO_PKG_VERSION").into() }
}

fn rebuild(inner: &Inner, base: &Registry, settings: &Settings, installed: &arcade_link::registry::Registry, last_error: Option<String>) {
    let registry = with_peers(base, settings, installed, &inner.locations).unwrap_or_else(|_| base.clone());
    let peers = ids::APPS
        .iter()
        .filter(|id| **id != ids::LENS)
        .map(|id| Peer {
            id: (*id).into(),
            state: client::app_state(&inner.locations, installed, id, &me()),
            link_enabled: installed.get(id).is_none_or(|m| m.settings.link_enabled),
        })
        .collect();
    {
        let mut state = inner.state.write().unwrap_or_else(|e| e.into_inner());
        *state = Snapshot { registry: Arc::new(registry), installed: installed.clone(), peers, revision: state.revision + 1, last_error };
    }
    if let Some(wake) = inner.wake.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        wake();
    }
}

/// Purely cached discovery; callers build palettes off the UI thread so input
/// metadata and encoded size checks cannot pause a frame either.
pub fn with_peers(base: &Registry, settings: &Settings, installed: &arcade_link::registry::Registry, locations: &Locations) -> Result<Registry> {
    let mut registry = base.clone();
    if !settings.link.enabled {
        return Ok(registry);
    }
    let clipboard = installed.get(ids::CLIPBOARD).filter(|m| settings.link.uses(&m.id) && m.settings.link_enabled);
    if clipboard.is_some() {
        registry.remove_actions(&["core.region.send", "core.url.send", "core.qr.send", "core.phone.send"]);
    }
    if installed.get(ids::LOOK).is_some_and(|m| settings.link.uses(&m.id) && m.usable_actions().any(|a| a.id == "look.preview")) {
        registry.remove_actions(&["core.path.quick-look"]);
    }
    let cache: PngCache = Arc::new(Mutex::new(None));
    let mut actions = Vec::new();
    for m in installed.peers(ids::LENS).filter(|m| settings.link.uses(&m.id)) {
        let mut featured = 0;
        for a in m.usable_actions() {
            let show = match m.id.as_str() {
                ids::LOOK => a.id == "look.preview",
                ids::CLIPBOARD => a.id == "clipboard.add",
                ids::WHEEL => a.id == "wheel.add_action",
                ids::BOX => {
                    if a.preset.is_some() && a.featured_for.iter().any(|t| arcade_link::content::type_matches(t, "file/image")) && featured < 5 {
                        featured += 1;
                        true
                    } else {
                        a.id == "box.open"
                    }
                }
                _ => false,
            };
            if show {
                let mut action = PeerAction::new(locations, m, a);
                action.png = cache.clone();
                actions.push(action);
            }
        }
    }
    // Saving a preset saves a runnable reference, never today's handoff path.
    if let Some(wheel) = installed.get(ids::WHEEL).filter(|m| settings.link.uses(&m.id)) {
        if let Some(add) = wheel
            .usable_actions()
            .find(|a| a.id == "wheel.add_action" && a.accepts.iter().any(|t| arcade_link::content::type_matches(t, "structured/arcade-action")))
        {
            let presets: Vec<_> = actions.iter().filter(|a| a.manifest.id == ids::BOX && a.peer.preset.is_some()).map(|a| a.peer.clone()).collect();
            for preset in presets {
                let mut action = PeerAction::new(locations, wheel, add);
                action.descriptor.id = preset_wheel_id(&format!("arcade.{}", preset.id));
                action.descriptor.in_palette = false;
                action.descriptor.accepts = vec![caps::REGION, caps::IMAGE, caps::ICON];
                action.saved_preset = Some(preset);
                actions.push(action);
            }
        }
    }
    if !actions.is_empty() {
        registry.register_plugin(PluginManifest::first_party("arcade", "Connected Arcade apps"), |r| {
            for action in actions {
                r.action(action);
            }
        })?;
    }
    Ok(registry)
}

pub fn preset_wheel_id(action_id: &str) -> String {
    format!("arcade.wheel.preset.{}", action_id.strip_prefix("arcade.").unwrap_or(action_id))
}

/// Lossless content mapping. Image bytes are created only at execution time;
/// the placeholder is used for registry matching and never leaves Lens.
pub fn content(item: &Item) -> Option<Content> {
    match &item.value {
        Value::Image(_) => Some(Content { kind: "file/image".into(), ..Default::default() }),
        Value::Path(p) if matches!(p.exists, Some(PathKind::File | PathKind::Directory)) => {
            let path = Path::new(p.resolved.as_deref()?);
            let kind = if p.exists == Some(PathKind::Directory) { "folder/reference".into() } else { arcade_link::content::file_type_for_path(path) };
            Some(Content { kind, path: Some(path.to_string_lossy().into_owned()), ..Default::default() })
        }
        Value::Path(_) => None,
        Value::File(f) => Some(Content::file(&f.path)),
        Value::Url(u) => Some(Content::url(&u.url)),
        Value::Secret(_) | Value::Window(_) | Value::Geometry(_) => None,
        _ => item.value.as_text().map(|text| {
            let mut c = Content::plain(text.into_owned());
            c.hints.push(item.capability.as_str().into());
            c
        }),
    }
}

fn effects(action: &arcade_link::Action) -> Effects {
    let mut effects = Effects::empty();
    for effect in &action.effects {
        effects |= match effect.as_str() {
            "clipboard" => Effects::CLIPBOARD,
            "writes-files" => Effects::WRITES_FILES,
            "overwrites-files" => Effects::OVERWRITES_FILES,
            "deletes-files" => Effects::DELETES_FILES,
            "network" => Effects::NETWORK,
            "uploads-content" => Effects::UPLOADS_CONTENT,
            "sends-to-device" => Effects::SENDS_TO_DEVICE,
            "launch-apps" | "opens-ui" => Effects::LAUNCHES_APP,
            "persists" => Effects::PERSISTS,
            "executes-commands" => Effects::EXECUTES_COMMAND,
            "window-control" => Effects::WINDOW_CONTROL,
            // Unknown effects cannot silently get classified as safe.
            _ => Effects::DANGEROUS,
        };
    }
    if action.privacy != "local" {
        effects |= Effects::NETWORK | Effects::UPLOADS_CONTENT;
    }
    effects
}

struct PeerAction {
    descriptor: ActionDescriptor,
    locations: Locations,
    manifest: Manifest,
    peer: arcade_link::Action,
    png: PngCache,
    saved_preset: Option<arcade_link::Action>,
}

impl PeerAction {
    fn new(locations: &Locations, manifest: &Manifest, peer: &arcade_link::Action) -> Self {
        let (label, group, key, priority, accepts) = match manifest.id.as_str() {
            ids::LOOK => ("Quick Look", ActionGroup::Inspect, Some('y'), 50, vec![caps::PATH, caps::URL, caps::FILE]),
            ids::CLIPBOARD => (
                "Send to my devices",
                ActionGroup::Share,
                Some('m'),
                65,
                vec![
                    caps::REGION,
                    caps::IMAGE,
                    caps::ICON,
                    caps::TEXT,
                    caps::URL,
                    caps::COMMAND,
                    caps::CODE,
                    caps::PATH,
                    caps::FILE,
                    caps::QR_CODE,
                    caps::EMAIL,
                    caps::PHONE,
                    caps::TABLE,
                ],
            ),
            ids::WHEEL => ("Add to Wheel", ActionGroup::Save, None, 38, vec![caps::URL, caps::COMMAND, caps::PATH, caps::FILE, caps::TEXT]),
            _ => (
                if peer.id == "box.open" { "More in Arcade Box…" } else { peer.title.as_str() },
                ActionGroup::Transform,
                None,
                48,
                vec![caps::REGION, caps::IMAGE, caps::ICON],
            ),
        };
        let mut declared = effects(peer);
        if manifest.id == ids::CLIPBOARD {
            declared |= Effects::SENDS_TO_DEVICE;
        }
        if manifest.id == ids::WHEEL {
            declared |= Effects::PERSISTS | Effects::LAUNCHES_APP;
        }
        if manifest.id == ids::BOX && !peer.produces.is_empty() {
            declared |= Effects::CLIPBOARD | Effects::LAUNCHES_APP;
        }
        Self {
            descriptor: ActionDescriptor {
                id: format!("arcade.{}", peer.id),
                label: if declared.intersects(Effects::OUTBOUND) { format!("{label} ↗") } else { label.into() },
                icon: manifest.id.clone(),
                group,
                accepts,
                produces: Produces::Nothing,
                priority,
                key,
                effects: declared,
                requires: HostFeatures::empty(),
                in_palette: true,
                params: vec![],
            },
            locations: locations.clone(),
            manifest: manifest.clone(),
            peer: peer.clone(),
            png: Arc::new(Mutex::new(None)),
            saved_preset: None,
        }
    }

    fn png(&self, image: &Arc<RgbaImage>) -> Result<Arc<Vec<u8>>> {
        let mut cache = self.png.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((old, bytes)) = cache.as_ref().filter(|(old, _)| old.ptr_eq(&Arc::downgrade(image))) {
            let _ = old;
            return Ok(bytes.clone());
        }
        let bytes = Arc::new(crate::util::encode(image, lens_core::settings::ImageFormat::Png)?);
        *cache = Some((Arc::downgrade(image), bytes.clone()));
        Ok(bytes)
    }

    fn input(&self, item: &Item) -> Result<(Content, Option<Handoff>)> {
        if let Some(preset) = &self.saved_preset {
            return Ok((
                Content::structured(
                    "arcade-action",
                    json!({
                        "app": ids::BOX, "action": preset.id, "version": preset.version, "title": preset.title,
                        "preset": preset.preset, "input": "lens-selection", "options": {}
                    }),
                ),
                None,
            ));
        }
        let mut content = content(item).ok_or_else(|| LensError::InvalidInput("No supported content".into()))?;
        if let Some(image) = item.value.as_image() {
            let handoff = Handoff::create(&self.locations, ids::LENS).map_err(|e| LensError::Failed(e.to_string()))?;
            content = handoff.file("region.png", &self.png(image)?).map_err(|e| LensError::Failed(e.to_string()))?;
            return Ok((content, Some(handoff)));
        }
        if content.text.as_ref().is_some_and(|text| text.len() > arcade_link::content::INLINE_TEXT_LIMIT) {
            let handoff = Handoff::create(&self.locations, ids::LENS).map_err(|e| LensError::Failed(e.to_string()))?;
            let file = handoff.file("selection.txt", content.text.take().unwrap().as_bytes()).map_err(|e| LensError::Failed(e.to_string()))?;
            content.path = file.path;
            content.size = file.size;
            content.owner = file.owner;
            return Ok((content, Some(handoff)));
        }
        Ok((content, None))
    }
}

impl Action for PeerAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn enabled(&self, settings: &Settings) -> bool {
        settings.link.uses(&self.manifest.id) && self.saved_preset.as_ref().is_none_or(|_| settings.link.uses(ids::BOX))
    }

    fn applies(&self, finding: &Finding, _: HostFeatures) -> bool {
        if self.saved_preset.is_some() {
            return true;
        }
        // Look's URL sandbox only previews file:// references.
        if self.manifest.id == ids::LOOK && matches!(&finding.value, Value::Url(url) if url.scheme != "file") {
            return false;
        }
        content(&Item::from(finding)).is_some_and(|c| arcade_link::content::accepts_content(&self.peer.accepts, &c))
    }

    fn guards_selection(&self) -> bool {
        self.descriptor.effects.intersects(Effects::OUTBOUND)
    }

    fn unavailable_reason(&self, item: &Item, _: &Settings) -> Option<String> {
        let limit = self.peer.max_bytes?;
        if self.saved_preset.is_some() {
            return None;
        }
        let size = match item.value.as_image() {
            Some(image) => match self.png(image) {
                Ok(bytes) => bytes.len() as u64,
                Err(e) => return Some(e.to_string()),
            },
            None => content(item)?.byte_size(),
        };
        (size > limit).then(|| LinkError::too_large(limit).user_message(&self.manifest.name))
    }

    fn preview(&self, item: &Item, _: &Settings) -> Option<String> {
        if let Some(image) = item.value.as_image() {
            Some(format!("{} × {} px image", image.width(), image.height()))
        } else {
            item.value.as_text().map(|text| text.into_owned())
        }
    }

    fn execute(&self, item: &Item, cx: &ActionContext) -> Result<ActionOutcome> {
        if !self.enabled(cx.settings) {
            return Err(LensError::Blocked("Connections are disabled".into()));
        }
        if let Some(cancel) = cx.cancel {
            cancel.check()?;
        }
        let (content, handoff) = self.input(item)?;
        let req = request(&self.peer).input(content);
        let fallback_path = req.inputs.first().and_then(|c| c.path.clone());
        let result = match invoke_with_handoff(&self.locations, &self.manifest, req, cx.cancel.cloned().unwrap_or_default(), JOB_TIMEOUT, handoff) {
            Ok(result) => result,
            Err(e) if self.manifest.id == ids::LOOK && cfg!(target_os = "linux") && !matches!(e, LensError::Cancelled) => {
                if let Some(path) = fallback_path {
                    cx.host.quick_look_fallback(Path::new(&path))?;
                    return Ok(ActionOutcome::done("Opened"));
                }
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let mut outcome = ActionOutcome::done(result.message.unwrap_or_else(|| self.descriptor.label.clone()));
        if self.manifest.id == ids::BOX {
            for output in result.outputs {
                if let Some(path) = output.path.as_deref().filter(|_| output.kind.starts_with("file/")) {
                    if output.kind == "file/image" && cx.host.features().contains(HostFeatures::CLIPBOARD_IMAGE) {
                        if let Ok(image) = image::open(path) {
                            cx.host.set_clipboard_image(&image.to_rgba8())?;
                            outcome.message = Some(format!("{} — copied image", self.descriptor.label));
                        } else {
                            cx.host.open_path(Path::new(path), OpenPathMode::Default)?;
                        }
                    } else {
                        cx.host.open_path(Path::new(path), OpenPathMode::Default)?;
                    }
                    outcome.output = Some(Item::new(caps::FILE, Value::File(FileValue { path: path.into(), mime: None })));
                    break;
                } else if let Some(text) = output.text {
                    cx.host.set_clipboard_text(&text)?;
                    outcome.output = Some(Item::new(caps::TEXT, Value::text(text)));
                    outcome.message = Some(format!("{} — copied", self.descriptor.label));
                    break;
                }
            }
        }
        Ok(outcome)
    }
}

fn request(action: &arcade_link::Action) -> InvokeRequest {
    let mut request = InvokeRequest::new(&action.id, ids::LENS);
    request.version = Some(action.version);
    request.preset = action.preset.clone();
    request.context.interactive = true;
    request
}

fn error(app: &str, error: LinkError) -> LensError {
    LensError::Failed(error.user_message(arcade_link::manifest::app_name(app)))
}

/// Bound the caller's wait even if a peer ignores job.cancel. The Link worker
/// holds handoffs until its result; cancellation is sent through CallOptions.
pub fn invoke(locations: &Locations, manifest: &Manifest, request: InvokeRequest, cancel: CancelToken, timeout: Duration) -> Result<InvokeResult> {
    invoke_with_handoff(locations, manifest, request, cancel, timeout, None)
}

fn invoke_with_handoff(
    locations: &Locations,
    manifest: &Manifest,
    request: InvokeRequest,
    cancel: CancelToken,
    timeout: Duration,
    handoff: Option<Handoff>,
) -> Result<InvokeResult> {
    let (tx, rx) = mpsc::channel();
    let loc = locations.clone();
    let m = manifest.clone();
    let token = cancel.clone();
    std::thread::spawn(move || {
        let result = client::invoke_action(&loc, &me(), &m, &request, CallOptions { cancel: Some(token.atomic()), ..Default::default() });
        // Interactive owners queue work in their UI before answering. Keep
        // their handoff until shared TTL cleanup; headless jobs drop theirs.
        if result.is_ok() && client::find_action(&m, &request).is_some_and(|a| a.interactive) {
            if let Some(handoff) = handoff {
                handoff.keep();
            }
        }
        let _ = tx.send(result);
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        cancel.check()?;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            cancel.cancel();
            return Err(error(&manifest.id, LinkError::new(arcade_link::ErrorCode::Timeout, "Job timed out")));
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(_) if cancel.is_cancelled() => return Err(LensError::Cancelled),
            Ok(result) => return result.map_err(|e| error(&manifest.id, e)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(LensError::Failed("Arcade action worker stopped".into())),
        }
    }
}
