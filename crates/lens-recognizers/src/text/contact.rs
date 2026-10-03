//! Email addresses and phone numbers.

use std::ops::Range;
use std::sync::LazyLock;

use lens_core::value::PhoneValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{overlaps, url, TextInput};

static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b[A-Z0-9][A-Z0-9._%+-]*@(?:[A-Z0-9](?:[A-Z0-9-]*[A-Z0-9])?\.)+[A-Z]{2,24}\b").unwrap());

static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\+\d{1,3}[\s.-]?)?(?:\(\d{1,5}\)[\s.-]?)?\d{2,5}(?:[\s.-]\d{2,5}){1,4}|\+?\d{10,13}").unwrap());

static PHONE_CONTEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:phone|tel|telephone|call|mobile|mob|cell|ph|whatsapp|fax|contact)\b[.:]?\s*$").unwrap());

pub fn emails(text: &str) -> Vec<Range<usize>> {
    let urls: Vec<_> = url::find(text).into_iter().map(|(r, _)| r).collect();
    EMAIL
        .find_iter(text)
        .map(|m| m.range())
        .filter(|r| !urls.iter().any(|u| overlaps(u, r)))
        // `git@github.com:owner/repo` is an SSH remote, not a mailbox.
        .filter(|r| !text[r.end..].starts_with(':'))
        .collect()
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    let email_ranges = emails(text);
    for r in &email_ranges {
        let email = &text[r.clone()];
        let domain = email.rsplit('@').next().unwrap_or_default().to_ascii_lowercase();
        out.push(Detection::new(caps::EMAIL, Value::Email(email.to_string())).span(r.clone()).confidence(0.95).detail("Domain", domain));
    }
    for m in PHONE.find_iter(text) {
        let r = m.range();
        if email_ranges.iter().any(|e| overlaps(e, &r)) {
            continue;
        }
        if let Some((value, confidence)) = phone(text, r.clone()) {
            out.push(Detection::new(caps::PHONE, Value::Phone(value)).span(r).confidence(confidence));
        }
    }
    out
}

fn phone(text: &str, r: Range<usize>) -> Option<(PhoneValue, f32)> {
    let raw = text[r.clone()].trim();
    // Must stand alone: not glued to letters, digits, decimals, or currency.
    let before = text[..r.start].chars().next_back();
    let mut after = text[r.end..].chars();
    let (a1, a2) = (after.next(), after.next());
    if before.is_some_and(|c| c.is_alphanumeric() || "$€£¥₹/.,:-_#".contains(c))
        || a1.is_some_and(|c| c.is_alphanumeric() || "/:-_%".contains(c))
        || (a1.is_some_and(|c| c == '.' || c == ',') && a2.is_some_and(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let international = raw.starts_with('+');
    if !(7..=15).contains(&digits.len()) {
        return None;
    }
    let separators: Vec<char> = raw.chars().filter(|c| " .-".contains(*c)).collect();
    let groups: Vec<&str> = raw.split([' ', '.', '-', '(', ')']).filter(|g| !g.is_empty()).collect();
    // Dotted quads are IPs; d.m.y-shaped values are dates; plain decimals are numbers.
    if separators.iter().all(|c| *c == '.') && !separators.is_empty() && groups.len() != 3 {
        return None;
    }
    if groups.len() == 3 && groups.iter().map(|g| g.len()).collect::<Vec<_>>() == [2, 2, 4] && !international {
        return None;
    }
    if groups.len() == 3 && groups[0].len() == 4 && groups[1].len() == 2 && groups[2].len() == 2 {
        return None; // 2024-03-05
    }
    let has_structure = international || raw.contains('(') || !separators.is_empty();
    if !has_structure && digits.len() != 10 {
        return None;
    }
    let context = PHONE_CONTEXT.is_match(&text[..r.start]);
    let mut confidence: f32 = if international {
        0.9
    } else if raw.contains('(') {
        0.85
    } else if !separators.is_empty() && digits.len() >= 10 {
        0.75
    } else if !separators.is_empty() {
        0.55
    } else {
        0.45
    };
    if context {
        confidence += 0.15;
    }
    if confidence < 0.5 {
        return None;
    }
    let normalized = if international { format!("+{digits}") } else { digits };
    Some((PhoneValue { raw: raw.to_string(), digits: normalized }, confidence.min(0.98)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn caps_of(text: &str) -> Vec<(String, String)> {
        detect(&TextInput { text, layout: None }, &cx()).into_iter().map(|d| (d.capability.to_string(), d.value.as_text().unwrap().into_owned())).collect()
    }

    #[test]
    fn emails() {
        assert_eq!(caps_of("Mail ada.lovelace+lens@math.example.co.uk."), vec![("email".into(), "ada.lovelace+lens@math.example.co.uk".into())]);
        assert!(caps_of("git@github.com:qa-p1/Arcade-lens.git").is_empty());
        assert!(caps_of("https://user@example.com/x").is_empty());
    }

    #[test]
    fn phones() {
        let found = caps_of("Call +91 98765 43210 or (555) 123-4567. Tel: 020 7946 0958");
        let phones: Vec<_> = found.iter().filter(|(c, _)| c == "phone").map(|(_, v)| v.as_str()).collect();
        assert_eq!(phones, vec!["+91 98765 43210", "(555) 123-4567", "020 7946 0958"]);
    }

    #[test]
    fn not_phones() {
        for t in ["192.168.1.34", "2024-03-05", "05.03.2024", "$1234567", "version 1.2.3", "Total: 12345", "ID 1234567", "3.14159265"] {
            assert!(caps_of(t).iter().all(|(c, _)| c != "phone"), "{t}");
        }
    }

    #[test]
    fn tel_uri() {
        let d = detect(&TextInput { text: "+1 415-555-0100", layout: None }, &cx());
        let Value::Phone(p) = &d[0].value else { panic!() };
        assert_eq!(p.tel_uri(), "tel:+14155550100");
    }
}
