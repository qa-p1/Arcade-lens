//! OCR through the user's Tesseract (the `tesseract` command): a system
//! install, or the copy any Arcade app downloaded. Lens bundles no model.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use image::RgbaImage;
use lens_core::cancel::CancelToken;
use lens_core::geometry::Rect;
use lens_core::value::{TextLayout, TextLine, Word};
use lens_core::{LensError, Result};

pub struct TesseractEngine {
    exe: PathBuf,
    /// Language data installed for this Tesseract (`eng`, `deu`, …).
    installed: Vec<String>,
}

impl TesseractEngine {
    /// Checks `exe` runs and has language data.
    pub fn new(exe: PathBuf) -> Result<Self> {
        let out = command(&exe).arg("--list-langs").output().map_err(|e| LensError::Failed(format!("Tesseract: {e}")))?;
        let installed: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("List of") && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(String::from)
            .collect();
        if installed.is_empty() {
            return Err(LensError::Failed("Tesseract has no language data installed".into()));
        }
        Ok(Self { exe, installed })
    }

    /// Settings use short codes (`en`); Tesseract wants its own (`eng`).
    /// Languages without data are skipped rather than failing the capture.
    fn language_arg(&self, wanted: &[String]) -> String {
        let mut picked: Vec<&str> = wanted.iter().map(|l| tesseract_code(l)).filter(|c| self.installed.iter().any(|i| i == c)).collect();
        picked.dedup();
        if picked.is_empty() {
            return if self.installed.iter().any(|i| i == "eng") { "eng".into() } else { self.installed[0].clone() };
        }
        picked.join("+")
    }
}

fn command(exe: &PathBuf) -> Command {
    let mut cmd = Command::new(exe);
    // One thread: OpenMP only slows Tesseract down on screen-sized images.
    cmd.env("OMP_THREAD_LIMIT", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn tesseract_code(lang: &str) -> &str {
    match lang.trim() {
        "en" => "eng",
        "de" => "deu",
        "fr" => "fra",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "nl" => "nld",
        "pl" => "pol",
        "sv" => "swe",
        "fi" => "fin",
        "tr" => "tur",
        "ru" => "rus",
        "uk" => "ukr",
        "ar" => "ara",
        "hi" => "hin",
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => "chi_sim",
        other => other,
    }
}

impl super::OcrEngine for TesseractEngine {
    fn name(&self) -> &str {
        "Tesseract"
    }

    fn recognize(&self, image: &RgbaImage, languages: &[String], cancel: &CancelToken) -> Result<TextLayout> {
        let fail = |e: String| LensError::Failed(format!("Tesseract: {e}"));
        let mut png = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| fail(e.to_string()))?;
        let mut child = command(&self.exe)
            .args(["stdin", "stdout", "-l", &self.language_arg(languages), "--psm", "3", "tsv"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| fail(e.to_string()))?;
        let mut stdin = child.stdin.take().expect("piped");
        let writer = std::thread::spawn(move || stdin.write_all(&png));
        let mut stdout = child.stdout.take().expect("piped");
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            stdout.read_to_string(&mut s).map(|_| s)
        });
        let status = loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(LensError::Cancelled);
            }
            match child.try_wait().map_err(|e| fail(e.to_string()))? {
                Some(status) => break status,
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        let _ = writer.join();
        let tsv = reader.join().map_err(|_| fail("output reader panicked".into()))?.map_err(|e| fail(e.to_string()))?;
        if !status.success() {
            return Err(fail(format!("exited with {status}")));
        }
        Ok(parse_tsv(&tsv))
    }
}

/// Word rows (level 5) grouped into lines by block, paragraph and line number.
fn parse_tsv(tsv: &str) -> TextLayout {
    let mut lines: BTreeMap<(u32, u32, u32, u32), Vec<Word>> = BTreeMap::new();
    for row in tsv.lines().skip(1) {
        let f: Vec<&str> = row.splitn(12, '\t').collect();
        if f.len() < 12 || f[0] != "5" {
            continue;
        }
        let text = f[11].trim();
        let n = |i: usize| f[i].parse::<i64>().unwrap_or(0);
        if text.is_empty() {
            continue;
        }
        let key = (n(1) as u32, n(2) as u32, n(3) as u32, n(4) as u32);
        let bbox = Rect::new(n(6) as i32, n(7) as i32, n(8).max(0) as u32, n(9).max(0) as u32);
        let confidence = (f[10].parse::<f32>().unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        lines.entry(key).or_default().push(Word { text: text.to_string(), bbox, confidence });
    }
    let mut out = TextLayout::default();
    for words in lines.into_values() {
        let left = words.iter().map(|w| w.bbox.x as i64).min().unwrap_or(0);
        let top = words.iter().map(|w| w.bbox.y as i64).min().unwrap_or(0);
        let right = words.iter().map(|w| w.bbox.right()).max().unwrap_or(0);
        let bottom = words.iter().map(|w| w.bbox.bottom()).max().unwrap_or(0);
        let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        let bbox = Rect::new(left as i32, top as i32, (right - left).max(0) as u32, (bottom - top).max(0) as u32);
        out.lines.push(TextLine { text, bbox, words });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_group_into_lines_with_union_boxes() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   1\t1\t0\t0\t0\t0\t0\t0\t500\t80\t-1\t\n\
                   4\t1\t1\t1\t1\t0\t14\t28\t317\t28\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t14\t28\t100\t28\t96.5\tHello\n\
                   5\t1\t1\t1\t1\t2\t130\t30\t201\t26\t91\tArcade\n\
                   5\t1\t1\t1\t2\t1\t14\t70\t40\t20\t88\tnext\n";
        let layout = parse_tsv(tsv);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "Hello Arcade");
        assert_eq!(layout.lines[0].bbox, Rect::new(14, 28, 317, 28));
        assert!((layout.lines[0].words[0].confidence - 0.965).abs() < 1e-6);
        assert_eq!(layout.lines[1].text, "next");
    }

    #[test]
    fn short_language_codes_map_to_tesseract_names() {
        let engine = TesseractEngine { exe: PathBuf::new(), installed: vec!["eng".into(), "deu".into()] };
        assert_eq!(engine.language_arg(&["en".into(), "de".into()]), "eng+deu");
        assert_eq!(engine.language_arg(&["ja".into()]), "eng");
    }
}
