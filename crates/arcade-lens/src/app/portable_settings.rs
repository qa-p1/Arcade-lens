//! A deliberately small allowlist for settings handoff.
//!
//! Service URL templates, executable paths, plugin state, chains, screenshot
//! paths, history and usage are local data and never enter this document.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use arcade_link::{Content, Handoff, LinkError, Locations};
use lens_core::settings::{Appearance, DateOrder, ImageFormat, Privacy};
use lens_core::Settings;
use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_LIST: usize = 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableSettings {
    pub schema: u32,
    pub app: String,
    pub version: String,
    /// Added in 0.4.0; older documents follow the system.
    #[serde(default)]
    pub appearance: Appearance,
    pub activation_shortcut: Option<String>,
    pub image_format: ImageFormat,
    pub ocr_languages: Vec<String>,
    pub primary_action_count: usize,
    pub preferred_actions: Vec<String>,
    pub disabled_recognizers: Vec<String>,
    pub disabled_actions: Vec<String>,
    pub action_keys: std::collections::BTreeMap<String, char>,
    pub date_order: DateOrder,
    pub home_currency: Option<String>,
    pub privacy: Privacy,
}

fn valid_text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

impl PortableSettings {
    pub fn export(settings: &Settings) -> Self {
        Self {
            schema: 1,
            app: crate::link::ME.into(),
            version: crate::link::VERSION.into(),
            appearance: settings.appearance,
            activation_shortcut: settings.activation_shortcut.as_deref().and_then(|s| arcade_link::accelerator::normalize(s).ok()),
            image_format: settings.image_format,
            ocr_languages: settings.ocr_languages.clone(),
            primary_action_count: settings.primary_action_count,
            preferred_actions: settings.preferred_actions.clone(),
            disabled_recognizers: settings.disabled_recognizers.clone(),
            disabled_actions: settings.disabled_actions.clone(),
            action_keys: settings.action_keys.clone(),
            date_order: settings.date_order,
            home_currency: settings.home_currency.clone(),
            privacy: settings.privacy.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), LinkError> {
        let bounded = [&self.ocr_languages, &self.preferred_actions, &self.disabled_recognizers, &self.disabled_actions];
        if self.schema != 1
            || self.app != crate::link::ME
            || !valid_text(&self.version, 128)
            || self.primary_action_count > 20
            || bounded.iter().any(|v| v.len() > MAX_LIST || v.iter().any(|s| !valid_text(s, 512)))
            || self.action_keys.len() > MAX_LIST
            || self.action_keys.keys().any(|s| !valid_text(s, 256))
            || self.ocr_languages.is_empty()
            || self.ocr_languages.iter().any(|s| !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            return Err(LinkError::denied("invalid Arcade Lens settings document"));
        }
        if let Some(shortcut) = &self.activation_shortcut {
            // The same rules as rebinding from Tools: one chord, a modifier or
            // function key, and never a reserved system shortcut.
            crate::app::hub::validate_shortcut(shortcut)?;
        }
        if let Some(currency) = &self.home_currency {
            if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_uppercase()) {
                return Err(LinkError::denied("home currency must be a three-letter ISO code"));
            }
        }
        Ok(())
    }

    /// Apply only exported fields, preserving paths, service configuration,
    /// plugins, Link preferences, and other machine-local settings.
    pub fn apply_to(&self, current: &Settings) -> Settings {
        let mut next = current.clone();
        next.appearance = self.appearance;
        next.activation_shortcut = self.activation_shortcut.clone();
        next.image_format = self.image_format;
        next.ocr_languages = self.ocr_languages.clone();
        next.primary_action_count = self.primary_action_count;
        next.preferred_actions = self.preferred_actions.clone();
        next.disabled_recognizers = self.disabled_recognizers.clone();
        next.disabled_actions = self.disabled_actions.clone();
        next.action_keys = self.action_keys.clone();
        next.date_order = self.date_order;
        next.home_currency = self.home_currency.clone();
        next.privacy = self.privacy.clone();
        next
    }
}

pub fn export_file(locations: &Locations, settings: &Settings) -> Result<Vec<Content>, LinkError> {
    let document = PortableSettings::export(settings);
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| LinkError::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let handoff = Handoff::create(locations, crate::link::ME).map_err(io_error)?;
    let file = handoff.file("arcade-lens-settings.json", &bytes).map_err(io_error)?;
    handoff.keep();
    Ok(vec![file])
}

pub fn read_import(inputs: &[Content]) -> Result<PortableSettings, LinkError> {
    if inputs.len() != 1 || !inputs[0].kind.starts_with("file/") {
        return Err(LinkError::denied("import one Arcade Lens settings file"));
    }
    let path = inputs[0].path.as_deref().ok_or_else(|| LinkError::denied("settings input has no file path"))?;
    let meta = fs::metadata(path).map_err(io_error)?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let bytes = fs::read(path).map_err(io_error)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let document: PortableSettings = serde_json::from_slice(&bytes).map_err(|e| LinkError::denied(&e.to_string()))?;
    document.validate()?;
    Ok(document)
}

/// Save an allowlisted import with a private copy of the prior settings for
/// rollback if the manifest write or live shortcut change fails.
pub fn save_import(paths: &crate::config::Paths, old: &Settings, new: &Settings) -> io::Result<PathBuf> {
    let settings_path = paths.settings();
    let backup_dir = paths.config.join("backups");
    arcade_link::paths::ensure_private_dir(&backup_dir)?;
    let backup = backup_dir.join(format!("settings-{}.toml", arcade_link::endpoint::new_token()?));
    let old_bytes = match fs::read(&settings_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::to_string_pretty(old).map_err(io::Error::other)?.into_bytes(),
        Err(e) => return Err(e),
    };
    arcade_link::paths::write_atomic(&backup, &old_bytes, true)?;
    if let Err(e) = crate::config::save_settings(paths, new) {
        let _ = restore_backup(&settings_path, &backup);
        return Err(e);
    }
    Ok(backup)
}

pub fn restore_backup(path: &Path, backup: &Path) -> io::Result<()> {
    let bytes = fs::read(backup)?;
    arcade_link::paths::write_atomic(path, &bytes, true)
}

pub fn io_error(e: io::Error) -> LinkError {
    LinkError::unavailable(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_round_trip_excludes_sensitive_and_machine_local_fields() {
        let settings = Settings {
            browser: Some("secret browser path".into()),
            screenshot_dir: Some(PathBuf::from("/private/screenshots")),
            providers: Default::default(),
            enabled_plugins: vec!["local-plugin".into()],
            link: lens_core::settings::LinkSettings { enabled: false, disabled_peers: vec!["arcade.box".into()] },
            activation_shortcut: Some("Ctrl+Alt+Shift+L".into()),
            ..Default::default()
        };
        let value = PortableSettings::export(&settings);
        let text = serde_json::to_string(&value).unwrap();
        // Privacy toggles (history/usage learning on or off) are preferences;
        // paths, services, plugins and Link peers are machine-local.
        for forbidden in ["browser", "screenshot", "providers", "duckduckgo", "enabledPlugins", "local-plugin", "disabledPeers", "arcade.box"] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
        let parsed: PortableSettings = serde_json::from_str(&text).unwrap();
        parsed.validate().unwrap();
        let local = Settings { link: Default::default(), browser: Some("local".into()), ..Default::default() };
        let imported = parsed.apply_to(&local);
        assert_eq!(imported.link, local.link);
        assert_eq!(imported.browser, local.browser);
        assert_eq!(imported.activation_shortcut, settings.activation_shortcut);
    }

    #[test]
    fn import_rejects_foreign_schema_and_unsafe_shortcuts() {
        let mut doc = PortableSettings::export(&Settings::default());
        doc.app = "arcade.find".into();
        assert!(doc.validate().is_err());
        doc.app = crate::link::ME.into();
        doc.activation_shortcut = Some("Alt+Tab".into());
        assert!(doc.validate().is_err());
    }
}
