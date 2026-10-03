# Arcade Lens

**Select anything on your screen. Do something useful with it.**

Press a shortcut and draw a rectangle around anything visible: text, a QR
code, an error, a color, a table, a file path or a window. Arcade Lens works
out everything the selection *is* and offers the few actions that matter,
right next to it. You never have to decide which utility to open first.

## Using it

```console
$ arcade-lens models download     # local OCR models (~12 MB, once)
$ arcade-lens start               # background instance with the global shortcut
$ arcade-lens autostart enable    # start at login
```

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

Other entry points, which reach the running instance when there is one:

```console
$ arcade-lens capture             # open the overlay now (bind this on Wayland)
$ arcade-lens settings
$ arcade-lens pin image.png
$ arcade-lens quit
$ arcade-lens install-launcher    # Linux: add to the applications menu
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
you get actions for all of them:

```console
$ arcade-lens analyze qr.png
Selection 132 × 132 px · analyzed in 18 ms · OCR: ocrs

FOUND
  #0   region       Image(132x132)
  #1   qr-code      https://arcade.example/lens
  #2   url          https://arcade.example/lens
  …
ACTIONS
  ⏎  Open ↗                       url #2
  c  Copy URL                     url #2
  q  Generate QR                  url #2
  …
```

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

## Plugins

Third-party recognizers and actions run out of process with declared
permissions, and stay disabled until the user enables them:

```console
$ arcade-lens plugins install ./examples/plugins/isbn
$ arcade-lens plugins enable dev.example.isbn
```

[docs/PLUGINS.md](docs/PLUGINS.md) documents the manifest, protocol and
security model. The Arcade Clipboard, Quick Look and Wheel integrations are in
[`examples/plugins`](examples/plugins).

## Platform support

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Capture | X11 (RandR monitors, per-monitor scale) | screenshot portal | xcap | xcap (needs Screen Recording permission) |
| Global shortcut | ✓ | bind `arcade-lens capture` in your compositor | ✓ | ✓ |
| Window detection & commands | ✓ (EWMH) | — | ✓ | detection only |
| OCR | ocrs (local) | ocrs | Windows.Media.Ocr | Apple Vision |
| Live pins | ✓ | — | ✓ | ✓ |
| Window recording | ✓ (needs ffmpeg) | — | ✓ (needs ffmpeg) | — |

**Testing status.** The Linux X11 build is exercised end to end under Xvfb:
shortcut, overlay, palette, pins, measure, annotate, settings and plugins.
The Windows and macOS code paths compile and lint cleanly for their targets
but have not yet been run on real machines.

## Building

Requires Rust 1.95+.

```console
$ cargo build --release
$ cargo test --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
```

## CLI

The CLI drives the same engine as the overlay, using image files:

```console
$ arcade-lens analyze screenshot.png [--all] [--json] [--timeline]
$ arcade-lens analyze --text "sudo pacman -S package"         # skip OCR
$ arcade-lens run core.table.csv screenshot.png               # run an action
$ arcade-lens run core.error.search-clean error.png --dry-run # show what would happen
$ arcade-lens run chain:clean-copy shot.png                   # run a chain
$ arcade-lens actions --capability url
$ arcade-lens recognizers
$ arcade-lens chains list
$ arcade-lens config path | show | init
$ arcade-lens reset-usage
```

Configuration lives in `settings.toml` (see `arcade-lens config path`).
`ARCADE_LENS_HOME` overrides the location.

## Known limitations

- **OCR accuracy.** On Linux, OCR uses ocrs, which misreads some glyphs (for
  example `₹`, and `l`/`1` in small UI fonts). Recognizers downstream are
  tolerant, but they cannot recover text OCR never produced.
- **Wayland** doesn't allow global shortcuts, window lists or freezing
  without the portal. There, the portal screenshot *is* the freeze.
- **Missing features:**
  - Share (a system share sheet) is not implemented on any platform.
  - Send to device uses KDE Connect on Linux.
  - macOS window commands need the Accessibility API, which is not
    implemented yet.
- **No accessibility metadata yet.** UI inspection is pixel-based; it doesn't
  read element names from accessibility APIs.
