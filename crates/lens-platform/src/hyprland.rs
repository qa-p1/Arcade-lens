//! Hyprland integration. Wayland has no global shortcut API Hyprland honours
//! without user configuration, so the activation shortcut becomes a
//! compositor key binding; window rules make pins float above everything
//! and let the overlay appear without an open animation.
//!
//! Hyprland ≥ 0.55 is configured in Lua (`hyprctl eval`); older versions use
//! `hyprctl keyword`. Both are runtime-only and dropped when the config is
//! reloaded, so callers re-apply them from [`on_config_reloaded`].
//!
//! Elsewhere (Windows, macOS) [`active`] is simply `false`.

use std::process::Command;
use std::sync::OnceLock;

use global_hotkey::hotkey::{Code, Modifiers};

pub fn active() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn hyprctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("hyprctl").args(args).output().map_err(|e| format!("hyprctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && text == "ok" {
        Ok(())
    } else {
        Err(format!("hyprctl {}: {text}", args.first().copied().unwrap_or_default()))
    }
}

fn lua() -> bool {
    static LUA: OnceLock<bool> = OnceLock::new();
    *LUA.get_or_init(|| hyprctl(&["eval", "return hl ~= nil"]).is_ok())
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

/// Single-quotes `s` for `sh`, which runs Hyprland's exec commands.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// X keysym name Hyprland binds by.
fn keysym(code: Code) -> Option<String> {
    let name = format!("{code:?}");
    if let Some(k) = name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")) {
        return Some(k.to_string());
    }
    if name.len() > 1 && name.starts_with('F') && name[1..].parse::<u8>().is_ok() {
        return Some(name);
    }
    let s = match code {
        Code::Space => "space",
        Code::Enter => "Return",
        Code::Escape => "Escape",
        Code::Tab => "Tab",
        Code::Backspace => "BackSpace",
        Code::Delete => "Delete",
        Code::Insert => "Insert",
        Code::Home => "Home",
        Code::End => "End",
        Code::PageUp => "Prior",
        Code::PageDown => "Next",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        Code::PrintScreen => "Print",
        Code::Pause => "Pause",
        Code::ScrollLock => "Scroll_Lock",
        Code::Minus => "minus",
        Code::Equal => "equal",
        Code::BracketLeft => "bracketleft",
        Code::BracketRight => "bracketright",
        Code::Backslash => "backslash",
        Code::Semicolon => "semicolon",
        Code::Quote => "apostrophe",
        Code::Backquote => "grave",
        Code::Comma => "comma",
        Code::Period => "period",
        Code::Slash => "slash",
        _ => return None,
    };
    Some(s.to_string())
}

/// A Lens shortcut as Hyprland modifiers and key, e.g. `(["CTRL", "ALT"], "L")`.
fn combo(shortcut: &str) -> Result<(Vec<&'static str>, String), String> {
    let hk = crate::shortcut::parse(shortcut)?;
    let mut mods = Vec::new();
    for (m, name) in [(Modifiers::SUPER, "SUPER"), (Modifiers::CONTROL, "CTRL"), (Modifiers::ALT, "ALT"), (Modifiers::SHIFT, "SHIFT")] {
        if hk.mods.contains(m) {
            mods.push(name);
        }
    }
    let key = keysym(hk.key).ok_or_else(|| format!("{shortcut} can't be bound in Hyprland; choose another key"))?;
    Ok((mods, key))
}

/// A key binding Lens added to the running compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    mods: Vec<&'static str>,
    key: String,
}

impl Binding {
    fn lua_keys(&self) -> String {
        let mut parts: Vec<&str> = self.mods.clone();
        parts.push(&self.key);
        lua_string(&parts.join(" + "))
    }

    fn legacy_keys(&self) -> String {
        format!("{}, {}", self.mods.join(" "), self.key)
    }

    pub fn remove(&self) {
        let _ = if lua() { hyprctl(&["eval", &format!("hl.unbind({})", self.lua_keys())]) } else { hyprctl(&["keyword", "unbind", &self.legacy_keys()]) };
    }
}

/// Binds `shortcut` to run the shell command `command`.
pub fn bind(shortcut: &str, command: &str) -> Result<Binding, String> {
    let (mods, key) = combo(shortcut)?;
    let b = Binding { mods, key };
    if lua() {
        hyprctl(&["eval", &format!("hl.bind({}, hl.dsp.exec_cmd({}))", b.lua_keys(), lua_string(command))])?;
    } else {
        hyprctl(&["keyword", "bind", &format!("{}, exec, {command}", b.legacy_keys())])?;
    }
    Ok(b)
}

/// Brings the window of process `pid` to the front.
pub fn focus_pid(pid: u32) {
    let _ = if lua() {
        hyprctl(&["eval", &format!("hl.dispatch(hl.dsp.focus({{ window = \"pid:{pid}\" }}))")])
    } else {
        hyprctl(&["dispatch", "focuswindow", &format!("pid:{pid}")])
    };
}

/// Adds window rules, each the body of a Lua `hl.window_rule{…}` table.
/// Only supported on Lua-configured Hyprland; older versions skip them.
pub fn add_window_rules(rules: &[&str]) {
    if !lua() {
        return;
    }
    for r in rules {
        if let Err(e) = hyprctl(&["eval", &format!("hl.window_rule({{ {r} }})")]) {
            eprintln!("arcade-lens: {e}");
        }
    }
}

#[cfg(unix)]
fn socket_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("hypr").join(&sig));
    runtime.filter(|d| d.exists()).or_else(|| Some(PathBuf::from("/tmp/hypr").join(sig)))
}

/// Calls `f` on a background thread whenever Hyprland reloads its config.
pub fn on_config_reloaded(f: impl Fn() + Send + 'static) {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixStream;
        let Some(dir) = socket_dir() else { return };
        let _ = std::thread::Builder::new().name("lens-hyprland".into()).spawn(move || {
            let Ok(stream) = UnixStream::connect(dir.join(".socket2.sock")) else { return };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if line.starts_with("configreloaded>>") {
                    f();
                }
            }
        });
    }
    #[cfg(not(unix))]
    let _ = f;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_translate_to_hyprland_keys() {
        let b = |s| combo(s).map(|(mods, key)| Binding { mods, key });
        let l = b("Ctrl+Alt+Shift+L").unwrap();
        assert_eq!(l.lua_keys(), "\"CTRL + ALT + SHIFT + L\"");
        assert_eq!(l.legacy_keys(), "CTRL ALT SHIFT, L");
        assert_eq!(b("Win+PrintScreen").unwrap().lua_keys(), "\"SUPER + Print\"");
        assert_eq!(b("Ctrl+F11").unwrap().key, "F11");
        assert_eq!(b("Alt+Space").unwrap().key, "space");
        assert_eq!(b("Ctrl+1").unwrap().key, "1");
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/a b/it's"), "'/a b/it'\\''s'");
        assert_eq!(lua_string("say \"hi\" \\"), "\"say \\\"hi\\\" \\\\\"");
    }
}
