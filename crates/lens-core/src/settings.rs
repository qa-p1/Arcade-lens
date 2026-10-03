//! User settings. Persisted as TOML by the application shell.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Global activation shortcut. `None` disables it. The default is only a
    /// proposal shown on first run; it deliberately avoids common OS bindings
    /// (Win+Shift+S, Cmd+Shift+4, Ctrl+Alt+L, Print Screen).
    pub activation_shortcut: Option<String>,
    pub browser: Option<String>,
    pub terminal: Option<String>,
    pub editor: Option<String>,
    pub screenshot_dir: Option<PathBuf>,
    pub image_format: ImageFormat,
    pub ocr_languages: Vec<String>,
    /// Number of actions shown before the overflow (`•••`) button.
    pub primary_action_count: usize,
    /// Action ids the user always wants promoted when applicable.
    pub preferred_actions: Vec<String>,
    pub disabled_recognizers: Vec<String>,
    pub disabled_actions: Vec<String>,
    /// Per-action keyboard overrides (action id → key).
    pub action_keys: BTreeMap<String, char>,
    pub providers: Providers,
    pub date_order: DateOrder,
    /// ISO 4217 code used as the default currency-conversion target.
    pub home_currency: Option<String>,
    pub privacy: Privacy,
    /// Third-party plugins the user has explicitly enabled (by id).
    pub enabled_plugins: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            activation_shortcut: Some("Ctrl+Alt+Shift+L".into()),
            browser: None,
            terminal: None,
            editor: None,
            screenshot_dir: None,
            image_format: ImageFormat::Png,
            ocr_languages: vec!["en".into()],
            primary_action_count: 5,
            preferred_actions: Vec::new(),
            disabled_recognizers: Vec::new(),
            disabled_actions: Vec::new(),
            action_keys: BTreeMap::new(),
            providers: Providers::default(),
            date_order: DateOrder::DayFirst,
            home_currency: None,
            privacy: Privacy::default(),
            enabled_plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
        }
    }
}

/// Preferred reading of all-numeric dates like `05/03/2024`. Ambiguous dates
/// are still flagged; this only decides which interpretation is listed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateOrder {
    DayFirst,
    MonthFirst,
}

/// URL templates for actions that inherently need an external service.
/// `{query}`, `{lat}` and `{lon}` are substituted (percent-encoded).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Providers {
    pub web_search: String,
    pub maps_search: String,
    pub maps_coordinates: String,
    pub product_search: String,
    pub whois: String,
    /// Reverse image search requires uploading pixels, so it has no default.
    pub reverse_image_search: Option<String>,
}

impl Default for Providers {
    fn default() -> Self {
        Self {
            web_search: "https://duckduckgo.com/?q={query}".into(),
            maps_search: "https://www.openstreetmap.org/search?query={query}".into(),
            maps_coordinates: "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}".into(),
            product_search: "https://duckduckgo.com/?q={query}+barcode".into(),
            whois: "https://who.is/whois/{query}".into(),
            reverse_image_search: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
    /// Keep a local history of selections. Off by default.
    pub history_enabled: bool,
    /// Learn which actions the user prefers (local counters only).
    pub learn_action_usage: bool,
    /// Hide outbound actions (search, upload, send) for selections that
    /// contain something that looks like a secret.
    pub guard_secrets: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Self { history_enabled: false, learn_action_usage: true, guard_secrets: true }
    }
}

/// Fills a provider template, percent-encoding every substituted value.
pub fn fill_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), &percent_encode(v));
    }
    out
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let s = Settings::default();
        let text = toml::to_string(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        let partial: Settings = toml::from_str("primary_action_count = 4\n[providers]\nweb_search = \"https://example.com/?q={query}\"").unwrap();
        assert_eq!(partial.primary_action_count, 4);
        assert_eq!(partial.providers.web_search, "https://example.com/?q={query}");
        assert_eq!(partial.providers.whois, Providers::default().whois);
    }

    #[test]
    fn templates_are_encoded() {
        assert_eq!(fill_template("https://x/?q={query}", &[("query", "a b&c=ü")]), "https://x/?q=a+b%26c%3D%C3%BC");
    }
}
