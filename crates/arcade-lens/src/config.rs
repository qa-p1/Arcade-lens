//! Locations and persistence of settings, chains, usage counters and models.
//!
//! Everything is local. Nothing here stores selection content: usage
//! counters record only which action was chosen for which capability.

use std::fs;
use std::path::{Path, PathBuf};

use lens_core::chain::Chain;
use lens_core::usage::UsageStore;
use lens_core::Settings;

use crate::datafile;

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

/// Settings from `settings.toml`. A missing file is the defaults. A file that can't be parsed is moved aside
/// (`settings.toml.unreadable-<seconds>`) and Lens starts on the defaults; one made by a newer Lens is read as
/// far as it can be and never written over (see [`crate::datafile`]).
pub fn load_settings(p: &Paths) -> Result<Settings, String> {
    let path = p.settings();
    let settings = datafile::load_toml::<Settings>(&path).map_err(|e| format!("{}: {e}", path.display()))?.unwrap_or_default();
    NOTIFICATIONS.store(settings.notifications, std::sync::atomic::Ordering::Relaxed);
    Ok(settings)
}

pub fn save_settings(p: &Paths, s: &Settings) -> std::io::Result<()> {
    datafile::guard_write(&p.settings(), datafile::Kind::Toml)?;
    write_atomic(&p.settings(), &toml::to_string_pretty(s).map_err(std::io::Error::other)?)?;
    NOTIFICATIONS.store(s.notifications, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// `chains.json` is a bare list in version 1 (what 0.4.x reads), so a newer file that needs more is an object,
/// `{"schema": N, "chains": [...]}`, which this build reads and never writes.
fn decode_chains(doc: serde_json::Value, schema: u32) -> Result<Vec<Chain>, String> {
    match doc {
        serde_json::Value::Array(_) => serde_json::from_value(doc).map_err(|e| e.to_string()),
        serde_json::Value::Object(mut map) if schema > lens_core::schema::CURRENT => match map.remove("chains") {
            // As far as possible: a chain this build can't read is left out, and the file is never written.
            Some(serde_json::Value::Array(list)) => Ok(list.into_iter().filter_map(|c| serde_json::from_value(c).ok()).collect()),
            _ => Err("no list of chains".into()),
        },
        _ => Err("not a list of chains".into()),
    }
}

pub fn load_chains(p: &Paths) -> Vec<Chain> {
    match datafile::load_json_with(&p.chains(), decode_chains) {
        Ok(Some(chains)) => chains,
        Ok(None) => lens_actions::default_chains(),
        Err(e) => {
            eprintln!("arcade-lens: can't read {}: {e}; using the starter chains", p.chains().display());
            lens_actions::default_chains()
        }
    }
}

pub fn save_chains(p: &Paths, chains: &[Chain]) -> std::io::Result<()> {
    datafile::guard_write(&p.chains(), datafile::Kind::Json)?;
    write_atomic(&p.chains(), &serde_json::to_string_pretty(chains).expect("chains serialize"))
}

pub fn load_usage(p: &Paths) -> UsageStore {
    datafile::load_json(&p.usage())
        .unwrap_or_else(|e| {
            eprintln!("arcade-lens: can't read {}: {e}; using empty counters", p.usage().display());
            None
        })
        .unwrap_or_default()
}

pub fn save_usage(p: &Paths, u: &UsageStore) -> std::io::Result<()> {
    datafile::guard_write(&p.usage(), datafile::Kind::Json)?;
    write_atomic(&p.usage(), &serde_json::to_string(u).expect("usage serialize"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{install, set_aside, temp};

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

    // FS-2 (D15): every data file carries a schema version, 0.4.2's files load with every value kept,
    // and an older Lens never loses or corrupts a newer file. Each test uses its own temp folders.
    fn home(name: &str) -> (PathBuf, Paths) {
        let root = temp(name);
        let p = Paths { config: root.join("config"), data: root.join("data") };
        fs::create_dir_all(&p.config).unwrap();
        fs::create_dir_all(&p.data).unwrap();
        (root, p)
    }

    fn toml_of(path: &Path) -> toml::Table {
        fs::read_to_string(path).unwrap().parse().unwrap()
    }

    fn json_of(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn v042_settings_load_with_every_value_kept() {
        let (root, p) = home("settings-fixture");
        install("settings.toml", &p.settings());
        let s = load_settings(&p).unwrap();
        assert_eq!(s.appearance, lens_core::settings::Appearance::Dark);
        assert!(!s.notifications);
        assert_eq!(s.activation_shortcut.as_deref(), Some("Ctrl+Alt+Shift+K"));
        assert_eq!((s.browser.as_deref(), s.terminal.as_deref(), s.editor.as_deref()), (Some("firefox"), Some("kitty"), Some("code")));
        assert_eq!(s.screenshot_dir, Some(PathBuf::from("/home/ada/Pictures/Shots")));
        assert_eq!(s.image_format, lens_core::settings::ImageFormat::Webp);
        assert_eq!(s.ocr_languages, ["en", "de"]);
        assert_eq!(s.primary_action_count, 4);
        assert_eq!(s.preferred_actions, ["core.copy", "chain:clean-copy"]);
        assert_eq!(s.disabled_recognizers, ["core.recognize.barcode"]);
        assert_eq!(s.disabled_actions, ["core.text.search"]);
        assert_eq!(s.action_keys.get("core.copy"), Some(&'y'));
        assert_eq!(s.action_keys.get("core.save"), Some(&'w'));
        assert_eq!(s.date_order, lens_core::settings::DateOrder::MonthFirst);
        assert_eq!(s.home_currency.as_deref(), Some("EUR"));
        assert_eq!(s.enabled_plugins, ["acme.shout"]);
        assert_eq!(s.providers.web_search, "https://example.org/?q={query}");
        assert_eq!(s.providers.whois, "https://whois.example.org/{query}");
        assert_eq!(s.providers.reverse_image_search.as_deref(), Some("https://images.example.org/?u={query}"));
        assert!(s.privacy.history_enabled && !s.privacy.learn_action_usage && s.privacy.guard_secrets);
        assert!(!s.link.enabled);
        assert_eq!(s.link.disabled_peers, ["arcade.box", "arcade.look"]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_v1_settings_file_is_saved_without_a_schema_field() {
        let (root, p) = home("settings-v1-shape");
        install("settings.toml", &p.settings());
        let s = load_settings(&p).unwrap();
        save_settings(&p, &s).unwrap();
        let saved = toml_of(&p.settings());
        assert!(!saved.contains_key("schema"), "0.4.x must read the file as before");
        assert_eq!(saved["primary_action_count"].as_integer(), Some(4));
        assert_eq!(load_settings(&p).unwrap(), s);
        save_settings(&p, &Settings::default()).unwrap();
        assert!(!toml_of(&p.settings()).contains_key("schema"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_settings_fields_default() {
        let (root, p) = home("settings-missing");
        fs::write(p.settings(), "").unwrap();
        assert_eq!(load_settings(&p).unwrap(), Settings::default());
        fs::write(p.settings(), "primary_action_count = 3\n[privacy]\nhistory_enabled = true\n[link]\ndisabled_peers = [\"arcade.box\"]\n").unwrap();
        let s = load_settings(&p).unwrap();
        let d = Settings::default();
        assert_eq!(s.primary_action_count, 3);
        assert!(s.privacy.history_enabled && s.privacy.guard_secrets && s.privacy.learn_action_usage);
        assert!(s.link.enabled);
        assert_eq!(s.link.disabled_peers, ["arcade.box"]);
        assert_eq!((s.appearance, s.activation_shortcut.clone(), s.providers.clone()), (d.appearance, d.activation_shortcut, d.providers));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unknown_settings_fields_survive_a_save() {
        let (root, p) = home("settings-unknown");
        fs::write(
            p.settings(),
            "future_option = \"keep\"\nprimary_action_count = 4\n[future_table]\nlevels = [1, 2]\n[providers]\nfuture_url = \"https://x.example/\"\n[privacy]\nfuture_switch = true\n[link]\nfuture_peer = \"a\"\n",
        )
        .unwrap();
        let mut s = load_settings(&p).unwrap();
        s.primary_action_count = 6;
        save_settings(&p, &s).unwrap();
        let saved = toml_of(&p.settings());
        assert_eq!(saved["future_option"].as_str(), Some("keep"));
        assert_eq!(saved["future_table"]["levels"].as_array().unwrap().len(), 2);
        assert_eq!(saved["providers"]["future_url"].as_str(), Some("https://x.example/"));
        assert_eq!(saved["privacy"]["future_switch"].as_bool(), Some(true));
        assert_eq!(saved["link"]["future_peer"].as_str(), Some("a"));
        assert_eq!(saved["primary_action_count"].as_integer(), Some(6));
        assert_eq!(load_settings(&p).unwrap(), s);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_newer_settings_file_is_read_and_never_written_over() {
        let (root, p) = home("settings-newer");
        let newer = "schema = 2\nprimary_action_count = 7\nappearance = \"light\"\nshiny = { a = 1 }\n";
        fs::write(p.settings(), newer).unwrap();
        let s = load_settings(&p).unwrap();
        assert_eq!((s.primary_action_count, s.appearance), (7, lens_core::settings::Appearance::Light));
        let error = save_settings(&p, &s).unwrap_err().to_string();
        assert!(error.contains("made by a newer Arcade Lens"), "{error}");
        assert_eq!(fs::read_to_string(p.settings()).unwrap(), newer);
        // A newer file this build cannot map is left as it is too, and Lens runs on defaults.
        let odd = "schema = 3\nappearance = { name = \"dark\" }\n";
        fs::write(p.settings(), odd).unwrap();
        let s = load_settings(&p).unwrap();
        assert_eq!(s.appearance, Settings::default().appearance);
        assert!(save_settings(&p, &s).is_err());
        assert_eq!(fs::read_to_string(p.settings()).unwrap(), odd);
        assert!(set_aside(&p.config, "settings.toml").is_empty());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unreadable_settings_file_is_moved_aside_not_destroyed() {
        let (root, p) = home("settings-unreadable");
        for (n, bytes) in [&b"primary_action_count = = 4"[..], b"primary_action_count = \"many\"\n", b"\xff\xfe\x00 not toml"].into_iter().enumerate() {
            fs::write(p.settings(), bytes).unwrap();
            let s = load_settings(&p).expect("Lens starts with defaults");
            assert_eq!(s, Settings::default(), "case {n}");
            let kept = set_aside(&p.config, "settings.toml");
            assert_eq!(kept.len(), 1, "case {n}");
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            assert!(!p.settings().exists());
            // The next save writes a good file next to it.
            save_settings(&p, &s).unwrap();
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            fs::remove_file(&kept[0]).unwrap();
            fs::remove_file(p.settings()).unwrap();
        }
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_file_that_turned_unreadable_before_a_save_is_kept_aside() {
        let (root, p) = home("settings-unreadable-save");
        fs::write(p.settings(), "this = [is not toml").unwrap();
        save_settings(&p, &Settings::default()).unwrap();
        let kept = set_aside(&p.config, "settings.toml");
        assert_eq!(kept.len(), 1);
        assert_eq!(fs::read(&kept[0]).unwrap(), b"this = [is not toml");
        assert!(load_settings(&p).is_ok());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn v042_chains_load_with_every_value_kept() {
        let (root, p) = home("chains-fixture");
        let bytes = install("chains.json", &p.chains());
        let chains = load_chains(&p);
        assert_eq!(chains.len(), 2);
        assert_eq!((chains[0].id.as_str(), chains[0].name.as_str(), chains[0].steps.len()), ("clean-copy", "Clean Copy", 3));
        let c = &chains[1];
        assert_eq!((c.id.as_str(), c.name.as_str()), ("small-webp", "Small WebP"));
        assert!(matches!(&c.steps[0], lens_core::chain::ChainStep::Take { capability, .. } if capability.as_str() == "region"));
        match &c.steps[1] {
            lens_core::chain::ChainStep::Run { action, params, .. } => {
                assert_eq!(action, "core.image.resize");
                assert_eq!(params["percent"], 33);
            }
            other => panic!("{other:?}"),
        }
        // Saving what was loaded writes the shape 0.4.2 wrote, a bare list with no schema.
        save_chains(&p, &chains).unwrap();
        assert_eq!(json_of(&p.chains()), serde_json::from_slice::<serde_json::Value>(&bytes).unwrap());
        assert!(json_of(&p.chains()).is_array());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_chain_fields_default() {
        let (root, p) = home("chains-missing");
        fs::write(p.chains(), r#"[{"id":"bare"},{"id":"named","name":"Named","steps":[{"step":"take","capability":"text"}]}]"#).unwrap();
        let chains = load_chains(&p);
        assert_eq!(chains.len(), 2);
        assert_eq!((chains[0].name.as_str(), chains[0].steps.len()), ("", 0));
        assert_eq!(chains[1].steps.len(), 1);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unknown_chain_fields_survive_a_save() {
        let (root, p) = home("chains-unknown");
        fs::write(
            p.chains(),
            r#"[{"id":"a","name":"A","hotkey":"F9","steps":[{"step":"take","capability":"text","min":2},{"step":"run","action":"core.copy","note":{"x":[1]}}]}]"#,
        )
        .unwrap();
        let mut chains = load_chains(&p);
        chains[0].name = "Renamed".into();
        save_chains(&p, &chains).unwrap();
        let saved = json_of(&p.chains());
        assert_eq!(saved[0]["name"], "Renamed");
        assert_eq!(saved[0]["hotkey"], "F9");
        assert_eq!(saved[0]["steps"][0]["min"], 2);
        assert_eq!(saved[0]["steps"][1]["note"]["x"][0], 1);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_newer_chains_file_is_read_and_never_written_over() {
        let (root, p) = home("chains-newer");
        let newer = r#"{"schema":2,"chains":[{"id":"n","name":"N","steps":[{"step":"take","capability":"text"}]}],"folders":["x"]}"#;
        fs::write(p.chains(), newer).unwrap();
        let chains = load_chains(&p);
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].id, "n");
        let error = save_chains(&p, &chains).unwrap_err().to_string();
        assert!(error.contains("made by a newer Arcade Lens"), "{error}");
        assert_eq!(fs::read_to_string(p.chains()).unwrap(), newer);
        assert!(set_aside(&p.config, "chains.json").is_empty());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unreadable_chains_file_is_moved_aside_not_destroyed() {
        let (root, p) = home("chains-unreadable");
        for bytes in [&b"[{\"id\": "[..], br#"[{"id":"a","steps":"none"}]"#, br#"{"chains":3}"#] {
            fs::write(p.chains(), bytes).unwrap();
            let chains = load_chains(&p);
            assert_eq!(chains.len(), 3, "the starter chains");
            let kept = set_aside(&p.config, "chains.json");
            assert_eq!(kept.len(), 1);
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            save_chains(&p, &chains).unwrap();
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            fs::remove_file(&kept[0]).unwrap();
            fs::remove_file(p.chains()).unwrap();
        }
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn v042_usage_loads_with_every_value_kept() {
        let (root, p) = home("usage-fixture");
        let bytes = install("usage.json", &p.usage());
        let u = load_usage(&p);
        assert!(!u.is_empty());
        let now = 1_760_000_000;
        assert!((u.boost("url", "core.url.open", now) - lens_core::usage::MAX_BOOST * 4.5 / 7.5).abs() < 1e-3);
        assert!(u.boost("text", "core.text.search", 1_760_200_000) > 0.0);
        save_usage(&p, &u).unwrap();
        assert_eq!(fs::read(p.usage()).unwrap(), bytes, "a v1 file is rewritten exactly as 0.4.2 wrote it");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_usage_fields_default_and_unknown_ones_survive() {
        let (root, p) = home("usage-unknown");
        fs::write(p.usage(), "{}").unwrap();
        assert!(load_usage(&p).is_empty());
        fs::write(p.usage(), r#"{"entries":{"url":{"core.copy":{"weight":2.0,"at":5,"note":"n"}}},"future":{"k":[1]}}"#).unwrap();
        let mut u = load_usage(&p);
        u.record("url", "core.url.open", 9);
        save_usage(&p, &u).unwrap();
        let saved = json_of(&p.usage());
        assert_eq!(saved["future"]["k"][0], 1);
        assert_eq!(saved["entries"]["url"]["core.copy"]["note"], "n");
        assert!(saved["entries"]["url"]["core.url.open"].is_object());
        assert!(saved.get("schema").is_none());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_newer_usage_file_is_read_and_never_written_over() {
        let (root, p) = home("usage-newer");
        let newer = r#"{"schema":2,"entries":{"url":{"core.copy":{"weight":2.0,"at":5}}},"model":"x"}"#;
        fs::write(p.usage(), newer).unwrap();
        let mut u = load_usage(&p);
        assert!(u.boost("url", "core.copy", 5) > 0.0);
        u.record("url", "core.copy", 6);
        assert!(save_usage(&p, &u).unwrap_err().to_string().contains("made by a newer Arcade Lens"));
        assert_eq!(fs::read_to_string(p.usage()).unwrap(), newer);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unreadable_usage_file_is_moved_aside_not_destroyed() {
        let (root, p) = home("usage-unreadable");
        for bytes in [&b"{\"entries\": "[..], br#"{"entries":{"url":{"a":{"weight":"heavy"}}}}"#, b"[]"] {
            fs::write(p.usage(), bytes).unwrap();
            let u = load_usage(&p);
            assert!(u.is_empty());
            let kept = set_aside(&p.data, "usage.json");
            assert_eq!(kept.len(), 1);
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            save_usage(&p, &u).unwrap();
            assert_eq!(fs::read(&kept[0]).unwrap(), bytes);
            fs::remove_file(&kept[0]).unwrap();
            fs::remove_file(p.usage()).unwrap();
        }
        fs::remove_dir_all(root).ok();
    }
}
