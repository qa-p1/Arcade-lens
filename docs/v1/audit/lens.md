# Arcade Lens 0.4.2 audit

Method: installed release AppImage in isolated HOME/XDG/private D-Bus, Xvfb :92. After the first-run crash, a byte-identical copy was silently installed **inside disposable HOME only** to get past the broken dialog. Source is read-only. Evidence: `lens/`.

## Coverage
| Feature | Tested? / method / inputs | Result |
|---|---|---|
| CLI help, version, manifest | CLI | pass; help.txt, manifest.json |
| Fresh profile --status | CLI + GUI startup | LN-1 |
| Resident --background, --status, --capture, --hide | CLI + Xvfb | pass after isolated installation |
| Region capture + Save PNG | Xvfb drag 340x200, Tab, "save", Enter; PIL pixel diff vs screen crop | pass: pixel-identical (diff bbox None); lens/ln-save.png |
| Save JPEG/WebP | earlier session; PIL | pass: 200x150 JPEG/WEBP (image-output-validation.json) |
| Window pick, colour pick (Link mode=color), measure | Xvfb | colour pick pass (#FF0000 after click); LN-10, LN-11; measure earlier pass |
| OCR on 28 generated images (fonts 8-24 pt, mono, serif, rotated, dark/light, multi-line, accents) | Link `lens.recognize` batch; lens/recognize-batch.json | pass for clean text; Tesseract eng only so accents lost (naive, ZUrich), 8 deg rotation garbles a letter; not Lens bugs |
| Content recognizers: URL, email, date, phone, currency, code/table, secrets (token, AWS key, password), QR (URL/text/Wi-Fi), flat colour, 6000x4000 and 1x1 images | same batch | pass; LN-9, LN-12, LN-13 |
| Expanded/filter/result panels, action chips, Escape ladder | Xvfb keys + --status | pass: Tab opens list, Esc back, Esc closes; result panel Esc back; footers match (ln-esc.png) |
| Confirm/choice panels | not reachable with the actions tried | untested |
| Annotation 8 tools, undo/redo, copy, save | earlier session | LN-5 |
| Pins: Esc, move/zoom/opacity/context | earlier + Link `lens.pin` here | Esc closes pin (pass); LN-7 |
| Link error paths (missing file, non-image, bad mode, empty inputs) | Link | pass: clear unsupported_input errors |
| Window recording | earlier | LN-6 |
| Settings: General/Shortcuts/Actions/Text recognition/Chains/Services/Plugins/Privacy/Connected apps/About | earlier screenshots, reviewed (lens/lens-settings.png), Esc closes Settings | pass |
| History off/on/clear/masking | earlier | LN-8 |
| Global shortcut recorder; light/dark | earlier (shortcut-*.png, settings-light.png) | pass |
| Focus-loss | Xvfb | LN-4 |
| Zombie children | fresh session: notify-send and Settings child remain defunct under resident (pids 214138, 221626); 8 capture cycles and pins left none | LN-3 reproduced |
| Idle/heavy resources, files | /proc, find | idle 80 MB RSS, 19 threads, 0.03% CPU over 30 s; 8 capture cycles +48 ticks, RSS flat; 6000x4000 OCR 0.44 s; writes only arcade/ dirs and ~/Arcade Lens (Pictures absent) |

## Findings
### LN-1 — broken — First-run install prompt crashes, blocking fresh-profile launch and --status
- Method: CLI and GUI startup in Xvfb, followed by source reading. `--background` bypasses prompting and does start; the later `--status`/foreground CLI is what fails.
- Steps: with a fresh isolated HOME, run original `Arcade-Lens.AppImage --status` (with no receipt). No install receipt is present.
- Expected: --status emits the promised one-line JSON without UI; a normal first launch shows a usable install/Just run it dialog.
- Actual: child install dialog panics: `FontFamily::Name("Arcade Semibold") is not bound to any fonts`; caller exits 1 with `The install dialog could not open`. --status prints no JSON. Repeated with --status. Silently installing a copy in disposable HOME bypasses the failure.
- Evidence: `lens/fresh-status-panic.txt`, `lens/startup-status.txt`. No owner installation was changed.
- Suspected source: `crates/arcade-lens/src/main.rs:92` calls first_run_install before handling status; `src/app/installer_ui.rs:308` first installs fonts during the update frame and uses the new named family in that same frame. `src/gui/theme.rs:127` / shared Link egui install_inter are the font setup path. This is a suspected immediate-mode font initialization ordering cause, not a source fix.

### LN-2 — broken — Copy image and OCR text report success but leave the clipboard empty without xclip
- Method: real Xvfb GUI selection/actions + GTK clipboard consumer + code reading.
- Steps: on X11 with no `xclip` installed, capture the known GTK text window; press Ctrl+C for the image, or Tab → filter "Copy Text" → Enter for OCR text. Read the private display's clipboard using `Gtk.Clipboard.wait_for_image` / `wait_for_text`.
- Expected: copy survives the capture window closing, or an explicit actionable dependency error.
- Actual: overlay closes as if copied, GTK returns None for both image and text. Repeated twice. No xclip is present; no Wayland clipboard tool was invoked. OCR itself works (recognized text is visible in expanded-copy-text.png).
- Evidence: `lens/expanded-copy-text.png`, `lens/ocr-copy-result.png`, `lens/zombie-after-copy.txt`; additional copy-success evidence continues below.
- Suspected source: `crates/arcade-lens/src/host.rs:155-178`, `:209-222`. In solo capture windows, without xclip, `with_clipboard` creates a temporary arboard clipboard and drops it immediately after set; the owning selection disappears. `gui/mod.rs:208-218` uses short-lived host for solo windows on X11 too.

### LN-3 — UX — Regular closed windows leave zombie child processes
- Method: GUI + /proc/ps in isolated session; source reading. Owner's reported PID was inspected read-only, never signaled.
- Steps: open Settings, close with Esc; open capture and close it after using an action. Before --status or reopening that same window, inspect resident children. Repeat settings/capture uses.
- Expected: every child exit is reaped promptly.
- Actual: `[arcade-lens] <defunct>` children remain attached to resident 126495; e.g. Settings child 134417 and capture child 135572 persisted through later Link picker calls. Also `[notify-send] <defunct>` 126511 remains. Owner's reported 27116 is still Z with PPid 27008.
- Evidence: `lens/zombie-after-natural-close.txt`, `lens/link-picker-tests.json`, `lens/zombie-after-copy.txt`.
- Fresh-session repeat: the Settings child (221626) and the startup `notify-send` (214138) were defunct under the resident (214108) immediately after Esc closed Settings; capture cycles and Link pins left none.
- Source/cause: `crates/arcade-lens/src/gui/wayland.rs:377-405` retains `Option<Child>` for capture/settings/history; no exit watcher reaps those slots. `open_once` (:259) only calls try_wait on the next open, and Status (:394) only reaps capture. Rust Child drop does not wait. The daemon loop blocks on `rx`, not child exit. `gui/mod.rs:159-167` fire-and-forget notify-send is also never waited.
- Important attribution limit: the three `lens.capture` Link picker trials (context arcade.box; success/cancel/success) returned correct output/cancellation and added **no** zombie. Its worker explicitly waits at `wayland.rs:241`. Thus the reported zombie is real and the ordinary-window cause is reproduced; attributing that particular old zombie to the region-picker worker would be unsupported. No fix applied.

### LN-4 — UX — Capture overlay closes on a focus change
- Method: Xvfb GUI with private GTK target and CLI status.
- Steps: start capture, draw a selection, focus Lens, wait >300 ms, focus Audit Target, wait 0.5 s.
- Expected: owner rule says focus blips/changes must never dismiss a popup.
- Actual: overlay closes; --status reports overlay:false. Pointer leave alone is tested separately.
- Evidence: `lens/focus-loss.png`, `lens/focus-status.json`.
- Source: `crates/arcade-lens/src/gui/mod.rs:650-677`, `close_overlay_on_focus_loss`, sets close_requested after 300 ms.

### LN-5 — UX — Escape does not close the annotation editor at its top view
- Method: GUI Xvfb + source.
- Steps: select → Annotate; draw pen/line/arrow/rectangle/ellipse/highlighter/pixelation and text; complete text entry with Enter, then press Escape.
- Expected: top-view Escape closes editor; inside text entry it cancels only text.
- Actual: after leaving text entry the annotation window remains; Done closes it. Text-entry cancel is implemented separately.
- Evidence: `lens/annotate-tools.png`, `lens/annotate-text.png`; test transcript in annotation evidence notes.
- Source: `crates/arcade-lens/src/gui/annotate.rs:330-385` handles Ctrl+C/S/Z/Y and tool keys; Escape only handled by text input, no top-level close binding.

### LN-6 — broken — Record Window leaves an orphan recorder without Stop UI
- Method: GUI Xvfb, actual FFmpeg and ffprobe. EWMH window-list property populated only on private :92 for known 900×600 GTK target (no WM).
- Steps: capture → click Audit Target window → Tab → filter Record → Enter.
- Expected: visible recording control with Stop, and recorder lifetime owned by Lens.
- Actual: overlay process exits; no Recording window appears. FFmpeg continues recording :92+140,120 at 900×600 after its Lens parent has exited. File remains unfinalized until the auditor sends SIGINT **only to that verified isolated FFmpeg**. Cannot stop through normal UI.
- Evidence: `lens/record-action.png`, `lens/recording.png`, `lens/recording-state.txt`, `lens/recording-ffprobe.json`, `lens/recording.mp4` (finalized by auditor cleanup).
- Suspected source: `gui/mod.rs` UiCommand::RecordWindow / solo close lifecycle, plus `gui/pins.rs:303-341`. Recording ownership is local to solo window state, but the successful overlay action requests that window close. Record control viewport never outlives the root. Needs implementation-level confirmation; observed release failure is definite.

### LN-7 — broken — Live pins lose their screen origin in the default process model
- Method: GUI Xvfb real screen capture, pin menu + geometry, code reading.
- Steps: capture a 200×150 region at (500,500) in the GTK test window; run Pin; open the pin's context menu.
- Expected: pin initially aligns with source; X11 pin offers Live refresh for a real screen-region source.
- Actual: pin appears at the generic default position instead of its source; menu has no Live item. Zoom/opacity controls work, but live update cannot be enabled.
- Evidence: `lens/pin-source-region.png`, `lens/pin-context-menu.png`, `lens/pin-zoomed.png`, `lens/pin-opacity.png`.
- Cause: `crates/arcade-lens/src/gui/mod.rs:92-96` solo add_pin discards origin and calls open_image with only image bytes. `gui/wayland.rs:88-110` serializes only PNG; solo Pin initializes source=None. `gui/pins.rs:216` hides Live when source is None. Default X11 uses the same solo process model (`gui/mod.rs:1172`).

### LN-8 — broken — History saves recognized secrets visibly in thumbnails
- Method: actual Lens GUI + OCR with a deliberately fabricated nonfunctional token image, filesystem output inspection, source reading.
- Steps: Settings → Privacy → Keep a history on. Analyze the 1100×180 test PNG containing `ghp_FAKE-TEST-TOKEN-REDACTED`; allow OCR to finish; Escape; inspect saved history PNG and JSON.
- Expected: "Thumbnails and what was recognized, with secrets masked" (the switch's own description); sensitive pixels should be masked or thumbnail omitted when a secret is detected.
- Actual: OCR identifies secret; JSON text/secret summaries correctly show `ghp_…abcd`. The saved thumbnail shows the **whole** token, fully readable. Nothing is masked in image pixels. No real credential was used.
- Evidence: `lens/history-enabled.png`, `lens/secret-palette.png`, `lens/history-20261106-193416871.json`, `lens/history-20261106-193416871.png`, `lens/history-on.png`.
- Cause: `crates/arcade-lens/src/gui/history.rs:94-106` thumbnails and saves the original image; `masked_summary` (:44-87) masks only JSON strings. This defeats the history UI's stated masking guarantee.

### LN-9 — wrong result — Colour codes written in text are not recognized
- Method: Link `lens.recognize` on generated image `fixtures/hex.png` ("Brand colour #FF8800 and rgb(12, 34, 56)").
- Expected: README promises colours; a hex/rgb in text should offer Copy/convert colour.
- Actual: only a text finding; colour recognizer (`core.color`) runs only on flat-colour image regions. Evidence: `recognize-batch.json`. Source: `crates/lens-recognizers/src/text/` has no colour recogniser (`image/color.rs` only).

### LN-10 — UX — Colour-pick loupe label shows the dimmed overlay colour
- Method: Link `lens.capture_and_act` mode=color, hover over the red block (255,0,0) in the target window.
- Actual: loupe caption reads `#161B1D 200, 540` while hovering; the panel and the clicked result correctly say `#FF0000`.
- Evidence: `lens/ln-c3.png` (hover), `lens/ln-c4c.png` (result). Source: not located (loupe sampling in the colour overlay).

### LN-11 — polish — `--status` reports `overlay:false` while a Link-opened overlay is on screen
- Method: Link `lens.capture_and_act` mode=color, then `arcade-lens --status` with the overlay visible.
- Actual: `{"overlay":false}` while the colour picker covers the screen; the CLI `--hide` path may not close it. Evidence: `lens/ln-c3.png` plus status output above.

### LN-12 — polish — QR images produce junk OCR text findings
- Method: Link `lens.recognize` on `fixtures/qr_text.png`, `qr_wifi.png`.
- Actual: correct qr-code finding plus a text finding with garbage ("=]k,", "Ofzs0", confidence 0.0-0.1) that would offer Copy Text. Suppress low-confidence OCR when a code was decoded.

### LN-13 — polish — IBAN digits flagged as a phone number
- Method: Link `lens.recognize` on `fixtures/iban.png` ("IBAN GB82 WEST 1234 5698 7654 32").
- Actual: `phone` finding "1234 5698 7654 32" at 0.75 confidence. Source: `crates/lens-recognizers/src/text/contact.rs`.

## Unconfirmed / needs real desktop
Wayland/compositor behavior, real tray hosting, and external service actions require further testing. Source suggests focus-loss auto-close and delayed child reaping; not yet counted without reproduction.
