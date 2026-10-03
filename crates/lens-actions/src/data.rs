//! Dates, timecodes, quantities, identifiers, secrets, QR codes and barcodes.

use std::sync::Arc;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use image::{DynamicImage, Luma, RgbaImage};
use lens_core::action::{ActionGroup as G, Choice, ConfirmRequest, Item};
use lens_core::builder::action;
use lens_core::host::HostFeatures as F;
use lens_core::registry::PluginRegistrar;
use lens_core::value::{DateTimeCandidate, ImageValue};
use lens_core::{caps, ActionContext, ActionOutcome, Effects, LensError, Result, Value};

use crate::util::*;

pub fn trim_float(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn qr_image(data: &str) -> Result<ActionOutcome> {
    let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| LensError::Failed(e.to_string()))?;
    let luma = code.render::<Luma<u8>>().module_dimensions(8, 8).build();
    let rgba: RgbaImage = DynamicImage::ImageLuma8(luma).to_rgba8();
    Ok(ActionOutcome::output(Item::new(caps::IMAGE, Value::Image(ImageValue { image: Arc::new(rgba), origin: None }))))
}

fn candidates(i: &Item) -> Result<&[DateTimeCandidate]> {
    match &i.value {
        Value::DateTime(d) => Ok(&d.candidates),
        _ => Err(LensError::InvalidInput("not a date".into())),
    }
}

fn candidate_choices(i: &Item) -> Vec<Choice> {
    match candidates(i) {
        Ok(c) if c.len() > 1 => c.iter().enumerate().map(|(n, c)| Choice { label: c.description.clone(), value: serde_json::json!(n) }).collect(),
        _ => vec![],
    }
}

/// The interpretation the user picked (or the only one).
fn chosen<'a>(i: &'a Item, cx: &ActionContext) -> Result<&'a DateTimeCandidate> {
    let c = candidates(i)?;
    let idx = cx.params.get("choice").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    c.get(idx).ok_or_else(|| LensError::InvalidInput("no such interpretation".into()))
}

/// Builds an iCalendar file. Times are floating (local), which is what a
/// time read off the screen means; we never invent a timezone.
pub fn ics(c: &DateTimeCandidate, title: &str, todo: bool, today: NaiveDate) -> Result<String> {
    let date = match &c.date {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|e| LensError::InvalidInput(e.to_string()))?,
        None => today,
    };
    let time = c
        .time
        .as_deref()
        .map(|t| NaiveTime::parse_from_str(t, "%H:%M:%S").or_else(|_| NaiveTime::parse_from_str(t, "%H:%M")))
        .transpose()
        .map_err(|e| LensError::InvalidInput(e.to_string()))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let uid = format!("{}-{}@arcade-lens", stamp, slug(&c.description, 24));
    let mut s = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Arcade//Lens//EN\r\nBEGIN:{}\r\nUID:{uid}\r\nDTSTAMP:{stamp}\r\nSUMMARY:{}\r\n",
        if todo { "VTODO" } else { "VEVENT" },
        ical_escape(title)
    );
    match (time, todo) {
        (Some(t), false) => {
            let start = NaiveDateTime::new(date, t);
            let end = start + Duration::hours(1);
            s.push_str(&format!("DTSTART:{}\r\nDTEND:{}\r\n", start.format("%Y%m%dT%H%M%S"), end.format("%Y%m%dT%H%M%S")));
        }
        (None, false) => {
            s.push_str(&format!("DTSTART;VALUE=DATE:{}\r\nDTEND;VALUE=DATE:{}\r\n", date.format("%Y%m%d"), (date + Duration::days(1)).format("%Y%m%d")))
        }
        (Some(t), true) => s.push_str(&format!("DUE:{}\r\n", NaiveDateTime::new(date, t).format("%Y%m%dT%H%M%S"))),
        (None, true) => s.push_str(&format!("DUE;VALUE=DATE:{}\r\n", date.format("%Y%m%d"))),
    }
    if todo {
        s.push_str("BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Reminder\r\nTRIGGER:PT0S\r\nEND:VALARM\r\nEND:VTODO\r\n");
    } else {
        s.push_str("END:VEVENT\r\n");
    }
    s.push_str("END:VCALENDAR\r\n");
    Ok(s)
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A local, offline description of a secret. JWT claims are decoded (not
/// verified) because they are often what the user wants to inspect.
pub fn inspect_secret(kind: &str, raw: &str) -> String {
    let mut lines = vec![format!("Type: {kind}"), format!("Length: {} characters", raw.chars().count())];
    let mut counts = std::collections::HashMap::new();
    for c in raw.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let n = raw.chars().count() as f64;
    let entropy: f64 = counts.values().map(|&k| -(k as f64 / n) * (k as f64 / n).log2()).sum::<f64>() * n;
    lines.push(format!("Estimated entropy: {entropy:.0} bits"));
    if kind == "JSON Web Token" {
        let parts: Vec<&str> = raw.split('.').collect();
        for (name, part) in ["Header", "Claims"].iter().zip(parts.iter().take(2)) {
            if let Some(json) = base64url_decode(part).and_then(|b| String::from_utf8(b).ok()) {
                lines.push(format!("{name}: {json}"));
            }
        }
        lines.push("Signature not verified.".into());
    }
    lines.join("\n")
}

pub fn register(r: &mut PluginRegistrar) {
    // Dates and times
    let dt = caps::DATE_TIME;
    r.action(
        action("core.date.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(dt.clone())
            .priority(70)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "date")),
    );
    r.action(
        action("core.date.copy-iso", "Copy as ISO 8601")
            .icon("calendar")
            .group(G::Transform)
            .accepts(dt.clone())
            .priority(55)
            .effects(COPY)
            .choices(|i, _| candidate_choices(i))
            .run(|i, cx| {
                let c = chosen(i, cx)?;
                let s = match (&c.date, &c.time) {
                    (Some(d), Some(t)) => format!("{d}T{t}"),
                    (Some(d), None) => d.clone(),
                    (None, Some(t)) => t.clone(),
                    (None, None) => return Err(LensError::InvalidInput("empty date".into())),
                };
                copy(cx, &s, "ISO date")
            }),
    );
    for (id, label, todo, priority, key) in
        [("core.date.event", "Create Calendar Event", false, 88, 'e'), ("core.date.reminder", "Create Reminder", true, 60, 'r')]
    {
        r.action(
            action(id, label)
                .icon(if todo { "bell" } else { "calendar-plus" })
                .group(G::Save)
                .accepts(dt.clone())
                .priority(priority)
                .key(key)
                .effects(SAVE | LAUNCH)
                .requires(F::SAVE_FILE | F::OPEN_PATH)
                // Ambiguous dates are never silently resolved: the user picks.
                .choices(|i, _| candidate_choices(i))
                .run(move |i, cx| {
                    let c = chosen(i, cx)?;
                    let title = cx.param_str("title").unwrap_or(if todo { "Reminder" } else { "New event" });
                    let body = ics(c, title, todo, chrono::Local::now().date_naive())?;
                    let out = save_text(cx, &format!("{}.ics", slug(&c.description, 40)), &body, "text/calendar")?;
                    if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
                        cx.host.open_path(&f.path, lens_core::host::OpenPathMode::Default)?;
                    }
                    Ok(out)
                }),
        );
    }

    // Timecodes
    let tc = caps::TIMECODE;
    let secs = |i: &Item| match &i.value {
        Value::Timecode(t) => Ok(t.seconds),
        _ => Err(LensError::InvalidInput("not a timecode".into())),
    };
    r.action(
        action("core.timecode.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(tc.clone())
            .priority(60)
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "timecode")),
    );
    r.action(
        action("core.timecode.seconds", "Copy as Seconds")
            .icon("timer")
            .group(G::Transform)
            .accepts(tc.clone())
            .priority(50)
            .effects(COPY)
            .run(move |i, cx| copy(cx, &trim_float(secs(i)?), "seconds")),
    );
    r.action(action("core.timecode.hms", "Copy as HH:MM:SS").icon("timer").group(G::Transform).accepts(tc).priority(45).effects(COPY).run(move |i, cx| {
        let s = secs(i)?.floor() as u64;
        copy(cx, &format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60), "timecode")
    }));

    // Quantities
    let q = caps::QUANTITY;
    r.action(
        action("core.quantity.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(q.clone())
            .priority(60)
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "value")),
    );
    r.action(
        action("core.quantity.convert", "Convert…")
            .icon("repeat")
            .group(G::Transform)
            .accepts(q)
            .priority(85)
            .key('v')
            .effects(COPY)
            .applies(|f, _| matches!(&f.value, Value::Quantity(v) if !v.conversions.is_empty()))
            .choices(|i, _| match &i.value {
                Value::Quantity(v) => {
                    v.conversions.iter().map(|(n, u)| Choice { label: format!("{n} {u}"), value: serde_json::json!(format!("{n} {u}")) }).collect()
                }
                _ => vec![],
            })
            .run(|_, cx| copy(cx, cx.param_str("choice").unwrap_or_default(), "conversion")),
    );

    // Hashes and UUIDs
    for (cap, what) in [(caps::HASH, "hash"), (caps::UUID, "UUID")] {
        let base = cap.as_str().to_string();
        r.action(
            action(format!("core.{base}.copy"), "Copy")
                .icon("copy")
                .group(G::Copy)
                .accepts(cap.clone())
                .priority(85)
                .key('c')
                .effects(COPY)
                .run(move |i, cx| copy(cx, &text_of(i)?, what)),
        );
        r.action(
            action(format!("core.{base}.search"), "Search")
                .icon("search")
                .group(G::Search)
                .accepts(cap.clone())
                .priority(55)
                .effects(SEARCH)
                .requires(F::OPEN_URI)
                .preview(|i, _| i.value.as_text().map(|s| s.into_owned()))
                .run(|i, cx| search(cx, &format!("\"{}\"", text_of(i)?))),
        );
        r.action(
            action(format!("core.{base}.compare"), "Compare with Clipboard")
                .icon("git-compare")
                .group(G::Inspect)
                .accepts(cap)
                .priority(50)
                .requires(F::CLIPBOARD_READ)
                .run(move |i, cx| {
                    let a = text_of(i)?.to_ascii_lowercase();
                    let b = cx.host.clipboard_text()?.trim().to_ascii_lowercase();
                    Ok(ActionOutcome::done(if a == b {
                        format!("✓ Matches the {what} on the clipboard")
                    } else {
                        format!("✗ Different from the clipboard ({} vs {} characters)", a.len(), b.len())
                    }))
                }),
        );
    }

    // Secrets: local-only actions. Outbound actions are suppressed by the palette's secret guard.
    let s = caps::SECRET;
    let secret = |i: &Item| match &i.value {
        Value::Secret(s) => Ok(s.clone()),
        _ => Err(LensError::InvalidInput("not a secret".into())),
    };
    r.action(
        action("core.secret.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(s.clone())
            .priority(70)
            .effects(COPY)
            .confirm(|_, _| {
                Some(ConfirmRequest {
                    title: "Copy secret".into(),
                    subject: "The value will be placed on the clipboard.".into(),
                    reasons: vec!["Clipboard managers and sync tools may store or upload it.".into()],
                })
            })
            .run(move |i, cx| copy(cx, secret(i)?.raw.expose(), "secret")),
    );
    r.action(
        action("core.secret.copy-redacted", "Copy Redacted")
            .icon("eye-off")
            .group(G::Copy)
            .accepts(s.clone())
            .priority(80)
            .key('c')
            .effects(COPY)
            .run(move |i, cx| copy(cx, &secret(i)?.masked, "redacted value")),
    );
    r.action(action("core.secret.inspect", "Inspect Locally").icon("shield").group(G::Inspect).accepts(s).priority(60).run(move |i, _| {
        let v = secret(i)?;
        Ok(ActionOutcome::done(inspect_secret(&v.kind, v.raw.expose())))
    }));

    // QR codes
    let qr = caps::QR_CODE;
    r.action(
        action("core.qr.show", "Decode QR").icon("qr").group(G::Inspect).accepts(qr.clone()).priority(40).run(|i, _| Ok(ActionOutcome::done(text_of(i)?))),
    );
    r.action(
        action("core.qr.copy", "Copy Contents")
            .icon("copy")
            .group(G::Copy)
            .accepts(qr.clone())
            .priority(80)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "QR contents")),
    );
    r.action(
        action("core.qr.send", "Send to Phone")
            .icon("smartphone")
            .group(G::Share)
            .accepts(qr.clone())
            .priority(50)
            .effects(Effects::SENDS_TO_DEVICE)
            .requires(F::SEND_TO_DEVICE)
            .run(|i, cx| {
                cx.host.send_to_device(Some(&text_of(i)?), None)?;
                Ok(ActionOutcome::done("Sent to phone"))
            }),
    );
    r.action(
        action("core.qr.copy-image", "Copy QR Image")
            .icon("image")
            .group(G::Copy)
            .accepts(qr.clone())
            .priority(55)
            .effects(COPY)
            .requires(F::CLIPBOARD_IMAGE)
            .run(|_, cx| {
                let sel = cx.selection.ok_or_else(|| LensError::InvalidInput("no selection".into()))?;
                copy_image(cx, &sel.image)
            }),
    );
    r.action(action("core.qr.save", "Save QR").icon("download").group(G::Save).accepts(qr).priority(45).effects(SAVE).requires(F::SAVE_FILE).run(|_, cx| {
        let sel = cx.selection.ok_or_else(|| LensError::InvalidInput("no selection".into()))?;
        save_image(cx, &sel.image, "QR", Some(lens_core::settings::ImageFormat::Png))
    }));

    // Barcodes
    let bc = caps::BARCODE;
    r.action(
        action("core.barcode.copy", "Copy Number")
            .icon("copy")
            .group(G::Copy)
            .accepts(bc.clone())
            .priority(85)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "barcode")),
    );
    r.action(
        action("core.barcode.product", "Search Product")
            .icon("shopping-bag")
            .group(G::Search)
            .accepts(bc.clone())
            .priority(80)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .applies(|f, _| matches!(&f.value, Value::Barcode(b) if b.format.starts_with("EAN") || b.format.starts_with("UPC")))
            .run(|i, cx| open(cx, &lens_core::settings::fill_template(&cx.settings.providers.product_search, &[("query", &text_of(i)?)]))),
    );
    r.action(
        action("core.barcode.search", "Search Web")
            .icon("search")
            .group(G::Search)
            .accepts(bc)
            .priority(50)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .run(|i, cx| search(cx, &text_of(i)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(date: Option<&str>, time: Option<&str>) -> DateTimeCandidate {
        DateTimeCandidate { date: date.map(String::from), time: time.map(String::from), description: "x".into() }
    }

    #[test]
    fn calendar_files() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        let e = ics(&cand(Some("2026-01-09"), Some("15:00")), "Standup, daily", false, today).unwrap();
        assert!(e.contains("DTSTART:20260109T150000\r\n") && e.contains("DTEND:20260109T160000\r\n"));
        assert!(e.contains("SUMMARY:Standup\\, daily\r\n"));
        let e = ics(&cand(Some("2026-12-31"), None), "NYE", false, today).unwrap();
        assert!(e.contains("DTSTART;VALUE=DATE:20261231\r\nDTEND;VALUE=DATE:20270101\r\n"));
        let t = ics(&cand(None, Some("09:30")), "Call", true, today).unwrap();
        assert!(t.contains("BEGIN:VTODO") && t.contains("DUE:20261003T093000"));
    }

    #[test]
    fn jwt_inspection_is_local() {
        let report = inspect_secret("JSON Web Token", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.abcdefghijk");
        assert!(report.contains(r#"Claims: {"sub":"12345"}"#), "{report}");
        assert!(report.contains(r#"Header: {"alg":"HS256"}"#));
    }

    #[test]
    fn floats() {
        assert_eq!(trim_float(149.99), "149.99");
        assert_eq!(trim_float(6138.0), "6138");
    }
}
