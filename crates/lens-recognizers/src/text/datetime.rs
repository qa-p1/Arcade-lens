//! Dates, times and media timecodes.
//!
//! Ambiguity is preserved rather than guessed away: `05/03/2024` yields both
//! 5 March and May 3 (ordered by the user's date-order setting), `2:30`
//! without AM/PM yields both 02:30 and 14:30, and a date without a year that
//! has already passed this year yields this year and next. Actions that need
//! a single interpretation must ask.

use std::ops::Range;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use lens_core::settings::DateOrder;
use lens_core::value::{DateTimeCandidate, DateTimeValue, TimecodeValue};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{overlaps, TextInput};

const MONTHS: &str = r"(?:jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)";
const WEEKDAY: &str = r"(?:(?:mon|tue|tues|wed|thu|thur|thurs|fri|sat|sun)(?:day|nesday|sday|urday)?\.?,?\s+)?";
const TIME: &str = r"(?:(?:,|\s+at|\s*@)?\s*(?P<time>\d{1,2}(?::\d{2}){1,2}(?:\s*[ap]\.?m\.?)?|\d{1,2}\s*[ap]\.?m\.?))?";

static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?P<y>\d{4})-(?P<m>\d{2})-(?P<d>\d{2})(?:[T ](?P<time>\d{2}:\d{2}(?::\d{2})?)(?:\.\d+)?(?P<tz>Z|[+-]\d{2}:?\d{2})?)?\b").unwrap()
});
static NUMERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\b(?P<a>\d{{1,4}})(?P<sep>[/.\-])(?P<b>\d{{1,2}})(?P<sep2>[/.\-])(?P<c>\d{{2,4}})\b{TIME}")).unwrap()
});
static TEXTUAL_DMY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\b{WEEKDAY}(?P<d>\d{{1,2}})(?:st|nd|rd|th)?\s+(?:of\s+)?(?P<mon>{MONTHS})\.?,?(?:\s+(?P<y>\d{{4}}))?\b{TIME}")).unwrap()
});
static TEXTUAL_MDY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\b{WEEKDAY}(?P<mon>{MONTHS})\.?\s+(?P<d>\d{{1,2}})(?:st|nd|rd|th)?\b(?:,?\s+(?P<y>\d{{4}}))?\b{TIME}")).unwrap()
});
static TIME_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?P<time>\d{1,2}:\d{2}(?::\d{2})?(?:\s*[ap]\.?m\.?)?|\d{1,2}\s*[ap]\.?m\.?)(?:\b|$)").unwrap());
static TIMECODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:(?P<h>\d{1,3}):)?(?P<m>[0-5]?\d):(?P<s>[0-5]\d)(?:[.,](?P<f>\d{1,3}))?\b").unwrap());

pub fn detect(input: &TextInput, cx: &RecognizeContext) -> Vec<Detection> {
    detect_at(input, cx.settings.date_order, chrono::Local::now().date_naive())
}

pub fn detect_at(input: &TextInput, order: DateOrder, today: NaiveDate) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    let mut taken: Vec<Range<usize>> = Vec::new();
    let push = |out: &mut Vec<Detection>, taken: &mut Vec<Range<usize>>, r: Range<usize>, candidates: Vec<DateTimeCandidate>, confidence: f32| {
        if candidates.is_empty() || taken.iter().any(|t| overlaps(t, &r)) {
            return;
        }
        taken.push(r.clone());
        let mut d = Detection::new(caps::DATE_TIME, Value::DateTime(DateTimeValue { raw: text[r.clone()].trim().to_string(), candidates: candidates.clone() }))
            .span(r)
            .confidence(confidence);
        d = if candidates.len() == 1 {
            d.detail("Means", candidates[0].description.clone())
        } else {
            d.detail("Ambiguous", candidates.iter().map(|c| c.description.as_str()).collect::<Vec<_>>().join("  or  "))
        };
        out.push(d);
    };

    for c in ISO.captures_iter(text) {
        let m = c.get(0).unwrap();
        let Some(date) = ymd(&c["y"], &c["m"], &c["d"]) else { continue };
        let times = c.name("time").map(|t| vec![t.as_str().to_string()]).unwrap_or_default();
        let mut cands = combine(&[date], &times);
        if let Some(tz) = c.name("tz") {
            for cand in &mut cands {
                cand.description.push_str(&format!(" ({})", if tz.as_str() == "Z" { "UTC" } else { tz.as_str() }));
            }
        }
        push(&mut out, &mut taken, m.range(), cands, 0.97);
    }

    for c in NUMERIC.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        // Versions (1.2.3) and IP fragments are not dates.
        let before = text[..r.start].chars().next_back();
        let after_dot_digit = text[r.end..].starts_with('.') && text[r.end..].chars().nth(1).is_some_and(|ch| ch.is_ascii_digit());
        if before.is_some_and(|ch| ch == '.' || ch.is_ascii_digit()) || after_dot_digit || c["sep"] != c["sep2"] {
            continue;
        }
        let (a, b, cc) = (&c["a"], &c["b"], &c["c"]);
        let mut dates = Vec::new();
        if a.len() == 4 {
            dates.extend(ymd(a, b, cc));
        } else {
            let year = expand_year(cc);
            let day_first = dmy(a, b, year);
            let month_first = dmy(b, a, year);
            let (first, second) = match order {
                DateOrder::DayFirst => (day_first, month_first),
                DateOrder::MonthFirst => (month_first, day_first),
            };
            dates.extend(first);
            if second != first {
                dates.extend(second);
            }
        }
        if dates.is_empty() {
            continue;
        }
        let times = c.name("time").map(|t| time_candidates(t.as_str())).unwrap_or_default();
        let dotted_short = &c["sep"] == "." && cc.len() == 2;
        push(&mut out, &mut taken, r, combine(&dates, &times), if dotted_short { 0.5 } else { 0.85 });
    }

    for re in [&*TEXTUAL_DMY, &*TEXTUAL_MDY] {
        for c in re.captures_iter(text) {
            let m = c.get(0).unwrap();
            let Some(month) = month_number(&c["mon"]) else { continue };
            let Ok(day) = c["d"].parse::<u32>() else { continue };
            let dates: Vec<NaiveDate> = match c.name("y") {
                Some(y) => y.as_str().parse().ok().and_then(|y| NaiveDate::from_ymd_opt(y, month, day)).into_iter().collect(),
                None => {
                    // No year: the next occurrence, unless it might mean the one just past.
                    let this = NaiveDate::from_ymd_opt(today.year(), month, day);
                    let next = NaiveDate::from_ymd_opt(today.year() + 1, month, day);
                    match (this, next) {
                        (Some(t), _) if t >= today => vec![t],
                        (Some(t), Some(n)) => vec![n, t],
                        (None, Some(n)) => vec![n],
                        _ => vec![],
                    }
                }
            };
            let times = c.name("time").map(|t| time_candidates(t.as_str())).unwrap_or_default();
            // "may 5" in prose is weak evidence; "May 5, 2025" is strong.
            let confidence = if c.name("y").is_some() { 0.92 } else { 0.7 };
            push(&mut out, &mut taken, m.range(), combine(&dates, &times), confidence);
        }
    }

    for c in TIME_ONLY.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        if text[..r.start].ends_with(|ch: char| ch == ':' || ch == '.' || ch.is_ascii_alphanumeric()) || text[r.end..].starts_with(':') {
            continue;
        }
        let times = time_candidates(&c["time"]);
        if times.is_empty() {
            continue;
        }
        let cands = times.iter().map(|t| DateTimeCandidate { date: None, time: Some(t.clone()), description: describe_time(t) }).collect();
        let explicit = c["time"].to_ascii_lowercase().contains('m') || c["time"].split(':').next().is_some_and(|h| h.parse::<u32>().is_ok_and(|h| h >= 13 || h == 0));
        push(&mut out, &mut taken, r, cands, if explicit { 0.8 } else { 0.55 });
    }

    // Timecodes overlap clock times on purpose (`01:42:18` is both).
    for c in TIMECODE.captures_iter(text) {
        let m = c.get(0).unwrap();
        let r = m.range();
        if text[..r.start].ends_with([':', '.']) || text[r.end..].starts_with(':') {
            continue;
        }
        let Some(h) = c.name("h") else {
            if c.name("f").is_none() {
                continue; // `12:30` alone is a clock time, not a timecode.
            }
            let secs = c["m"].parse::<f64>().unwrap() * 60.0 + c["s"].parse::<f64>().unwrap();
            out.push(timecode(m.as_str(), secs, r, 0.6));
            continue;
        };
        if out.iter().any(|d| d.capability == caps::DATE_TIME && d.span.as_ref().is_some_and(|s| overlaps(s, &r) && s.len() > r.len() + 1)) {
            continue; // Part of a full timestamp like 2024-03-05 14:30:12.
        }
        let hours: f64 = h.as_str().parse().unwrap();
        let frac = c.name("f").map_or(0.0, |f| format!("0.{}", f.as_str()).parse::<f64>().unwrap());
        let secs = hours * 3600.0 + c["m"].parse::<f64>().unwrap() * 60.0 + c["s"].parse::<f64>().unwrap() + frac;
        out.push(timecode(m.as_str(), secs, r, if hours >= 24.0 { 0.9 } else { 0.6 }));
    }
    out
}

fn timecode(raw: &str, seconds: f64, r: Range<usize>, confidence: f32) -> Detection {
    let total = seconds.floor() as u64;
    let human = match (total / 3600, (total % 3600) / 60, total % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s}s"),
        (h, m, s) => format!("{h}h {m}m {s}s"),
    };
    Detection::new(caps::TIMECODE, Value::Timecode(TimecodeValue { raw: raw.to_string(), seconds }))
        .span(r)
        .confidence(confidence)
        .detail("Duration", format!("{human} ({} seconds)", seconds))
}

fn ymd(y: &str, m: &str, d: &str) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

fn dmy(d: &str, m: &str, y: i32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, m.parse().ok()?, d.parse().ok()?)
}

fn expand_year(y: &str) -> i32 {
    let v: i32 = y.parse().unwrap_or(0);
    match y.len() {
        2 if v < 70 => 2000 + v,
        2 => 1900 + v,
        _ => v,
    }
}

fn month_number(s: &str) -> Option<u32> {
    let s = s.to_ascii_lowercase();
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| s.starts_with(m)).map(|i| i as u32 + 1)
}

/// 24h `HH:MM[:SS]` interpretations of a time string.
fn time_candidates(raw: &str) -> Vec<String> {
    let lower = raw.to_ascii_lowercase().replace(['.', ' '], "");
    let (clock, meridiem) = if let Some(c) = lower.strip_suffix("am") {
        (c.to_string(), Some(false))
    } else if let Some(c) = lower.strip_suffix("pm") {
        (c.to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<u32> = clock.split(':').filter_map(|p| p.parse().ok()).collect();
    let (h, m, s) = match parts.as_slice() {
        [h] => (*h, 0, None),
        [h, m] => (*h, *m, None),
        [h, m, s] => (*h, *m, Some(*s)),
        _ => return vec![],
    };
    if m > 59 || s.is_some_and(|s| s > 59) {
        return vec![];
    }
    let fmt = |h: u32| match s {
        Some(s) => format!("{h:02}:{m:02}:{s:02}"),
        None => format!("{h:02}:{m:02}"),
    };
    match meridiem {
        Some(pm) if (1..=12).contains(&h) => vec![fmt(if pm { h % 12 + 12 } else { h % 12 })],
        Some(_) => vec![],
        None if h > 23 => vec![],
        None if (1..=12).contains(&h) && parts.len() == 2 && h < 10 && !clock.starts_with('0') => vec![fmt(h), fmt(h + 12)],
        None => vec![fmt(h)],
    }
}

fn describe_time(t: &str) -> String {
    t.to_string()
}

fn combine(dates: &[NaiveDate], times: &[String]) -> Vec<DateTimeCandidate> {
    let mut out = Vec::new();
    for d in dates {
        if times.is_empty() {
            out.push(DateTimeCandidate { date: Some(d.to_string()), time: None, description: d.format("%A, %-d %B %Y").to_string() });
        }
        for t in times {
            out.push(DateTimeCandidate { date: Some(d.to_string()), time: Some(t.clone()), description: format!("{} at {t}", d.format("%A, %-d %B %Y")) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, order: DateOrder) -> Vec<Detection> {
        detect_at(&TextInput { text, layout: None }, order, NaiveDate::from_ymd_opt(2026, 10, 3).unwrap())
    }

    fn dates(text: &str, order: DateOrder) -> Vec<Vec<(Option<String>, Option<String>)>> {
        run(text, order)
            .into_iter()
            .filter(|d| d.capability == caps::DATE_TIME)
            .map(|d| match d.value {
                Value::DateTime(v) => v.candidates.into_iter().map(|c| (c.date, c.time)).collect(),
                _ => unreachable!(),
            })
            .collect()
    }

    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    #[test]
    fn iso_is_unambiguous() {
        assert_eq!(dates("deploy 2024-03-05T14:30:00Z", DateOrder::DayFirst), vec![vec![(s("2024-03-05"), s("14:30:00"))]]);
    }

    #[test]
    fn numeric_dates_keep_both_readings() {
        assert_eq!(dates("due 05/03/2024", DateOrder::DayFirst), vec![vec![(s("2024-03-05"), None), (s("2024-05-03"), None)]]);
        assert_eq!(dates("due 05/03/2024", DateOrder::MonthFirst), vec![vec![(s("2024-05-03"), None), (s("2024-03-05"), None)]]);
        // Only one valid reading.
        assert_eq!(dates("due 25/12/2024", DateOrder::MonthFirst), vec![vec![(s("2024-12-25"), None)]]);
        // Same both ways.
        assert_eq!(dates("on 07/07/2025", DateOrder::DayFirst), vec![vec![(s("2025-07-07"), None)]]);
    }

    #[test]
    fn textual_dates_and_times() {
        assert_eq!(dates("Meet on Friday, 9 January 2026 at 3pm", DateOrder::DayFirst), vec![vec![(s("2026-01-09"), s("15:00"))]]);
        assert_eq!(dates("March 5th, 2025 2:30 PM", DateOrder::DayFirst), vec![vec![(s("2025-03-05"), s("14:30"))]]);
        // No year, still ahead: this year. Already passed: next year first, then this year.
        assert_eq!(dates("on Dec 24", DateOrder::DayFirst), vec![vec![(s("2026-12-24"), None)]]);
        assert_eq!(dates("on 5 March", DateOrder::DayFirst), vec![vec![(s("2027-03-05"), None), (s("2026-03-05"), None)]]);
    }

    #[test]
    fn bare_times() {
        assert_eq!(dates("standup at 9:30", DateOrder::DayFirst), vec![vec![(None, s("09:30")), (None, s("21:30"))]]);
        assert_eq!(dates("standup at 14:05", DateOrder::DayFirst), vec![vec![(None, s("14:05"))]]);
        assert_eq!(dates("at 11 am", DateOrder::DayFirst), vec![vec![(None, s("11:00"))]]);
    }

    #[test]
    fn timecodes() {
        let found = run("jump to 01:42:18 in the video", DateOrder::DayFirst);
        let tc: Vec<_> = found.iter().filter(|d| d.capability == caps::TIMECODE).collect();
        assert_eq!(tc.len(), 1);
        let Value::Timecode(t) = &tc[0].value else { panic!() };
        assert_eq!(t.seconds, 6138.0);
        assert!(run("2024-03-05 14:30:12", DateOrder::DayFirst).iter().all(|d| d.capability != caps::TIMECODE));
    }

    #[test]
    fn not_dates() {
        for t in ["version 1.2.3", "192.168.1.34", "ratio 3/4", "10.0.19045"] {
            assert!(dates(t, DateOrder::DayFirst).is_empty(), "{t}: {:?}", dates(t, DateOrder::DayFirst));
        }
    }
}
