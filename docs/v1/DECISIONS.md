# Lens 1.0 decisions

- D1. LN-8: a history entry whose selection holds a recognised secret stores a solid placeholder thumbnail, not the
  picture. Findings carry only text spans, no pixel regions, so masking regions is not possible; skipping the pixels is.
- D2. RD-LENS-1: Ctrl+C arrives as `egui::Event::Copy` (no Key event). `gui/keys.rs` maps it back to a Ctrl+C key
  press for the overlay/result panel and consumes it in the annotator, so one code path serves both forms.
- D3. LN-1: `--status` and `--hide` join the list of commands that skip the first-run install dialog; the install
  dialog applies its style in the eframe creator so fonts exist before its first frame.
- D4. LN-3: Link v1.0.6 `runtime::reap`/`spawn_reaped` reap fire-and-forget children (notify-send, pin/annotate
  windows); tracked window slots (capture, Settings, History) are reaped by the background loop on every command and
  on a 2 s tick (`reap_exited`), so a closed window is a zombie for at most 2 s.
- D5. LN-4: the overlay no longer closes on focus change (fleet popup rule); only Escape, an action or `--hide`.
- D6. Builds from main and pull requests are labelled with the Cargo version (`0.4.2`), not `0.4.2-dev.<sha>`, like
  every other fleet app's. The binary reports its label (link.rs VERSION), so a `-dev` build is a prerelease: Link's
  ecosystem e2e published the main AppImage as a stable release for Tools' catalog-bundle and update checks, and
  Tools rightly refused a prerelease tag on the stable channel ("No stable release is published", Link e2e run
  42988304697, three checks). Release builds still get their label from the release workflow (rc, nightly).
