# Changelog

All notable changes to Arcade Lens. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Other Arcade apps, your file manager's menu and the browser extension can use what Lens finds, one action each: "Read text", "Scan QR codes and barcodes", "Pick colours", "Find links", "Find emails and phone numbers", "Find dates and times", "Find addresses", "Read a table" and the rest. Each works on an image (Lens reads its text first when it needs to) and, where it reads text, on selected text, and returns only what it was asked for. On Linux, "Read text" says it needs Tesseract instead of failing.

### Security

- A plugin that reads text no longer sees secrets in it: they are redacted from what it is handed, and secret findings are not handed over at all, unless the plugin declares the new `read-secrets` permission (a secret scanner, for example), which Settings › Plugins shows.
- Lens's settings, chains, usage counters, history and logs are private to your account: Lens makes its config and data folders `0700` when it starts and writes its files `0600`.

### Fixed

- A small pin's right-click menu was cut off by the pin's window. The pin now grows while its menu is open (the picture keeps its size) and returns to its size when the menu closes.

### Changed

- Lens moves to Arcade Link 1.1.0. The actions of other apps Lens offers on a finding come from Link's shared list of what each installed app can do (best matches first, up to six per app), not from a list built into Lens, so a new Arcade app's actions appear without a Lens update.

## [1.0.1] - 2026-11-22

### Changed

- Lens moves from Arcade Link 1.0.13 to 1.0.15. "Download Tesseract" in Settings runs curl and the downloaded engine's checks with your own environment rather than the AppImage's.

### Fixed

- The install dialog of a downloaded AppImage (and the uninstall dialog) was a page-sized window with a large empty area below its buttons. It is now as tall as what it shows, opens centred, and grows if a notice or an error appears.
- Linux: with Appearance set to System, Lens was dark on desktops that don't tell apps their colour scheme (openbox and other X11 sessions without the desktop portal), while GTK apps such as Arcade Tools were light. Lens now asks the desktop portal and, without an answer, reads GNOME's and GTK's settings as GTK does, so it is light there unless the desktop prefers dark. It reads them again each time a window opens. Windows and macOS are unchanged.

## [1.0.0] - 2026-11-16

### Added

- The AppImage carries the scalable and the symbolic `dev.arcade.Lens` icons, so desktops and trays that use them find them.
- Colour codes written in text (`#RRGGBB`, `#RGB`, `rgb()` and `rgba()`) are recognized as colours (LN-9).

### Changed

- A capture stays open when another window or app takes focus. Only Esc, an action or `--hide` closes it (LN-4, RD-LENS-5).
- A background start from the AppImage, and a restart, run from an unpacked copy under `<data>/arcade/runtime/`, so no FUSE helper stays running. `--status`, `--hide` and `--quit` never start Lens or make a copy.
- Lens uses less memory while it runs in the background: it gives back memory it no longer needs five seconds after it starts, and each time a capture overlay that runs in the background instance is hidden.
- Uninstalling Lens also removes its unpacked runtime copies. Other Arcade apps' copies stay.
- Lens moves from Arcade Link 0.4.14 to 1.0.11. The shared design kit is unchanged.
- Releases start as release candidates on a hidden test channel and are promoted to stable, byte for byte with a signed manifest, only after the release gates pass on the published files. The gates include an upgrade from the previous release that checks the new version reads and keeps a user's settings, chain and learned usage, leaves one launcher and one login item, and keeps the data on uninstall.
- A signed prerelease, `vX.Y.Z-nightly.YYYYMMDD.N`, is published each night from the packages `main` already built, when `main` has changed and its checks pass. It replaces the rolling `nightly` prerelease.
- Promotion to stable refuses to publish without the update signing key, so nothing is published unsigned.

### Fixed

- Linux: Restart, and any start from the installed AppImage, could hang without opening Lens when the desktop did not report its colour scheme. The unpacked copy inherited settings that pointed into the AppImage's mount, which closes as the copy starts (Arcade Link 1.0.13).
- "Remove settings and data" when uninstalling leaves nothing of Lens's behind, and a plain uninstall keeps only Lens's settings and data; CI checks both on the AppImage.
- A settings, chains, usage or window-position file Lens can't read is kept next to it as `<name>.unreadable-<time>` and Lens starts on the defaults, instead of refusing to start (settings) or replacing the file at the next save. Settings Lens doesn't know, from a newer Lens or added by hand, are kept when Lens saves, and an older Lens never writes over a file a newer one made.
- "Analyze with Lens" (Arcade Link's `lens.analyze`) opens on Wayland. It stopped with "monitor geometry is not exposed on Wayland" before showing anything; on Hyprland it now covers the focused monitor.
- `--status` and `--hide` no longer open the first-run install dialog (LN-1).
- The first-run install dialog opens on a fresh profile instead of failing with "The install dialog could not open": its fonts are installed before its first frame (LN-1).
- Ctrl+C copies in the capture overlay, the result panel and the annotator on Wayland (RD-LENS-1).
- Closed Settings, History and capture windows no longer stay behind as defunct processes. Lens reaps them within 2 seconds (LN-3).
- Esc closes the annotator at its top view; while you type text it still cancels only the text (LN-5).
- Esc in the actions list with a filter typed clears the filter first, then leaves the list (RD-LENS-2).
- Esc on a pin with its menu open closes only the menu (RD-LENS-3).
- A copy from a capture window that would be lost when the window closes is an error that names the package to install (`xclip`, or `wl-clipboard` on Wayland), not a success with an empty clipboard (LN-2).
- A window recording never outlives Lens: its ffmpeg is stopped and the file finished when the recording ends or Lens drops it, instead of recording on with no way to stop it (LN-6).
- A pin opened from a capture keeps the screen area it came from, so X11 pins offer Live (LN-7).
- `--status` reports `"overlay":true` while a capture overlay opened by another Arcade app is on screen (LN-11).
- Text that OCR reads off a QR code's modules or a photo's texture no longer adds a junk text result: lines it barely believes are dropped (LN-12).
- Digit groups in an IBAN are no longer offered as a phone number (LN-13).
- Plain lines of text with wide gaps between words are no longer detected as a table; the columns must line up (RD-LENS-6).

### Removed

- Record Window is not offered on Wayland, which has no window list or screen grab for it (LN-6).

### Security

- Lens hands images to its own windows (Wayland) and to KDE Connect through a private per-user folder (`$XDG_RUNTIME_DIR/arcade-lens`, or `arcade-lens-<uid>` in the temporary folder), created `0700` and refused if it is a symlink, belongs to someone else or is open to group or other. Before, a folder another user created first could receive your screenshots.
- Send to Phone writes the image with mode `0600`, under a name unique to each share, and deletes shares older than an hour.
- A chain with a step that sends something out is no longer offered, and is refused even when confirmed, if the selection contains a secret and "guard secrets" is on, the same as a single action.
- A history entry whose selection holds a recognized secret stores a plain grey tile instead of the picture, so the history no longer keeps the secret's pixels in its thumbnail (LN-8). Entries saved by earlier versions are not rewritten; clear the history to remove them.

## [0.4.2] - 2026-11-06

- Keep the resolved palette local to each GUI thread, so another context cannot change a frame's light/dark colours. A concurrent-context regression covers the Linux CI failure.
- Clear inherited AppImage library and launcher paths when starting an explicit restart successor, so the fresh launcher uses its own environment.
- Keep Settings keyboard shortcuts from consuming keys while the shortcut recorder is active. Ctrl/Cmd+K can be recorded instead of switching to Actions; Ctrl+W/Q and F1 also remain available to the recorder. Normal Settings shortcuts resume when recording ends.
- Test real egui input through Settings key handling and the shared recorder, including Ctrl/Cmd+K capture and ordinary Actions-filter navigation.

## [0.4.1] - 2026-11-01

The first release of the new look. The `v0.4.0` tag was made but never
released: its release build failed the capture focus check, fixed here.

- A capture keeps asking for focus until the window is actually mapped (for
  at most 15 s), then for a moment more while a window manager settles
  focus. On a heavily loaded machine without a window manager, a second
  capture in the resident UI could open without keyboard focus, so Esc went
  to another app.
- A new look shared with the other Arcade apps: Lens's teal accent, the Inter
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
- Annotate: a mark starts exactly where you press, not a few pixels further
  along. The stroke width control shows its track, undo and redo have clearer
  icons, and the toolbar fits on one row.
- Built against Arcade Link 0.4.14. Licences are listed in
  THIRD_PARTY_NOTICES.md.

## [0.3.4] - 2026-10-25

- Copy actions work on Wayland again: each capture runs in its own process, and
  the copied text or image disappeared when that process closed. Lens now
  hands the clipboard to `wl-copy` (or `xclip`), so it stays after the overlay
  closes. On X11 the clipboard no longer empties after each copy either.
- No more flicker on Hyprland when the overlay opens or closes. It used to
  appear as a tiled window that pushed the other windows aside before going
  full screen, draw its first frame in a corner, and on closing show a black
  frame while the desktop faded back in. The overlay now opens floating and
  full screen at your monitor's size, and closes straight to the desktop.

## [0.3.3] - 2026-10-19

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.
- Restarting in the background no longer keeps the previous image mounted.

## [0.3.2] - 2026-10-19

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.

## [0.3.1] - 2026-10-18

- Updating or installing from your Downloads folder, restarting into the
  background and starting other Arcade apps no longer leave the old AppImage
  mounted and its runtime (about 20 MB) running. A program the app started
  inherited an open handle on its AppImage mount and kept it busy.

## [0.3.0] - 2026-10-18

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

## [0.2.0] - 2026-10-14

- Add Arcade Link v0.3.0 hub methods, tray hosting and portable settings.
- Add receipt-owned installation, repair and removal, with Linux first-run setup and macOS Applications support.
- Use the shared per-user Windows installer integration and release tools from Link v0.3.0.
- Add user guides, generated shortcut tables and CI checks with isolated profiles on Rust 1.88.

[Unreleased]: https://github.com/qa-p1/Arcade-lens/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/qa-p1/Arcade-lens/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/qa-p1/Arcade-lens/compare/v0.4.2...v1.0.0
[0.4.2]: https://github.com/qa-p1/Arcade-lens/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/qa-p1/Arcade-lens/compare/v0.3.4...v0.4.1
[0.3.4]: https://github.com/qa-p1/Arcade-lens/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/qa-p1/Arcade-lens/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/qa-p1/Arcade-lens/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/qa-p1/Arcade-lens/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/qa-p1/Arcade-lens/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/qa-p1/Arcade-lens/compare/v0.1.1...v0.2.0
