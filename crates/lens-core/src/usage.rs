//! Local action-preference learning.
//!
//! A deterministic, decaying counter per (capability, action). No content is
//! stored — only which action was chosen for which kind of thing. It lives in
//! a local file, needs no account, and [`UsageStore::reset`] clears it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Half-life of a recorded choice.
const HALF_LIFE_DAYS: f64 = 30.0;
/// Maximum ranking boost from learned usage.
pub const MAX_BOOST: f32 = 25.0;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageStore {
    #[serde(default)]
    entries: BTreeMap<String, BTreeMap<String, UsageStat>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct UsageStat {
    /// Decayed count as of `at`.
    weight: f64,
    /// Unix seconds.
    at: u64,
}

impl UsageStore {
    pub fn record(&mut self, capability: &str, action: &str, now: u64) {
        let stat = self.entries.entry(capability.into()).or_default().entry(action.into()).or_insert(UsageStat { weight: 0.0, at: now });
        stat.weight = decayed(stat, now) + 1.0;
        stat.at = now;
    }

    /// Ranking boost in `[0, MAX_BOOST]`, saturating so a few choices matter
    /// and hundreds don't drown out relevance.
    pub fn boost(&self, capability: &str, action: &str, now: u64) -> f32 {
        let w = self.entries.get(capability).and_then(|m| m.get(action)).map_or(0.0, |s| decayed(s, now));
        (MAX_BOOST as f64 * w / (w + 3.0)) as f32
    }

    pub fn reset(&mut self) {
        self.entries.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn decayed(s: &UsageStat, now: u64) -> f64 {
    let days = now.saturating_sub(s.at) as f64 / 86_400.0;
    s.weight * 0.5f64.powf(days / HALF_LIFE_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_saturates_and_decays() {
        let mut u = UsageStore::default();
        assert_eq!(u.boost("url", "core.url.open", 0), 0.0);
        for _ in 0..3 {
            u.record("url", "core.url.open", 0);
        }
        let b = u.boost("url", "core.url.open", 0);
        assert!((b - MAX_BOOST / 2.0).abs() < 1e-4, "{b}");
        for _ in 0..1000 {
            u.record("url", "core.url.open", 0);
        }
        assert!(u.boost("url", "core.url.open", 0) < MAX_BOOST);
        // Different capability is independent.
        assert_eq!(u.boost("color", "core.url.open", 0), 0.0);
        let later = u.boost("url", "core.url.open", 86_400 * 365 * 2);
        assert!(later < 1.0, "{later}");
        u.reset();
        assert!(u.is_empty());
    }

    #[test]
    fn serializes() {
        let mut u = UsageStore::default();
        u.record("color", "core.color.copy-hex", 100);
        let json = serde_json::to_string(&u).unwrap();
        assert!(!json.contains('#'), "stores no content");
        assert_eq!(serde_json::from_str::<UsageStore>(&json).unwrap(), u);
    }
}
