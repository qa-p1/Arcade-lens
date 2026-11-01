# Changelog

## 0.4.0 — 2026-11-01

- A new look shared with the other Arcade apps: the lime accent, the Inter
  typeface, and light and dark themes that follow Settings › Appearance at
  once. This covers the capture overlay, the action palette, the annotate
  toolbar, pins, the recording chip, History, Settings and the install prompt.
- Settings has the standard page order and words. Changes apply as you make
  them. Settings and History open on the monitor under the pointer and
  remember their size and position on each monitor.
- The capture closes when you switch to another app, as well as on Esc.
- The tray menu reads Open Arcade Lens, Capture text, Pick color, Settings,
  Restart Arcade Lens and Quit Arcade Lens. A click captures (on macOS it opens
  the menu), and the tooltip shows what Lens does.
- Messages say what couldn't be done and why, and point to the right page in
  Settings. The Notifications switch in Settings › General now turns desktop
  notifications off.
- New command-line options: `--status` (one line of JSON), `--show`, `--hide`
  and `--toggle`. `--settings` and `--help` are listed.
- Lens now uses the standard Arcade identifiers (`dev.arcade.Lens`) and
  folders. Updating from 0.3 moves your settings and history, and replaces the
  old menu entry and login item instead of leaving a second one. Your
  shortcuts and preferences stay.
- Box presets and Look previews started from a capture now finish, and their
  errors are reported. Before, they could stop when the capture closed.
- Quit and Restart also close an open Settings or History window.
- Built against Arcade Link 0.4.13. Licences are listed in
  THIRD_PARTY_NOTICES.md.

## 0.3.4 — 2026-10-25

- Copy actions work on Wayland again: each capture runs in its own process, and
  the copied text or image disappeared when that process closed. Lens now
  hands the clipboard to `wl-copy` (or `xclip`), so it stays after the overlay
  closes. On X11 the clipboard no longer empties after each copy either.
- No more flicker on Hyprland when the overlay opens or closes. It used to
  appear as a tiled window that pushed the other windows aside before going
  full screen, draw its first frame in a corner, and on closing show a black
  frame while the desktop faded back in. The overlay now opens floating and
  full screen at your monitor's size, and closes straight to the desktop.

## 0.3.3 — 2026-10-19

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.
- Restarting in the background no longer keeps the previous image mounted.

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
