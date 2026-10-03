//! The desktop [`Host`]: real clipboard, launching, terminals and files.
//!
//! Overlay-only services (pins, annotation, measure mode, window control)
//! belong to the overlay shell and are reported as unsupported here, so the
//! palette simply doesn't offer them.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use image::RgbaImage;
use lens_core::host::{Host, HostFeatures, OpenPathMode, SaveRequest};
use lens_core::{LensError, Result, Settings};

pub struct DesktopHost {
    settings: Settings,
    collections: PathBuf,
    features: HostFeatures,
    /// Long-lived processes own the clipboard themselves; short-lived CLI
    /// runs hand it to wl-copy/xclip so it survives process exit.
    long_lived: bool,
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(cmd)).find(|p| p.is_file())
}

fn spawn(program: &str, args: &[&str]) -> Result<()> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| LensError::Failed(format!("{program}: {e}")))
}

/// Single-quotes a string for POSIX shells.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn expand_home(p: &Path) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => home().join(rest),
        Err(_) => p.to_path_buf(),
    }
}

fn home() -> PathBuf {
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

/// Picks `name`, or `name (2)`, `name (3)`… so nothing is ever overwritten.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("unbounded")
}

/// Terminal emulators and how to make each run `bash -c <script>`.
const TERMINALS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("foot", &[]),
    ("xfce4-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("xterm", &["-e"]),
];

impl DesktopHost {
    pub fn new(settings: Settings, collections: PathBuf) -> Self {
        let mut f =
            HostFeatures::OPEN_URI | HostFeatures::OPEN_PATH | HostFeatures::SAVE_FILE | HostFeatures::EDITOR | HostFeatures::PERSIST | HostFeatures::DOWNLOAD;
        f |= HostFeatures::CLIPBOARD_TEXT | HostFeatures::CLIPBOARD_IMAGE | HostFeatures::CLIPBOARD_READ;
        if cfg!(any(target_os = "macos", windows)) || which("dbus-send").is_some() {
            f |= HostFeatures::REVEAL_PATH;
        }
        if cfg!(target_os = "macos") {
            f |= HostFeatures::QUICK_LOOK;
        }
        if settings.browser.as_deref().is_some_and(|b| private_flag(b).is_some()) {
            f |= HostFeatures::PRIVATE_BROWSING;
        }
        let host = Self { settings, collections, features: f, long_lived: false };
        let features = if host.terminal_program().is_some() { f | HostFeatures::TERMINAL } else { f };
        Self { features, ..host }
    }

    pub fn long_lived(mut self) -> Self {
        self.long_lived = true;
        self
    }

    fn terminal_program(&self) -> Option<(String, Vec<String>)> {
        if cfg!(any(target_os = "macos", windows)) {
            return Some((String::new(), vec![]));
        }
        if let Some(t) = self.settings.terminal.clone().or_else(|| std::env::var("TERMINAL").ok()) {
            let mut parts = t.split_whitespace().map(String::from);
            let program = parts.next()?;
            let mut args: Vec<String> = parts.collect();
            if args.is_empty() {
                let name = Path::new(&program).file_name()?.to_string_lossy().to_string();
                args = TERMINALS.iter().find(|(n, _)| *n == name).map(|(_, a)| a.iter().map(|s| s.to_string()).collect()).unwrap_or_else(|| vec!["-e".into()]);
            }
            return Some((program, args));
        }
        TERMINALS.iter().find(|(n, _)| which(n).is_some()).map(|(n, a)| (n.to_string(), a.iter().map(|s| s.to_string()).collect()))
    }

    fn save_dir(&self, requested: Option<PathBuf>) -> PathBuf {
        requested
            .map(|d| expand_home(&d))
            .or_else(|| self.settings.screenshot_dir.as_deref().map(expand_home))
            .or_else(|| directories::UserDirs::new().and_then(|u| u.picture_dir().map(|p| p.join("Arcade Lens"))))
            .unwrap_or_else(|| home().join("Arcade Lens"))
    }

    fn pipe_to(&self, program: &str, args: &[&str], bytes: &[u8]) -> Result<()> {
        let mut child = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        child.stdin.take().expect("piped").write_all(bytes)?;
        let status = child.wait()?;
        status.success().then_some(()).ok_or_else(|| LensError::Failed(format!("{program} exited with {status}")))
    }

    /// On Linux the clipboard belongs to a running process; a short-lived CLI
    /// would lose it on exit, so hand it to wl-copy/xclip when available.
    fn linux_clipboard(&self, mime: &str, bytes: &[u8]) -> Option<Result<()>> {
        if !cfg!(target_os = "linux") || self.long_lived {
            return None;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wl-copy").is_some() {
            return Some(self.pipe_to("wl-copy", &["--type", mime], bytes));
        }
        if which("xclip").is_some() {
            return Some(self.pipe_to("xclip", &["-selection", "clipboard", "-t", mime, "-i"], bytes));
        }
        None
    }
}

fn private_flag(browser: &str) -> Option<&'static str> {
    let b = browser.to_ascii_lowercase();
    if b.contains("firefox") || b.contains("librewolf") {
        Some("--private-window")
    } else if b.contains("edge") {
        Some("--inprivate")
    } else if b.contains("chrom") || b.contains("brave") || b.contains("vivaldi") {
        Some("--incognito")
    } else {
        None
    }
}

fn open_default(target: &str) -> Result<()> {
    if cfg!(target_os = "macos") {
        spawn("open", &[target])
    } else if cfg!(windows) {
        spawn("rundll32", &["url.dll,FileProtocolHandler", target])
    } else {
        spawn("xdg-open", &[target])
    }
}

impl Host for DesktopHost {
    fn features(&self) -> HostFeatures {
        self.features
    }

    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        if let Some(r) = self.linux_clipboard("text/plain;charset=utf-8", text.as_bytes()) {
            return r;
        }
        arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| LensError::Failed(format!("clipboard: {e}")))
    }

    fn set_clipboard_image(&self, image: &RgbaImage) -> Result<()> {
        let png = lens_actions::util::encode(image, lens_core::settings::ImageFormat::Png)?;
        if let Some(r) = self.linux_clipboard("image/png", &png) {
            return r;
        }
        let data = arboard::ImageData { width: image.width() as usize, height: image.height() as usize, bytes: image.as_raw().into() };
        arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| LensError::Failed(format!("clipboard: {e}")))
    }

    fn clipboard_text(&self) -> Result<String> {
        arboard::Clipboard::new().and_then(|mut c| c.get_text()).map_err(|e| LensError::Failed(format!("clipboard: {e}")))
    }

    fn open_uri(&self, uri: &str, private: bool) -> Result<()> {
        match (&self.settings.browser, private) {
            (Some(b), true) => {
                let flag = private_flag(b).ok_or_else(|| LensError::Unsupported("private browsing for this browser".into()))?;
                spawn(b, &[flag, uri])
            }
            (Some(b), false) if uri.starts_with("http") => spawn(b, &[uri]),
            _ => open_default(uri),
        }
    }

    fn open_path(&self, path: &Path, mode: OpenPathMode) -> Result<()> {
        let p = path.to_string_lossy();
        match mode {
            OpenPathMode::Default | OpenPathMode::ImageEditor => open_default(&p),
            OpenPathMode::Editor => match &self.settings.editor {
                Some(e) => spawn(e, &[&p]),
                None => open_default(&p),
            },
            OpenPathMode::Reveal if cfg!(target_os = "macos") => spawn("open", &["-R", &p]),
            OpenPathMode::Reveal if cfg!(windows) => spawn("explorer", &[&format!("/select,{p}")]),
            OpenPathMode::Reveal => {
                let uri = format!("array:string:file://{p}");
                spawn(
                    "dbus-send",
                    &[
                        "--session",
                        "--dest=org.freedesktop.FileManager1",
                        "--type=method_call",
                        "/org/freedesktop/FileManager1",
                        "org.freedesktop.FileManager1.ShowItems",
                        &uri,
                        "string:",
                    ],
                )
            }
            OpenPathMode::Terminal => self.terminal(Some(path), None, false),
            OpenPathMode::QuickLook if cfg!(target_os = "macos") => spawn("qlmanage", &["-p", &p]),
            OpenPathMode::QuickLook => Err(LensError::Unsupported("Quick Look".into())),
        }
    }

    fn terminal(&self, cwd: Option<&Path>, command: Option<&str>, execute: bool) -> Result<()> {
        if cfg!(windows) {
            // PowerShell cannot pre-fill its prompt, so "paste" means: copy, then open.
            let mut args = vec!["-NoExit".to_string()];
            match (command, execute) {
                (Some(c), true) => args.extend(["-Command".into(), c.to_string()]),
                (Some(c), false) => {
                    self.set_clipboard_text(c)?;
                    self.notify("Command copied — paste it with Ctrl+V");
                }
                _ => {}
            }
            let mut cmd = Command::new("powershell");
            cmd.args(&args);
            if let Some(d) = cwd {
                cmd.current_dir(d);
            }
            return cmd.spawn().map(drop).map_err(|e| LensError::Failed(e.to_string()));
        }
        // bash's `read -e -i` pre-fills an editable prompt: the user sees the
        // exact command and nothing runs until they press Enter.
        let script = match (command, execute) {
            (Some(c), true) => format!("{c}; exec \"${{SHELL:-bash}}\""),
            (Some(c), false) => {
                format!("read -r -e -i {} -p '$ ' __lens_cmd && history -s \"$__lens_cmd\" && eval \"$__lens_cmd\"; exec \"${{SHELL:-bash}}\"", sh_quote(c))
            }
            (None, _) => "exec \"${SHELL:-bash}\"".to_string(),
        };
        if cfg!(target_os = "macos") {
            let cd = cwd.map(|d| format!("cd {}; ", sh_quote(&d.to_string_lossy()))).unwrap_or_default();
            let full = format!("{cd}bash -c {}", sh_quote(&script));
            let apple = full.replace('\\', "\\\\").replace('"', "\\\"");
            return spawn(
                "osascript",
                &["-e", &format!("tell application \"Terminal\" to do script \"{apple}\""), "-e", "tell application \"Terminal\" to activate"],
            );
        }
        let (program, mut args) =
            self.terminal_program().ok_or_else(|| LensError::Unsupported("no terminal emulator found; set `terminal` in settings".into()))?;
        args.extend(["bash".into(), "-c".into(), script]);
        let mut cmd = Command::new(&program);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        cmd.spawn().map(drop).map_err(|e| LensError::Failed(format!("{program}: {e}")))
    }

    fn save_file(&self, r: SaveRequest) -> Result<PathBuf> {
        let dir = self.save_dir(r.directory);
        fs::create_dir_all(&dir)?;
        let name: String = r.suggested_name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
        let path = unique_path(&dir, &name);
        // create_new: never overwrite, even if a file appeared since unique_path.
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        f.write_all(&r.bytes)?;
        Ok(path)
    }

    fn download(&self, url: &str) -> Result<PathBuf> {
        let bytes = crate::net::get_bytes(url, 512 * 1024 * 1024).map_err(|e| LensError::Failed(format!("download: {e}")))?;
        let name =
            url.split(['?', '#']).next().and_then(|u| u.rsplit('/').find(|s| !s.is_empty())).filter(|n| !n.contains(':')).unwrap_or("download").to_string();
        let dir = directories::UserDirs::new().and_then(|u| u.download_dir().map(Path::to_path_buf)).unwrap_or_else(home);
        self.save_file(SaveRequest { suggested_name: name, bytes, directory: Some(dir), mime: "application/octet-stream".into() })
    }

    fn persist(&self, collection: &str, entry: serde_json::Value) -> Result<()> {
        fs::create_dir_all(&self.collections)?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.collections.join(format!("{collection}.jsonl")))?;
        writeln!(f, "{entry}")?;
        Ok(())
    }

    fn notify(&self, message: &str) {
        eprintln!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn never_overwrites() {
        let dir = std::env::temp_dir().join(format!("lens-host-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let host = DesktopHost::new(Settings::default(), dir.join("c"));
        let req = || SaveRequest { suggested_name: "shot.png".into(), bytes: vec![1, 2, 3], directory: Some(dir.clone()), mime: "image/png".into() };
        let a = host.save_file(req()).unwrap();
        let b = host.save_file(req()).unwrap();
        assert_eq!(a.file_name().unwrap(), "shot.png");
        assert_eq!(b.file_name().unwrap(), "shot (2).png");
        fs::remove_dir_all(dir).ok();
    }
}
