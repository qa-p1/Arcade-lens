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
- D7. Dead code (step 4): only what no tool or search can find a user for was removed; everything else stays, with
  the reason below. Tools: `cargo machete` (no unused dependency), rustc `dead_code` through clippy with
  `-D warnings` (clean before and after), and, because rustc cannot judge `pub` items in library crates, a
  throwaway build with every `pub` item of one library crate narrowed to `pub(crate)`, items the other crates
  need restored from the compiler's privacy errors, and the remaining `dead_code` warnings checked by grep over
  crates, tests, examples, scripts, packaging, CI and docs.
  - Removed, no reference anywhere: lens-core `ActionRef`, `Analysis::cancel_token`, `Engine::settings`,
    `Registry::plugins`, `DateTimeValue::is_ambiguous`, `Host::share_text` (trait default, `RecordingHost`
    override and `HostCall::ShareText`; no host implements or calls it); lens-core `Rect::is_empty`;
    lens-recognizers `units::find_unit` and `register` (only a rustdoc link named it; `register_builtins` is what
    Lens calls); lens-platform `Shortcuts::current_id`.
  - Removed, referenced only by its own unit test: lens-core `geometry::monitor_for` (the app picks monitors in
    arcade-lens `gui/geometry.rs`); the test keeps its `Rect::to_logical` assertion.
  - `allow(dead_code)` on `autostart::write_if_changed` became the `cfg` its callers have (linux, macos).
  - `[dev-dependencies]` entries that repeated a normal dependency (lens-actions: lens-recognizers; lens-plugins:
    lens-core) were dropped; `Cargo.lock` is unchanged.
  - Kept although the compiler sees no non-test use, because tests of other crates (or a test pinning documented
    behaviour) use them: `HostCall`, `RecordingHost`, `NullEnvironment`, `Engine::registry`, `Analysis::wait` (the
    prompt-cancel test), `AnalysisReport::{with, first, has}`, `UsageStore::is_empty` (the reset test),
    `arcade::with_peers` (nine calls in `tests/arcade.rs`; the app calls `with_cached_peers`), and
    `PaletteEntry::needs_confirmation` (asserted by the dangerous-action tests). Never-read data fields kept as
    descriptive API: `ParamSpec::default`, `PluginStatus::name`.
  - Kept: the `allow(unreachable_code)` / `allow(unreachable_patterns)` on per-platform fall-through branches in
    lens-platform, net.rs and tray.rs (each branch is live on some platform), `allow(clippy::...)` (not dead
    code), and Link's `watch` feature (implied by `trayhost`, kept explicit because the registry relies on it).
  - `scripts/test-arcade-release.py` (Lens's package names through the release manifest writer CI's
    `release-manifest` job runs) was never run: the hyphen kept it out of `scripts/check.py`'s `test_*.py`
    discovery. It passes and is now `scripts/test_lens_release.py`, so `check.py` runs it.
  - Kept: `tools/test_*.py` (vendored copies, run by hand), `packaging/release-notes.sh` and
    `scripts/third-party-notices.py` (maintainer tools documented in their own headers; the second regenerates
    `THIRD_PARTY_NOTICES.md`), and the `signals` and `pdf` examples (CI compiles them under `--all-targets`).
