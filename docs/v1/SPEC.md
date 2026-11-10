# Spec 7: Arcade Lens 1.0.0

**Outcome:** Lens 1.0.0. It delivers:
- a clean first launch;
- copy works everywhere;
- no secrets leak into history;
- no zombies or orphan recorders;
- the Escape ladder in every window;
- accurate recognizers.

Starting point: `qa-p1/Arcade-lens`, Lens 0.4.2. Read `AUDIT/lens.md`, `AUDIT/real-desktop.md` §Lens and
`AUDIT/box-real.md` BP-6.

## Findings to fix

| ID | P | What | Where |
|---|---|---|---|
| LN-1 | P0 | On a fresh profile, the first-run install dialog panics (`FontFamily::Name("Arcade Semibold") is not bound to any fonts`). That blocks first launch and `--status`. Install fonts before the first frame, and handle `--status` before any first-run UI. | `crates/arcade-lens/src/main.rs:92`, `src/app/installer_ui.rs:308`, `src/gui/theme.rs:127` |
| LN-8 | P0 | History thumbnails show detected secrets in full; only the JSON text is masked. Mask secret regions in the thumbnail, or skip the thumbnail. | `crates/arcade-lens/src/gui/history.rs:44-106` |
| RD-LENS-1 | P0 | Ctrl+C does nothing on Wayland in the overlay, the annotator and the result panel: egui-winit 0.33 delivers `Event::Copy`, which Lens never handles. | `gui/overlay.rs:950-1017`, `gui/annotate.rs:348` |
| LN-3 / BP-6 | P1 | Closed Settings, capture and history children, and `notify-send`, stay as zombies. Reap on exit, using Link's spawn helper. | `gui/wayland.rs:259,377-405`, `gui/mod.rs:159-167` |
| LN-6 | P1 | X11 Record Window leaves an orphan ffmpeg with no Stop UI. The recorder belongs to the resident, with a visible Stop. On Wayland, hide the action and explain why (no window list). | `gui/mod.rs` RecordWindow, `gui/pins.rs:303-341` |
| LN-7 | P1 | X11 pins lose their source origin, so Live refresh is missing. | `gui/mod.rs:92-96`, `gui/wayland.rs:88-110`, `gui/pins.rs:216` |
| LN-2 | P1 | On X11 without `xclip`, copy reports success but the clipboard ends up empty. Keep the clipboard owner alive through Link, or show an actionable error. | `host.rs:155-222`, `gui/mod.rs:208-218` |
| LN-4 / RD-LENS-5 | P1 | A focus change closes the capture overlay and drops the selection (the fleet popup rule). | `gui/mod.rs:650-677` |
| LN-5 | P1 | Escape doesn't close the annotation editor at its top view. | `gui/annotate.rs:330-385` |
| RD-LENS-2 / RD-LENS-3 | P1 | Escape in the actions list with a filter typed leaves the list instead of clearing the filter; Escape on a pin with its menu open closes the whole pin. | overlay / pins |
| RD-LENS-4 | P2 | The pin context menu is clipped by the small pin window. | pins |
| LN-9 / LN-10 / LN-12 / LN-13 / RD-LENS-6 | P2 | Hex and rgb colour text isn't recognized; the loupe shows the dimmed colour; QR images add junk OCR findings; IBANs flagged as phone numbers; three plain lines detected as a "Table". | recognizers |
| LN-11 | P2 | `--status` reports `overlay:false` while a Link-opened overlay is on screen. | status |

## CI

Measured: main CI takes 8–24 minutes. Apply `PLAN.md` step 6.

## Acceptance

All `PLAN.md` exit gates, plus:
- Fresh-profile first launch and `--status` tests.
- Secret-masking test on a history thumbnail (pixel check).
- Ctrl+C on Wayland in all three views.
- A zombie check after 20 open/close cycles.
- The recognizer test set from `AUDIT/lens/fixtures/` passes.

## Memory

Items U1 and W0. Budget: background 8 PSS / 8 RSS (measured 6/5).
