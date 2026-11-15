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
- D8. Memory (step 5): AUDIT/memory.md lists two items for Lens: W0 (Lens 15/12 → 6/4 MB RSS/PSS measured) and U1. The
  resident trims once five seconds after it starts and after each hide of the in-process overlay
  (`ARCADE_LENS_RESIDENT_UI=1` on X11), through Link's `runtime::trim` (RT-5); its capture, settings and editor
  windows are separate short-lived processes already. A resident start in an AppImage relaunches from
  `<data>/arcade/runtime/lens-<version>-<key>/` (`runtime::relaunch_unpacked`, RT-7) and the launcher exits 0; the
  window processes it spawns run from the same copy. One-shot commands (`--status`, `--hide`, `--quit`, and any
  command a running instance takes) never relaunch. The budget (8 PSS / 8 RSS) is measured on the published rc by the
  gates' budget job.
- D9. Release gates (step 7): `.github/workflows/gates.yml` runs PLAN's exit gates on the published files of an rc
  through Link's reusable `app-gates.yml@v1.0.9` (published files and SHA256SUMS, the rc's manifest, conformance, N-1,
  budgets, the delta through Tools' updater, the Windows and macOS smoke on the published installers), with Lens's
  Linux checks on the published AppImage (the package smoke, the upgrade, the tray's lifecycle) and the Tier 2
  desktop smoke. "Promote" runs only after a green Gates run on the rc it promotes. Lens had no upgrade test:
  `scripts/verify-upgrade.py` installs the real previous release from a download folder, gives it a user's settings
  (non-default values, Link off), a saved chain and learned usage, runs it, installs the candidate over it and checks
  that the new version reads the settings (`--status` shows Link off), every setting value is kept, the chain and
  usage files are unchanged, there is one launcher and one login item, and an uninstall without `--remove-data` keeps
  the data. CI runs it from 0.4.2 on every build. No Tier 1 script yet: the overlay's focus rule is checked on Xvfb
  (`tests/overlay-focus.py`, CI); LN-10 and RD-LENS-4 still need the Hyprland harness.
- D10. Link 1.0.11 (step 8 prep): Lens moves from Link 1.0.7. What touches Lens: 1.0.11 publishes release files
  under names GitHub keeps and gives the unpacked copy's marker (`ARCADE_UNPACKED`) its own folder, so a restart
  successor (`gui/background.rs`, started from `$APPIMAGE`) no longer inherits "already unpacked" and stays on the
  FUSE mount; 1.0.10's uninstall removes Lens's own copies under `<data>/arcade/runtime/lens-*`; 1.0.9 is the gates
  workflow D9 already uses; 1.0.8's changes are in the Qt/QML and Dart kits Lens does not use. The vendored files
  and the design kit are byte-identical at v1.0.11 (only the recorded tag moves). Every Link workflow is pinned to
  `v1.0.11`.
- D11. Security, licences and notices (Round 5). `deny.toml` is Arcade Box's, with `allow-git` for Arcade Link only; the
  licence list holds only licences the tree needs, plus `Ubuntu-font-1.0` for egui's bundled Ubuntu Light. Advisories
  with a fix inside the range Lens already depends on were fixed with `cargo update -p … --precise …` (rustls 0.23.46,
  rustls-webpki 0.103.17, webpki-roots and webpki-root-certs 1.0.12, tokio 1.54.1, rustix 1.1.6, async-executor 1.14.1,
  clipboard-win 5.4.3), and `cargo check --workspace --all-targets` passes on Rust 1.88. Seven are accepted, each with
  its reason in `deny.toml`: RUSTSEC-2024-0429 (glib 0.18) and RUSTSEC-2024-0370 (proc-macro-error) only through
  `tray-icon`'s GTK backend, which no Lens build compiles (Lens takes `tray-icon` on Windows and macOS, `ksni` on
  Linux); RUSTSEC-2024-0436 (paste, a build-time macro in the exr decoder rxing's `image` enables); RUSTSEC-2026-0192
  (ttf-parser, which reads only the fonts compiled in); RUSTSEC-2026-0370/0371/0372 (zvariant, zbus_names,
  zvariant_derive, merged into zbus 6 while `ashpd` and `ksni` use zbus 5). The notices list only the crates a shipped
  build contains: `cargo tree --target` for Linux x86_64, Windows x64 and both macOS targets, not every crate in the
  lock file. The AppImage bundles no library of its own (`build-appimage.sh` packs the binary, launcher, icon and
  licences, with no linuxdeploy), so there is no native-library generator; `--check-appimage` enforces that, and
  records the six components of the AppImage runtime. Every package now carries the licences and the notices.

