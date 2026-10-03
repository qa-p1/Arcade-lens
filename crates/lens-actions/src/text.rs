//! Text and table actions.

use lens_core::action::{ActionGroup as G, Choice, Item};
use lens_core::builder::action;
use lens_core::host::HostFeatures as F;
use lens_core::registry::PluginRegistrar;
use lens_core::value::Table;
use lens_core::{caps, ActionOutcome, Effects, LensError, Result, Value};

use crate::util::*;

/// Straight quotes, plain spaces, no ligatures or invisible characters.
pub fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => out.push('"'),
            '\u{2013}' | '\u{2014}' | '\u{2212}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '\u{00A0}' | '\u{2007}' | '\u{2009}' | '\u{202F}' => out.push(' '),
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{00AD}' => {}
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            c => out.push(c),
        }
    }
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// Joins lines within paragraphs (keeping blank-line breaks) and repairs
/// words hyphenated across lines.
pub fn remove_line_breaks(s: &str) -> String {
    let mut paragraphs = Vec::new();
    for para in s.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let mut out = String::new();
        for line in para.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if out.ends_with('-') && line.starts_with(|c: char| c.is_lowercase()) && out.chars().rev().nth(1).is_some_and(char::is_alphabetic) {
                out.pop();
            } else if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(line);
        }
        paragraphs.push(out);
    }
    paragraphs.join("\n\n")
}

pub fn join_lines(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn trim_whitespace(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in s.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() && out.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

pub fn title_case(s: &str) -> String {
    let all_caps = !s.chars().any(char::is_lowercase);
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' {
            if start {
                out.extend(c.to_uppercase());
            } else if all_caps {
                out.extend(c.to_lowercase());
            } else {
                out.push(c);
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// Light Markdown: bullets and numbered lists normalized, paragraphs reflowed.
pub fn to_markdown(s: &str) -> String {
    let mut lines = Vec::new();
    for line in plain(s).lines() {
        let t = line.trim_start();
        let indent = (line.len() - t.len()) / 2;
        let pad = "  ".repeat(indent);
        let converted = if let Some(rest) = t.strip_prefix(['•', '◦', '▪', '‣', '●', '○', '■', '–', '*']) {
            format!("{pad}- {}", rest.trim_start())
        } else if let Some((n, rest)) = t.split_once(')').filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())) {
            format!("{pad}{n}. {}", rest.trim_start())
        } else {
            line.trim_end().to_string()
        };
        lines.push(converted);
    }
    lines.join("\n")
}

fn note_name(text: &str, ext: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("Note");
    let s = slug(first, 48);
    format!("{}.{ext}", if s.is_empty() { "Note".to_string() } else { s })
}

pub fn register(r: &mut PluginRegistrar) {
    let t = caps::TEXT;
    r.action(
        action("core.text.copy", "Copy Text")
            .icon("type")
            .group(G::Copy)
            .accepts(t.clone())
            .passthrough()
            .priority(90)
            .key('t')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "text")),
    );
    r.action(
        action("core.text.copy-plain", "Copy Without Formatting")
            .icon("type")
            .group(G::Copy)
            .accepts(t.clone())
            .priority(50)
            .effects(COPY)
            .run(|i, cx| copy(cx, &plain(&text_of(i)?), "plain text")),
    );
    r.action(
        action("core.text.copy-markdown", "Copy as Markdown")
            .icon("markdown")
            .group(G::Copy)
            .accepts(t.clone())
            .priority(40)
            .effects(COPY)
            .run(|i, cx| copy(cx, &to_markdown(&text_of(i)?), "Markdown")),
    );
    r.action(
        action("core.text.copy-line", "Copy Line…")
            .icon("text-cursor")
            .group(G::Copy)
            .accepts(t.clone())
            .priority(38)
            .effects(COPY)
            .applies(|f, _| f.value.as_text().is_some_and(|t| t.lines().filter(|l| !l.trim().is_empty()).count() > 1))
            .choices(|i, _| {
                let text = i.value.as_text().unwrap_or_default();
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(50)
                    .map(|l| Choice { label: l.trim().to_string(), value: serde_json::json!(l.trim()) })
                    .collect()
            })
            .run(|_, cx| copy(cx, cx.param_str("choice").unwrap_or_default(), "line")),
    );
    let transform = |id: &str, label: &str, priority: i32, f: fn(&str) -> String| {
        action(id, label)
            .icon("wand")
            .group(G::Transform)
            .accepts(caps::TEXT)
            .produces(caps::TEXT)
            .priority(priority)
            .run(move |i, _| Ok(text_output(f(&text_of(i)?))))
    };
    r.action(transform("core.text.upper", "UPPERCASE", 30, |s| s.to_uppercase()));
    r.action(transform("core.text.lower", "lowercase", 30, |s| s.to_lowercase()));
    r.action(transform("core.text.title", "Title Case", 28, title_case));
    r.action(transform("core.text.remove-line-breaks", "Remove Line Breaks", 45, remove_line_breaks));
    r.action(transform("core.text.trim", "Trim Whitespace", 32, trim_whitespace));
    r.action(transform("core.text.join", "Join Lines", 34, join_lines));
    r.action(
        action("core.text.search", "Search Web")
            .icon("search")
            .group(G::Search)
            .accepts(t.clone())
            .priority(50)
            .key('w')
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(|i, _| i.value.as_text().map(|s| join_lines(&s).chars().take(300).collect()))
            .run(|i, cx| search(cx, &join_lines(&text_of(i)?).chars().take(300).collect::<String>())),
    );
    r.action(
        action("core.text.save-note", "Save as Note")
            .icon("notebook")
            .group(G::Save)
            .accepts(t.clone())
            .produces(caps::FILE)
            .priority(30)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| {
                let text = text_of(i)?;
                save_text(cx, &note_name(&text, "md"), &to_markdown(&text), "text/markdown")
            }),
    );
    r.action(
        action("core.text.save-file", "Create Text File")
            .icon("file-text")
            .group(G::Save)
            .accepts(t.clone())
            .produces(caps::FILE)
            .priority(28)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| {
                let text = text_of(i)?;
                save_text(cx, &note_name(&text, "txt"), &text, "text/plain")
            }),
    );
    r.action(
        action("core.text.send", "Send to Device")
            .icon("device")
            .group(G::Share)
            .accepts(t)
            .priority(25)
            .effects(Effects::SENDS_TO_DEVICE)
            .requires(F::SEND_TO_DEVICE)
            .run(|i, cx| {
                cx.host.send_to_device(Some(&text_of(i)?), None)?;
                Ok(ActionOutcome::done("Sent"))
            }),
    );

    // Tables
    fn table(i: &Item) -> Result<&Table> {
        match &i.value {
            Value::Table(t) => Ok(t),
            _ => Err(LensError::InvalidInput("not a table".into())),
        }
    }
    let tb = caps::TABLE;
    r.action(
        action("core.table.copy", "Copy Table")
            .icon("table")
            .group(G::Copy)
            .accepts(tb.clone())
            .priority(92)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &table(i)?.to_tsv(), "table")),
    );
    r.action(
        action("core.table.csv", "Copy as CSV")
            .icon("table")
            .group(G::Transform)
            .accepts(tb.clone())
            .priority(80)
            .effects(COPY)
            .run(|i, cx| copy(cx, &table(i)?.to_csv(), "CSV")),
    );
    r.action(
        action("core.table.tsv", "Copy as TSV")
            .icon("table")
            .group(G::Transform)
            .accepts(tb.clone())
            .priority(60)
            .effects(COPY)
            .run(|i, cx| copy(cx, &table(i)?.to_tsv(), "TSV")),
    );
    r.action(
        action("core.table.markdown", "Copy as Markdown")
            .icon("markdown")
            .group(G::Copy)
            .accepts(tb.clone())
            .priority(70)
            .effects(COPY)
            .run(|i, cx| copy(cx, &table(i)?.to_markdown(), "Markdown table")),
    );
    r.action(
        action("core.table.save-csv", "Save CSV")
            .icon("download")
            .group(G::Save)
            .accepts(tb.clone())
            .produces(caps::FILE)
            .priority(65)
            .key('s')
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| save_text(cx, &timestamp_name("Table", "csv"), &table(i)?.to_csv(), "text/csv")),
    );
    r.action(
        action("core.table.to-csv", "Convert to CSV")
            .icon("table")
            .group(G::Transform)
            .accepts(tb)
            .produces(caps::TEXT)
            .chain_only()
            .run(|i, _| Ok(text_output(table(i)?.to_csv()))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms() {
        assert_eq!(
            remove_line_breaks("The quick brown\nfox jumps over the exam-\nple.\n\nNew para-\ngraph\nhere."),
            "The quick brown fox jumps over the example.\n\nNew paragraph here."
        );
        assert_eq!(remove_line_breaks("well-\nKnown"), "well- Known");
        assert_eq!(join_lines("a\n  b\n\nc"), "a b c");
        assert_eq!(trim_whitespace("  a   b \n\n\n  c  \n\n"), "a b\n\nc");
        assert_eq!(title_case("the NASA launch report"), "The NASA Launch Report");
        assert_eq!(title_case("THE NASA LAUNCH"), "The Nasa Launch");
        assert_eq!(plain("“Smart” quotes — and ﬁle\u{00A0}names…"), "\"Smart\" quotes - and file names...");
        assert_eq!(to_markdown("Shopping:\n• milk\n  ◦ oat\n1) eggs"), "Shopping:\n- milk\n  - oat\n1. eggs");
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name("\n  Meeting notes: Q3 plan\nmore", "md"), "Meeting-notes-Q3-plan.md");
    }
}
