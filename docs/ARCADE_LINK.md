# Arcade Link

Arcade Lens works with the other Arcade apps (Box, Look, Wheel, Clipboard)
through [Arcade Link](https://github.com/qa-p1/Arcade-link): a file-based
registry and one local socket per running app. Lens works exactly the same
when no other Arcade app is installed.

## What Lens exposes

| Action | Accepts | Returns | Notes |
|---|---|---|---|
| `lens.capture` | — | `file/image` (PNG handoff file), `screen/region` (`{rect, monitor}`, physical pixels) | Lens's freeze overlay, multi-monitor and DPI-correct. The first selection is handed back; Esc cancels (`denied`, `user_cancelled`). |
| `lens.capture_and_act` | — | — | The full Lens flow (select, then the palette). `options.mode` accepts `palette` (default), `measure`, `pin`, or `color`; see modes below. |
| `lens.analyze` | `file/image` | — | The palette over an image, shown as Lens's frozen overlay with the image selected. |
| `lens.recognize` | `file/image`, `text/plain` | `text/plain` (OCR text, if any), `structured/findings` | Headless. `options.ocrOnly: true` runs only OCR (Arcade Box's OCR provider). Also served one-shot (`arcade-lens --arcade-invoke`), so Lens needn't be running; the OCR model loads on first use. |
| `lens.pin` | `file/image` | — | A floating pin. |

Interactive actions start Lens in the background if it isn't running. On
Wayland each of them opens its own window process, like Lens's own windows.
`app.status.status.mode` reports `background` for an instance started with
`--background`, or `foreground` for a first launch into Settings/capture.
The value describes startup and is unchanged when a window is opened later.

## Capture modes and input lifetime

`lens.capture_and_act` accepts `options.mode`:

- `palette` (default): select a region and choose an action.
- `measure`: drag to measure, with the existing ruler and snapping.
- `pin`: select a region and open it directly as a floating pin.
- `color`: click a pixel to sample its exact color, or drag a region for
  color and representative palette findings. The palette offers HEX, RGB,
  HSL and color export actions.

The mode reaches Wayland's window process as well. Unknown modes fail with
`unsupported_input`. `lens.capture` always returns the selected pixels.

`lens.pin` and `lens.analyze` decode and own the image before returning
success. The caller may delete its handoff immediately after success; the
GUI uses Lens's decoded pixels. Wayland copies those pixels into its own
private window handoff before acknowledging the request.

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

## Verification

Build Lens with `cargo build --release -p arcade-lens`, then run
`python3 ../Arcade-link/tools/e2e.py --only lens`. The Lens check module
uses the real binary in a private Xvfb/D-Bus session and tests one-shot
and resident recognition on a generated PNG, a 240 × 160 pixel capture,
Escape cancellation (`denied: user_cancelled`), Analyze, Pin, and
Capture and act. It does not use the live desktop or the user's profile.
Recognition works without downloaded OCR models; these checks verify
image findings, and do not claim OCR model coverage.

## Connected actions

Lens caches installed peers on a directory watcher. Registry reads, live
presence checks, input size checks and palette rebuilding run on workers.
Opening or filtering the palette does no IPC or disk access. Late entries
use the existing palette stabilizer and keep surviving actions in place.

| Finding | Connected action |
|---|---|
| Existing path, file URL | **Quick Look** (`Y`) in Arcade Look |
| Region or image | Box's featured image presets, plus **More in Arcade Box…** |
| Text, URL, image, file | **Send to my devices ↗** (`M`) through Arcade Clipboard |
| URL, command, path, text | **Add to Wheel**, confirmed in Wheel Settings |
| Finding matching a saved pipeline's first input | **▶ Pipeline name** in Arcade Box |

Box's `box.pipelines` list is fetched on a registry worker when its manifest
changes or a background instance appears. Installed, stopped Box uses its
headless one-shot mode. The palette reads only the cached list; it hides
interactive-first pipelines and checks each first input type. Invocation
sends `box.pipeline.run` with `options.pipeline`. The pipeline's declared
effects feed Lens's existing confirmation and secret guard.

Owning apps have monochrome glyph badges. Outbound actions show ↗ and
the payload preview. Image sends are removed when Lens finds a secret,
including at execution time. Oversized inputs stay in More with the
standard limit reason and have no shortcut. Clipboard's Private mode
refusal never falls through to KDE Connect. Without Clipboard, the existing
KDE Connect actions are unchanged; without any peers, fixture palettes
match the standalone registry exactly.

Settings → **Connected apps** has **Connect with other Arcade apps** and
per-peer **Use with Arcade Lens** toggles. Save applies them. The master
switch removes Lens's actions and stops its listener. Get delegates to
`tools.install` with `options.app` if available, otherwise opens the app's
releases page. Diagnostics and shortcut clash warnings use cached state.

Quick Look prefers the Link on all three desktop platforms. On Linux, an
unsuccessful preview falls back to GNOME's D-Bus previewer, then the default
opener. Look currently supports `file://` URLs; web URLs are hidden because
its no-execution sandbox rejects them. Headless recognition starts no
consumer watcher or peer. Linux X11 is exercised in the isolated runner;
Linux Wayland uses the same cache in its existing window processes.
Windows/macOS integration code is build only here and has not been run.

The runner waits for Lens's full-size overlay and focuses it synchronously
before typing in Xvfb (which has no window manager). A visible 1 × 1 root during startup or child-window
creation is not an overlay. Checks also wait for unmapping between actions,
so an Escape cannot be sent to the previous window during a transition.
Automatic focus after rapid remapping remains unreliable under Xvfb: the
native-focus check was not consistently successful. Automation's explicit
focus does not prove that Lens always takes keyboard focus itself.
