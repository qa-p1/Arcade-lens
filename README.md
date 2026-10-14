<p align="center">
  <img src="assets/logo.svg" width="96" alt="Arcade Lens logo">
</p>

<h1 align="center">Arcade Lens</h1>

<p align="center">Select anything on screen and act on it.</p>

<p align="center">
  <a href="https://github.com/qa-p1/Arcade-lens/releases/latest"><img src="https://img.shields.io/github/v/release/qa-p1/Arcade-lens" alt="Latest release"></a>
  <a href="https://github.com/qa-p1/Arcade-lens/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/qa-p1/Arcade-lens/ci.yml?branch=main&label=CI" alt="CI status"></a>
</p>

---

Arcade Lens lets you select something visible on your screen and choose a useful next step. It can recognize text, links, codes, colors, tables, and other common screen content, then offer relevant actions beside the selection. Recognition is local: there is no cloud inference or account.

It runs on **Linux** (x86_64; X11 and Wayland), **Windows** (x64) and **macOS** (universal: Apple Silicon and Intel). Linux Hyprland is used daily; Windows and macOS are built and tested in CI but have not been run interactively.

<p align="center"><img src="docs/user/images/settings.png" width="720" alt="Arcade Lens General settings for saving selections, OCR and desktop integration"></p>

## Download and install

Download the file for your system from [Releases](https://github.com/qa-p1/Arcade-lens/releases). Or install it from **[Arcade Tools](https://github.com/qa-p1/Arcade-tools) → Apps**, which can also update and remove it.

| Platform | File | How to install |
|---|---|---|
| Linux x86_64 | `ArcadeLens-<version>-linux-x86_64.AppImage` | Run it. The first run asks **Install Arcade Lens?** and installs to `~/Applications/Arcade/`, adds a menu entry, an `arcade-lens` link in `~/.local/bin` and an optional start at login. **Just run it once** skips installing. |
| Windows x64 | `ArcadeLens-<version>-windows-x64-setup.exe` | Run Setup. Installs for your user without admin, adds a Start menu entry, optional desktop shortcut and start at login, and puts `arcade-lens` on PATH. |
| Windows x64, portable | `ArcadeLens-<version>-windows-x64-portable.exe` | Run it directly, without installing. |
| macOS (universal) | `ArcadeLens-<version>-macos-universal.dmg` | Open the dmg and drag Arcade Lens to Applications. Run from the dmg or Downloads, it offers to move itself to Applications. |

- Scripted Linux install: `arcade-lens --install --silent` from the AppImage (add `--no-launch`, or `--channel C`). `--repair` rewrites the launcher, command and login entries; `--integration-status` prints the integration as JSON.
- Linux OCR needs Tesseract with English language data. Install it with your package manager, or use **Settings → General → Download Tesseract** for a private copy shared by Arcade apps. Windows and macOS use the system OCR.
- Builds are unsigned. On Windows, SmartScreen may warn: choose **More info → Run anyway**. On macOS the app is not notarized: right-click it and choose **Open** the first time.
- If FUSE is missing on Linux, run the AppImage with `--appimage-extract-and-run`.

Pushes to `main` that change the version publish a new latest release; other `main` builds replace the **nightly** prerelease. Releases include `arcade-release.json` and `SHA256SUMS.txt` for Arcade Tools. See [Getting started](docs/user/getting-started.md).

## Features

- **Select on screen:** draw a region, or click a highlighted window where window detection is supported. Lens captures all monitors and freezes the image first, so menus and tooltips stay selectable.
- **Copy, save, pin, annotate, measure:** floating pins (zoom, opacity, live refresh where supported), an annotation editor with pixelation, and a measuring mode.
- **Recognize content:** 26 built-in recognizers and 156 built-in actions in a ranked, keyboard-addressable palette. Links, emails, phone numbers, addresses, IPs, dates and times, currencies, quantities, coordinates, shell commands, code, errors and stack traces, paths, hashes, UUIDs, secrets, tables, subtitles, Git commits, colors and contrast, QR codes, barcodes and more.
- **Several findings at once:** a QR image can offer image actions, QR decoding and actions for the URL in its payload; OCR text feeds URL, command, error, table, path, date and other recognizers.
- **Safe by default:** outbound actions are hidden when content looks like a password, key or token. Commands read from the screen never run without confirmation; dangerous ones need Ctrl+Enter or a click.
- **Customizable:** shortcut, OCR languages, screenshot format and folder, preferred or disabled actions, recognizers, action keys, saved chains, providers, plugins, connected apps, privacy and history (off by default).
- **Chains** combine compatible actions into a repeatable workflow.
- **Plugins:** third-party recognizers and actions run out of process with declared permissions and stay disabled until you enable them. See [plugins](docs/PLUGINS.md) and the [ISBN example](examples/plugins/isbn).

More in [Features](docs/user/features.md) and [architecture](docs/ARCHITECTURE.md).

## Works with other Arcade apps

Lens works on its own: local capture and recognition stay available when it is used alone. It connects to other installed Arcade apps automatically over Arcade Link, **without needing Arcade Tools**. Turn this on or off with **Connect with other Arcade apps** in Lens's Connected apps settings, with a switch per app.

| App | What Lens can do with it |
|---|---|
| Arcade Box | Run image presets or saved Box pipelines on a selection. |
| Arcade Clipboard | Send supported text, links, images or files to your devices. On Linux, KDE Connect is the fallback. |
| Arcade Look | Quick Look a selected path or supported file. |
| Arcade Shelf | Add a capture, recognized text, link or file to Shelf. |
| Arcade Wheel | Add a selected action to Wheel. |
| Arcade Find | Search recognized text or a file path in Find. |
| Arcade Tools | Install, update or remove Lens from its **Apps** tab. |

The expanded palette marks outbound actions with **↗** and shows exactly what they send. Other apps can ask Lens to capture a region, analyze an image, recognize text and codes, or pin an image.

When Arcade Tools is running, its tray icon hosts Lens's menu, including **Capture**, **Capture text** and **Pick color**. When Tools quits, crashes or turns hosting off, Lens's own icon returns at once. With **Connect with other Arcade apps** off, Lens always shows its own icon.

See [how Lens works with other Arcade apps](docs/user/with-other-apps.md) and the [Arcade Link reference](docs/ARCADE_LINK.md).

## Usage

Press **Ctrl+Alt+Shift+L** (⌃⌥⇧L on macOS) and drag a rectangle. Press **Enter** to run the highlighted action, or **Space** / **Tab** to browse all actions. Arrow keys move the selection, Shift+arrows resize it, Ctrl+C copies it as an image, **M** enters measure mode and Escape closes. Change the shortcut in **Settings → Shortcut**; the single-letter action keys in **Settings → Actions**.

On Hyprland, Lens binds the shortcut while it runs. On other Wayland desktops, bind `arcade-lens --capture` in the desktop settings. The full list is in the [shortcut reference](docs/user/shortcuts.md).

### Command line

The launcher commands reach the running instance:

```
arcade-lens               Open Settings; Lens keeps running in the tray
arcade-lens --background  Start in the background (used at login)
arcade-lens --capture     Select something on screen now
arcade-lens --restart     Restart the background instance
arcade-lens --quit        Quit the background instance
arcade-lens --version     Print the version
arcade-lens --install [--silent] [--no-launch] [--channel C]
                          Install this AppImage / macOS bundle
arcade-lens --uninstall [--silent] [--remove-data]
                          Remove this installation via its receipt
arcade-lens --repair      Rewrite the launcher, command and login entries
arcade-lens --integration-status
                          Print the integration as JSON (no side effects)
arcade-lens --arcade-manifest
                          Print Lens's Arcade Link manifest (no side effects)
```

## Settings and data

Configuration lives in `settings.toml` at the path shown in **Settings → About**. `ARCADE_LENS_HOME` overrides the location. Isolated profiles and builds run from `target` do not register a login item or application-menu entry.

Selections are temporary unless you enable history; usage learning stores only local counts of which action you chose.

Arcade Tools can export and import Lens's settings: activation shortcut, image format, OCR languages, action order and keys, disabled recognizers and actions, date order, home currency and privacy switches. Imports are validated and the current settings are backed up first. **Never exported:** history, captures and device data.

## Uninstall

- **Linux:** run `arcade-lens --uninstall`, use the launcher's **Uninstall** action, or remove Lens in Arcade Tools.
- **Windows:** **Settings → Apps** (Apps & features), or Arcade Tools.
- **macOS:** `arcade-lens --uninstall` removes the installation recorded in its receipt, or remove Lens in Arcade Tools.

Your settings stay unless you choose to remove them (`--remove-data`).

## Platform notes

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Capture | X11, per-monitor scale | screenshot portal | xcap | xcap, Screen Recording permission |
| Global shortcut | Native | Hyprland runtime binding; bind the command elsewhere | Native | Native |
| Window detection and commands | EWMH | Not available | Available | Detection only |
| OCR | Tesseract | Tesseract | Windows.Media.Ocr | Apple Vision |
| Live pins | Available | Not available | Available | Available |
| Window recording | FFmpeg required | Not available | FFmpeg required | Not available |
| Tray | StatusNotifierItem | StatusNotifierItem | Notification area | Menu bar |
| Login startup | XDG autostart | XDG autostart | Startup shortcut when installed, Run key when portable | LaunchAgent |

- Tesseract can misread small interface text such as lowercase "l" and the digit "1". Lens enlarges small selections before OCR, but it cannot recover characters the engine did not detect.
- On Wayland, the screenshot portal provides the frozen image; Lens has no window list or window commands, and live pins and window recording are unavailable. With Hyprland permission enforcement enabled, allow the screenshot tools in `hyprland.lua` to avoid repeated prompts:

  ```lua
  hl.permission("/usr/(bin|local/bin)/grim", "screencopy", "allow")
  hl.permission("/usr/(lib|libexec|lib64)/xdg-desktop-portal-hyprland", "screencopy", "allow")
  ```

- A system share sheet and accessibility-based UI inspection are not implemented. macOS window commands need an Accessibility API integration that is not implemented yet.
- The Send to my devices action uses Arcade Clipboard when available; on Linux, KDE Connect is the fallback.
- Debug logging with `LENS_DEBUG=1` does not include selected screen content.

## Build from source

Requires Rust 1.88 or newer. On Linux, install the Wayland and XKB development packages (`libxkbcommon-dev libwayland-dev` on Debian/Ubuntu).

```sh
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For a Linux source install, run `./scripts/install.sh`. It builds Lens, installs it in `~/.local/bin`, starts it, and registers the tray and application-menu entries. Run it again after pulling changes to rebuild and restart.

Arcade Link is pinned to a release tag in `Cargo.toml`. A local dependency override can point to a checkout for development. CI checks every push on Linux, Windows and macOS and builds the packages from [packaging/](packaging/).

## Documentation

- [User guide](docs/user/): [getting started](docs/user/getting-started.md), [features](docs/user/features.md), [shortcuts](docs/user/shortcuts.md), [FAQ](docs/user/faq.md), [troubleshooting](docs/user/troubleshooting.md)
- [Architecture](docs/ARCHITECTURE.md): dataflow and safety model
- [Arcade Link](docs/ARCADE_LINK.md): actions, capture modes and peer integration
- [Plugins](docs/PLUGINS.md): plugin API and permissions
- [Status](docs/STATUS.md): verification record and remaining limits
- [Changelog](CHANGELOG.md)
- [Vendored files](VENDORED): source and license details for copied files

## License

MIT OR Apache-2.0.

---

<p align="center">
  Part of the <b>Arcade</b> suite:
  <a href="https://github.com/qa-p1/Arcade-Find">Find</a> ·
  <a href="https://github.com/qa-p1/Arcade-lens">Lens</a> ·
  <a href="https://github.com/qa-p1/Arcade-look">Look</a> ·
  <a href="https://github.com/qa-p1/Arcade-box">Box</a> ·
  <a href="https://github.com/qa-p1/Arcade-clipboard">Clipboard</a> ·
  <a href="https://github.com/qa-p1/Arcade-Shelf">Shelf</a> ·
  <a href="https://github.com/qa-p1/Arcade-wheel">Wheel</a> ·
  <a href="https://github.com/qa-p1/Arcade-tools">Tools</a> ·
  <a href="https://github.com/qa-p1/Arcade-catalog">Docs</a>
</p>
