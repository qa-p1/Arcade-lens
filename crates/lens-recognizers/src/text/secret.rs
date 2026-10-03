//! Secrets: API keys, tokens, passwords, private keys.
//!
//! A detected secret makes the palette conservative: outbound actions whose
//! payload would contain it are removed (see `lens_core::palette`). Secret
//! values are wrapped in `Sensitive` so they never appear in debug output.

use std::ops::Range;
use std::sync::LazyLock;

use lens_core::value::{SecretValue, Sensitive};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

struct Pattern {
    kind: &'static str,
    re: Regex,
    /// Capture group holding the secret itself (0 = whole match).
    group: usize,
    confidence: f32,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    let p = |kind, re: &str, group, confidence| Pattern { kind, re: Regex::new(re).unwrap(), group, confidence };
    vec![
        p("Private key", r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----[\s\S]*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----|$)", 0, 0.99),
        p("AWS access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", 0, 0.97),
        p("AWS secret key", r#"(?i)aws_?secret_?access_?key\s*[:=]\s*["']?([A-Za-z0-9/+=]{40})\b"#, 1, 0.97),
        p("GitHub token", r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255})\b", 0, 0.99),
        p("GitLab token", r"\bglpat-[A-Za-z0-9_\-]{20,}\b", 0, 0.98),
        p("Slack token", r"\bxox[abposr]-[A-Za-z0-9-]{10,}\b", 0, 0.98),
        p("Slack webhook", r"https://hooks\.slack\.com/services/[A-Za-z0-9/]+", 0, 0.97),
        p("Stripe key", r"\b[rs]k_(?:live|test)_[A-Za-z0-9]{16,}\b", 0, 0.98),
        p("Google API key", r"\bAIza[0-9A-Za-z_\-]{35}\b", 0, 0.97),
        p("Anthropic API key", r"\bsk-ant-[A-Za-z0-9_\-]{20,}", 0, 0.99),
        p("OpenAI API key", r"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_\-]{20,}", 0, 0.9),
        p("npm token", r"\bnpm_[A-Za-z0-9]{36}\b", 0, 0.97),
        p("SendGrid key", r"\bSG\.[A-Za-z0-9_\-]{22}\.[A-Za-z0-9_\-]{43}\b", 0, 0.98),
        p("JSON Web Token", r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}", 0, 0.95),
        p("Bearer token", r"(?i)\bbearer\s+([A-Za-z0-9._~+/\-]{20,}=*)", 1, 0.9),
        p("URL credentials", r"[a-zA-Z][a-zA-Z0-9+.\-]*://[^\s:/@]+:([^\s/@]+)@", 1, 0.95),
        p(
            "Password or key",
            r#"(?i)\b(?:password|passwd|pwd|pass|secret|client_?secret|api_?key|apikey|access_?token|auth_?token|private_?key|token)\b["']?\s*[:=]\s*["']?([^\s"',;]{6,})"#,
            1,
            0.85,
        ),
    ]
});

/// Values that are obviously placeholders, not secrets.
fn placeholder(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    v.starts_with('$')
        || v.starts_with('<')
        || v.starts_with("{{")
        || v.starts_with("${")
        || v.chars().all(|c| c == '*' || c == 'x' || c == 'X' || c == '•' || c == '.')
        || ["your", "example", "placeholder", "changeme", "redacted", "xxxx", "none", "null", "true", "false", "process.env", "os.environ", "env("]
            .iter()
            .any(|p| l.contains(p))
}

pub fn mask(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= 16 {
        format!("{}…{}", chars[..4].iter().collect::<String>(), chars[chars.len() - 4..].iter().collect::<String>())
    } else {
        "•".repeat(chars.len().min(8))
    }
}

/// All secrets in `text` with their byte ranges, kind, and confidence.
pub fn find(text: &str) -> Vec<(Range<usize>, &'static str, f32)> {
    let mut out: Vec<(Range<usize>, &'static str, f32)> = Vec::new();
    for p in PATTERNS.iter() {
        for c in p.re.captures_iter(text) {
            let Some(m) = c.get(p.group) else { continue };
            if p.group != 0 && placeholder(m.as_str()) {
                continue;
            }
            let r = m.range();
            if out.iter().any(|(o, ..)| o.start < r.end && r.start < o.end) {
                continue;
            }
            out.push((r, p.kind, p.confidence));
        }
    }
    out.sort_by_key(|(r, ..)| r.start);
    out
}

/// Replaces every detected secret with a masked form.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for (r, kind, _) in find(text).into_iter().rev() {
        out.replace_range(r, &format!("[{kind} redacted]"));
    }
    out
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    find(input.text)
        .into_iter()
        .map(|(r, kind, confidence)| {
            let raw = &input.text[r.clone()];
            let masked = if kind == "Private key" { "-----BEGIN … PRIVATE KEY-----".to_string() } else { mask(raw) };
            Detection::new(caps::SECRET, Value::Secret(SecretValue { kind: kind.into(), masked: masked.clone(), raw: Sensitive::new(raw) }))
                .span(r)
                .confidence(confidence)
                .detail("Looks like", kind)
                .detail("Value", masked)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<&'static str> {
        find(text).into_iter().map(|(_, k, _)| k).collect()
    }

    #[test]
    fn known_formats() {
        assert_eq!(kinds("export GITHUB_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"), vec!["GitHub token"]);
        assert_eq!(kinds("key AKIAIOSFODNN7EXAMPLE"), vec!["AWS access key"]);
        assert_eq!(kinds("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----"), vec!["Private key"]);
        assert_eq!(kinds("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.abcdefghijk"), vec!["JSON Web Token"]);
        assert_eq!(kinds("postgres://admin:hunter2pass@db.internal:5432/app"), vec!["URL credentials"]);
        assert_eq!(kinds("ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz"), vec!["Anthropic API key"]);
    }

    #[test]
    fn assignments_but_not_placeholders() {
        assert_eq!(kinds("password: Tr0ub4dor&3"), vec!["Password or key"]);
        for t in ["password: ********", "api_key = \"${API_KEY}\"", "token: <your-token-here>", "password = os.environ['PW']", "the password field"] {
            assert!(kinds(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn redaction() {
        let r = redact("curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz123456' https://api.example.com");
        assert!(!r.contains("abcdefghijklmnop"), "{r}");
        assert!(r.contains("https://api.example.com"));
        assert_eq!(mask("ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"), "ghp_…6789");
    }
}
