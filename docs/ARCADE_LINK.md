# Arcade Link

Arcade Lens works with the other Arcade apps (Box, Look, Wheel, Clipboard)
through [Arcade Link](https://github.com/qa-p1/Arcade-link): a file-based
registry and one local socket per running app. Lens works exactly the same
when no other Arcade app is installed.

## What Lens exposes

| Action | Accepts | Returns | Notes |
|---|---|---|---|
| `lens.capture` | — | `file/image` (PNG handoff file), `screen/region` (`{rect, monitor}`, physical pixels) | Lens's freeze overlay, multi-monitor and DPI-correct. The first selection is handed back; Esc cancels (`denied`, `user_cancelled`). |
| `lens.capture_and_act` | — | — | The full Lens flow (select, then the palette). `options.mode: "measure"` starts in measure mode. |
| `lens.analyze` | `file/image` | — | The palette over an image, shown as Lens's frozen overlay with the image selected. |
| `lens.recognize` | `file/image`, `text/plain` | `text/plain` (OCR text, if any), `structured/findings` | Headless. `options.ocrOnly: true` runs only OCR (Arcade Box's OCR provider). Also served one-shot (`arcade-lens --arcade-invoke`), so Lens needn't be running; the OCR model loads on first use. |
| `lens.pin` | `file/image` | — | A floating pin. |

Interactive actions start Lens in the background if it isn't running. On
Wayland each of them opens its own window process, like Lens's own windows.

## Settings

`[link]` in `settings.toml`: `enabled` ("Connect with other Arcade apps")
and `disabled_peers` (the per-app "Use with Arcade Lens" toggles). With the
switch off, Lens's manifest lists no actions and nothing listens.

## Command line

```sh
arcade-lens --arcade-manifest     # Lens's manifest (no side effects)
arcade-lens --arcade-invoke       # one Link request on stdin (lens.recognize), no window
arcade-link invoke lens lens.capture --json     # with the Arcade Link debug CLI
```

## Platforms

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Exposed actions | tested (Xvfb) | build only (window processes, as for Lens's own windows) | build only | build only |
