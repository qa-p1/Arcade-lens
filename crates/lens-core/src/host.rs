//! Platform services that actions call into.
//!
//! The application shell implements [`Host`] once per platform (clipboard,
//! launching, pins, terminals, window control). Actions stay platform
//! independent; anything a platform cannot do reports `Unsupported` and is
//! hidden from the palette through [`HostFeatures`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bitflags::bitflags;
use image::RgbaImage;

use crate::error::{LensError, Result};
use crate::geometry::Rect;
use crate::selection::WindowInfo;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct HostFeatures: u32 {
        const CLIPBOARD_TEXT   = 1 << 0;
        const CLIPBOARD_IMAGE  = 1 << 1;
        const OPEN_URI         = 1 << 2;
        const PRIVATE_BROWSING = 1 << 3;
        const OPEN_PATH        = 1 << 4;
        const REVEAL_PATH      = 1 << 5;
        const EDITOR           = 1 << 6;
        const TERMINAL         = 1 << 7;
        const SAVE_FILE        = 1 << 8;
        const PIN              = 1 << 9;
        const ANNOTATE         = 1 << 10;
        const SHARE            = 1 << 11;
        const SEND_TO_DEVICE   = 1 << 12;
        const QUICK_LOOK       = 1 << 13;
        const WINDOW_TOPMOST   = 1 << 14;
        const WINDOW_MOVE      = 1 << 15;
        const WINDOW_CLOSE     = 1 << 16;
        const RECORD_WINDOW    = 1 << 17;
        const MEASURE          = 1 << 18;
        const IMAGE_EDITOR     = 1 << 19;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPathMode {
    Default,
    Reveal,
    Editor,
    Terminal,
    QuickLook,
    ImageEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowCommand {
    ToggleAlwaysOnTop,
    MoveToNextMonitor,
    MoveToWorkspace(u32),
    Close,
    Record,
}

#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub suggested_name: String,
    pub bytes: Vec<u8>,
    /// Target directory; `None` means the configured default.
    pub directory: Option<PathBuf>,
    pub mime: String,
}

pub trait Host: Send + Sync {
    fn features(&self) -> HostFeatures;

    fn set_clipboard_text(&self, _text: &str) -> Result<()> {
        Err(unsupported("clipboard"))
    }
    fn set_clipboard_image(&self, _image: &RgbaImage) -> Result<()> {
        Err(unsupported("image clipboard"))
    }
    fn open_uri(&self, _uri: &str, _private: bool) -> Result<()> {
        Err(unsupported("opening links"))
    }
    fn open_path(&self, _path: &Path, _mode: OpenPathMode) -> Result<()> {
        Err(unsupported("opening files"))
    }
    /// Opens a terminal with `command` typed in. Only runs it if `execute`;
    /// callers must have obtained explicit confirmation first.
    fn terminal(&self, _cwd: Option<&Path>, _command: Option<&str>, _execute: bool) -> Result<()> {
        Err(unsupported("terminal"))
    }
    /// Writes a file without overwriting; returns the final path.
    fn save_file(&self, _request: SaveRequest) -> Result<PathBuf> {
        Err(unsupported("saving files"))
    }
    fn pin(&self, _image: Arc<RgbaImage>, _origin: Option<Rect>) -> Result<()> {
        Err(unsupported("pins"))
    }
    fn annotate(&self, _image: Arc<RgbaImage>) -> Result<()> {
        Err(unsupported("annotation"))
    }
    fn share_text(&self, _text: &str) -> Result<()> {
        Err(unsupported("sharing"))
    }
    fn share_image(&self, _image: &RgbaImage) -> Result<()> {
        Err(unsupported("sharing"))
    }
    fn send_to_device(&self, _text: Option<&str>, _image: Option<&RgbaImage>) -> Result<()> {
        Err(unsupported("connected devices"))
    }
    fn window_command(&self, _window: &WindowInfo, _command: WindowCommand) -> Result<()> {
        Err(unsupported("window control"))
    }
    fn notify(&self, _message: &str) {}
}

fn unsupported(what: &str) -> LensError {
    LensError::Unsupported(what.to_string())
}

/// A call recorded by [`RecordingHost`].
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    ClipboardText(String),
    ClipboardImage { width: u32, height: u32 },
    OpenUri { uri: String, private: bool },
    OpenPath { path: PathBuf, mode: OpenPathMode },
    Terminal { cwd: Option<PathBuf>, command: Option<String>, execute: bool },
    SaveFile { name: String, bytes: usize, mime: String },
    Pin { width: u32, height: u32 },
    Annotate,
    ShareText(String),
    ShareImage,
    SendToDevice { text: Option<String>, image: bool },
    Window { window: String, command: WindowCommand },
    Notify(String),
}

/// A host that performs nothing and records every call. Used for tests and
/// the CLI's dry-run mode.
pub struct RecordingHost {
    features: HostFeatures,
    calls: Mutex<Vec<HostCall>>,
}

impl RecordingHost {
    pub fn new(features: HostFeatures) -> Self {
        Self { features, calls: Mutex::new(Vec::new()) }
    }

    pub fn all() -> Self {
        Self::new(HostFeatures::all())
    }

    pub fn calls(&self) -> Vec<HostCall> {
        self.calls.lock().unwrap().clone()
    }

    pub fn take_calls(&self) -> Vec<HostCall> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn record(&self, c: HostCall) {
        self.calls.lock().unwrap().push(c);
    }
}

impl Host for RecordingHost {
    fn features(&self) -> HostFeatures {
        self.features
    }
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        self.record(HostCall::ClipboardText(text.into()));
        Ok(())
    }
    fn set_clipboard_image(&self, image: &RgbaImage) -> Result<()> {
        self.record(HostCall::ClipboardImage { width: image.width(), height: image.height() });
        Ok(())
    }
    fn open_uri(&self, uri: &str, private: bool) -> Result<()> {
        self.record(HostCall::OpenUri { uri: uri.into(), private });
        Ok(())
    }
    fn open_path(&self, path: &Path, mode: OpenPathMode) -> Result<()> {
        self.record(HostCall::OpenPath { path: path.into(), mode });
        Ok(())
    }
    fn terminal(&self, cwd: Option<&Path>, command: Option<&str>, execute: bool) -> Result<()> {
        self.record(HostCall::Terminal { cwd: cwd.map(Into::into), command: command.map(Into::into), execute });
        Ok(())
    }
    fn save_file(&self, r: SaveRequest) -> Result<PathBuf> {
        let path = r.directory.clone().unwrap_or_else(|| PathBuf::from("/dry-run")).join(&r.suggested_name);
        self.record(HostCall::SaveFile { name: r.suggested_name, bytes: r.bytes.len(), mime: r.mime });
        Ok(path)
    }
    fn pin(&self, image: Arc<RgbaImage>, _origin: Option<Rect>) -> Result<()> {
        self.record(HostCall::Pin { width: image.width(), height: image.height() });
        Ok(())
    }
    fn annotate(&self, _image: Arc<RgbaImage>) -> Result<()> {
        self.record(HostCall::Annotate);
        Ok(())
    }
    fn share_text(&self, text: &str) -> Result<()> {
        self.record(HostCall::ShareText(text.into()));
        Ok(())
    }
    fn share_image(&self, _image: &RgbaImage) -> Result<()> {
        self.record(HostCall::ShareImage);
        Ok(())
    }
    fn send_to_device(&self, text: Option<&str>, image: Option<&RgbaImage>) -> Result<()> {
        self.record(HostCall::SendToDevice { text: text.map(Into::into), image: image.is_some() });
        Ok(())
    }
    fn window_command(&self, window: &WindowInfo, command: WindowCommand) -> Result<()> {
        self.record(HostCall::Window { window: window.id.clone(), command });
        Ok(())
    }
    fn notify(&self, message: &str) {
        self.record(HostCall::Notify(message.into()));
    }
}
