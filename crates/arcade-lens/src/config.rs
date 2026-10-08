//! Locations and persistence of settings, chains, usage counters and models.
//!
//! Everything is local. Nothing here stores selection content: usage
//! counters record only which action was chosen for which capability.

use std::fs;
use std::path::{Path, PathBuf};

use lens_core::chain::Chain;
use lens_core::usage::UsageStore;
use lens_core::Settings;

#[derive(Clone, Debug)]
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
        match directories::ProjectDirs::from("dev", "Arcade", "Arcade Lens") {
            Some(p) => Paths { config: p.config_dir().into(), data: p.data_dir().into() },
            None => Paths { config: PathBuf::from(".arcade-lens/config"), data: PathBuf::from(".arcade-lens/data") },
        }
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

pub fn load_settings(p: &Paths) -> Result<Settings, String> {
    match fs::read_to_string(p.settings()) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", p.settings().display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_settings(p: &Paths, s: &Settings) -> std::io::Result<()> {
    write_atomic(&p.settings(), &toml::to_string_pretty(s).expect("settings serialize"))
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
