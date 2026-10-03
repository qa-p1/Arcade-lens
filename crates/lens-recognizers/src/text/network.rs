//! IPv4/IPv6 addresses and domain names.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::sync::LazyLock;

use lens_core::value::{IpScope, IpValue};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{contact, overlaps, url, TextInput};

static IPV4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}(?::(\d{1,5}))?\b").unwrap());
static IPV6: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([0-9A-Fa-f:.]{2,45})\](?::(\d{1,5}))?|[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}(?:%\w+)?").unwrap());
static DOMAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+([a-z]{2,24})\b\.?").unwrap());

/// Generic and popular country TLDs. A curated list (rather than every TLD in
/// existence) keeps `self.name` or `file.zip` from being reported as domains.
const TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "mil", "int", "info", "biz", "dev", "app", "io", "ai", "co", "me", "tv", "xyz", "site", "online", "tech",
    "store", "shop", "blog", "cloud", "page", "wiki", "news", "live", "art", "design", "email", "pro", "team", "tools", "systems", "games",
    "uk", "us", "ca", "au", "nz", "de", "fr", "nl", "be", "ch", "at", "se", "no", "dk", "fi", "es", "pt", "it", "ie", "pl", "cz", "ru", "ua",
    "jp", "kr", "cn", "tw", "hk", "sg", "in", "id", "my", "th", "vn", "ph", "br", "ar", "mx", "cl", "za", "ng", "ke", "eg", "tr", "il", "ae",
    "sa", "eu", "gg", "fm", "ly", "sh", "to", "is", "so", "rs", "md", "py", "pl", "cc",
];

/// TLDs that are also common file extensions: `main.rs`, `README.md`,
/// `setup.py`, `run.sh`. Only accepted with a known site or 3+ labels.
const EXTENSION_TLDS: &[&str] = &["rs", "md", "py", "sh", "pl", "so", "cc", "to", "is", "me", "in", "at", "id", "io", "ai", "co", "ly", "tv", "zip", "mov"];
const KNOWN_EXTENSION_SITES: &[&str] = &["docs.rs", "crates.io", "github.io", "bit.ly", "t.co", "youtu.be", "notion.so", "x.ai"];

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    let urls: Vec<Range<usize>> = url::find(text).into_iter().map(|(r, _)| r).collect();
    let emails = contact::emails(text);
    let mut ip_ranges = Vec::new();

    for c in IPV4.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        // Reject version strings like 1.2.3.4.5 and dotted runs.
        let before = text[..r.start].chars().next_back();
        let after: String = text[r.end..].chars().take(2).collect();
        if before.is_some_and(|ch| ch == '.' || ch.is_ascii_digit()) || (after.starts_with('.') && after[1..].starts_with(|ch: char| ch.is_ascii_digit())) {
            continue;
        }
        let addr_str = m.as_str().split(':').next().unwrap();
        let Ok(addr) = addr_str.parse::<Ipv4Addr>() else { continue };
        let port = c.get(1).and_then(|p| p.as_str().parse::<u16>().ok());
        ip_ranges.push(r.clone());
        out.push(ip_detection(IpAddr::V4(addr), port, r));
    }

    for c in IPV6.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        let (candidate, port) = match c.get(1) {
            Some(inner) => (inner.as_str(), c.get(2).and_then(|p| p.as_str().parse::<u16>().ok())),
            None => (m.as_str(), None),
        };
        let candidate = candidate.split('%').next().unwrap();
        // Require real IPv6 shape: `::` compression or all eight groups.
        // This rejects clock times and timecodes such as 01:42:18.
        let groups = candidate.split(':').filter(|g| !g.is_empty()).count();
        if !(candidate.contains("::") && groups >= 1 || groups == 8) || candidate == "::" {
            continue;
        }
        if text[r.end..].starts_with(|ch: char| ch.is_ascii_alphanumeric()) || text[..r.start].ends_with(|ch: char| ch.is_ascii_alphanumeric()) {
            continue;
        }
        if let Ok(addr) = candidate.parse::<Ipv6Addr>() {
            ip_ranges.push(r.clone());
            out.push(ip_detection(IpAddr::V6(addr), port, r));
        }
    }

    for c in DOMAIN.captures_iter(text) {
        let m = c.get(0).unwrap();
        let raw = m.as_str().trim_end_matches('.');
        let r = m.start()..m.start() + raw.len();
        if urls.iter().chain(&emails).chain(&ip_ranges).any(|u| overlaps(u, &r)) {
            continue;
        }
        let tld = c.get(1).unwrap().as_str().to_ascii_lowercase();
        let host = raw.to_ascii_lowercase();
        let labels = host.split('.').count();
        if !TLDS.contains(&tld.as_str()) {
            continue;
        }
        if EXTENSION_TLDS.contains(&tld.as_str()) && labels < 3 && !KNOWN_EXTENSION_SITES.contains(&host.as_str()) {
            continue;
        }
        // Path components, member access and calls are not domains.
        let before = text[..r.start].chars().next_back();
        let after = text[r.end..].chars().next();
        if before.is_some_and(|ch| "/\\@._-$".contains(ch) || ch.is_alphanumeric()) || after.is_some_and(|ch| "(/\\_=-".contains(ch) || ch.is_alphanumeric()) {
            continue;
        }
        let confidence = if labels >= 3 || host.starts_with("www.") { 0.85 } else { 0.75 };
        out.push(Detection::new(caps::DOMAIN, Value::Domain(host)).span(r).confidence(confidence));
    }
    out
}

fn ip_detection(addr: IpAddr, port: Option<u16>, r: Range<usize>) -> Detection {
    let scope = scope(&addr);
    let label = match scope {
        IpScope::Loopback => "Loopback",
        IpScope::Private => "Private network",
        IpScope::LinkLocal => "Link-local",
        IpScope::Public => "Public",
        IpScope::Unspecified => "Unspecified",
        IpScope::Multicast => "Multicast",
        IpScope::Documentation => "Documentation range",
    };
    let version = if addr.is_ipv4() { "IPv4" } else { "IPv6" };
    Detection::new(caps::IP_ADDRESS, Value::Ip(IpValue { addr, port, scope }))
        .span(r)
        .confidence(if addr.is_ipv4() { 0.95 } else { 0.9 })
        .detail("Type", format!("{version} · {label}"))
}

pub fn scope(addr: &IpAddr) -> IpScope {
    match addr {
        IpAddr::V4(a) => {
            let o = a.octets();
            if a.is_loopback() {
                IpScope::Loopback
            } else if a.is_unspecified() {
                IpScope::Unspecified
            } else if a.is_private() || (o[0] == 100 && (64..128).contains(&o[1])) {
                IpScope::Private
            } else if a.is_link_local() {
                IpScope::LinkLocal
            } else if a.is_multicast() {
                IpScope::Multicast
            } else if a.is_documentation() {
                IpScope::Documentation
            } else {
                IpScope::Public
            }
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            if a.is_loopback() {
                IpScope::Loopback
            } else if a.is_unspecified() {
                IpScope::Unspecified
            } else if a.is_multicast() {
                IpScope::Multicast
            } else if (s[0] & 0xffc0) == 0xfe80 {
                IpScope::LinkLocal
            } else if (s[0] & 0xfe00) == 0xfc00 {
                IpScope::Private
            } else if s[0] == 0x2001 && s[1] == 0x0db8 {
                IpScope::Documentation
            } else {
                IpScope::Public
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn found(text: &str) -> Vec<(String, String)> {
        detect(&TextInput { text, layout: None }, &cx()).into_iter().map(|d| (d.capability.to_string(), d.value.as_text().unwrap().into_owned())).collect()
    }

    #[test]
    fn ipv4() {
        assert_eq!(found("ssh to 192.168.1.34 now"), vec![("ip-address".into(), "192.168.1.34".into())]);
        assert_eq!(found("bind 10.0.0.5:8080"), vec![("ip-address".into(), "10.0.0.5:8080".into())]);
        assert!(found("version 1.2.3.4.5").is_empty());
        assert!(found("999.1.1.1").is_empty());
    }

    #[test]
    fn ipv6() {
        assert_eq!(found("addr fe80::1ff:fe23:4567:890a here"), vec![("ip-address".into(), "fe80::1ff:fe23:4567:890a".into())]);
        assert_eq!(found("[2001:db8::1]:443"), vec![("ip-address".into(), "[2001:db8::1]:443".into())]);
        assert!(found("at 01:42:18 and 12:30").is_empty());
        assert!(found("std::vec::Vec").is_empty());
    }

    #[test]
    fn domains() {
        assert_eq!(found("Visit example.com today"), vec![("domain".into(), "example.com".into())]);
        assert_eq!(found("served by api.github.com."), vec![("domain".into(), "api.github.com".into())]);
        assert_eq!(found("see docs.rs"), vec![("domain".into(), "docs.rs".into())]);
        for t in ["edit src/main.rs", "open README.md", "run setup.py", "self.name = x", "console.log(x)", "https://example.com/a", "me@example.com", "file.zip"] {
            assert!(found(t).iter().all(|(c, _)| c != "domain"), "{t}: {:?}", found(t));
        }
    }

    #[test]
    fn scopes() {
        assert_eq!(scope(&"8.8.8.8".parse().unwrap()), IpScope::Public);
        assert_eq!(scope(&"172.20.1.1".parse().unwrap()), IpScope::Private);
        assert_eq!(scope(&"::1".parse().unwrap()), IpScope::Loopback);
        assert_eq!(scope(&"fd00::1".parse().unwrap()), IpScope::Private);
    }
}
