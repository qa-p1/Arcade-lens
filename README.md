# Arcade Lens

**Select anything on your screen. Do something useful with it.**

Press a shortcut and draw a rectangle around anything visible: text, a QR
code, an error, a color, a table, a file path or a window. Arcade Lens works
out everything the selection *is* and offers the few actions that matter,
right next to it. You never have to decide which utility to open first.

## Installing

Download the package for your system from the
[latest release](https://github.com/qa-p1/Arcade-lens/releases/latest). The
[nightly](https://github.com/qa-p1/Arcade-lens/releases/tag/nightly)
prerelease has the latest build of `main`.

| System | Package | Setup |
|---|---|---|
| Windows (x64) | `…-windows-x64-setup.exe` | Installs for your user (no admin), adds a Start menu entry, optionally starts Lens at sign-in, and launches it. Uninstall from Settings → Apps. A portable `.exe` is also provided. |
| Linux (x86_64) | `…-linux-x86_64.AppImage` | `chmod +x` it and run it. On first start it adds itself to the applications menu and to login startup. Text recognition uses your Tesseract; see below. |
| macOS 12.3+ (Apple silicon and Intel) | `…-macos-universal.dmg` | Drag Arcade Lens to Applications and open it. It lives in the menu bar and adds itself to login items. The app isn't notarized: the first time, right-click it and choose Open. |

To build and install from source on Linux instead:

```console
$ ./scripts/install.sh
```

This builds Lens, installs it to `~/.local/bin` and starts it. Lens then
lives in the tray, starts at login, and appears in the applications menu as
**Arcade Lens**. Run the script again after pulling changes; the running
instance restarts on the new build. On Linux, text recognition uses
Tesseract: install it from your package manager (`tesseract` plus its English
data), or let Lens fetch a private copy (~24 MB) from **Settings → General →
Download Tesseract**. The copy is shared by every Arcade app. Windows and
macOS use the OCR built into the system.

## Using it

Press **Ctrl+Alt+Shift+L**, which you can change in Settings. Lens captures
every monitor first and then freezes the screen, so menus and tooltips can be
selected.

| Input | Effect |
|---|---|
| drag | select a region (handles to resize, drag inside to move) |
| click on a highlighted window | select that window |
| arrows / Shift+arrows | nudge / resize the selection (Ctrl: ×10) |
| the key shown on an action, or click | run it |
| Enter | run the default (highlighted) action |
| Space / Tab | all actions, grouped by what they apply to; type to filter |
| Ctrl+C | copy the region as an image |
| M | measure mode: drag to measure, snaps to edges; C copies the size |
| Esc | back / close |

The palette works progressively:
- Copy, Save, Pin and Annotate are available the instant the selection is
  made.
- OCR-based results (links, commands, errors, tables…) slot in as their
  recognizers finish. Entries already on screen don't move.
- Outbound actions are marked ↗, and the expanded list shows exactly what
  each one would send.

The **tray icon** shows that Lens is running. Click it for Settings, or
open its menu for **Open Lens** (start a capture), **Open Settings**,
**Restart Arcade Lens** and **Quit Arcade Lens**, the same menu every Arcade
app has. Start at login is in Settings. Opening Arcade Lens from the applications menu
opens Settings; its right-click menu has Capture Screen and Quit.

For a compositor or launcher binding, the same commands are available as
options, and they reach the running instance:

```console
$ arcade-lens                 # Settings (starts Lens if it isn't running)
$ arcade-lens --capture       # select something now
$ arcade-lens --background    # start without a window (what login uses)
$ arcade-lens --restart
$ arcade-lens --quit
```

**Pins** float above other windows:
- Drag to move, scroll to zoom, and Ctrl+scroll to change opacity.
- Right-click for copy, save, annotate, a live refresh (where supported) and
  close.
- Double-click or Esc closes a pin.

**Annotate** opens the selection in an editor with a pen, a highlighter,
lines, arrows, rectangles, ellipses, text and pixelation (for redaction),
plus undo/redo. From there it can be copied, saved or pinned.

## How it works

A selection is never classified into a single type. A QR code containing a
URL is a screenshot, an image, a QR code *and* a URL at the same time, and
you get actions for all of them: Open, Copy URL and Generate QR for the URL,
alongside Copy, Save, Pin and Annotate for the image.

There are 25 built-in recognizers forming a dataflow graph (pixels → OCR →
text → URL, command, error…) and 156 built-in actions. Ranking picks the 4–6
that matter, and learns locally which ones you prefer. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## What it recognizes

| Pixels | Text (from OCR, QR payloads or plugins) |
|---|---|
| any region (always) | URLs, emails, phone numbers, postal addresses |
| single colors, palettes | dates & times (ambiguity preserved), timecodes, timestamps |
| UI elements: size, background/foreground, WCAG contrast | shell commands, with risk analysis |
| photos/illustrations, icons | source code + language, line-number gutter removal |
| QR codes, barcodes (EAN/UPC/Code 128/…) | errors & stack traces → cleaned, privacy-safe search |
| windows (X11, Windows, macOS) | file paths (checked for existence), IPv4/IPv6, domains |
| video frames (media-player windows) | hashes (never over-claimed), UUIDs, secrets/tokens |
| | currency, physical quantities (local unit conversion), coordinates |
| | tables (CSV / TSV / Markdown), git commits on a forge page |
| | document text (clean copy, PDF, print), subtitles |

## Safety and privacy

- Everything is recognized **locally**. There is no account and no cloud
  inference.
- If a selection contains something that looks like a secret, actions that
  would send it anywhere are removed, and the engine refuses them even if
  they are invoked directly.
- Commands read from the screen are **never** run without confirmation:
  - Dangerous actions need Ctrl+Enter or a click; a plain Enter is not
    enough.
  - "Open Terminal + Paste" pre-fills an editable prompt and runs nothing.
- Ambiguous values (e.g. `05/03/2024`) are never silently resolved.
- Selections are ephemeral. History is off unless enabled in Settings.
  Usage learning stores only "which action for which kind of thing"
  counters, and can be reset.
- Debug logging (`LENS_DEBUG=1`) never includes selection content.

## Works with other Arcade apps

When the other Arcade apps are installed, Lens offers Quick Look for paths,
Box's image presets and saved pipelines, Send to my devices through
Clipboard, and Add to Wheel. The expanded palette shows what each action
sends. Selections containing secrets cannot be sent.

Manage these connections in **Settings → Connected apps**, with a master
switch and a **Use with Arcade Lens** switch for each installed app. Lens
keeps its capture, palette and local recognition when used alone.

Other apps can ask Lens to capture, recognize, analyze or pin an image.
See [the exposed actions, capture modes and platform status](docs/ARCADE_LINK.md).

## Plugins

Third-party recognizers and actions run out of process with declared
permissions, and stay disabled until the user enables them:

Copy a plugin's directory into the plugins folder (**Settings → Plugins →
Open plugins folder**), then enable it there.

[docs/PLUGINS.md](docs/PLUGINS.md) documents the manifest, protocol and
security model. The [`isbn` example](examples/plugins/isbn) shows a small
recognizer. Arcade app connections are built into Lens.

## Platform support

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Capture | X11 (RandR monitors, per-monitor scale) | screenshot portal | xcap | xcap (needs Screen Recording permission) |
| Global shortcut | ✓ | Hyprland: automatic; elsewhere bind `arcade-lens --capture` | ✓ | ✓ |
| Window detection & commands | ✓ (EWMH) | — | ✓ | detection only |
| OCR | Tesseract | Tesseract | Windows.Media.Ocr | Apple Vision |
| Live pins | ✓ | — | ✓ | ✓ |
| Window recording | ✓ (needs ffmpeg) | — | ✓ (needs ffmpeg) | — |
| Tray icon | StatusNotifierItem | StatusNotifierItem | notification area | menu bar |
| Start at login | XDG autostart | XDG autostart | Run key | LaunchAgent |

**Testing status.** The Linux X11 build is exercised end to end under Xvfb:
shortcut, overlay, palette, pins, measure, annotate, settings and plugins.
The Windows and macOS code paths compile and lint cleanly for their targets
but have not yet been run on real machines.

## Building

Requires Rust 1.95+. CI (`.github/workflows/ci.yml`) checks every push on
all three systems. Pushes to `main` also build the packages with the scripts
in [`packaging/`](packaging), and when the build passes:

- if the `version` in `Cargo.toml` has no release yet, CI tags it
  `v<version>` and publishes it as the latest release (so to release, bump
  the version);
- otherwise it replaces the `nightly` prerelease.

Pushing a `v<version>` tag, or publishing a release on GitHub (which
creates the tag), builds that tag and attaches the packages to its release.
Packages include `arcade-release.json` and `SHA256SUMS.txt` for Arcade Tools.
CI checks out the shared Arcade Link dependency from the repository variables
`ARCADE_LINK_REPOSITORY` and `ARCADE_LINK_REF`; their defaults pin the source
recorded in [VENDORED](VENDORED). Publishing that dependency repository and
replacing the local path dependency are release-owner steps.

```console
$ cargo build --release
$ cargo test --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
```

Configuration lives in `settings.toml` (its location is shown in Settings →
About). `ARCADE_LENS_HOME` overrides the location; instances started with it
leave the login item and applications menu alone, as do builds run from
`target/`.

## Known limitations

- **OCR accuracy.** On Linux, OCR uses Tesseract, which can misread small UI
  fonts (`l`/`1`). Lens upscales small selections first; recognizers downstream
  are tolerant, but they cannot recover text OCR never produced.
- **Wayland** doesn't allow global shortcuts, window lists or freezing
  without the portal. There, the portal screenshot *is* the freeze. With
  Hyprland's `ecosystem.enforce_permissions` on, allow the portal's
  screenshot tool in `hyprland.lua` or Hyprland asks before every capture:

  ```lua
  hl.permission("/usr/(bin|local/bin)/grim", "screencopy", "allow")
  hl.permission("/usr/(lib|libexec|lib64)/xdg-desktop-portal-hyprland", "screencopy", "allow")
  ```
- **Missing features:**
  - Share (a system share sheet) is not implemented on any platform.
  - Send to device uses Arcade Clipboard when available, with KDE Connect
    as the Linux fallback.
  - macOS window commands need the Accessibility API, which is not
    implemented yet.
- **No accessibility metadata yet.** UI inspection is pixel-based; it doesn't
  read element names from accessibility APIs.
