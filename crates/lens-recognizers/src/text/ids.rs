//! Hashes and UUIDs.

use std::sync::LazyLock;

use lens_core::value::{HashValue, UuidValue};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{overlaps, TextInput};

static UUID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b").unwrap());
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{7,128}\b").unwrap());
static COMMIT_CONTEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(commit|sha|rev|revision|hash|cherry-pick|checkout|revert)\b|^\s*$").unwrap());

/// Hex digests are only labelled by length, and always with every plausible
/// algorithm: a 64-char digest could be SHA-256, SHA3-256 or BLAKE2s.
fn candidates(len: usize) -> Option<Vec<&'static str>> {
    Some(match len {
        32 => vec!["MD5", "128-bit hash (e.g. NTLM, MD4)"],
        40 => vec!["SHA-1", "Git commit"],
        56 => vec!["SHA-224", "SHA3-224"],
        64 => vec!["SHA-256", "SHA3-256", "BLAKE2s/BLAKE3"],
        96 => vec!["SHA-384", "SHA3-384"],
        128 => vec!["SHA-512", "SHA3-512", "BLAKE2b"],
        _ => return None,
    })
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    let mut uuids = Vec::new();
    for m in UUID.find_iter(text) {
        let s = m.as_str();
        let version = s[14..15].parse::<u8>().ok().filter(|v| (1..=8).contains(v) && "89abAB".contains(&s[19..20]));
        uuids.push(m.range());
        let mut d = Detection::new(caps::UUID, Value::Uuid(UuidValue { text: s.to_string(), version })).span(m.range()).confidence(0.97);
        if let Some(v) = version {
            d = d.detail("Version", format!("UUID v{v}"));
        }
        out.push(d);
    }
    for m in HEX.find_iter(text) {
        let r = m.range();
        let s = m.as_str();
        if uuids.iter().any(|u| overlaps(u, &r)) {
            continue;
        }
        let before = &text[..r.start];
        // 0x… is a number/address; #… is a color; `-` joins identifiers.
        if before.ends_with("0x") || before.ends_with("0X") || before.ends_with('#') || before.ends_with('-') || text[r.end..].starts_with('-') {
            continue;
        }
        let has_letter = s.chars().any(|c| c.is_ascii_alphabetic());
        let has_digit = s.chars().any(|c| c.is_ascii_digit());
        if !has_letter || !has_digit {
            continue;
        }
        let mixed_case = s.chars().any(|c| c.is_ascii_lowercase()) && s.chars().any(|c| c.is_ascii_uppercase());
        let (cands, confidence) = match candidates(s.len()) {
            Some(c) => (c, if mixed_case { 0.5 } else { 0.9 }),
            None if (7..=12).contains(&s.len()) && !mixed_case => {
                // Short Git hashes only with nearby context or at a line start (git log --oneline).
                let line_start = before.rsplit('\n').next().unwrap_or_default();
                if COMMIT_CONTEXT.is_match(line_start) {
                    (vec!["Git commit (abbreviated)"], 0.7)
                } else {
                    continue;
                }
            }
            None => continue,
        };
        let label = cands.join(" / ");
        out.push(
            Detection::new(caps::HASH, Value::Hash(HashValue { hex: s.to_string(), candidates: cands.into_iter().map(String::from).collect() }))
                .span(r)
                .confidence(confidence)
                .detail("Possible formats", label),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn found(text: &str) -> Vec<(String, String)> {
        detect(&TextInput { text, layout: None }, &cx())
            .into_iter()
            .map(|d| (d.capability.to_string(), d.details.last().map(|x| x.1.clone()).unwrap_or_default()))
            .collect()
    }

    #[test]
    fn uuids() {
        assert_eq!(found("id 550e8400-e29b-41d4-a716-446655440000"), vec![("uuid".into(), "UUID v4".into())]);
    }

    #[test]
    fn hashes_are_never_overclaimed() {
        let sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(found(sha256), vec![("hash".into(), "SHA-256 / SHA3-256 / BLAKE2s/BLAKE3".into())]);
        assert_eq!(found("d41d8cd98f00b204e9800998ecf8427e"), vec![("hash".into(), "MD5 / 128-bit hash (e.g. NTLM, MD4)".into())]);
        assert_eq!(found("commit 3f2a9c1b"), vec![("hash".into(), "Git commit (abbreviated)".into())]);
        assert_eq!(found("3f2a9c1 Fix the parser"), vec![("hash".into(), "Git commit (abbreviated)".into())]);
    }

    #[test]
    fn non_hashes() {
        for t in ["deadbeef cafe", "0x7ffeefbff5c8", "#18181B", "1234567890", "accede", "word 3f2a9c1b here"] {
            assert!(found(t).is_empty(), "{t}");
        }
    }
}
