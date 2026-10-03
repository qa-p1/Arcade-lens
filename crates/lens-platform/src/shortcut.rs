//! Global activation shortcut: parsing, conflict detection, registration.

use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// Normalizes user spellings (`Win`, `Meta`, `Cmd`, `Print`) and parses.
pub fn parse(s: &str) -> Result<HotKey, String> {
    let norm: Vec<String> = s
        .split('+')
        .map(|t| t.trim())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "win" | "meta" | "super" | "logo" | "cmd" | "command" => "Super".to_string(),
            "control" | "ctrl" => "Ctrl".into(),
            "option" | "alt" => "Alt".into(),
            "print" | "prtsc" | "printscreen" => "PrintScreen".into(),
            "esc" => "Escape".into(),
            _ => t.to_string(),
        })
        .collect();
    HotKey::from_str(&norm.join("+")).map_err(|e| format!("invalid shortcut \"{s}\": {e}"))
}

/// Shortcuts the operating system or desktop commonly owns. Lens never
/// claims these silently; the settings UI warns before using them.
const RESERVED: &[(&str, &str)] = &[
    ("Super+Shift+S", "Windows Snipping Tool"),
    ("PrintScreen", "the system screenshot tool"),
    ("Shift+PrintScreen", "the system screenshot tool"),
    ("Ctrl+PrintScreen", "the system screenshot tool"),
    ("Super+Shift+3", "macOS full-screen screenshot"),
    ("Super+Shift+4", "macOS region screenshot"),
    ("Super+Shift+5", "macOS screenshot toolbar"),
    ("Ctrl+Alt+L", "lock screen (GNOME/Ubuntu)"),
    ("Super+L", "lock screen"),
    ("Ctrl+Alt+Delete", "the system"),
    ("Ctrl+Alt+T", "open terminal (Ubuntu)"),
    ("Super+Space", "input source / Spotlight"),
    ("Ctrl+Space", "input method / Spotlight"),
    ("Alt+Tab", "window switching"),
    ("Super+Tab", "window switching"),
    ("Ctrl+C", "copy"),
    ("Ctrl+V", "paste"),
    ("Super+C", "copy (macOS)"),
    ("Super+V", "paste (macOS) / clipboard history (Windows)"),
];

/// Returns what a shortcut is known to conflict with, if anything.
pub fn known_conflict(s: &str) -> Option<&'static str> {
    let key = parse(s).ok()?;
    RESERVED.iter().find(|(r, _)| parse(r).ok() == Some(key)).map(|(_, what)| *what)
}

pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
}

impl Shortcuts {
    /// Must be created on the main thread (required on macOS).
    pub fn new() -> Result<Self, String> {
        Ok(Self { manager: GlobalHotKeyManager::new().map_err(|e| e.to_string())?, current: None })
    }

    /// Replaces the activation shortcut. `None` disables it. On failure the
    /// previous shortcut stays active.
    pub fn set(&mut self, s: Option<&str>) -> Result<(), String> {
        let next = s.map(parse).transpose()?;
        if next == self.current {
            return Ok(());
        }
        if let Some(n) = next {
            self.manager.register(n).map_err(|e| match e {
                global_hotkey::Error::AlreadyRegistered(_) | global_hotkey::Error::FailedToRegister(_) => {
                    format!("{} is already used by another application", s.unwrap_or_default())
                }
                e => e.to_string(),
            })?;
        }
        if let Some(old) = self.current.take() {
            let _ = self.manager.unregister(old);
        }
        self.current = next;
        Ok(())
    }

    pub fn current_id(&self) -> Option<u32> {
        self.current.map(|h| h.id())
    }
}

/// Calls `f` whenever a registered shortcut is pressed (not released).
pub fn on_pressed(f: impl Fn(u32) + Send + Sync + 'static) {
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.state() == HotKeyState::Pressed {
            f(e.id());
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_conflicts() {
        assert_eq!(parse("ctrl+alt+shift+l").unwrap(), parse("Control+Option+Shift+L").unwrap());
        assert_eq!(parse("Win+Shift+S").unwrap(), parse("Super+Shift+S").unwrap());
        assert!(parse("Ctrl+").is_err());
        assert_eq!(known_conflict("Win+Shift+S"), Some("Windows Snipping Tool"));
        assert_eq!(known_conflict("Ctrl+Alt+L"), Some("lock screen (GNOME/Ubuntu)"));
        assert_eq!(known_conflict("Ctrl+Alt+Shift+L"), None);
    }
}
