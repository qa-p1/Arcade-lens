//! Table reconstruction.
//!
//! With OCR geometry, rows are OCR lines and columns are found by merging
//! the horizontal extents of cells across rows, which handles left-, right-
//! and center-aligned columns alike. Without geometry (e.g. QR payloads or
//! engines without word boxes), tab-, pipe- and space-aligned text is parsed.

use std::sync::LazyLock;

use lens_core::value::{Table, TextLayout};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

static MULTISPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static MD_RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[|│+]?\s*:?[-─=]{3,}:?\s*(?:[|│+]\s*:?[-─=]{3,}:?\s*)*[|│+]?\s*$").unwrap());
static NUMERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{Sc}+\-−]?\s?[\d.,]+\s?(?:%|[kKmM])?$").unwrap());

struct Cell {
    x0: i32,
    x1: i32,
    text: String,
}

pub fn from_layout(layout: &TextLayout) -> Option<Table> {
    let lines: Vec<_> = layout.lines.iter().filter(|l| !l.words.is_empty()).collect();
    if lines.len() < 2 {
        return None;
    }
    let mut heights: Vec<u32> = lines.iter().map(|l| l.bbox.height).collect();
    heights.sort_unstable();
    let gap = (heights[heights.len() / 2] as f32 * 0.9).max(6.0) as i32;

    let rows: Vec<Vec<Cell>> = lines
        .iter()
        .map(|l| {
            let mut words: Vec<_> = l.words.iter().collect();
            words.sort_by_key(|w| w.bbox.x);
            let mut cells: Vec<Cell> = Vec::new();
            for w in words {
                let x1 = w.bbox.x + w.bbox.width as i32;
                match cells.last_mut() {
                    Some(c) if w.bbox.x - c.x1 <= gap => {
                        c.text.push(' ');
                        c.text.push_str(&w.text);
                        c.x1 = x1;
                    }
                    _ => cells.push(Cell { x0: w.bbox.x, x1, text: w.text.clone() }),
                }
            }
            cells
        })
        .collect();

    // Column spans from multi-cell rows only, so titles spanning the table don't merge columns.
    let mut spans: Vec<(i32, i32)> = rows.iter().filter(|r| r.len() >= 2).flatten().map(|c| (c.x0, c.x1)).collect();
    spans.sort_unstable();
    let mut columns: Vec<(i32, i32)> = Vec::new();
    for (a, b) in spans {
        match columns.last_mut() {
            Some(c) if a < c.1 => c.1 = c.1.max(b),
            _ => columns.push((a, b)),
        }
    }
    if columns.len() < 2 {
        return None;
    }
    let multi = rows.iter().filter(|r| r.len() >= 2).count();
    if multi < 2 || multi * 10 < rows.len() * 6 {
        return None;
    }
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let mut out = vec![String::new(); columns.len()];
            for c in r {
                let overlap = |col: &(i32, i32)| (c.x1.min(col.1) - c.x0.max(col.0)).max(0);
                let (i, _) = columns.iter().enumerate().max_by_key(|(_, col)| overlap(col)).unwrap();
                if !out[i].is_empty() {
                    out[i].push(' ');
                }
                out[i].push_str(&c.text);
            }
            out
        })
        .collect();
    Some(with_header(table_rows))
}

pub fn from_text(text: &str) -> Option<(Table, f32)> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty() && !MD_RULE.is_match(l)).collect();
    if lines.len() < 2 {
        return None;
    }
    let pipes = lines.iter().filter(|l| l.contains('|') || l.contains('│')).count();
    let (rows, confidence): (Vec<Vec<String>>, f32) = if lines.iter().all(|l| l.contains('\t')) {
        (lines.iter().map(|l| l.split('\t').map(|c| c.trim().to_string()).collect()).collect(), 0.85)
    } else if pipes * 10 >= lines.len() * 8 {
        let rows = lines
            .iter()
            .map(|l| {
                let t = l.trim().trim_start_matches(['|', '│']).trim_end_matches(['|', '│']);
                t.split(['|', '│']).map(|c| c.trim().to_string()).collect()
            })
            .collect();
        (rows, 0.85)
    } else {
        (lines.iter().map(|l| MULTISPACE.split(l.trim()).map(|c| c.to_string()).collect()).collect(), 0.7)
    };
    let mut counts: Vec<usize> = rows.iter().map(Vec::len).collect();
    counts.sort_unstable();
    let mode = *counts.iter().max_by_key(|c| counts.iter().filter(|x| x == c).count())?;
    let consistent = rows.iter().filter(|r| r.len() == mode).count();
    if mode < 2 || consistent < 2 || consistent * 10 < rows.len() * 7 {
        return None;
    }
    let cells: Vec<&String> = rows.iter().flatten().collect();
    let avg_len = cells.iter().map(|c| c.chars().count()).sum::<usize>() as f32 / cells.len() as f32;
    if avg_len > 40.0 {
        return None; // Paragraphs separated by double spaces, not a table.
    }
    Some((with_header(rows), confidence))
}

fn with_header(rows: Vec<Vec<String>>) -> Table {
    let first_numeric = rows.first().is_some_and(|r| r.iter().any(|c| NUMERIC.is_match(c.trim())));
    let later_numeric = rows.iter().skip(1).any(|r| r.iter().any(|c| NUMERIC.is_match(c.trim())));
    let first_caps = rows.first().is_some_and(|r| r.iter().all(|c| c.chars().any(char::is_alphabetic) && !c.chars().any(char::is_lowercase)));
    let has_header = !first_numeric && (later_numeric || first_caps);
    Table { rows, has_header }
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let found = match input.layout.and_then(from_layout) {
        Some(t) => Some((t, 0.85)),
        None => from_text(input.text),
    };
    let Some((table, confidence)) = found else { return vec![] };
    let dims = format!("{} rows × {} columns", table.rows.len(), table.columns());
    vec![Detection::new(caps::TABLE, Value::Table(table)).confidence(confidence).detail("Size", dims)]
}

#[cfg(test)]
mod tests {
    use lens_core::geometry::Rect;
    use lens_core::value::{TextLine, Word};

    use super::*;

    #[test]
    fn space_aligned_text() {
        let (t, _) = from_text("NAME       PRICE      QTY\nSSD        ₹7,999      2\nRAM        ₹4,499      4").unwrap();
        assert!(t.has_header);
        assert_eq!(t.rows[1], vec!["SSD", "₹7,999", "2"]);
    }

    #[test]
    fn markdown_and_tabs() {
        let (t, _) = from_text("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |").unwrap();
        assert_eq!(t.rows, vec![vec!["a", "b"], vec!["1", "2"], vec!["3", "4"]]);
        let (t, _) = from_text("x\ty\n1\t2").unwrap();
        assert_eq!(t.to_csv(), "x,y\n1,2\n");
    }

    #[test]
    fn prose_is_not_a_table() {
        assert!(from_text("Hello there.\nThis is a letter.\nRegards").is_none());
        assert!(from_text("one line only    with gaps").is_none());
    }

    fn word(text: &str, x: i32, y: i32) -> Word {
        Word { text: text.into(), bbox: Rect::new(x, y, text.len() as u32 * 9, 16), confidence: 0.95 }
    }

    fn line(words: Vec<Word>) -> TextLine {
        let y = words[0].bbox.y;
        TextLine { text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "), bbox: Rect::new(0, y, 400, 16), words }
    }

    #[test]
    fn geometric_columns_with_right_aligned_numbers() {
        let layout = TextLayout {
            lines: vec![
                line(vec![word("Inventory", 0, 0), word("report", 90, 0)]),
                line(vec![word("NAME", 0, 30), word("UNIT", 150, 30), word("PRICE", 160 + 36, 30), word("QTY", 330, 30)]),
                line(vec![word("SSD", 0, 60), word("₹7,999", 180, 60), word("2", 348, 60)]),
                line(vec![word("RAM", 0, 90), word("kit", 36, 90), word("₹4,499", 180, 90), word("4", 348, 90)]),
            ],
        };
        let t = from_layout(&layout).unwrap();
        assert_eq!(t.columns(), 3, "{:?}", t.rows);
        assert_eq!(t.rows[1], vec!["NAME", "UNIT PRICE", "QTY"]);
        assert_eq!(t.rows[3], vec!["RAM kit", "₹4,499", "4"]);
        assert_eq!(t.rows[0], vec!["Inventory report", "", ""]);
    }
}
