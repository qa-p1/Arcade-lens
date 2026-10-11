# Arcade Lens

Arcade Lens lets you select something visible on your screen and choose a useful next step. It can recognize text, links, codes, colors, tables, and other common screen content, then offer relevant actions beside the selection.

## Install

Install Lens from **Arcade Tools → Apps**, or install it by hand using the [getting-started guide](docs/user/getting-started.md).

## Works with other Arcade apps

Lens can send a selection to Box, Clipboard, Look, Shelf, Wheel, or Find. See [how Lens works with other Arcade apps](docs/user/with-other-apps.md).

Read the [Arcade Lens user guide](docs/user/) for setup, features, shortcuts, and help.

## Development

### Architecture and recognition

Lens captures a frozen image and runs built-in recognizers over it. There are 26 recognizers and 156 built-in actions; the result is a ranked, keyboard-addressable palette. Recognition is local. See [architecture](docs/ARCHITECTURE.md) for the dataflow and safety model.

A selection can produce several findings at once. For example, a QR image may offer image actions, QR decoding, and actions for a URL in its payload. OCR text can lead to URL, command, error, table, path, date, time, quantity, and other recognizers.

### Plugins

Third-party recognizers and actions run out of process with declared permissions. They remain disabled until a user enables them. Copy a plugin directory into the folder opened from **Settings → Plugins → Open plugins folder**, then enable it in Settings.

See [plugin manifests, protocol, and security](docs/PLUGINS.md) and the [ISBN example](examples/plugins/isbn).

### Command line and source install

The launcher commands reach the running instance:

    arcade-lens                 # open Settings, or start Lens
    arcade-lens --capture       # start a capture
    arcade-lens --background    # start without a window
    arcade-lens --restart
    arcade-lens --quit

For a Linux source install, run **./scripts/install.sh**. It builds Lens, installs it in **~/.local/bin**, starts it, and registers the tray and application-menu entries. Run it again after pulling changes to rebuild and restart. Linux OCR uses Tesseract with English language data; the explicit Settings download is a private copy shared by Arcade apps.

### Platform implementation

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Capture | X11, per-monitor scale | screenshot portal | xcap | xcap, Screen Recording permission |
| Global shortcut | Native | Hyprland runtime binding; bind the command elsewhere | Native | Native |
| Window detection and commands | EWMH | Not available | Available | Detection only |
| OCR | Tesseract | Tesseract | Windows.Media.Ocr | Apple Vision |
| Live pins | Available | Not available | Available | Available |
| Window recording | FFmpeg required | Not available | FFmpeg required | Not available |
| Tray | StatusNotifierItem | StatusNotifierItem | Notification area | Menu bar |
| Login startup | XDG autostart | XDG autostart | Run key | LaunchAgent |

Linux Hyprland actions are used daily. Windows and macOS are built and tested in CI but have not been run interactively. See [status](docs/STATUS.md) for the current evidence and limits.

### Build and release

Requires Rust 1.95 or newer. CI checks pushes on Linux, Windows, and macOS. Pushes to main build the packages from **packaging/**. When a version has no release yet, CI tags it and publishes it as the latest release; otherwise it replaces the nightly prerelease. Pushing a version tag or publishing a GitHub release builds that tag and attaches its packages. Packages include **arcade-release.json** and **SHA256SUMS.txt** for Arcade Tools.

    cargo build --release
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings

Arcade Link is pinned to a release tag in Cargo.toml. A local dependency override can point to a checkout for development.

Configuration lives in **settings.toml** at the path shown in Settings → About. **ARCADE_LENS_HOME** overrides the location. Isolated profiles and builds run from target do not register a login item or application-menu entry.

### Known limitations

- Tesseract can misread small interface text such as lowercase “l” and the digit “1”. Lens enlarges small selections before OCR, but it cannot recover characters the engine did not detect.
- On Wayland, the screenshot portal provides the frozen image; Lens has no window list or window commands, and live pins and window recording are unavailable. With Hyprland permission enforcement enabled, allow the screenshot tools in `hyprland.lua` to avoid repeated prompts:

      hl.permission("/usr/(bin|local/bin)/grim", "screencopy", "allow")
      hl.permission("/usr/(lib|libexec|lib64)/xdg-desktop-portal-hyprland", "screencopy", "allow")

- A system share sheet and accessibility-based UI inspection are not implemented. macOS window commands need an Accessibility API integration that is not implemented yet.
- The Send to my devices action uses Arcade Clipboard when available; on Linux, KDE Connect is the fallback.
- Debug logging with **LENS_DEBUG=1** does not include selected screen content.

### Technical references

- [Arcade Link](docs/ARCADE_LINK.md): actions, capture modes, and peer integration.
- [Plugins](docs/PLUGINS.md): plugin API and permissions.
- [Status](docs/STATUS.md): verification record and remaining limits.
- [Vendored files](VENDORED): source and license details for copied files.

The project license is MIT OR Apache-2.0.
