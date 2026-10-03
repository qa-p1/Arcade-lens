//! Latitude/longitude pairs.
//!
//! A pair of decimals is only treated as coordinates when it has enough
//! precision, falls within valid ranges, and is not part of a longer list of
//! numbers; hemisphere letters, degree signs or nearby words like "location"
//! raise confidence.

use std::sync::LazyLock;

use lens_core::value::GeoValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

static DECIMAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?P<lat>[-+−]?\d{1,2}\.\d{3,})\s*°?\s*(?P<ns>[NS])?\s*[,;/ ]\s*(?P<lon>[-+−]?\d{1,3}\.\d{3,})\s*°?\s*(?P<ew>[EW])?\b").unwrap()
});
static DMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?P<la>\d{1,2})°\s*(?P<lam>\d{1,2})['′]\s*(?:(?P<las>\d{1,2}(?:\.\d+)?)["″]?\s*)?(?P<ns>[NS])[,\s]+(?P<lo>\d{1,3})°\s*(?P<lom>\d{1,2})['′]\s*(?:(?P<los>\d{1,2}(?:\.\d+)?)["″]?\s*)?(?P<ew>[EW])"#).unwrap()
});
static CONTEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(lat|lng|lon|long|latitude|longitude|coordinates?|coords?|location|gps|position)\b").unwrap());

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    for c in DMS.captures_iter(text) {
        let f = |d: &str, m: &str, s: Option<regex::Match>| {
            d.parse::<f64>().unwrap() + m.parse::<f64>().unwrap() / 60.0 + s.map_or(0.0, |s| s.as_str().parse::<f64>().unwrap() / 3600.0)
        };
        let mut lat = f(&c["la"], &c["lam"], c.name("las"));
        let mut lon = f(&c["lo"], &c["lom"], c.name("los"));
        if c["ns"].eq_ignore_ascii_case("s") {
            lat = -lat;
        }
        if c["ew"].eq_ignore_ascii_case("w") {
            lon = -lon;
        }
        if valid(lat, lon) {
            out.push(make(lat, lon, c.get(0).unwrap().range(), 0.95));
        }
    }
    if !out.is_empty() {
        return out;
    }
    for c in DECIMAL.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        let num = |s: &str| s.replace('−', "-").parse::<f64>().ok();
        let (Some(mut lat), Some(mut lon)) = (num(&c["lat"]), num(&c["lon"])) else { continue };
        if c.name("ns").is_some_and(|h| h.as_str().eq_ignore_ascii_case("s")) {
            lat = -lat.abs();
        }
        if c.name("ew").is_some_and(|h| h.as_str().eq_ignore_ascii_case("w")) {
            lon = -lon.abs();
        }
        if !valid(lat, lon) {
            continue;
        }
        // Part of a longer numeric sequence (a CSV row, a vector) → not a location.
        let before = text[..r.start].trim_end();
        let after = text[r.end..].trim_start();
        if before.ends_with([',', ';']) && before.trim_end_matches([',', ';']).trim_end().ends_with(|ch: char| ch.is_ascii_digit())
            || after.starts_with([',', ';']) && after[1..].trim_start().starts_with(|ch: char| ch.is_ascii_digit() || ch == '-')
            || text[..r.start].ends_with(|ch: char| ch.is_ascii_digit() || ch == '.')
        {
            continue;
        }
        let marked = c.name("ns").is_some() || c.name("ew").is_some() || m.as_str().contains('°');
        let context = CONTEXT.is_match(text);
        let confidence = match (marked, context) {
            (true, _) => 0.95,
            (false, true) => 0.85,
            (false, false) => 0.65,
        };
        out.push(make(lat, lon, r, confidence));
    }
    out
}

fn valid(lat: f64, lon: f64) -> bool {
    (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && !(lat == 0.0 && lon == 0.0)
}

fn make(lat: f64, lon: f64, r: std::ops::Range<usize>, confidence: f32) -> Detection {
    let ns = if lat >= 0.0 { 'N' } else { 'S' };
    let ew = if lon >= 0.0 { 'E' } else { 'W' };
    Detection::new(caps::COORDINATES, Value::Coordinates(GeoValue { lat, lon }))
        .span(r)
        .confidence(confidence)
        .detail("Position", format!("{:.4}° {ns}, {:.4}° {ew}", lat.abs(), lon.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn coords(text: &str) -> Vec<(f64, f64)> {
        detect(&TextInput { text, layout: None }, &cx())
            .into_iter()
            .map(|d| match d.value {
                Value::Coordinates(g) => ((g.lat * 1e4).round() / 1e4, (g.lon * 1e4).round() / 1e4),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn decimal_pairs() {
        assert_eq!(coords("Indore: 22.7196, 75.8577"), vec![(22.7196, 75.8577)]);
        assert_eq!(coords("40.6892° N, 74.0445° W"), vec![(40.6892, -74.0445)]);
        assert_eq!(coords("-33.8568 151.2153"), vec![(-33.8568, 151.2153)]);
    }

    #[test]
    fn dms() {
        assert_eq!(coords(r#"48°51'29.6"N 2°17'40.2"E"#), vec![(48.8582, 2.2945)]);
    }

    #[test]
    fn rejects_numbers_that_are_not_places() {
        for t in ["1.5, 2.5", "3.14159, 2.71828, 1.41421", "pi 3.1415 and 95.000", "123.456, 200.123", "0.000, 0.000"] {
            assert!(coords(t).is_empty(), "{t}: {:?}", coords(t));
        }
    }
}
