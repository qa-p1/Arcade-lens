//! Locations and persistence of settings, chains, usage counters and models.
//!
//! Everything is local. Nothing here stores selection content: usage
//! counters record only which action was chosen for which capability.

use std::fs;
use std::path::{Path, PathBuf};

use lens_core::chain::Chain;
use lens_core::usage::UsageStore;
use lens_core::Settings;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        if let Some(dir) = std::env::var_os("ARCADE_LENS_HOME") {
            let base = PathBuf::from(dir);
            return Paths { config: base.join("config"), data: base.join("data") };
        }
        // The shared Arcade layout: ~/.config/arcade/lens and
        // ~/.local/share/arcade/lens on Linux, Arcade\Lens on Windows,
        // Application Support/Arcade/Lens on macOS.
        let dirs = arcade_link::paths::app_dirs(crate::link::ME);
        let current = Paths { config: dirs.config, data: dirs.data };
        // Until the old folders have moved (or if moving them failed), keep
        // using them, so no process starts on default settings.
        match Self::legacy() {
            Some(old) if !current.settings().exists() && old.settings().exists() => old,
            _ => current,
        }
    }

    /// Where Lens kept its files before 0.4.
    fn legacy() -> Option<Paths> {
        use arcade_link::paths::Kind;
        let dirs = arcade_link::paths::legacy_dirs(crate::link::ME);
        let find = |kind: Kind| dirs.iter().find(|d| d.kind == kind).map(|d| d.path.clone());
        let data = find(Kind::Data)?;
        Some(Paths { config: find(Kind::Config).unwrap_or_else(|| data.clone()), data })
    }

    /// True for `ARCADE_LENS_HOME` (tests, portable runs): no migration.
    pub fn isolated() -> bool {
        std::env::var_os("ARCADE_LENS_HOME").is_some()
    }

    /// Moves Lens's files from the folders it used before 0.4 (`arcadelens`
    /// on Linux, `Arcade\Arcade Lens` on Windows, `dev.Arcade.Arcade-Lens`
    /// on macOS): copy, verify, then remove, through Arcade Link. A Lens
    /// from before the move that is still running is asked to quit first,
    /// so nothing is written to the old folders mid-copy. Returns whether
    /// anything moved; on a conflict the old folders stay and the error says
    /// why. Safe to run on every start.
    pub fn migrate() -> Result<bool, String> {
        if Self::isolated() {
            return Ok(false);
        }
        let legacy = arcade_link::paths::legacy_dirs(crate::link::ME);
        if !legacy.iter().any(|d| d.path.is_dir()) {
            return Ok(false);
        }
        for dir in legacy.iter().filter(|d| d.kind == arcade_link::paths::Kind::Data) {
            let endpoint = dir.path.join("instance");
            if lens_platform::ipc::send(&endpoint, "quit").is_ok() {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while lens_platform::ipc::send(&endpoint, "ping").is_ok() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        let report = arcade_link::paths::migrate_app_dirs(crate::link::ME);
        if !report.kept.is_empty() {
            let reasons = report.kept.iter().map(|(path, reason)| format!("{}: {reason}", path.display())).collect::<Vec<_>>().join("; ");
            return Err(format!("Couldn't move Lens's settings to the new folder. The originals were kept. {reasons}"));
        }
        Ok(!report.moved.is_empty())
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.toml")
    }
    pub fn chains(&self) -> PathBuf {
        self.config.join("chains.json")
    }
    pub fn usage(&self) -> PathBuf {
        self.data.join("usage.json")
    }
    pub fn collections(&self) -> PathBuf {
        self.data.join("collections")
    }
    pub fn plugins(&self) -> PathBuf {
        self.config.join("plugins")
    }
    pub fn history(&self) -> PathBuf {
        self.data.join("history")
    }
    pub fn endpoint(&self) -> PathBuf {
        self.data.join("instance")
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)
}

/// The Notifications setting, for [`crate::gui::notify`]: every process reads
/// settings through here, so the switch takes effect wherever it last loaded.
static NOTIFICATIONS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn notifications_enabled() -> bool {
    NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn load_settings(p: &Paths) -> Result<Settings, String> {
    let settings: Settings = match fs::read_to_string(p.settings()) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", p.settings().display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => return Err(e.to_string()),
    };
    NOTIFICATIONS.store(settings.notifications, std::sync::atomic::Ordering::Relaxed);
    Ok(settings)
}

pub fn save_settings(p: &Paths, s: &Settings) -> std::io::Result<()> {
    write_atomic(&p.settings(), &toml::to_string_pretty(s).expect("settings serialize"))?;
    NOTIFICATIONS.store(s.notifications, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

pub fn load_chains(p: &Paths) -> Vec<Chain> {
    fs::read_to_string(p.chains()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(lens_actions::default_chains)
}

pub fn save_chains(p: &Paths, chains: &[Chain]) -> std::io::Result<()> {
    write_atomic(&p.chains(), &serde_json::to_string_pretty(chains).expect("chains serialize"))
}

pub fn load_usage(p: &Paths) -> UsageStore {
    fs::read_to_string(p.usage()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_usage(p: &Paths, u: &UsageStore) -> std::io::Result<()> {
    write_atomic(&p.usage(), &serde_json::to_string(u).expect("usage serialize"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_in_temp_home() {
        let dir = std::env::temp_dir().join(format!("lens-config-test-{}", std::process::id()));
        let p = Paths { config: dir.join("c"), data: dir.join("d") };
        assert_eq!(load_settings(&p).unwrap(), Settings::default());
        assert_eq!(load_chains(&p).len(), 3);
        let s = Settings { primary_action_count: 4, ..Default::default() };
        save_settings(&p, &s).unwrap();
        assert_eq!(load_settings(&p).unwrap(), s);
        let mut u = UsageStore::default();
        u.record("url", "core.url.open", 1);
        save_usage(&p, &u).unwrap();
        assert_eq!(load_usage(&p), u);
        fs::remove_dir_all(dir).ok();
    }
}
