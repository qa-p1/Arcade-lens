//! Postal addresses (heuristic).
//!
//! Address formats vary enormously, so this deliberately recognizes only
//! well-structured cases: a house number plus a street-type word, or a block
//! of lines ending in a postal code alongside address vocabulary. Results
//! carry moderate confidence.

use std::sync::LazyLock;

use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

const STREET: &str = r"(?:Street|St|Avenue|Ave|Road|Rd|Boulevard|Blvd|Lane|Ln|Drive|Dr|Court|Ct|Way|Place|Pl|Terrace|Parkway|Pkwy|Highway|Hwy|Square|Sq|Circle|Cir|Marg|Nagar|Path|Strasse|Straße|Rue|Via|Calle)";

static STREET_LINE: LazyLock<Regex> = LazyLock::new(|| {
    let city = r"(?:,\s*[A-Z][a-z][A-Za-z.'’-]*(?:\s[A-Z][a-z][A-Za-z.'’-]*){0,3})";
    let postal = r"(?:,?\s+[A-Z]{2}\s+\d{5}(?:-\d{4})?|,?\s+\d{6}|,?\s+[A-Z]{1,2}\d[A-Z\d]?\s*\d[A-Z]{2})";
    Regex::new(&format!(
        r"\b\d{{1,6}}[A-Za-z]?(?:[-/]\d+)?,?\s+(?:[A-Z0-9][\w.'’-]*\s+){{1,5}}{STREET}\b\.?(?:,?\s+(?:Apt|Suite|Ste|Unit|Flat|Floor|#)\.?\s*[\w-]+)?{city}{{0,3}}{postal}?"
    ))
    .unwrap()
});
static POSTAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:[A-Z]{2}\s+\d{5}(?:-\d{4})?|[1-9]\d{2}\s?\d{3}|[A-Z]{1,2}\d[A-Z\d]?\s*\d[A-Z]{2}|\d{5})\b").unwrap());
static VOCAB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)\b(?:{STREET}|Sector|Block|Colony|Near|Opp|Opposite|District|Dist|Tehsil|PO|Village|Building|Bldg|Floor|Flat|Apt|Suite|Postcode|ZIP|PIN)\b"
    ))
    .unwrap()
});

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    // Multi-line blocks: 2–5 short lines, last one has a postal code, and the
    // block uses address vocabulary.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut i = 0;
    while i < lines.len() {
        let mut matched = false;
        for len in (2..=5).rev() {
            let Some(block) = lines.get(i..i + len) else { continue };
            if block.iter().any(|l| l.is_empty() || l.len() > 80) {
                continue;
            }
            let last = block[len - 1];
            let joined = block.join(", ");
            if POSTAL.is_match(last) && VOCAB.is_match(&joined) && block[0].chars().any(|c| c.is_ascii_digit()) {
                out.push(Detection::new(caps::ADDRESS, Value::Address(joined)).confidence(0.7));
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }
    if out.is_empty() {
        for m in STREET_LINE.find_iter(text) {
            let s = m.as_str().trim_end_matches([',', '.', ' ']);
            let has_postal = POSTAL.is_match(s);
            out.push(Detection::new(caps::ADDRESS, Value::Address(s.to_string())).span(m.start()..m.start() + s.len()).confidence(if has_postal {
                0.8
            } else {
                0.6
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::texts;

    #[test]
    fn single_line() {
        assert_eq!(
            texts(detect, "Ship to 1600 Amphitheatre Parkway, Mountain View, CA 94043 please"),
            vec!["1600 Amphitheatre Parkway, Mountain View, CA 94043"]
        );
        assert_eq!(texts(detect, "Office: 221B Baker Street, London NW1 6XE"), vec!["221B Baker Street, London NW1 6XE"]);
    }

    #[test]
    fn multi_line_block() {
        assert_eq!(
            texts(detect, "Deliver to:\n12 MG Road\nNear City Mall\nIndore, Madhya Pradesh 452001"),
            vec!["12 MG Road, Near City Mall, Indore, Madhya Pradesh 452001"]
        );
    }

    #[test]
    fn not_addresses() {
        assert!(texts(detect, "We walked 3 miles down the road today").is_empty());
        assert!(texts(detect, "Version 2 Way forward").is_empty());
    }
}
