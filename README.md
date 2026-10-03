# Arcade Lens

**Select anything on your screen. Do something useful with it.**

Press a shortcut, draw a rectangle around anything visible — text, a QR code,
an error, a color, a table, a file path, a window — and Arcade Lens works out
everything it *is* and offers the few actions that matter, right next to the
selection. You never have to decide which utility to open first.

> **Status: early development.** The capability engine, 22 recognizers
> (covering 30 capabilities), ~140 actions, ranking, safety policy, chains,
> local OCR and a CLI harness are implemented and tested. The native overlay (global shortcut → freeze →
> draw → palette) is the next milestone; see [Roadmap](#roadmap).

## How it works

A selection is never classified into a single type. A QR code containing a
URL is simultaneously a screenshot, an image, a QR code *and* a URL, and you
get actions for all of them:

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

Recognition is progressive: Copy / Save / Pin are available instantly, and
text, URL, and other actions appear as recognizers finish (`--timeline` shows
the arrival order).

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design: the
dataflow recognizer graph, typed values, the action provider model, ranking,
the safety model and the plugin API.

## What it recognizes

| Pixels | Text (from OCR or QR payloads) |
|---|---|
| any region (always) | URLs, emails, phone numbers, postal addresses |
| single colors, palettes | dates & times (ambiguity preserved), timecodes |
| UI elements: size, position, background/foreground, WCAG contrast | shell commands with risk analysis |
| photos/illustrations, icons | source code + language, line-number gutter removal |
| QR codes, barcodes (EAN/UPC/Code 128/…) | errors & stack traces with a cleaned, privacy-safe search query |
| OS windows (when the platform reports them) | file paths (checked for existence), IPv4/IPv6, domains |
| | hashes (never over-claimed), UUIDs, secrets/tokens |
| | currency, physical quantities (local unit conversion), coordinates |
| | tables (geometric column reconstruction) |

## Safety and privacy

* Everything is recognized **locally**. No account, no cloud inference.
* Outbound actions (search, maps, WHOIS…) are marked, and show exactly what
  they will send. If a selection contains something that looks like a
  secret, actions that would send it anywhere are removed.
* Commands read from the screen are **never** run without confirmation; the
  confirmation lists the exact text and its risks (sudo, deletion,
  pipe-to-shell, downloads…). "Open Terminal + Paste" pre-fills an editable
  prompt instead of running anything.
* Ambiguous values (e.g. `05/03/2024`) are never silently resolved.
* Selections are ephemeral. Usage learning stores only "which action for
  which kind of thing" counters, locally, and can be reset.

## Building

Requires Rust 1.80+.

```console
$ cargo build --release
$ ./target/release/arcade-lens models download   # local OCR models (~12 MB)
```

## CLI

The CLI drives the same engine as the overlay, on image files:

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

Configuration lives in `settings.toml` (see `arcade-lens config path`);
`ARCADE_LENS_HOME` overrides the location.

## Development

```console
$ cargo test --workspace
$ cargo clippy --workspace --all-targets
```

## Roadmap

1. **Overlay shell** — per-platform capture, freeze, selection overlay, the
   spatial action palette and keyboard handling, global shortcut.
2. **Pins** — frozen pins first; live pins as an optional capability.
3. **Platform OCR** — Windows.Media.Ocr and Apple Vision engines.
4. **Measure mode** — edge snapping, distances, guides.
5. **Settings app** — shortcuts, providers, actions, chains, plugins, privacy.
6. **Out-of-process plugins** — versioned, permissioned third-party
   recognizers and actions; Arcade ecosystem integrations (Clipboard Mesh,
   Quick Look, Wheel).
