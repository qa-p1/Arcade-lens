//! Arcade Link: Lens's presence among the other Arcade apps.
//!
//! The background instance writes Lens's manifest and listens on its
//! endpoint, on a thread started after the IPC listener, so startup never
//! waits for it. With "Connect with other Arcade apps" off, the manifest has
//! no actions and nothing listens.

use std::sync::{Arc, Mutex, OnceLock};

use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::{ids, Action, InvokeRequest, LinkError, Locations, Manifest, Presence};
use lens_core::Settings;

/// The manifest for these settings (also printed by `--arcade-manifest`).
pub fn manifest(settings: &Settings) -> Manifest {
    let mut m = Manifest::new(ids::LENS, env!("CARGO_PKG_VERSION"), &arcade_link::manifest::current_executable());
    m.launch.background = vec!["--background".into()];
    if let Some(s) = &settings.activation_shortcut {
        m.shortcuts.push(arcade_link::manifest::Shortcut { id: "capture".into(), accelerator: s.clone() });
    }
    m.settings.link_enabled = settings.link.enabled;
    m.actions = actions();
    m
}

/// The actions Lens exposes.
pub fn actions() -> Vec<Action> {
    Vec::new()
}

struct LensHandler;

impl Handler for LensHandler {
    fn describe(&self) -> Vec<Action> {
        actions()
    }

    fn invoke(&self, request: InvokeRequest, _ctx: &InvokeContext) -> Result<Reply, LinkError> {
        Err(LinkError::unavailable(format!("Arcade Lens has no action {}", request.action)))
    }
}

static PRESENCE: OnceLock<Mutex<Option<Arc<Presence>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<Presence>>> {
    PRESENCE.get_or_init(|| Mutex::new(None))
}

/// Starts Lens's presence on a background thread.
pub fn start(settings: &Settings) {
    let m = manifest(settings);
    std::thread::Builder::new()
        .name("lens-link".into())
        .spawn(move || {
            let p = Presence::start(Locations::discover(), m, Arc::new(LensHandler));
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
