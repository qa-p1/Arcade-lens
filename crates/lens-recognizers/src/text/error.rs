//! Technical errors: tracebacks, compiler errors, panics, HTTP errors…
//!
//! Besides recognizing an error, this builds a *cleaned* search query: the
//! informative core of the message with machine-specific noise (paths,
//! usernames, timestamps, addresses, hashes, ids) and secrets removed, so
//! "Search Cleaned Error" finds other people's reports of the same problem
//! without leaking anything about this machine.

use std::sync::LazyLock;

use lens_core::value::ErrorValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{secret, TextInput};

struct Kind {
    name: &'static str,
    /// Matches somewhere in the text to identify this kind.
    trigger: Regex,
    /// Picks the headline line (falls back to the trigger line).
    headline: Option<Regex>,
    confidence: f32,
}

static KINDS: LazyLock<Vec<Kind>> = LazyLock::new(|| {
    let k = |name, trigger: &str, headline: Option<&str>, confidence| Kind {
        name,
        trigger: Regex::new(trigger).unwrap(),
        headline: headline.map(|h| Regex::new(h).unwrap()),
        confidence,
    };
    vec![
        k(
            "Python traceback",
            r"(?m)^Traceback \(most recent call last\):",
            Some(r"(?m)^(?:[\w.]+\.)?\w*(?:Error|Exception|Warning|Exit|Interrupt|Iteration)\b.*$"),
            0.97,
        ),
        // `[:;]`: OCR frequently reads the colon after `error[E0382]` as a semicolon.
        k("Rust compiler error", r"(?m)^error(?:\[E\d{4}\][:;]|:) .+", Some(r"(?m)^error(?:\[E\d{4}\][:;]|:) .+$"), 0.95),
        k("Rust panic", r"thread '[^']*' panicked at", Some(r"(?m)^thread '[^']*' panicked at .*$"), 0.95),
        k("Go panic", r"(?m)^panic: .+", Some(r"(?m)^panic: .+$"), 0.93),
        k(
            "Java exception",
            r#"(?m)(?:^Exception in thread "[^"]*" |^Caused by: |^)(?:[a-z][\w$]*\.)+[A-Z][\w$]*(?:Exception|Error)\b.*\n\s+at "#,
            Some(r"(?m)(?:[a-z][\w$]*\.)+[A-Z][\w$]*(?:Exception|Error)\b.*$"),
            0.95,
        ),
        k(".NET exception", r"(?m)^(?:Unhandled exception\. )?System\.[\w.]+Exception: .+", Some(r"(?m)System\.[\w.]+Exception: .+$"), 0.93),
        k(
            "JavaScript error",
            r"(?m)^(?:Uncaught )?(?:\w+)?(?:Error|Exception): .+(?:\n\s+at .+)?",
            Some(r"(?m)^(?:Uncaught )?(?:\w+)?(?:Error|Exception): .+$"),
            0.85,
        ),
        k("Compiler error", r"(?m)^[^\s:]+:\d+:\d+: (?:fatal )?error: .+", Some(r"(?m)(?:fatal )?error: .+$"), 0.92),
        k("Segmentation fault", r"(?i)\bsegmentation fault\b|\bSIGSEGV\b|\bcore dumped\b", Some(r"(?i).*(?:segmentation fault|SIGSEGV).*$"), 0.92),
        k("npm error", r"(?m)^npm ERR! .+|^npm error .+", Some(r"(?m)^npm (?:ERR!|error) (?:code )?.+$"), 0.88),
        k(
            "HTTP error",
            r"\b(?:HTTP/\d(?:\.\d)?\s+)?[45]\d{2}\s+(?:Bad Request|Unauthorized|Forbidden|Not Found|Method Not Allowed|Conflict|Gone|Payload Too Large|Too Many Requests|Internal Server Error|Not Implemented|Bad Gateway|Service Unavailable|Gateway Timeout)\b",
            Some(r"\b(?:HTTP/\d(?:\.\d)?\s+)?[45]\d{2}\s+[A-Z][\w ]+"),
            0.85,
        ),
        k("Exception", r"\b\w+(?:Exception|Error): \S.+", Some(r"\b\w+(?:Exception|Error): \S.+$"), 0.7),
        k(
            "Error message",
            r"(?m)^\s*(?:\[?(?:ERROR|FATAL|CRITICAL)\]?:?|error:|fatal:|E\d{4}:)\s+\S.+",
            Some(r"(?m)^\s*(?:\[?(?:ERROR|FATAL|CRITICAL)\]?:?|error:|fatal:|E\d{4}:)\s+\S.+$"),
            0.65,
        ),
    ]
});

static STACK_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s+(?:at\s|File "|\d+:\s+0x|-->\s|in\s\S+\s\(|\.\.\.\s\d+\smore)|^\s*goroutine \d+ |^\s+\S+\.go:\d+"#).unwrap());

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let Some(kind) = KINDS.iter().find(|k| k.trigger.is_match(text)) else { return vec![] };
    let trigger_line = kind.trigger.find(text).map(|m| m.as_str().lines().next().unwrap_or_default().to_string()).unwrap_or_default();
    let mut headline = kind
        .headline
        .as_ref()
        .and_then(|h| {
            // Python reports the actual exception last; most others first.
            if kind.name == "Python traceback" {
                h.find_iter(text).last()
            } else {
                h.find(text)
            }
        })
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or(trigger_line);
    // Newer Rust panics put the message on the following line.
    if kind.name == "Rust panic" {
        if let Some(next) = text.split(&headline).nth(1).and_then(|rest| rest.lines().nth(1)) {
            let next = next.trim();
            if !next.is_empty() && !next.starts_with("note:") {
                headline = format!("panicked: {next}");
            }
        }
    }
    let has_stack = STACK_LINE.is_match(text);
    let cleaned = clean(&headline);
    vec![Detection::new(
        caps::ERROR,
        Value::Error(ErrorValue {
            raw: text.trim().to_string(),
            kind: kind.name.into(),
            headline: headline.clone(),
            cleaned_query: cleaned.clone(),
            stack_trace: has_stack.then(|| text.trim().to_string()),
        }),
    )
    .confidence(kind.confidence)
    .detail("Kind", kind.name)
    .detail("Search", cleaned)]
}

static NOISE: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // Order matters: whole paths before their pieces.
        (r#"(?:[A-Za-z]:\\|\\\\)[^\s"'`:]*\\([^\\\s"'`:]+)"#, "$1"),
        (r#"(?:~|/(?:home|Users|root|tmp|var|opt|usr|private|mnt|srv|workspace)\b)[^\s"'`:]*/([^/\s"'`:]+)"#, "$1"),
        (r"(?:[^\s/]+/)+([^/\s]+\.\w{1,8})", "$1"),
        (r"\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b", ""),
        (r"\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)?\b", ""),
        (r"\b\d{1,2}:\d{2}:\d{2}(?:\.\d+)?\b", ""),
        (r"\b0x[0-9a-fA-F]{4,}\b", ""),
        (r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b", ""),
        (r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b", ""),
        (r"\b[0-9a-f]{12,}\b", ""),
        (r"\b(?:pid|PID|tid|id|ID)[=: ]\s*\d+\b", ""),
        (r"\b\d{5,}\b", ""),
        (r":\d+(?::\d+)?\b", ""),
        (r"\s+", " "),
    ]
    .into_iter()
    .map(|(p, r)| (Regex::new(p).unwrap(), r))
    .collect()
});

static REDACTION_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]* redacted\]").unwrap());

/// Removes machine-specific noise and any secrets from an error line.
pub fn clean(line: &str) -> String {
    let mut s = secret::redact(line);
    // Drop redaction markers entirely: they carry no search value.
    s = REDACTION_MARKER.replace_all(&s, "").into_owned();
    for (re, rep) in NOISE.iter() {
        s = re.replace_all(&s, *rep).into_owned();
    }
    s.trim().trim_matches([',', ';', ':', '-']).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn err(text: &str) -> Option<ErrorValue> {
        detect(&TextInput { text, layout: None }, &cx()).into_iter().next().map(|d| match d.value {
            Value::Error(e) => e,
            _ => unreachable!(),
        })
    }

    #[test]
    fn python() {
        let e = err("Traceback (most recent call last):\n  File \"/home/alice/proj/app.py\", line 3, in <module>\n    main()\n  File \"/home/alice/proj/app.py\", line 2, in main\n    raise KeyError('user_id')\nKeyError: 'user_id'").unwrap();
        assert_eq!(e.kind, "Python traceback");
        assert_eq!(e.headline, "KeyError: 'user_id'");
        assert!(e.stack_trace.is_some());
    }

    #[test]
    fn rust() {
        let e = err("error[E0382]: borrow of moved value: `x`\n --> src/main.rs:4:20\n  |").unwrap();
        assert_eq!(e.headline, "error[E0382]: borrow of moved value: `x`");
        assert_eq!(e.cleaned_query, "error[E0382]: borrow of moved value: `x`");
        assert_eq!(err("error[E0382]; borrow of moved value: config").unwrap().kind, "Rust compiler error");
        let e =
            err("thread 'main' panicked at src/main.rs:2:5:\nindex out of bounds: the len is 3 but the index is 7\nnote: run with `RUST_BACKTRACE=1`").unwrap();
        assert_eq!(e.headline, "panicked: index out of bounds: the len is 3 but the index is 7");
    }

    #[test]
    fn java_and_js() {
        let e = err("Exception in thread \"main\" java.lang.NullPointerException: Cannot invoke \"String.length()\"\n\tat com.example.App.main(App.java:5)")
            .unwrap();
        assert_eq!(e.kind, "Java exception");
        assert!(e.headline.starts_with("java.lang.NullPointerException"));
        let e = err("Uncaught TypeError: Cannot read properties of undefined (reading 'map')\n    at App (App.jsx:12:5)").unwrap();
        assert_eq!(e.kind, "JavaScript error");
    }

    #[test]
    fn http_and_segfault() {
        assert_eq!(err("GET /api → 503 Service Unavailable").unwrap().kind, "HTTP error");
        assert_eq!(err("zsh: segmentation fault (core dumped)  ./a.out").unwrap().kind, "Segmentation fault");
        assert!(err("Everything is fine, no errors here.").is_none());
    }

    #[test]
    fn cleaning_removes_machine_noise_and_secrets() {
        let q = clean("2024-03-05 14:30:12 ERROR /home/alice/work/app/src/db.rs:42:7 connection to 10.0.0.5:5432 failed for alice@example.com pid=48213 at 0x7ffd5e8c (token sk-ant-api03-abcdefghijklmnopqrstu)");
        for leaked in ["alice", "10.0.0.5", "48213", "0x7ffd", "2024-03-05", "sk-ant", "/home"] {
            assert!(!q.contains(leaked), "{leaked} leaked into {q:?}");
        }
        assert!(q.contains("db.rs") && q.contains("connection to") && q.contains("failed"), "{q}");
        assert_eq!(clean(r"FileNotFoundError: C:\Users\Bob\AppData\Local\Temp\x9f.tmp not found"), "FileNotFoundError: x9f.tmp not found");
    }
}
