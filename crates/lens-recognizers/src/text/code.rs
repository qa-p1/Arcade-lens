//! Source code detection, language guessing and line-number gutter removal.
//!
//! Entirely lexical: symbol density, indentation, and per-language keyword
//! and idiom scores. No models, deterministic output.

use std::sync::LazyLock;

use lens_core::value::CodeValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

static GUTTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d{1,5})(\s*[│|¦┃:])?([ \t]*)(.*)$").unwrap());
static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[a-z]+[A-Z]\w*\b|\b[a-z]+_[a-z_]+\b").unwrap());

struct GutterLine<'a> {
    number: u32,
    boxed: bool,
    gap: &'a str,
    rest: &'a str,
}

fn gutter(line: &str) -> Option<GutterLine<'_>> {
    let c = GUTTER.captures(line)?;
    let g = GutterLine { number: c[1].parse().ok()?, boxed: c.get(2).is_some(), gap: c.get(3).unwrap().as_str(), rest: c.get(4).unwrap().as_str() };
    // `3rd`, `404Error`: digits glued to text are not a gutter.
    (g.boxed || !g.gap.is_empty() || g.rest.is_empty()).then_some(g)
}

/// Removes a leading line-number gutter (`12 │ code`, `12: code`, `12  code`)
/// when line numbers are present on most lines and increase consecutively.
/// Indentation of the code itself is preserved.
pub fn strip_line_numbers(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let nonempty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    if nonempty < 2 {
        return None;
    }
    let parsed: Vec<Option<GutterLine>> = lines.iter().map(|l| gutter(l)).collect();
    let numbered: Vec<&GutterLine> = parsed.iter().flatten().collect();
    if numbered.len() * 10 < nonempty * 8 {
        return None;
    }
    let consecutive = numbered.windows(2).filter(|w| w[1].number == w[0].number + 1).count();
    if consecutive * 10 < (numbered.len() - 1) * 8 {
        return None;
    }
    // Separator style must be consistent so `1 apple` / `2 pears` lists survive.
    let boxed = numbered.iter().filter(|g| g.boxed).count();
    if boxed != 0 && boxed != numbered.len() {
        return None;
    }
    if boxed == 0 && numbered.iter().all(|g| g.rest.split_whitespace().count() > 3 && !g.rest.contains(['{', '(', '=', ';', ':'])) {
        return None; // A numbered prose list, not a code gutter.
    }
    let common = numbered.iter().filter(|g| !g.rest.is_empty()).map(|g| g.gap.len()).min().unwrap_or(0);
    let content: Vec<String> = parsed
        .iter()
        .zip(&lines)
        .map(|(g, l)| match g {
            Some(g) if g.rest.is_empty() => String::new(),
            Some(g) => format!("{}{}", &g.gap[common.min(g.gap.len())..], g.rest),
            None => l.to_string(),
        })
        .collect();
    Some(content.join("\n"))
}

struct Lang {
    name: &'static str,
    patterns: &'static [&'static str],
}

static LANGS: LazyLock<Vec<(&'static str, Vec<Regex>)>> = LazyLock::new(|| {
    let langs = [
        Lang { name: "Rust", patterns: &[r"\bfn\s+\w+", r"\blet\s+mut\b", r"\bimpl\b", r"\bpub\s+(fn|struct|enum|mod)\b", r"\buse\s+\w+(::\w+)+", r"\w+!\(", r"&str\b", r"\bSome\(|\bOk\(|\bErr\(", r"->\s*\w+", r"::<"] },
        Lang { name: "Python", patterns: &[r"(?m)^\s*def\s+\w+\(.*\):\s*$", r"(?m)^\s*(from\s+\S+\s+)?import\s+\w+", r"\bself\.", r"(?m)^\s*(if|elif|for|while|with|try|except|class)\b.*:\s*$", r"\bNone\b|\bTrue\b|\bFalse\b", r"\bprint\("] },
        Lang { name: "JavaScript", patterns: &[r"\b(const|let|var)\s+\w+\s*=", r"=>", r"\bfunction\s*\w*\(", r"console\.\w+\(", r"\brequire\(", r"\bexport\s+(default|const|function)", r"===|!=="] },
        Lang { name: "TypeScript", patterns: &[r"\binterface\s+\w+", r":\s*(string|number|boolean|any|void)\b", r"\btype\s+\w+\s*=", r"<\w+>\(", r"\bimport\s+.*\bfrom\s+['\x22]"] },
        Lang { name: "Java", patterns: &[r"\bpublic\s+(static\s+)?(class|void|final)\b", r"System\.out\.print", r"\bprivate\s+\w+", r"\bnew\s+\w+\(", r"@Override"] },
        Lang { name: "C#", patterns: &[r"\busing\s+System", r"\bnamespace\s+\w+", r"\bpublic\s+(async\s+)?\w+\s+\w+\(", r"Console\.Write", r"\bvar\s+\w+\s*="] },
        Lang { name: "C/C++", patterns: &[r"#include\s*[<\x22]", r"\bint\s+main\s*\(", r"\bprintf\(", r"std::", r"->\w+", r"\b(nullptr|NULL)\b", r"\bsizeof\("] },
        Lang { name: "Go", patterns: &[r"\bfunc\s+(\(\w+\s+\*?\w+\)\s*)?\w+\(", r"\bpackage\s+\w+", r":=", r"\bfmt\.\w+", r"\berr\s*!=\s*nil"] },
        Lang { name: "Shell", patterns: &[r"(?m)^#!/bin/(ba)?sh", r"\$\{?\w+\}?", r"(?m)^\s*(if|then|fi|done|esac)\b", r"\becho\s", r"\|\s*grep\b"] },
        Lang { name: "HTML", patterns: &[r"<(div|span|html|body|head|script|a|p|ul|li)\b[^>]*>", r"</\w+>", r"<!DOCTYPE"] },
        Lang { name: "CSS", patterns: &[r"(?m)^\s*[.#]?[\w-]+\s*\{", r"(?m)^\s*[\w-]+\s*:\s*[^;]+;\s*$", r"@media\b", r"\b\d+(px|rem|em)\b"] },
        Lang { name: "SQL", patterns: &[r"(?i)\bselect\b.+\bfrom\b", r"(?i)\binsert\s+into\b", r"(?i)\bcreate\s+table\b", r"(?i)\bwhere\b", r"(?i)\bjoin\b.+\bon\b"] },
        Lang { name: "JSON", patterns: &[r#"^\s*[\{\[]"#, r#""\w+"\s*:\s*"#, r#"\}\s*,?\s*$"#] },
        Lang { name: "YAML", patterns: &[r"(?m)^\s*[\w-]+:\s+\S", r"(?m)^\s*-\s+\w", r"(?m)^---\s*$"] },
        Lang { name: "Ruby", patterns: &[r"(?m)^\s*def\s+\w+\s*$", r"(?m)^\s*end\s*$", r"\bputs\s", r"\battr_\w+", r"do\s*\|\w+\|"] },
        Lang { name: "PHP", patterns: &[r"<\?php", r"\$\w+\s*=", r"->\w+\(", r"\becho\s"] },
        Lang { name: "Kotlin", patterns: &[r"\bfun\s+\w+\(", r"\bval\s+\w+", r"\bvar\s+\w+:\s*\w+"] },
        Lang { name: "Swift", patterns: &[r"\bfunc\s+\w+\(", r"\bimport\s+(UIKit|SwiftUI|Foundation)", r"\bguard\s+let\b", r"\bvar\s+\w+:\s*\w+"] },
    ];
    langs.into_iter().map(|l| (l.name, l.patterns.iter().map(|p| Regex::new(p).unwrap()).collect())).collect()
});

pub fn language(text: &str) -> Option<&'static str> {
    let mut best: Option<(&str, f32)> = None;
    for (name, pats) in LANGS.iter() {
        let hits = pats.iter().filter(|p| p.is_match(text)).count();
        let score = hits as f32 / (pats.len() as f32).sqrt();
        if hits >= 2 && best.is_none_or(|(_, b)| score > b) {
            best = Some((name, score));
        }
    }
    best.map(|(n, _)| n)
}

/// 0..1 likelihood that `text` is code rather than prose.
pub fn code_score(text: &str) -> f32 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return 0.0;
    }
    let total: usize = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let symbols = text.chars().filter(|c| "{}()[];=<>:&|!*/+-_.,\"'#$@%\\".contains(*c)).count();
    let symbol_ratio = symbols as f32 / total as f32;
    let code_endings = lines.iter().filter(|l| l.trim_end().ends_with([';', '{', '}', ')', ':', ',', ']'])).count() as f32 / lines.len() as f32;
    let indented = lines.iter().filter(|l| l.starts_with("  ") || l.starts_with('\t')).count() as f32 / lines.len() as f32;
    let sentences = lines.iter().filter(|l| {
        let t = l.trim();
        t.ends_with('.') && t.split_whitespace().count() > 5 && !t.contains(['(', '=', '{'])
    });
    let prose = sentences.count() as f32 / lines.len() as f32;
    let camel_or_snake = IDENTIFIER.find_iter(text).count() as f32 / lines.len() as f32;
    let lang_bonus = if language(text).is_some() { 0.25 } else { 0.0 };
    let s = symbol_ratio.min(0.35) / 0.35 * 0.35 + code_endings * 0.2 + indented * 0.1 + camel_or_snake.min(1.0) * 0.1 + lang_bonus - prose * 0.6;
    s.clamp(0.0, 1.0)
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let stripped = strip_line_numbers(input.text);
    let body = stripped.as_deref().unwrap_or(input.text);
    if body.trim().len() < 8 {
        return vec![];
    }
    let score = code_score(body);
    if score < 0.45 {
        return vec![];
    }
    let lang = language(body);
    let mut d = Detection::new(
        caps::CODE,
        Value::Code(CodeValue { text: input.text.to_string(), without_line_numbers: stripped.clone(), language: lang.map(String::from) }),
    )
    .confidence(score.min(0.95));
    if let Some(l) = lang {
        d = d.detail("Language", l);
    }
    if stripped.is_some() {
        d = d.detail("Line numbers", "Detected");
    }
    vec![d]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn run(text: &str) -> Option<CodeValue> {
        detect(&TextInput { text, layout: None }, &cx()).into_iter().next().map(|d| match d.value {
            Value::Code(c) => c,
            _ => unreachable!(),
        })
    }

    #[test]
    fn gutter_removal() {
        let src = "1 │ const foo = 1;\n2 │ const bar = foo + 1;\n3 │\n4 │ console.log(bar);";
        assert_eq!(strip_line_numbers(src).unwrap(), "const foo = 1;\nconst bar = foo + 1;\n\nconsole.log(bar);");
        let src = "10  fn main() {\n11      println!(\"hi\");\n12  }";
        assert_eq!(strip_line_numbers(src).unwrap(), "fn main() {\n    println!(\"hi\");\n}");
        assert!(strip_line_numbers("1 apple and some more words here\n2 pears and some more words here").is_none());
        assert!(strip_line_numbers("1 │ a\n7 │ b\n3 │ c").is_none());
    }

    #[test]
    fn languages() {
        let c = run("fn main() {\n    let mut v: Vec<&str> = Vec::new();\n    println!(\"{:?}\", v);\n}").unwrap();
        assert_eq!(c.language.as_deref(), Some("Rust"));
        let c = run("def greet(name):\n    if name is None:\n        return\n    print(f\"hi {name}\")").unwrap();
        assert_eq!(c.language.as_deref(), Some("Python"));
        let c = run("1 │ const foo = () => 42;\n2 │ console.log(foo());").unwrap();
        assert_eq!(c.language.as_deref(), Some("JavaScript"));
        assert_eq!(c.without_line_numbers.as_deref(), Some("const foo = () => 42;\nconsole.log(foo());"));
        assert_eq!(run("SELECT name, price FROM products WHERE qty > 2;").unwrap().language.as_deref(), Some("SQL"));
    }

    #[test]
    fn prose_is_not_code() {
        assert!(run("The quick brown fox jumps over the lazy dog. It was a sunny day in the park, and everyone was happy.").is_none());
        assert!(run("Meeting notes:\nWe agreed to ship the release next week.\nAlice will update the docs before then.").is_none());
    }
}
