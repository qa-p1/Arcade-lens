//! Currency amounts.

use std::sync::LazyLock;

use lens_core::value::CurrencyValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

/// (symbol, ISO code assumed for it, whether that assumption is ambiguous)
const SYMBOLS: &[(&str, &str, bool)] = &[
    ("US$", "USD", false), ("CA$", "CAD", false), ("C$", "CAD", false), ("A$", "AUD", false), ("AU$", "AUD", false), ("NZ$", "NZD", false),
    ("HK$", "HKD", false), ("S$", "SGD", false), ("R$", "BRL", false), ("Rs.", "INR", true), ("Rs", "INR", true), ("₹", "INR", false),
    ("€", "EUR", false), ("£", "GBP", false), ("¥", "JPY", true), ("₩", "KRW", false), ("₽", "RUB", false), ("₺", "TRY", false),
    ("₪", "ILS", false), ("₫", "VND", false), ("₱", "PHP", false), ("฿", "THB", false), ("₦", "NGN", false), ("$", "USD", true),
];
const CODES: &[&str] = &[
    "USD", "EUR", "GBP", "INR", "JPY", "CNY", "RMB", "AUD", "CAD", "CHF", "SEK", "NOK", "DKK", "NZD", "SGD", "HKD", "KRW", "BRL", "MXN", "RUB",
    "ZAR", "TRY", "AED", "SAR", "PLN", "CZK", "HUF", "ILS", "THB", "IDR", "MYR", "PHP", "VND", "NGN", "EGP", "PKR", "BDT", "LKR", "NPR", "BTC", "ETH",
];

const AMOUNT: &str = r"\d{1,3}(?:[,.\x{2009}\x{202F}']\d{2,3})+(?:[.,]\d{1,2})?|\d+(?:[.,]\d{1,2})?";

static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    let syms: Vec<String> = SYMBOLS.iter().map(|(s, ..)| regex::escape(s)).collect();
    Regex::new(&format!(r"(?P<sym>{})\s?(?P<amt>{AMOUNT})(?:\s?(?P<mult>[kKmMbB]n?|million|billion|thousand|lakh|crore)\b)?", syms.join("|"))).unwrap()
});
static CODE: LazyLock<Regex> = LazyLock::new(|| {
    let codes = CODES.join("|");
    Regex::new(&format!(r"\b(?:(?P<pre>{codes})\s?(?P<amt1>{AMOUNT})|(?P<amt2>{AMOUNT})\s?(?P<post>{codes}))\b")).unwrap()
});
static SUFFIX_SYMBOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"(?P<amt>{AMOUNT})\s?(?P<sym>€|£|₹|₽|₺|₫|zł|kr)")).unwrap());

/// Parses an amount written with any common grouping/decimal convention.
/// A lone separator followed by exactly three digits is a thousands
/// separator (`$1,299`, `€1.299`), which is what prices almost always mean.
pub fn parse_amount(s: &str) -> Option<f64> {
    let s: String = s.chars().filter(|c| !matches!(c, ' ' | '\'' | '\u{2009}' | '\u{202F}')).collect();
    let last_sep = s.rfind([',', '.']);
    let normalized = match last_sep {
        None => s.clone(),
        Some(i) => {
            let decimals = s.len() - i - 1;
            let sep = s.as_bytes()[i] as char;
            let other_seps = s[..i].contains(if sep == ',' { '.' } else { ',' });
            let repeated = s[..i].contains(sep);
            let is_decimal = decimals != 3 && !repeated || other_seps;
            if is_decimal {
                format!("{}.{}", s[..i].replace([',', '.'], ""), &s[i + 1..])
            } else {
                s.replace([',', '.'], "")
            }
        }
    };
    normalized.parse().ok()
}

fn multiplier(m: Option<&str>) -> f64 {
    match m.map(|m| m.to_ascii_lowercase()) {
        Some(m) if m == "k" || m == "thousand" => 1e3,
        Some(m) if m == "m" || m == "million" => 1e6,
        Some(m) if m == "b" || m == "bn" || m == "billion" => 1e9,
        Some(m) if m == "lakh" => 1e5,
        Some(m) if m == "crore" => 1e7,
        _ => 1.0,
    }
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out: Vec<Detection> = Vec::new();
    let mut add = |r: std::ops::Range<usize>, amount: f64, code: Option<&str>, symbol: Option<&str>, assumed: bool| {
        if out.iter().any(|d| d.span.as_ref().is_some_and(|s| s.start < r.end && r.start < s.end)) {
            return;
        }
        let raw = text[r.clone()].trim().to_string();
        let mut d = Detection::new(
            caps::CURRENCY,
            Value::Currency(CurrencyValue { raw, amount, code: code.map(String::from), symbol: symbol.map(String::from) }),
        )
        .span(r)
        .confidence(if assumed { 0.8 } else { 0.92 });
        if let Some(c) = code {
            d = d.detail("Currency", if assumed { format!("{c} (assumed from {})", symbol.unwrap_or("symbol")) } else { c.to_string() });
        }
        out.push(d);
    };
    for c in CODE.captures_iter(text) {
        let (code, amt) = match (c.name("pre"), c.name("post")) {
            (Some(p), _) => (p.as_str(), &c["amt1"]),
            (_, Some(p)) => (p.as_str(), &c["amt2"]),
            _ => continue,
        };
        if let Some(a) = parse_amount(amt) {
            let code = if code == "RMB" { "CNY" } else { code };
            add(c.get(0).unwrap().range(), a, Some(code), None, false);
        }
    }
    for c in PREFIX.captures_iter(text) {
        let m = c.get(0).unwrap();
        let sym = &c["sym"];
        // `Rs` needs a word boundary so "Hrs 10" doesn't count.
        if sym.starts_with("Rs") && text[..m.start()].ends_with(|ch: char| ch.is_alphanumeric()) {
            continue;
        }
        let Some((_, code, ambiguous)) = SYMBOLS.iter().find(|(s, ..)| *s == sym) else { continue };
        if let Some(a) = parse_amount(&c["amt"]) {
            add(m.range(), a * multiplier(c.name("mult").map(|x| x.as_str())), Some(code), Some(sym), *ambiguous);
        }
    }
    for c in SUFFIX_SYMBOL.captures_iter(text) {
        let sym = &c["sym"];
        let code = match sym {
            "zł" => "PLN",
            "kr" => "SEK",
            s => SYMBOLS.iter().find(|(x, ..)| *x == s).map(|(_, c, _)| *c).unwrap_or("EUR"),
        };
        if let Some(a) = parse_amount(&c["amt"]) {
            add(c.get(0).unwrap().range(), a, Some(code), Some(sym), sym == "kr");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn money(text: &str) -> Vec<(f64, Option<String>)> {
        detect(&TextInput { text, layout: None }, &cx())
            .into_iter()
            .map(|d| match d.value {
                Value::Currency(c) => (c.amount, c.code),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("1,299"), Some(1299.0));
        assert_eq!(parse_amount("1.299"), Some(1299.0));
        assert_eq!(parse_amount("1.299,50"), Some(1299.5));
        assert_eq!(parse_amount("1,299.50"), Some(1299.5));
        assert_eq!(parse_amount("12,50"), Some(12.5));
        assert_eq!(parse_amount("1,00,000"), Some(100000.0));
        assert_eq!(parse_amount("149.99"), Some(149.99));
    }

    #[test]
    fn currencies() {
        assert_eq!(money("only $149.99!"), vec![(149.99, Some("USD".into()))]);
        assert_eq!(money("SSD ₹7,999"), vec![(7999.0, Some("INR".into()))]);
        assert_eq!(money("Preis: 12,50 €"), vec![(12.5, Some("EUR".into()))]);
        assert_eq!(money("costs 250 CHF"), vec![(250.0, Some("CHF".into()))]);
        assert_eq!(money("raised $2.5M"), vec![(2_500_000.0, Some("USD".into()))]);
        assert_eq!(money("US$ 20"), vec![(20.0, Some("USD".into()))]);
        assert!(money("version 1.2, 15 items").is_empty());
    }
}
