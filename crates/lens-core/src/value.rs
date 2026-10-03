//! Typed payloads carried by findings and passed between chained actions.
//!
//! Values are deliberately typed instead of being plain strings: an action
//! that converts a table to CSV receives a [`Table`], not OCR text it has to
//! re-parse, and the chain engine can reject incompatible steps up front.

use std::borrow::Cow;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::geometry::Rect;
use crate::selection::WindowInfo;

#[derive(Clone)]
pub enum Value {
    Image(ImageValue),
    Text(TextValue),
    Url(UrlValue),
    Email(String),
    Phone(PhoneValue),
    Address(String),
    DateTime(DateTimeValue),
    Timecode(TimecodeValue),
    Command(CommandValue),
    Code(CodeValue),
    Error(ErrorValue),
    Path(PathValue),
    Ip(IpValue),
    Domain(String),
    Hash(HashValue),
    Uuid(UuidValue),
    Secret(SecretValue),
    Color(Rgb),
    Palette(Vec<PaletteColor>),
    Table(Table),
    Barcode(BarcodeValue),
    Currency(CurrencyValue),
    Quantity(QuantityValue),
    Coordinates(GeoValue),
    Window(WindowInfo),
    Geometry(GeometryValue),
    File(FileValue),
    /// Plugin-defined payload.
    Custom { type_name: String, data: serde_json::Value, text: Option<String> },
}

impl Value {
    pub fn text(s: impl Into<String>) -> Self {
        Value::Text(TextValue { text: s.into(), layout: None })
    }

    /// The textual representation used by copy actions and text transforms.
    /// Secrets intentionally return the *raw* value: only explicit, local
    /// actions (Copy) consume this, and the safety policy guards the rest.
    pub fn as_text(&self) -> Option<Cow<'_, str>> {
        Some(match self {
            Value::Image(_) | Value::Window(_) => return None,
            Value::Text(t) => Cow::Borrowed(&t.text),
            Value::Url(u) => Cow::Borrowed(&u.url),
            Value::Email(s) | Value::Address(s) | Value::Domain(s) => Cow::Borrowed(s),
            Value::Phone(p) => Cow::Borrowed(&p.raw),
            Value::DateTime(d) => Cow::Borrowed(&d.raw),
            Value::Timecode(t) => Cow::Borrowed(&t.raw),
            Value::Command(c) => Cow::Borrowed(&c.command),
            Value::Code(c) => Cow::Borrowed(&c.text),
            Value::Error(e) => Cow::Borrowed(&e.raw),
            Value::Path(p) => Cow::Borrowed(&p.raw),
            Value::Ip(ip) => Cow::Owned(ip.display()),
            Value::Hash(h) => Cow::Borrowed(&h.hex),
            Value::Uuid(u) => Cow::Borrowed(&u.text),
            Value::Secret(s) => Cow::Borrowed(s.raw.expose()),
            Value::Color(c) => Cow::Owned(c.hex()),
            Value::Palette(p) => Cow::Owned(p.iter().map(|c| c.color.hex()).collect::<Vec<_>>().join("\n")),
            Value::Table(t) => Cow::Owned(t.to_tsv()),
            Value::Barcode(b) => Cow::Borrowed(&b.payload),
            Value::Currency(c) => Cow::Borrowed(&c.raw),
            Value::Quantity(q) => Cow::Borrowed(&q.raw),
            Value::Coordinates(g) => Cow::Owned(g.display()),
            Value::Geometry(g) => Cow::Owned(format!("{} × {}", g.rect.width, g.rect.height)),
            Value::File(f) => Cow::Owned(f.path.display().to_string()),
            Value::Custom { text, .. } => return text.as_deref().map(Cow::Borrowed),
        })
    }

    pub fn as_image(&self) -> Option<&Arc<RgbaImage>> {
        match self {
            Value::Image(i) => Some(&i.image),
            _ => None,
        }
    }

    /// Key used to merge duplicate findings (e.g. the same URL found by OCR
    /// and inside a QR code).
    pub fn dedup_key(&self) -> Option<String> {
        match self {
            Value::Image(_) | Value::Geometry(_) => None,
            Value::Url(u) => Some(u.url.trim_end_matches('/').to_ascii_lowercase()),
            Value::Email(e) => Some(e.to_ascii_lowercase()),
            Value::Domain(d) => Some(d.to_ascii_lowercase()),
            Value::Phone(p) => Some(p.digits.clone()),
            Value::Window(w) => Some(w.id.clone()),
            other => other.as_text().map(|t| t.into_owned()),
        }
    }
}

impl fmt::Debug for Value {
    /// Debug output never includes secret material or image pixels.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Image(i) => write!(f, "Image({}x{})", i.image.width(), i.image.height()),
            Value::Secret(s) => write!(f, "Secret({:?}, {})", s.kind, s.masked),
            Value::Color(c) => write!(f, "Color({})", c.hex()),
            Value::Window(w) => write!(f, "Window({:?})", w.title),
            other => write!(f, "{:?}", other.as_text().unwrap_or_default()),
        }
    }
}

#[derive(Clone)]
pub struct ImageValue {
    pub image: Arc<RgbaImage>,
    /// Where the pixels came from, in virtual-desktop physical pixels.
    pub origin: Option<Rect>,
}

#[derive(Clone, Debug)]
pub struct TextValue {
    pub text: String,
    /// OCR geometry, when the text came from pixels. Used for table and
    /// code-gutter reconstruction.
    pub layout: Option<Arc<TextLayout>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    /// Relative to the selection's top-left corner.
    pub bbox: Rect,
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub bbox: Rect,
    pub confidence: f32,
}

impl TextLayout {
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlValue {
    pub url: String,
    pub scheme: String,
    pub host: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneValue {
    pub raw: String,
    /// Digits only, with a leading `+` when an international prefix was present.
    pub digits: String,
}

impl PhoneValue {
    pub fn tel_uri(&self) -> String {
        format!("tel:{}", self.digits)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DateTimeValue {
    pub raw: String,
    /// All plausible interpretations, most likely first (per locale settings).
    pub candidates: Vec<DateTimeCandidate>,
}

impl DateTimeValue {
    /// True when interpretations disagree, so actions must ask the user.
    pub fn is_ambiguous(&self) -> bool {
        self.candidates.len() > 1
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DateTimeCandidate {
    /// ISO-8601 calendar date, if a date was present.
    pub date: Option<String>,
    /// 24h `HH:MM[:SS]`, if a time was present.
    pub time: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimecodeValue {
    pub raw: String,
    pub seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandValue {
    pub command: String,
    pub risks: Vec<CommandRisk>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandRisk {
    Privileged,
    PackageManagement,
    Deletion,
    FilesystemModification,
    PipeToInterpreter,
    NetworkDownload,
    ShellScript,
    Redirection,
    Chained,
    /// Text may contain OCR substitutions (e.g. `l` vs `1`) in sensitive spots.
    LowOcrConfidence,
}

impl CommandRisk {
    pub fn describe(&self) -> &'static str {
        match self {
            CommandRisk::Privileged => "runs with elevated privileges",
            CommandRisk::PackageManagement => "installs or removes packages",
            CommandRisk::Deletion => "deletes files",
            CommandRisk::FilesystemModification => "modifies files or permissions",
            CommandRisk::PipeToInterpreter => "pipes data into an interpreter",
            CommandRisk::NetworkDownload => "downloads from the network",
            CommandRisk::ShellScript => "executes a script",
            CommandRisk::Redirection => "writes output to a file",
            CommandRisk::Chained => "runs several commands",
            CommandRisk::LowOcrConfidence => "was read from pixels with low confidence",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CodeValue {
    pub text: String,
    /// The code with a detected line-number gutter removed.
    pub without_line_numbers: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorValue {
    pub raw: String,
    pub kind: String,
    /// The single most informative line.
    pub headline: String,
    /// Headline with machine-specific noise and sensitive values removed.
    pub cleaned_query: String,
    pub stack_trace: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathValue {
    pub raw: String,
    /// Path with `~` and `file://` resolved, when resolvable locally.
    pub resolved: Option<String>,
    pub flavor: PathFlavor,
    pub exists: Option<PathKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathFlavor {
    Unix,
    HomeRelative,
    Relative,
    Windows,
    Unc,
    FileUrl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathKind {
    File,
    Directory,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpValue {
    pub addr: IpAddr,
    pub port: Option<u16>,
    pub scope: IpScope,
}

impl IpValue {
    pub fn display(&self) -> String {
        match (self.addr, self.port) {
            (IpAddr::V6(a), Some(p)) => format!("[{a}]:{p}"),
            (a, Some(p)) => format!("{a}:{p}"),
            (a, None) => a.to_string(),
        }
    }

    pub fn url_host(&self) -> String {
        match (self.addr, self.port) {
            (IpAddr::V6(a), Some(p)) => format!("[{a}]:{p}"),
            (IpAddr::V6(a), None) => format!("[{a}]"),
            (a, Some(p)) => format!("{a}:{p}"),
            (a, None) => a.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpScope {
    Loopback,
    Private,
    LinkLocal,
    Public,
    Unspecified,
    Multicast,
    Documentation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashValue {
    pub hex: String,
    /// Plausible formats; never a single guess when the length is ambiguous.
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UuidValue {
    pub text: String,
    pub version: Option<u8>,
}

#[derive(Clone)]
pub struct SecretValue {
    pub kind: String,
    pub masked: String,
    pub raw: Sensitive,
}

/// A string that never appears in `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct Sensitive(String);

impl Sensitive {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Sensitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaletteColor {
    pub color: Rgb,
    /// Fraction of sampled pixels represented by this color.
    pub coverage: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub has_header: bool,
}

impl Table {
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    fn cell(&self, r: usize, c: usize) -> &str {
        self.rows.get(r).and_then(|row| row.get(c)).map_or("", String::as_str)
    }

    pub fn to_delimited(&self, sep: char) -> String {
        let cols = self.columns();
        let mut out = String::new();
        for r in 0..self.rows.len() {
            let cells: Vec<String> = (0..cols)
                .map(|c| {
                    let v = self.cell(r, c);
                    if sep == ',' && (v.contains(',') || v.contains('"') || v.contains('\n')) {
                        format!("\"{}\"", v.replace('"', "\"\""))
                    } else if sep == '\t' {
                        v.replace(['\t', '\n'], " ")
                    } else {
                        v.to_string()
                    }
                })
                .collect();
            out.push_str(&cells.join(&sep.to_string()));
            out.push('\n');
        }
        out
    }

    pub fn to_csv(&self) -> String {
        self.to_delimited(',')
    }

    pub fn to_tsv(&self) -> String {
        self.to_delimited('\t')
    }

    pub fn to_markdown(&self) -> String {
        let cols = self.columns();
        if cols == 0 {
            return String::new();
        }
        let esc = |s: &str| s.replace('|', "\\|");
        let row = |r: usize| format!("| {} |", (0..cols).map(|c| esc(self.cell(r, c))).collect::<Vec<_>>().join(" | "));
        let mut lines = Vec::new();
        let body_start = if self.has_header {
            lines.push(row(0));
            1
        } else {
            lines.push(format!("| {} |", (1..=cols).map(|c| format!("Column {c}")).collect::<Vec<_>>().join(" | ")));
            0
        };
        lines.push(format!("|{}|", vec!["---"; cols].join("|")));
        for r in body_start..self.rows.len() {
            lines.push(row(r));
        }
        lines.join("\n") + "\n"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarcodeValue {
    pub format: String,
    pub payload: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrencyValue {
    pub raw: String,
    pub amount: f64,
    /// ISO 4217 code when determinable; symbols like `$` are ambiguous.
    pub code: Option<String>,
    pub symbol: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuantityValue {
    pub raw: String,
    pub amount: f64,
    pub unit: String,
    pub dimension: String,
    /// Precomputed local conversions: (formatted value, unit symbol).
    pub conversions: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoValue {
    pub lat: f64,
    pub lon: f64,
}

impl GeoValue {
    pub fn display(&self) -> String {
        format!("{:.6}, {:.6}", self.lat, self.lon).replace(".000000", ".0")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometryValue {
    pub rect: Rect,
    pub scale_factor: f64,
    pub background: Option<Rgb>,
    pub foreground: Option<Rgb>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileValue {
    pub path: std::path::PathBuf,
    pub mime: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table {
            rows: vec![
                vec!["NAME".into(), "PRICE".into(), "QTY".into()],
                vec!["SSD".into(), "₹7,999".into(), "2".into()],
                vec!["RAM".into(), "₹4,499".into(), "4".into()],
            ],
            has_header: true,
        }
    }

    #[test]
    fn table_exports() {
        let t = table();
        assert_eq!(t.to_csv(), "NAME,PRICE,QTY\nSSD,\"₹7,999\",2\nRAM,\"₹4,499\",4\n");
        assert_eq!(t.to_tsv(), "NAME\tPRICE\tQTY\nSSD\t₹7,999\t2\nRAM\t₹4,499\t4\n");
        assert_eq!(t.to_markdown(), "| NAME | PRICE | QTY |\n|---|---|---|\n| SSD | ₹7,999 | 2 |\n| RAM | ₹4,499 | 4 |\n");
    }

    #[test]
    fn secrets_do_not_leak_through_debug() {
        let v = Value::Secret(SecretValue { kind: "github-token".into(), masked: "ghp_…abcd".into(), raw: Sensitive::new("ghp_supersecretvalue1234abcd") });
        let dbg = format!("{v:?}");
        assert!(!dbg.contains("supersecret"), "{dbg}");
    }
}
