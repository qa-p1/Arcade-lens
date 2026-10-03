//! File system paths: Unix, home-relative, relative, Windows, UNC, file URLs.
//!
//! Each path is checked against the local file system so the palette only
//! offers "Open File" for files that actually exist.

use std::ops::Range;
use std::sync::LazyLock;

use lens_core::value::{PathFlavor, PathKind, PathValue};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::{overlaps, trim_token, url, TextInput};

static PATTERNS: LazyLock<Vec<(PathFlavor, Regex)>> = LazyLock::new(|| {
    let seg = r"[\w.@%+~\-]";
    vec![
        (PathFlavor::FileUrl, Regex::new(r"file://[^\s<>\x22']+").unwrap()),
        (PathFlavor::Unc, Regex::new(r"\\\\[\w.\-]+\\[\w$.\-]+(?:\\[^\s\\/:*?\x22<>|]+)*\\?").unwrap()),
        (PathFlavor::Windows, Regex::new(r#"\b[A-Za-z]:\\(?:[^\s\\/:*?"<>|]+\\?)*"#).unwrap()),
        (PathFlavor::HomeRelative, Regex::new(&format!(r"~/(?:{seg}+/?)*")).unwrap()),
        (PathFlavor::Relative, Regex::new(&format!(r"\.\.?/(?:{seg}+/?)+")).unwrap()),
        (PathFlavor::Unix, Regex::new(&format!(r"/(?:{seg}+/)*{seg}+/?")).unwrap()),
        // `src/main.rs`, `.github/workflows/ci.yml`: needs a slash and an extension.
        (PathFlavor::Relative, Regex::new(r"\b\.?[\w\-]+(?:/[\w.\-]+)*/[\w\-][\w.\-]*\.[A-Za-z0-9]{1,8}\b").unwrap()),
    ]
});

static QUOTED_WINDOWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([A-Za-z]:\\[^"\r\n]+)""#).unwrap());
static LINE_COL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:(\d+)(?::(\d+))?").unwrap());

const UNIX_ROOTS: &[&str] = &[
    "home",
    "usr",
    "etc",
    "var",
    "tmp",
    "opt",
    "bin",
    "sbin",
    "lib",
    "lib64",
    "dev",
    "proc",
    "sys",
    "mnt",
    "media",
    "srv",
    "root",
    "run",
    "boot",
    "nix",
    "snap",
    "Users",
    "Applications",
    "Library",
    "System",
    "Volumes",
    "private",
    "workspace",
    "workspaces",
];

pub fn find(text: &str, cx: &RecognizeContext) -> Vec<(Range<usize>, PathValue, Option<String>)> {
    let urls: Vec<_> = url::find(text).into_iter().map(|(r, _)| r).collect();
    let mut taken: Vec<Range<usize>> = Vec::new();
    let mut out = Vec::new();

    for c in QUOTED_WINDOWS.captures_iter(text) {
        let m = c.get(1).unwrap();
        taken.push(m.range());
        out.push((m.range(), value(m.as_str(), PathFlavor::Windows, cx), None));
    }

    for (flavor, re) in PATTERNS.iter() {
        for m in re.find_iter(text) {
            let raw = trim_token(m.as_str());
            if raw.is_empty() {
                continue;
            }
            let r = m.start()..m.start() + raw.len();
            if taken.iter().any(|t| overlaps(t, &r)) || (*flavor != PathFlavor::FileUrl && urls.iter().any(|u| overlaps(u, &r))) {
                continue;
            }
            let before = text[..r.start].chars().next_back();
            match flavor {
                PathFlavor::Unix => {
                    // Must start a token, and not be `//comment` or `a/b` fractions.
                    if before.is_some_and(|ch| ch.is_alphanumeric() || "/.~:_-\\".contains(ch)) || raw.starts_with("//") {
                        continue;
                    }
                    let segments: Vec<&str> = raw.split('/').filter(|s| !s.is_empty()).collect();
                    let first = segments.first().copied().unwrap_or_default();
                    if segments.len() < 2 && !UNIX_ROOTS.contains(&first) {
                        continue;
                    }
                    if segments.len() < 3 && !UNIX_ROOTS.contains(&first) && !raw.contains('.') {
                        // `/api/users` style route fragments are too ambiguous.
                        continue;
                    }
                }
                PathFlavor::Relative => {
                    // Must start a token, and the first segment must not look like a domain.
                    let first = raw.split('/').next().unwrap_or_default();
                    if before.is_some_and(|ch| ch.is_alphanumeric() || "/.@_-".contains(ch)) || !raw.starts_with('.') && first.contains('.') {
                        continue;
                    }
                }
                _ => {}
            }
            let line_col = LINE_COL.captures(&text[r.end..]).map(|lc| match lc.get(2) {
                Some(col) => format!("line {}, column {}", &lc[1], col.as_str()),
                None => format!("line {}", &lc[1]),
            });
            taken.push(r.clone());
            out.push((r, value(raw, *flavor, cx), line_col));
        }
    }
    out.sort_by_key(|(r, ..)| r.start);
    out
}

fn value(raw: &str, flavor: PathFlavor, cx: &RecognizeContext) -> PathValue {
    let resolved = match flavor {
        PathFlavor::Unix | PathFlavor::Windows | PathFlavor::Unc => Some(raw.to_string()),
        PathFlavor::HomeRelative => cx.env.home_dir().map(|h| format!("{}{}", h.display(), &raw[1..])),
        PathFlavor::FileUrl => Some(percent_decode(raw.trim_start_matches("file://").trim_start_matches("localhost"))),
        PathFlavor::Relative => None,
    };
    let exists = resolved.as_deref().and_then(|p| cx.env.path_kind(p));
    PathValue { raw: raw.to_string(), resolved, flavor, exists }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn detect(input: &TextInput, cx: &RecognizeContext) -> Vec<Detection> {
    find(input.text, cx)
        .into_iter()
        .map(|(r, v, line_col)| {
            let confidence = match (v.flavor, v.exists) {
                (_, Some(PathKind::File | PathKind::Directory)) => 0.97,
                (PathFlavor::FileUrl | PathFlavor::Windows | PathFlavor::Unc, _) => 0.85,
                (PathFlavor::Unix | PathFlavor::HomeRelative, _) => 0.8,
                (PathFlavor::Relative, _) => 0.65,
            };
            let status = match v.exists {
                Some(PathKind::File) => "File exists",
                Some(PathKind::Directory) => "Folder exists",
                Some(PathKind::Missing) => "Not found on this computer",
                None => "Not checked",
            };
            let mut d = Detection::new(caps::PATH, Value::Path(v)).span(r).confidence(confidence).detail("Status", status);
            if let Some(lc) = line_col {
                d = d.detail("Location", lc);
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use lens_core::recognizer::Environment;
    use lens_core::Settings;

    use super::*;
    use crate::text::testing::{cx, cx_with};

    struct FakeFs;
    impl Environment for FakeFs {
        fn path_kind(&self, p: &str) -> Option<PathKind> {
            Some(match p {
                "/home/user/Work/project/src/main.rs" => PathKind::File,
                "/home/user/Work" => PathKind::Directory,
                _ => PathKind::Missing,
            })
        }
        fn home_dir(&self) -> Option<PathBuf> {
            Some("/home/user".into())
        }
    }

    fn paths(text: &str) -> Vec<String> {
        find(text, &cx()).into_iter().map(|(_, v, _)| v.raw).collect()
    }

    #[test]
    fn flavors() {
        assert_eq!(paths("open /home/user/Work/project/src/main.rs:12:5 now"), vec!["/home/user/Work/project/src/main.rs"]);
        assert_eq!(paths(r"C:\Users\Ada\Documents\report.docx, then"), vec![r"C:\Users\Ada\Documents\report.docx"]);
        assert_eq!(paths(r#"in "C:\Program Files\Arcade\lens.exe""#), vec![r"C:\Program Files\Arcade\lens.exe"]);
        assert_eq!(paths(r"share \\fileserver\team\specs\v2.pdf"), vec![r"\\fileserver\team\specs\v2.pdf"]);
        assert_eq!(paths("edit ~/.config/arcade/lens.toml"), vec!["~/.config/arcade/lens.toml"]);
        assert_eq!(paths("at src/engine/mod.rs:40"), vec!["src/engine/mod.rs"]);
        assert_eq!(paths("see ./scripts/build.sh"), vec!["./scripts/build.sh"]);
        assert_eq!(paths("file:///tmp/My%20File.txt"), vec!["file:///tmp/My%20File.txt"]);
        assert_eq!(paths("cd /etc"), vec!["/etc"]);
    }

    #[test]
    fn non_paths() {
        for t in ["https://example.com/a/b.html", "and/or", "1/2 cup", "// comment", "example.com/a.html", "GET /api/users", "50/50"] {
            assert!(paths(t).is_empty(), "{t}: {:?}", paths(t));
        }
    }

    #[test]
    fn existence_and_resolution() {
        let cx = cx_with(Settings::default(), Arc::new(FakeFs));
        let found = detect(&TextInput { text: "/home/user/Work/project/src/main.rs:12:5 and ~/Work and /nope/missing.txt", layout: None }, &cx);
        let vals: Vec<_> = found
            .iter()
            .map(|d| match &d.value {
                Value::Path(p) => (p.resolved.clone().unwrap(), p.exists),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            vals,
            vec![
                ("/home/user/Work/project/src/main.rs".into(), Some(PathKind::File)),
                ("/home/user/Work".into(), Some(PathKind::Directory)),
                ("/nope/missing.txt".into(), Some(PathKind::Missing)),
            ]
        );
        assert!(found[0].details.contains(&("Location".into(), "line 12, column 5".into())));
        let file_url = find("file:///tmp/My%20File.txt", &cx);
        assert_eq!(file_url[0].1.resolved.as_deref(), Some("/tmp/My File.txt"));
    }
}
