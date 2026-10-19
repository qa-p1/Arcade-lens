# Changelog

## 0.3.2 — 2026-10-19

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.

## 0.3.1 — 2026-10-18

- Updating or installing from your Downloads folder, restarting into the
  background and starting other Arcade apps no longer leave the old AppImage
  mounted and its runtime (about 20 MB) running. A program the app started
  inherited an open handle on its AppImage mount and kept it busy.

## 0.3.0 — 2026-10-18

- Lower idle memory: on X11 the background instance keeps no window until you
  capture.
- Recognized text keeps time zones and paragraph breaks; secrets are masked in
  the capture history.
- The capture palette explains when text recognition is unavailable, and its
  "C" hint no longer types into the filter.
- Logs are written to the folder Lens advertises, also when it was started
  without a terminal.
- Installer: updating replaces the running copy, the prompt matches the other
  Arcade apps and shows the real Start at login state, the login entry is
  escaped correctly, and upgrading names the older copy it replaces.
- Built against Arcade Link commit `2fc7c91`.

## 0.2.0 — 2026-10-14

- Add Arcade Link v0.3.0 hub methods, tray hosting and portable settings.
- Add receipt-owned installation, repair and removal, with Linux first-run setup and macOS Applications support.
- Use the shared per-user Windows installer integration and release tools from Link v0.3.0.
- Add user guides, generated shortcut tables and CI checks with isolated profiles on Rust 1.88.
