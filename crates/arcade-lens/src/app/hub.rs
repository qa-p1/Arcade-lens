//! Link's optional app methods and transactional settings changes.

use std::sync::{Mutex, MutexGuard, OnceLock};

use arcade_link::app::{ImportResult, ShortcutSetResult, ShortcutVia};
use arcade_link::manifest::{MenuItem, MenuKind};
use arcade_link::{Content, LinkError};
use crate::config::{self, Paths};

static CHANGES: OnceLock<Mutex<()>> = OnceLock::new();

fn changes() -> MutexGuard<'static, ()> {
    CHANGES.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
}

pub const CAPTURE_MENU: &str = "capture";
pub const CAPTURE_TEXT_MENU: &str = "capture_text";
pub const PICK_COLOR_MENU: &str = "pick_color";

pub fn menu_items() -> Vec<MenuItem> {
    [
        (CAPTURE_MENU, "Capture"),
        (CAPTURE_TEXT_MENU, "Capture text"),
        (PICK_COLOR_MENU, "Pick color"),
    ]
    .into_iter()
    .map(|(id, title)| MenuItem { id: id.into(), title: title.into(), kind: MenuKind::Action, checked: None, enabled: true, effects: vec!["opens-ui".into()] })
    .collect()
}

pub fn methods() -> Vec<String> {
    use arcade_link::wire::method as m;
    [m::APP_SETTINGS, m::APP_RESTART, m::APP_MENU, m::APP_MENU_INVOKE, m::APP_SHORTCUTS_SET, m::APP_SETTINGS_EXPORT, m::APP_SETTINGS_IMPORT]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn validate_shortcut(input: &str) -> Result<String, LinkError> {
    let canonical = arcade_link::accelerator::normalize(input).map_err(|e| LinkError::denied(&e.to_string()))?;
    if canonical.contains(' ') || ["Ctrl", "Alt", "Shift", "Super"].contains(&canonical.as_str()) {
        return Err(LinkError::denied("Lens requires one shortcut chord with a key"));
    }
    if !canonical.contains('+') && !(canonical.starts_with('F') && canonical[1..].parse::<u8>().is_ok()) {
        return Err(LinkError::denied("a global shortcut needs a modifier or function key"));
    }
    for (reserved, owner) in [
        ("Alt+Tab", "window switching"),
        ("Super+Tab", "window switching"),
        ("Super+L", "the lock screen"),
        ("Ctrl+Alt+Delete", "the system"),
        ("Ctrl+Alt+L", "the lock screen"),
        ("Ctrl+Alt+T", "the terminal"),
        ("PrintScreen", "the system screenshot tool"),
        ("Shift+PrintScreen", "the system screenshot tool"),
        ("Super+Shift+4", "the macOS screenshot tool"),
    ] {
        if arcade_link::accelerator::normalize(reserved).ok().as_deref() == Some(&canonical) {
            return Err(LinkError::denied(&format!("reserved by {owner}")));
        }
    }
    lens_platform::shortcut::parse(&canonical).map_err(|e| LinkError::denied(&e))?;
    Ok(canonical)
}

/// The event loop applies the OS binding before the persistent update. If
/// writing the settings or manifest fails, the caller restores the old live
/// binding as well as the previous settings.
pub fn set_shortcut(
    paths: &Paths,
    input: &str,
    mut apply: impl FnMut(Option<&str>) -> Result<ShortcutSetResult, LinkError>,
) -> Result<ShortcutSetResult, LinkError> {
    let canonical = validate_shortcut(input)?;
    let _guard = changes();
    let old = config::load_settings(paths).map_err(LinkError::unavailable)?;
    let live = apply(Some(&canonical))?;
    let mut new = old.clone();
    new.activation_shortcut = Some(canonical);
    if let Err(e) = config::save_settings(paths, &new) {
        let _ = apply(old.activation_shortcut.as_deref());
        return Err(LinkError::unavailable(e.to_string()));
    }
    if let Err(e) = crate::link::publish_settings(&new, paths) {
        let _ = config::save_settings(paths, &old);
        let _ = crate::link::publish_settings(&old, paths);
        let _ = apply(old.activation_shortcut.as_deref());
        return Err(e);
    }
    Ok(live)
}

pub fn import_settings(
    paths: &Paths,
    inputs: &[Content],
    mut apply: impl FnMut(Option<&str>) -> Result<ShortcutSetResult, LinkError>,
) -> Result<ImportResult, LinkError> {
    let document = super::portable_settings::read_import(inputs)?;
    let _guard = changes();
    let old = config::load_settings(paths).map_err(LinkError::unavailable)?;
    let new = document.apply_to(&old);
    apply(new.activation_shortcut.as_deref())?;
    let backup = super::portable_settings::save_import(paths, &old, &new).map_err(super::portable_settings::io_error)?;
    if let Err(e) = crate::link::publish_settings(&new, paths) {
        let _ = super::portable_settings::restore_backup(&paths.settings(), &backup);
        let _ = crate::link::publish_settings(&old, paths);
        let _ = apply(old.activation_shortcut.as_deref());
        return Err(e);
    }
    Ok(ImportResult { imported: true, restart_required: false })
}

/// Replies to the running instance's IPC requests carry Link errors intact.
pub fn encode_reply<T: serde::Serialize>(result: Result<T, LinkError>) -> String {
    serde_json::to_string(&result).unwrap_or_else(|e| format!("{{\"Err\":{}}}", serde_json::json!(LinkError::internal(e.to_string()))))
}

pub fn decode_reply<T: serde::de::DeserializeOwned>(reply: &str) -> Result<T, LinkError> {
    serde_json::from_str::<Result<T, LinkError>>(reply).map_err(|e| LinkError::unavailable(format!("invalid reply from Arcade Lens: {e}")))?
}

/// Sends a request to the UI thread and waits for its answer off the UI thread.
pub fn ask<T>(send: impl FnOnce(std::sync::mpsc::SyncSender<Result<T, LinkError>>) -> bool) -> Result<T, LinkError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if !send(tx) {
        return Err(LinkError::unavailable("Arcade Lens is shutting down"));
    }
    rx.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| LinkError::unavailable("Arcade Lens didn't answer in time"))?
}

pub fn apply_result(via: ShortcutVia, hint: Option<String>) -> ShortcutSetResult {
    ShortcutSetResult { applied: via != ShortcutVia::Manual, via, hint }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_cache_has_the_three_lens_actions() {
        let menu = menu_items();
        assert_eq!(menu.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), [CAPTURE_MENU, CAPTURE_TEXT_MENU, PICK_COLOR_MENU]);
        assert!(menu.iter().all(|m| m.enabled && m.kind == MenuKind::Action));
    }

    #[test]
    fn shortcut_validation_canonicalizes_and_rejects_reserved_keys() {
        assert_eq!(validate_shortcut("ctrl+alt+shift+l").unwrap(), "Ctrl+Alt+Shift+L");
        assert!(validate_shortcut("Alt+Tab").is_err());
        assert!(validate_shortcut("Ctrl+K Ctrl+S").is_err());
    }
}
