//! Link tray-host watcher lifetime and configuration for the Lens process.

use std::sync::{Mutex, OnceLock};

use arcade_link::trayhost::{TrayHostWatcher, TrayState, WatcherConfig};
use lens_core::Settings;

static WATCHER: OnceLock<Mutex<Option<TrayHostWatcher>>> = OnceLock::new();

pub fn start(settings: &Settings, callback: impl FnMut(TrayState) + Send + 'static) -> Result<(), String> {
    let callback = Mutex::new(callback);
    let watcher = TrayHostWatcher::start(
        WatcherConfig {
            locations: arcade_link::Locations::discover(),
            app: arcade_link::PeerInfo { id: crate::link::ME.into(), version: env!("CARGO_PKG_VERSION").into() },
            link_enabled: settings.link.enabled,
            has_tray: true,
        },
        move |state| callback.lock().unwrap_or_else(|e| e.into_inner())(state),
    )
    .map_err(|e| e.to_string())?;
    *WATCHER.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
    Ok(())
}

pub fn set_link_enabled(enabled: bool) {
    if let Some(watcher) = WATCHER.get().and_then(|w| w.lock().ok()) {
        if let Some(watcher) = watcher.as_ref() {
            watcher.set_link_enabled(enabled);
        }
    }
}

pub fn stop() {
    if let Some(watcher) = WATCHER.get().and_then(|w| w.lock().ok()?.take()) {
        watcher.stop();
    }
}
