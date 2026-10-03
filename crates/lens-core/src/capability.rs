//! Capabilities: open-ended labels describing what a piece of recognized
//! content *is* (and therefore what can be done with it).
//!
//! Capabilities are strings rather than a closed enum so plugins can add new
//! ones without touching the core. Built-in capabilities live in [`caps`].
//! A [`CapabilityGraph`] records "is-a" relationships (a URL is text, a
//! region is an image), which the chain engine uses for type compatibility.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(Cow<'static, str>);

impl Capability {
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    pub fn custom(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Built-in capabilities.
pub mod caps {
    use super::Capability;

    /// Matches every capability. Only meaningful as an action input in chains.
    pub const ANY: Capability = Capability::new("*");

    // Pixel-level
    pub const REGION: Capability = Capability::new("region");
    pub const IMAGE: Capability = Capability::new("image");
    pub const ICON: Capability = Capability::new("icon");
    pub const COLOR: Capability = Capability::new("color");
    pub const PALETTE: Capability = Capability::new("palette");
    pub const UI_ELEMENT: Capability = Capability::new("ui-element");
    pub const WINDOW: Capability = Capability::new("window");
    pub const QR_CODE: Capability = Capability::new("qr-code");
    pub const BARCODE: Capability = Capability::new("barcode");

    // Text and structured text
    pub const TEXT: Capability = Capability::new("text");
    pub const TABLE: Capability = Capability::new("table");
    pub const CODE: Capability = Capability::new("code");
    pub const COMMAND: Capability = Capability::new("command");
    pub const ERROR: Capability = Capability::new("error");
    pub const URL: Capability = Capability::new("url");
    pub const EMAIL: Capability = Capability::new("email");
    pub const PHONE: Capability = Capability::new("phone");
    pub const ADDRESS: Capability = Capability::new("address");
    pub const DATE_TIME: Capability = Capability::new("date-time");
    pub const TIMECODE: Capability = Capability::new("timecode");
    pub const PATH: Capability = Capability::new("path");
    pub const IP_ADDRESS: Capability = Capability::new("ip-address");
    pub const DOMAIN: Capability = Capability::new("domain");
    pub const HASH: Capability = Capability::new("hash");
    pub const UUID: Capability = Capability::new("uuid");
    pub const SECRET: Capability = Capability::new("secret");
    pub const CURRENCY: Capability = Capability::new("currency");
    pub const QUANTITY: Capability = Capability::new("quantity");
    pub const COORDINATES: Capability = Capability::new("coordinates");

    pub const GIT_COMMIT: Capability = Capability::new("git-commit");
    pub const DOCUMENT: Capability = Capability::new("document");
    pub const SUBTITLE: Capability = Capability::new("subtitle");
    pub const MEDIA_FRAME: Capability = Capability::new("media-frame");

    // Produced by actions
    pub const FILE: Capability = Capability::new("file");
}

/// Directed "is-a" graph between capabilities.
#[derive(Debug, Clone, Default)]
pub struct CapabilityGraph {
    parents: HashMap<Capability, Vec<Capability>>,
}

impl CapabilityGraph {
    pub fn with_builtins() -> Self {
        use caps::*;
        let mut g = Self::default();
        g.add(REGION, IMAGE);
        g.add(ICON, IMAGE);
        for textual in [
            TABLE,
            CODE,
            COMMAND,
            ERROR,
            URL,
            EMAIL,
            PHONE,
            ADDRESS,
            DATE_TIME,
            TIMECODE,
            PATH,
            IP_ADDRESS,
            DOMAIN,
            HASH,
            UUID,
            CURRENCY,
            QUANTITY,
            COORDINATES,
        ] {
            g.add(textual, TEXT);
        }
        g.add(COMMAND, CODE);
        g.add(DOCUMENT, TEXT);
        g.add(SUBTITLE, TEXT);
        g.add(GIT_COMMIT, URL);
        g.add(MEDIA_FRAME, IMAGE);
        g
    }

    pub fn add(&mut self, child: Capability, parent: Capability) {
        let entry = self.parents.entry(child).or_default();
        if !entry.contains(&parent) {
            entry.push(parent);
        }
    }

    /// True if `cap` equals `target`, `target` is [`caps::ANY`], or `cap`
    /// transitively descends from `target`.
    pub fn is_a(&self, cap: &Capability, target: &Capability) -> bool {
        if cap == target || *target == caps::ANY {
            return true;
        }
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([cap.clone()]);
        while let Some(c) = queue.pop_front() {
            for p in self.parents.get(&c).into_iter().flatten() {
                if p == target {
                    return true;
                }
                if seen.insert(p.clone()) {
                    queue.push_back(p.clone());
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::caps::*;
    use super::*;

    #[test]
    fn hierarchy() {
        let g = CapabilityGraph::with_builtins();
        assert!(g.is_a(&URL, &TEXT));
        assert!(g.is_a(&COMMAND, &TEXT));
        assert!(g.is_a(&REGION, &IMAGE));
        assert!(g.is_a(&COLOR, &ANY));
        assert!(!g.is_a(&TEXT, &URL));
        assert!(!g.is_a(&IMAGE, &TEXT));
    }

    #[test]
    fn custom_capabilities_compare_with_builtins() {
        let mut g = CapabilityGraph::with_builtins();
        let isbn = Capability::custom("com.example.isbn");
        g.add(isbn.clone(), BARCODE);
        assert!(g.is_a(&isbn, &BARCODE));
        assert_eq!(Capability::custom("url"), URL);
    }
}
