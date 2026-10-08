# Arcade Lens: status

Verified 2026-10-08 on branch `arcade/link` (version 0.1.0, Arcade Link
`v0.1.0`). This page records what is implemented and how it was checked; the
other documents describe how it works.

## Implemented

- Freeze-first capture on every monitor, region and window selection, the
  progressive palette (26 recognizers with OCR, 156 actions), pins, annotate,
  measure, chains, usage learning, plugins and the standard tray menu.
- OCR: Windows.Media.Ocr and Apple Vision on Windows and macOS; Tesseract on
  Linux (system install, or a per-user copy downloaded from Settings and
  shared by every Arcade app). No OCR model or engine is bundled.
- Arcade Link: `lens.capture`, `lens.capture_and_act` (palette, measure, pin,
  color), `lens.analyze`, `lens.recognize` (also one-shot) and `lens.pin`;
  Quick Look, Box presets and pipelines, Send to my devices, Add to Wheel and
  Get inside the palette; the Connected apps page.

## Verification

| Check | Result |
|---|---|
| `cargo test --workspace` | 141 passed, 1 ignored (the mock-peer test, run through the isolated runner) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| CI (Linux, Windows, macOS) | passing at `5ae1789` |
| Arcade Link e2e, `lens` group and cross-app flows | all passing (74/74 ecosystem checks) |
| Stress: 180 concurrent `lens.recognize` calls (6 at a time) | 0 failures, p50 0.19 s; RSS flat at about 125 MiB |
| Benchmark against the 2026-10-05 baseline | startup 2.9 → 3.0 ms, warm invoke 1.5 → 1.4 ms, idle RSS 80 → 82 MiB, idle CPU 0 |

## Limits

- Windows and macOS are compiled and tested in CI but have not been run
  interactively.
- Wayland: no window list or window commands; the portal screenshot is the
  freeze; live pins and window recording are X11/Windows only. Exposed Link
  actions on Wayland are used daily on Hyprland but are not in the isolated
  runner.
- Tesseract can misread small UI fonts; Lens upscales small selections first.
- Share sheet, macOS window commands (Accessibility) and accessibility-based
  UI inspection are not implemented.

## Documents

| Document | Contents |
|---|---|
| [README](../README.md) | Install, use, recognizers, privacy, platform table, building |
| [ARCHITECTURE](ARCHITECTURE.md) | Crates, process model, engine, ranking, safety model, platform layer |
| [ARCADE_LINK](ARCADE_LINK.md) | Exposed actions, capture modes, connected actions, verification commands |
| [PLUGINS](PLUGINS.md) | Out-of-process plugin manifest, protocol and permissions |
