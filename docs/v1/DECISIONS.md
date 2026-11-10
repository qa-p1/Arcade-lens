# Lens 1.0 decisions

- D1. LN-8: a history entry whose selection holds a recognised secret stores a solid placeholder thumbnail, not the
  picture. Findings carry only text spans, no pixel regions, so masking regions is not possible; skipping the pixels is.
- D2. RD-LENS-1: Ctrl+C arrives as `egui::Event::Copy` (no Key event). `gui/keys.rs` maps it back to a Ctrl+C key
  press for the overlay/result panel and consumes it in the annotator, so one code path serves both forms.
- D3. LN-1: `--status` and `--hide` join the list of commands that skip the first-run install dialog; the install
  dialog applies its style in the eframe creator so fonts exist before its first frame.
- D4. LN-3: Link v1.0.6 `runtime::reap`/`spawn_reaped` reap fire-and-forget children (notify-send, pin/annotate
  windows); tracked window slots are reaped by a watcher thread that also reports the exit.
- D5. LN-4: the overlay no longer closes on focus change (fleet popup rule); only Escape, an action or `--hide`.
