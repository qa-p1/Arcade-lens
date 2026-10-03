//! X11 backend: RandR monitors, per-monitor capture, EWMH windows.

use std::collections::HashMap;

use image::RgbaImage;
use lens_core::geometry::{MonitorInfo, Point, Rect};
use lens_core::host::WindowCommand;
use lens_core::selection::WindowInfo;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, ImageFormat, ImageOrder, InputFocus, MapState, StackMode, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::PlatformError;

pub fn available() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

struct X {
    conn: RustConnection,
    root: Window,
    atoms: HashMap<&'static str, Atom>,
}

const ATOMS: &[&str] = &[
    "_NET_CLIENT_LIST_STACKING",
    "_NET_CLIENT_LIST",
    "_NET_WM_NAME",
    "UTF8_STRING",
    "_NET_WM_PID",
    "_NET_FRAME_EXTENTS",
    "_NET_WM_STATE",
    "_NET_WM_STATE_HIDDEN",
    "_NET_WM_STATE_ABOVE",
    "_NET_WM_WINDOW_TYPE",
    "_NET_WM_WINDOW_TYPE_DOCK",
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_CLOSE_WINDOW",
    "_NET_MOVE_RESIZE_WINDOW",
    "_NET_WM_DESKTOP",
    "_NET_NUMBER_OF_DESKTOPS",
    "_NET_ACTIVE_WINDOW",
    "RESOURCE_MANAGER",
];

fn err(e: impl std::fmt::Display) -> PlatformError {
    PlatformError(format!("X11: {e}"))
}

impl X {
    fn connect() -> Result<X, PlatformError> {
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        let root = conn.setup().roots[screen].root;
        let cookies: Vec<_> = ATOMS.iter().map(|a| conn.intern_atom(false, a.as_bytes())).collect::<Result<_, _>>().map_err(err)?;
        let mut atoms = HashMap::new();
        for (name, c) in ATOMS.iter().zip(cookies) {
            atoms.insert(*name, c.reply().map_err(err)?.atom);
        }
        Ok(X { conn, root, atoms })
    }

    fn atom(&self, name: &str) -> Atom {
        self.atoms[name]
    }

    fn prop32(&self, win: Window, prop: &str, ty: impl Into<Atom>) -> Vec<u32> {
        self.conn
            .get_property(false, win, self.atom(prop), ty, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn prop_string(&self, win: Window, prop: Atom, ty: Atom) -> Option<String> {
        let r = self.conn.get_property(false, win, prop, ty, 0, 1024).ok()?.reply().ok()?;
        (!r.value.is_empty()).then(|| String::from_utf8_lossy(&r.value).trim_end_matches('\0').to_string())
    }

    fn scale(&self) -> f64 {
        // Xft.dpi is what toolkits (and winit) use for the scale factor on X11.
        self.prop_string(self.root, self.atom("RESOURCE_MANAGER"), AtomEnum::STRING.into())
            .and_then(|db| db.lines().find_map(|l| l.strip_prefix("Xft.dpi:").and_then(|v| v.trim().parse::<f64>().ok())))
            .map_or(1.0, |dpi| (dpi / 96.0).max(0.5))
    }

    fn monitors(&self) -> Vec<MonitorInfo> {
        let scale = self.scale();
        let reply = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok());
        let mut out: Vec<MonitorInfo> = reply
            .map(|r| {
                r.monitors
                    .iter()
                    .map(|m| {
                        let name = self
                            .conn
                            .get_atom_name(m.name)
                            .ok()
                            .and_then(|c| c.reply().ok())
                            .map(|n| String::from_utf8_lossy(&n.name).into_owned())
                            .unwrap_or_else(|| "Display".into());
                        MonitorInfo {
                            id: name.clone(),
                            name,
                            rect: Rect::new(m.x as i32, m.y as i32, m.width as u32, m.height as u32),
                            scale_factor: scale,
                            refresh_rate_hz: None,
                            is_primary: m.primary,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if out.is_empty() {
            if let Ok(g) = self.conn.get_geometry(self.root).map_err(err).and_then(|c| c.reply().map_err(err)) {
                out.push(MonitorInfo {
                    id: "screen".into(),
                    name: "Screen".into(),
                    rect: Rect::new(0, 0, g.width as u32, g.height as u32),
                    scale_factor: scale,
                    refresh_rate_hz: None,
                    is_primary: true,
                });
            }
        }
        if !out.iter().any(|m| m.is_primary) {
            if let Some(m) = out.first_mut() {
                m.is_primary = true;
            }
        }
        out
    }

    fn capture(&self, r: Rect) -> Result<RgbaImage, PlatformError> {
        let img = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, r.x as i16, r.y as i16, r.width as u16, r.height as u16, !0)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let setup = self.conn.setup();
        let bpp = setup.pixmap_formats.iter().find(|f| f.depth == img.depth).map_or(32, |f| f.bits_per_pixel);
        if bpp != 32 {
            return Err(PlatformError(format!("unsupported X11 pixel format: depth {} at {bpp} bpp", img.depth)));
        }
        let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let mut rgba = Vec::with_capacity((r.width * r.height * 4) as usize);
        for px in img.data.chunks_exact(4) {
            let (b, g, red) = if lsb { (px[0], px[1], px[2]) } else { (px[3], px[2], px[1]) };
            rgba.extend_from_slice(&[red, g, b, 255]);
        }
        RgbaImage::from_raw(r.width, r.height, rgba).ok_or_else(|| PlatformError("X11 returned a short image".into()))
    }

    fn rect_of(&self, win: Window) -> Option<Rect> {
        let g = self.conn.get_geometry(win).ok()?.reply().ok()?;
        let t = self.conn.translate_coordinates(win, self.root, 0, 0).ok()?.reply().ok()?;
        let ext = self.prop32(win, "_NET_FRAME_EXTENTS", AtomEnum::CARDINAL);
        let (l, r, top, b) = if ext.len() == 4 { (ext[0] as i32, ext[1] as i32, ext[2] as i32, ext[3] as i32) } else { (0, 0, 0, 0) };
        Some(Rect::new(t.dst_x as i32 - l, t.dst_y as i32 - top, (g.width as i32 + l + r).max(1) as u32, (g.height as i32 + top + b).max(1) as u32))
    }

    fn windows(&self, exclude_pid: Option<u32>) -> Vec<WindowInfo> {
        let mut ids = self.prop32(self.root, "_NET_CLIENT_LIST_STACKING", AtomEnum::WINDOW);
        let managed = !ids.is_empty();
        if !managed {
            // No EWMH window manager: fall back to mapped top-level windows.
            ids = self.conn.query_tree(self.root).ok().and_then(|c| c.reply().ok()).map(|t| t.children).unwrap_or_default();
        }
        let skip_types = [self.atom("_NET_WM_WINDOW_TYPE_DOCK"), self.atom("_NET_WM_WINDOW_TYPE_DESKTOP")];
        let mut out = Vec::new();
        // Stacking order is bottom-to-top; we want front-most first.
        for &w in ids.iter().rev() {
            let Some(attrs) = self.conn.get_window_attributes(w).ok().and_then(|c| c.reply().ok()) else { continue };
            if attrs.map_state != MapState::VIEWABLE || (!managed && attrs.override_redirect) {
                continue;
            }
            if self.prop32(w, "_NET_WM_STATE", AtomEnum::ATOM).contains(&self.atom("_NET_WM_STATE_HIDDEN")) {
                continue;
            }
            if self.prop32(w, "_NET_WM_WINDOW_TYPE", AtomEnum::ATOM).iter().any(|t| skip_types.contains(t)) {
                continue;
            }
            let pid = self.prop32(w, "_NET_WM_PID", AtomEnum::CARDINAL).first().copied();
            if exclude_pid.is_some() && pid == exclude_pid {
                continue;
            }
            let title = self
                .prop_string(w, self.atom("_NET_WM_NAME"), self.atom("UTF8_STRING"))
                .or_else(|| self.prop_string(w, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
                .unwrap_or_default();
            let class = self.prop_string(w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
            // WM_CLASS is "instance\0Class"; the class is the friendlier app name.
            let app_name = class.and_then(|c| c.split('\0').rfind(|s| !s.is_empty()).map(String::from));
            if title.is_empty() && app_name.is_none() {
                continue;
            }
            let Some(rect) = self.rect_of(w) else { continue };
            out.push(WindowInfo { id: format!("x11:{w:#x}"), title, app_name, rect, pid });
        }
        out
    }

    fn client_message(&self, win: Window, ty: &str, data: [u32; 5]) -> Result<(), PlatformError> {
        let ev = ClientMessageEvent::new(32, win, self.atom(ty), data);
        self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev).map_err(err)?;
        self.conn.flush().map_err(err)
    }
}

pub fn monitors() -> Result<Vec<MonitorInfo>, PlatformError> {
    Ok(X::connect()?.monitors())
}

pub fn capture_rect(r: Rect) -> Result<RgbaImage, PlatformError> {
    X::connect()?.capture(r)
}

pub fn capture_all() -> Result<Vec<(MonitorInfo, RgbaImage)>, PlatformError> {
    let x = X::connect()?;
    x.monitors().into_iter().map(|m| x.capture(m.rect).map(|img| (m, img))).collect()
}

pub fn cursor() -> Option<Point> {
    let x = X::connect().ok()?;
    let p = x.conn.query_pointer(x.root).ok()?.reply().ok()?;
    Some(Point { x: p.root_x as i32, y: p.root_y as i32 })
}

pub fn windows(exclude_pid: Option<u32>) -> Vec<WindowInfo> {
    X::connect().map(|x| x.windows(exclude_pid)).unwrap_or_default()
}

pub fn workspaces() -> u32 {
    X::connect().ok().and_then(|x| x.prop32(x.root, "_NET_NUMBER_OF_DESKTOPS", AtomEnum::CARDINAL).first().copied()).unwrap_or(0)
}

fn parse_id(w: &WindowInfo) -> Result<Window, PlatformError> {
    w.id.strip_prefix("x11:0x").and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or_else(|| PlatformError(format!("not an X11 window: {}", w.id)))
}

pub fn window_command(w: &WindowInfo, cmd: &WindowCommand) -> Result<(), PlatformError> {
    let x = X::connect()?;
    let win = parse_id(w)?;
    match cmd {
        // 2 = _NET_WM_STATE_TOGGLE, source 1 = application.
        WindowCommand::ToggleAlwaysOnTop => x.client_message(win, "_NET_WM_STATE", [2, x.atom("_NET_WM_STATE_ABOVE"), 0, 1, 0]),
        WindowCommand::Close => x.client_message(win, "_NET_CLOSE_WINDOW", [0, 1, 0, 0, 0]),
        WindowCommand::MoveToWorkspace(n) => x.client_message(win, "_NET_WM_DESKTOP", [*n, 1, 0, 0, 0]),
        WindowCommand::MoveToNextMonitor => {
            let monitors = x.monitors();
            if monitors.len() < 2 {
                return Err(PlatformError("only one monitor".into()));
            }
            let center = Point { x: w.rect.x + w.rect.width as i32 / 2, y: w.rect.y + w.rect.height as i32 / 2 };
            let cur = monitors.iter().position(|m| m.rect.contains(center)).unwrap_or(0);
            let (from, to) = (&monitors[cur].rect, &monitors[(cur + 1) % monitors.len()].rect);
            let nx = to.x + (w.rect.x - from.x).clamp(0, (to.width as i32 - w.rect.width as i32).max(0));
            let ny = to.y + (w.rect.y - from.y).clamp(0, (to.height as i32 - w.rect.height as i32).max(0));
            // Flags: x and y present (bits 8, 9), source = application (bit 12).
            let flags = (1 << 8) | (1 << 9) | (1 << 12);
            x.client_message(win, "_NET_MOVE_RESIZE_WINDOW", [flags, nx as u32, ny as u32, 0, 0])
        }
        WindowCommand::Record => Err(PlatformError("recording is handled by the application".into())),
    }
}

/// Gives keyboard focus to one of our own windows (overlays must receive
/// keys even when the window manager would not focus an undecorated window).
pub fn focus(window: u32) {
    if let Ok(x) = X::connect() {
        let _ = x.conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = x.conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME);
        let _ = x.conn.flush();
    }
}
