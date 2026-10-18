# Arcade Lens: status

## Release status (2026-10-18)

- Latest release: [v0.3.2](https://github.com/qa-p1/Arcade-lens/releases/tag/v0.3.2), published 2026-10-19 (releases every descriptor an AppImage start inherits; see CHANGELOG.md). The CI and e2e notes below were recorded for v0.2.0.
- Previously released: [v0.3.1](https://github.com/qa-p1/Arcade-lens/releases/tag/v0.3.1), published 2026-10-18 (fixes for what verifying the release before it found).
- Previously released: [v0.3.0](https://github.com/qa-p1/Arcade-lens/releases/tag/v0.3.0), published 2026-10-18 (fixes from the October audit and stress test).
- Previously released: [v0.2.0](https://github.com/qa-p1/Arcade-lens/releases/tag/v0.2.0) with a Linux self-installing AppImage, a per-user Windows installer, a macOS dmg, `arcade-release.json` and `SHA256SUMS.txt`.
- Merged to `main` from PR #3 (f10e308) after CI passed on Linux, Windows and macOS: [run 38607114141](https://github.com/qa-p1/Arcade-lens/actions/runs/38607114141). That covers the build, tests, clippy or analyzers, installer smoke tests on each OS, and the Linux AppImage install, repair and uninstall check in a temporary profile.
- Ecosystem e2e (Arcade Link `tools/e2e.py`, real CI builds of all eight apps): install, tray hosting (hosted by Tools, own icon back within 1 s after Tools is killed, no flash on restart) and shortcuts pass. Tools 0.2.0 backs up and restores Lens's settings (the backup incompatibility found by the e2e matrix was fixed in Tools, cf8f0c5).
- Not yet verified: a real Windows or macOS desktop (CI runners only), and the owner's real-desktop checks. Builds are unsigned, so SmartScreen and Gatekeeper ask for approval.

## Arcade Tools hub integration (2026-10-12, branch `arcade/tools-hub`)

Lens uses Arcade Link tag `v0.3.0` (`33650a8`). It implements the seven optional
hub methods (Capture, Capture text and Pick color as tray extras), the
`TrayHostWatcher` with `status.tray`, the manifest `menu`/`logs`/`docs`/`install`
fields, the self-installing AppImage (replacing its own `.desktop`/autostart
writer, keeping the APPDIR fix) with `--install`, `--uninstall`, `--repair`,
`--integration-status` and `--restart`, the shared per-user Inno installer with
receipt and PATH shim, the macOS move-to-Applications prompt, portable
settings export/import, and `docs/user/`. The portable exe is published with
`kind: "portable"`, so Arcade Tools never installs it.

- CI run [38279548995](https://github.com/qa-p1/Arcade-lens/actions/runs/38279548995),
  2026-10-12, all green on Rust 1.88: fmt, clippy `-D warnings`, docs/shortcut
  and vendored-source checks and the tests (149 pass, 2 ignored on Linux, in a
  temporary profile) on Linux, Windows and macOS; AppImage install, repair and
  uninstall in a temporary profile; Windows silent install/uninstall on the
  ephemeral runner account; DMG copy in a temporary home.
- Windows and macOS were exercised in CI only. The hub features have not been
  run on a real desktop yet; that check comes with the released packages.

## Earlier verification

Verified 2026-10-08 on branch `arcade/link` (version 0.1.0, Arcade Link
`v0.1.0`). This page records what is implemented and how it was checked; the
other documents describe how it works.

Arcade Link pin: `v0.2.0` since 2026-10-10, which adds Arcade Shelf and
Arcade Find to the Connected apps page (glyphs, description, Get link) and
the palette ("Add to Shelf" for captures, text, links and files; "Search in
Find" for text and paths). With
that pin, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings` and `cargo test --workspace` (142 passed, 1 ignored) pass on Linux.
Against the real apps (2026-10-10, Arcade Shelf 0.1.0 and Arcade Find 0.2.1
under Xvfb), `real_shelf_and_find_take_lens_entries` in
`crates/lens-actions/tests/arcade.rs` passes: Lens's "Add to Shelf" stores a
text finding and a capture ("Added 1 item to Quick Shelf" from Shelf, the text
in Shelf's store), and "Search in Find" opens Find with the text as its query
and the matching file listed. Run it with `ARCADE_SHELF_BIN` and
`ARCADE_FIND_BIN` set (`cargo test -p lens-actions --test arcade --
--ignored real_shelf_and_find`). The rest of this page was verified on
`v0.1.0`.

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
| Benchmark against the 2026-10-05 baseline | startup 2.9 → 3.0 ms, warm invoke 1.5 → 1.5 ms, idle RSS 80.2 → 81.4 MiB, idle CPU median 0 ms/5 s |

## Limits

- The idle CPU median is zero, but two of five samples recorded 10 ms. It is
  not proof of zero wakeups; see the [benchmark report](https://github.com/qa-p1/Arcade-Link/blob/main/COMPLETION_REPORT.md).

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
