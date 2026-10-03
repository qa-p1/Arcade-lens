//! Context-dependent actions: documents, media, commits, symbols, reverse
//! image search and workspaces.

use std::io::Cursor;
use std::sync::LazyLock;

use image::RgbaImage;
use lens_core::action::{ActionGroup as G, Choice};
use lens_core::builder::action;
use lens_core::host::{HostFeatures as F, SaveRequest, WindowCommand};
use lens_core::registry::PluginRegistrar;
use lens_core::{caps, ActionContext, ActionOutcome, Effects, LensError, Result, Value};
use regex::Regex;

use crate::util::*;

/// A single-page PDF containing `img` as a JPEG, sized at 96 dpi.
pub fn pdf_from_image(img: &RgbaImage) -> Result<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut jpeg = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92).encode_image(&rgb).map_err(|e| LensError::Failed(e.to_string()))?;
    let jpeg = jpeg.into_inner();
    let (w, h) = (img.width(), img.height());
    let (pw, ph) = (w as f64 * 72.0 / 96.0, h as f64 * 72.0 / 96.0);
    let content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q\n");
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    obj(
        &mut out,
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>").as_bytes(),
    );
    let mut img_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    img_obj.extend_from_slice(&jpeg);
    img_obj.extend_from_slice(b"\nendstream");
    obj(&mut out, &img_obj);
    obj(&mut out, format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).as_bytes());
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    Ok(out)
}

fn selection_image<'a>(cx: &'a ActionContext) -> Result<&'a RgbaImage> {
    cx.selection.map(|s| &*s.image).ok_or_else(|| LensError::InvalidInput("no selection".into()))
}

fn save_pdf(cx: &ActionContext, name: &str) -> Result<(ActionOutcome, std::path::PathBuf)> {
    let bytes = pdf_from_image(selection_image(cx)?)?;
    let path = cx.host.save_file(SaveRequest {
        suggested_name: timestamp_name(name, "pdf"),
        bytes,
        directory: cx.param_str("directory").map(Into::into),
        mime: "application/pdf".into(),
    })?;
    Ok((saved(path.clone(), "application/pdf"), path))
}

static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]{2,}(?:::[A-Za-z_][A-Za-z0-9_]*)*\b").unwrap());
const KEYWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "let",
    "mut",
    "pub",
    "use",
    "impl",
    "self",
    "Self",
    "struct",
    "enum",
    "match",
    "return",
    "true",
    "false",
    "None",
    "Some",
    "def",
    "class",
    "import",
    "from",
    "const",
    "var",
    "function",
    "new",
    "this",
    "else",
    "while",
    "async",
    "await",
    "static",
    "void",
    "int",
    "string",
    "String",
    "with",
    "not",
    "println",
    "print",
    "console",
    "log",
    "elif",
    "None",
    "null",
    "undefined",
    "public",
    "private",
    "package",
    "func",
    "type",
];

/// Identifiers in code, most frequent first.
pub fn symbols(code: &str) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for m in IDENT.find_iter(code) {
        let s = m.as_str();
        if KEYWORDS.contains(&s) || s.chars().all(|c| c.is_ascii_digit() || c == '_') {
            continue;
        }
        match counts.iter_mut().find(|(n, _)| n == s) {
            Some((_, c)) => *c += 1,
            None => counts.push((s.to_string(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.len().cmp(&a.0.len())));
    counts.into_iter().take(8).map(|(s, _)| s).collect()
}

pub fn register(r: &mut PluginRegistrar) {
    // Reverse image search: copy the pixels, open the configured service. Never uploads by itself.
    r.action(
        action("core.image.reverse-search", "Reverse Image Search")
            .icon("image-search")
            .group(G::Search)
            .accepts_all([caps::REGION, caps::IMAGE])
            .priority(35)
            .effects(Effects::CLIPBOARD | Effects::NETWORK | Effects::LAUNCHES_APP)
            .requires(F::CLIPBOARD_IMAGE | F::OPEN_URI)
            .enabled_when(|s| s.providers.reverse_image_search.is_some())
            .run(|i, cx| {
                let url = cx
                    .settings
                    .providers
                    .reverse_image_search
                    .clone()
                    .ok_or_else(|| LensError::InvalidInput("no reverse image search service configured".into()))?;
                cx.host.set_clipboard_image(image_of(i)?)?;
                cx.host.open_uri(&url, false)?;
                Ok(ActionOutcome::done("Image copied — paste it into the search page"))
            }),
    );

    // Code: look up a symbol.
    r.action(
        action("core.code.search-symbol", "Search Symbol…")
            .icon("search")
            .group(G::Search)
            .accepts(caps::CODE)
            .priority(45)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .choices(|i, _| {
                let text = i.value.as_text().unwrap_or_default();
                symbols(&text).into_iter().map(|s| Choice { label: s.clone(), value: serde_json::json!(s) }).collect()
            })
            .run(|i, cx| {
                let sym = cx.param_str("choice").ok_or_else(|| LensError::InvalidInput("no symbol chosen".into()))?.to_string();
                let lang = match &i.value {
                    Value::Code(c) => c.language.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                search(cx, format!("{sym} {lang}").trim())
            }),
    );

    // Commits on code forges.
    r.action(
        action("core.commit.open", "Open Commit")
            .icon("git-commit")
            .group(G::Open)
            .accepts(caps::GIT_COMMIT)
            .priority(92)
            .key('o')
            .effects(OPEN_WEB)
            .requires(F::OPEN_URI)
            .run(|i, cx| open(cx, &text_of(i)?)),
    );
    r.action(
        action("core.commit.copy-url", "Copy Commit URL")
            .icon("link")
            .group(G::Copy)
            .accepts(caps::GIT_COMMIT)
            .priority(70)
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "commit URL")),
    );

    // Documents.
    let d = caps::DOCUMENT;
    r.action(
        action("core.document.copy-clean", "Copy Clean Text")
            .icon("type")
            .group(G::Copy)
            .accepts(d.clone())
            .priority(93)
            .key('t')
            .effects(COPY)
            .run(|i, cx| copy(cx, &crate::text::remove_line_breaks(&crate::text::plain(&text_of(i)?)), "clean text")),
    );
    r.action(
        action("core.document.pdf", "Create PDF")
            .icon("file-pdf")
            .group(G::Save)
            .accepts(d.clone())
            .produces(caps::FILE)
            .priority(60)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|_, cx| Ok(save_pdf(cx, "Selection")?.0)),
    );
    r.action(
        action("core.document.print", "Print Selection")
            .icon("printer")
            .group(G::System)
            .accepts(d.clone())
            .priority(45)
            .effects(SAVE | Effects::LAUNCHES_APP)
            .requires(F::SAVE_FILE | F::PRINT)
            .run(|_, cx| {
                let (_, path) = save_pdf(cx, "Print")?;
                cx.host.print(&path)?;
                Ok(ActionOutcome::done("Sent to the printer"))
            }),
    );
    r.action(
        action("core.document.save-image", "Save Page as Image")
            .icon("download")
            .group(G::Save)
            .accepts(d)
            .produces(caps::FILE)
            .priority(40)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|_, cx| save_image(cx, selection_image(cx)?, "Page", None)),
    );

    // Media frames and subtitles.
    let m = caps::MEDIA_FRAME;
    r.action(
        action("core.media.copy-frame", "Copy Frame")
            .icon("film")
            .group(G::Copy)
            .accepts(m.clone())
            .priority(90)
            .key('c')
            .effects(COPY)
            .requires(F::CLIPBOARD_IMAGE)
            .run(|i, cx| copy_image(cx, image_of(i)?)),
    );
    r.action(
        action("core.media.save-frame", "Save Frame")
            .icon("download")
            .group(G::Save)
            .accepts(m.clone())
            .produces(caps::FILE)
            .priority(80)
            .key('s')
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| save_image(cx, image_of(i)?, "Frame", None)),
    );
    r.action(
        action("core.media.pin-frame", "Pin Frame")
            .icon("pin")
            .group(G::Edit)
            .accepts(m)
            .priority(75)
            .key('p')
            .effects(Effects::PERSISTS)
            .requires(F::PIN)
            .run(|i, cx| {
                let Value::Image(v) = &i.value else { return Err(LensError::InvalidInput("not an image".into())) };
                cx.host.pin(v.image.clone(), v.origin)?;
                Ok(ActionOutcome::done("Pinned"))
            }),
    );
    r.action(
        action("core.subtitle.copy", "Copy Subtitle")
            .icon("captions")
            .group(G::Copy)
            .accepts(caps::SUBTITLE)
            .priority(95)
            .key('t')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "subtitle")),
    );
    r.action(
        action("core.subtitle.search", "Search Quote")
            .icon("search")
            .group(G::Search)
            .accepts(caps::SUBTITLE)
            .priority(55)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(|i, _| i.value.as_text().map(|t| format!("\"{}\"", crate::text::join_lines(&t))))
            .run(|i, cx| search(cx, &format!("\"{}\"", crate::text::join_lines(&text_of(i)?)))),
    );

    // Windows: move to a virtual desktop.
    r.action(
        action("core.window.workspace", "Move to Workspace…")
            .icon("layout")
            .group(G::System)
            .accepts(caps::WINDOW)
            .priority(30)
            .effects(Effects::WINDOW_CONTROL)
            .requires(F::WORKSPACES)
            .choices(|_, _| (1..=8).map(|n| Choice { label: format!("Workspace {n}"), value: serde_json::json!(n - 1) }).collect())
            .run(|i, cx| {
                let Value::Window(w) = &i.value else { return Err(LensError::InvalidInput("not a window".into())) };
                let n = cx.params.get("choice").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                cx.host.window_command(w, WindowCommand::MoveToWorkspace(n))?;
                Ok(ActionOutcome::done(format!("Moved to workspace {}", n + 1)))
            }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_structure() {
        let img = RgbaImage::from_pixel(96, 48, image::Rgba([200, 30, 30, 255]));
        let pdf = pdf_from_image(&img).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/MediaBox [0 0 72.00 36.00]"));
        // startxref points at the xref table.
        let sx: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[sx..].starts_with(b"xref"));
        // Every object offset in the table points at "N 0 obj".
        let table = String::from_utf8_lossy(&pdf[sx..]).into_owned();
        for (n, line) in table.lines().skip(3).take(5).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "object {}", n + 1);
        }
    }

    #[test]
    fn symbol_extraction() {
        let s = symbols("fn parse_config(path: &Path) -> Config {\n    let cfg = Config::load(path);\n    validate_config(&cfg);\n    cfg\n}");
        assert!(s.contains(&"Config".to_string()) && s.contains(&"parse_config".to_string()) && s.contains(&"validate_config".to_string()), "{s:?}");
        assert!(!s.contains(&"let".to_string()));
    }
}
