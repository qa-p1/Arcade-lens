//! Recognizers that combine text with selection context: Git commits on
//! code forges, document pages, and subtitles inside media players.

use std::sync::LazyLock;

use lens_core::geometry::Point;
use lens_core::selection::WindowInfo;
use lens_core::value::UrlValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

static FORGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(github\.com|gitlab\.com|codeberg\.org|bitbucket\.org)/([\w.\-]+)/([\w.\-]+?)(?:\.git)?(?:[/#?\s]|$)").unwrap());
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap());

/// Window under the selection's center, if the platform reported windows.
pub fn window_under(cx: &RecognizeContext) -> Option<&WindowInfo> {
    let r = cx.selection.rect;
    let c = Point { x: r.x + r.width as i32 / 2, y: r.y + r.height as i32 / 2 };
    cx.selection.context.windows.iter().find(|w| w.rect.contains(c))
}

fn window_matches(w: &WindowInfo, names: &[&str]) -> bool {
    let hay = format!("{} {}", w.app_name.as_deref().unwrap_or_default(), w.title).to_lowercase();
    names.iter().any(|n| hay.contains(n))
}

pub const MEDIA_APPS: &[&str] = &[
    "vlc",
    "mpv",
    "totem",
    "celluloid",
    "smplayer",
    "iina",
    "quicktime",
    "media player",
    "movies & tv",
    "plex",
    "kodi",
    "youtube",
    "netflix",
    "prime video",
    "twitch",
    "disney+",
    "vimeo",
];
const DOCUMENT_APPS: &[&str] = &[
    "evince",
    "okular",
    "atril",
    "zathura",
    "xreader",
    "document viewer",
    "papers",
    "acrobat",
    "preview",
    "sumatra",
    "foxit",
    "libreoffice",
    "soffice",
    "winword",
    "microsoft word",
    "pages",
    ".pdf",
    "google docs",
];

pub fn git_commits(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let Some(repo) = FORGE.captures(input.text) else { return vec![] };
    let (host, owner, name) = (&repo[1], &repo[2], &repo[3]);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in HEX.find_iter(input.text) {
        let h = m.as_str();
        // Needs a digit and a letter, and must not be part of the repo URL itself.
        if !h.chars().any(|c| c.is_ascii_digit()) || !h.chars().any(|c| c.is_ascii_alphabetic()) || repo.get(0).is_some_and(|r| r.range().contains(&m.start()))
        {
            continue;
        }
        if !seen.insert(h.to_string()) {
            continue;
        }
        let path = match host {
            "gitlab.com" => format!("{owner}/{name}/-/commit/{h}"),
            "bitbucket.org" => format!("{owner}/{name}/commits/{h}"),
            _ => format!("{owner}/{name}/commit/{h}"),
        };
        let url = format!("https://{host}/{path}");
        out.push(
            Detection::new(caps::GIT_COMMIT, Value::Url(UrlValue { url, scheme: "https".into(), host: Some(host.to_string()) }))
                .span(m.range())
                .confidence(0.8)
                .detail("Repository", format!("{owner}/{name}"))
                .detail("Commit", h.chars().take(10).collect::<String>()),
        );
    }
    out
}

/// Prose-like text in a document viewer (or long flowing prose anywhere).
pub fn document(input: &TextInput, cx: &RecognizeContext) -> Vec<Detection> {
    let lines: Vec<&str> = input.text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 3 {
        return vec![];
    }
    let in_viewer = window_under(cx).is_some_and(|w| window_matches(w, DOCUMENT_APPS));
    let letters = input.text.chars().filter(|c| c.is_alphabetic()).count() as f32;
    let symbols = input.text.chars().filter(|c| "{}[]();=<>|$#_\\".contains(*c)).count() as f32;
    let avg_len = lines.iter().map(|l| l.chars().count()).sum::<usize>() as f32 / lines.len() as f32;
    let prose =
        letters / (input.text.chars().filter(|c| !c.is_whitespace()).count().max(1) as f32) > 0.75 && symbols / letters.max(1.0) < 0.02 && avg_len > 35.0;
    if !(in_viewer || (prose && lines.len() >= 4)) {
        return vec![];
    }
    let mut d = Detection::new(caps::DOCUMENT, Value::text(input.text.to_string()))
        .confidence(if in_viewer { 0.85 } else { 0.6 })
        .detail("Lines", lines.len().to_string());
    if let Some(w) = window_under(cx).filter(|_| in_viewer) {
        d = d.detail("Document", w.title.clone());
    }
    vec![d]
}

/// Text in the lower part of a video player window is very likely a subtitle.
pub fn subtitle(input: &TextInput, cx: &RecognizeContext) -> Vec<Detection> {
    let Some(layout) = input.layout else { return vec![] };
    if !window_under(cx).is_some_and(|w| window_matches(w, MEDIA_APPS)) {
        return vec![];
    }
    let h = cx.selection.rect.height as i32;
    let lines: Vec<&str> = layout.lines.iter().filter(|l| l.bbox.y > h * 6 / 10).map(|l| l.text.trim()).filter(|t| !t.is_empty()).collect();
    if lines.is_empty() {
        return vec![];
    }
    vec![Detection::new(caps::SUBTITLE, Value::text(lines.join("\n"))).confidence(0.75)]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::RgbaImage;
    use lens_core::cancel::CancelToken;
    use lens_core::geometry::Rect;
    use lens_core::recognizer::NullEnvironment;
    use lens_core::selection::SelectionContext;
    use lens_core::value::{TextLayout, TextLine};
    use lens_core::{Selection, Settings, Signals};

    use super::*;

    fn cx_in(app: &str, title: &str) -> RecognizeContext {
        let img = RgbaImage::new(400, 300);
        let mut sel = Selection::new(Rect::new(100, 100, 400, 300), img.clone());
        sel.context = SelectionContext {
            windows: vec![WindowInfo { id: "w".into(), title: title.into(), app_name: Some(app.into()), rect: Rect::new(0, 0, 1000, 800), pid: None }],
            ..Default::default()
        };
        RecognizeContext {
            selection: Arc::new(sel),
            signals: Arc::new(Signals::compute(&img)),
            cancel: CancelToken::new(),
            env: Arc::new(NullEnvironment),
            settings: Arc::new(Settings::default()),
        }
    }

    #[test]
    fn commit_links() {
        let t = "Fixed in 3f2a9c1b on https://github.com/qa-p1/Arcade-lens/pull/4";
        let d = git_commits(&TextInput { text: t, layout: None }, &cx_in("firefox", "PR"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].value.as_text().unwrap(), "https://github.com/qa-p1/Arcade-lens/commit/3f2a9c1b");
        let t = "see https://gitlab.com/group/proj and commit 9fceb02d0ae598e95dc970b74767f19372d61af8";
        let d = git_commits(&TextInput { text: t, layout: None }, &cx_in("x", "y"));
        assert!(d[0].value.as_text().unwrap().ends_with("/group/proj/-/commit/9fceb02d0ae598e95dc970b74767f19372d61af8"));
        assert!(git_commits(&TextInput { text: "commit 3f2a9c1b without a repo", layout: None }, &cx_in("x", "y")).is_empty());
    }

    #[test]
    fn documents_and_subtitles() {
        let page = "The committee reviewed the proposal in detail during the session.\nMembers agreed that the timeline was realistic and well planned.\nA final decision will be published after the next meeting.";
        assert_eq!(document(&TextInput { text: page, layout: None }, &cx_in("evince", "report.pdf")).len(), 1);
        assert!(document(&TextInput { text: "fn main() {\n  let x = 1;\n  println!(\"{x}\");\n}", layout: None }, &cx_in("code", "main.rs")).is_empty());

        let layout = TextLayout {
            lines: vec![
                TextLine { text: "Channel logo".into(), bbox: Rect::new(10, 10, 100, 20), words: vec![] },
                TextLine { text: "I never said that.".into(), bbox: Rect::new(80, 250, 240, 24), words: vec![] },
            ],
        };
        let d = subtitle(&TextInput { text: "Channel logo\nI never said that.", layout: Some(&layout) }, &cx_in("mpv", "movie.mkv - mpv"));
        assert_eq!(d[0].value.as_text().unwrap(), "I never said that.");
        assert!(subtitle(&TextInput { text: "x", layout: Some(&layout) }, &cx_in("gedit", "notes")).is_empty());
    }
}
