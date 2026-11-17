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
- D11. Tier 1 on headless Hyprland (step 7): `tests/desktop/lens_desktop.py` runs on Link's harness
  (`desktop-e2e.yml`) in CI on every build and in the gates on the published rc, from the AppImage as a user starts it
  (without the harness's `ARCADE_UNPACKED`). The harness has no screenshot portal, so the overlay is opened on a
  known image with `--window analyze` (the overlay Link's `lens.analyze` opens; same selection and keys as a
  capture). It checks: every window's class is `dev.arcade.Lens` (ID-1); the overlay maps floating and full-screen
  through the resident's window rules and has keyboard focus; Ctrl+C on a selection puts an image on the Wayland
  clipboard (RD-LENS-1, until now only a unit test of the key mapping); Escape closes the overlay and its process
  exits; on a pin with its menu open the first Escape closes only the menu (RD-LENS-3); Settings floats and Ctrl+W
  closes it; `--status` and `--quit`. Tier 1 blocks the gates on any failure. LN-10 and RD-LENS-4 (P2) need to read
  what egui draws, which the harness cannot; they are in `BACKLOG.md` with the reason.
- D12. Security, licences and notices (Round 5). `deny.toml` is Arcade Box's, with `allow-git` for Arcade Link only; the
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
- D13. Security hardening of three open `SECURITY.md` rows. One function, `gui/private_dir.rs`, returns the folder for
  files handed to another process: `$XDG_RUNTIME_DIR/arcade-lens` if that is a directory the user owns, else
  `<temp>/arcade-lens-<uid>`; created `0700`, then verified with `symlink_metadata` (real directory, our uid, no
  group/other bits) and refused otherwise, so a folder planted by another user is never written into. The Wayland
  hand-over and Send to Phone both use it; shares are `0600`, named by pid, time and a counter, and shares over an hour
  old are deleted when a new one is written (KDE Connect reads the file after the command returns). A chain with an
  outbound step follows the secret guard in `Chain::run` and the palette: any secret finding refuses it, confirmed or
  not (stricter than the single-action text check, since a chain's later steps are not previewed). Not changed, in
  `BACKLOG.md`: plugins with `read-text` still receive secrets, and settings, chains, history and logs keep the umask.

- D14. Link 1.0.12's workflows. Link 1.0.11's `setup-arcade-link` cached its CLI under `runner.os` only, so a gates job
  on ubuntu-22.04 could restore a CLI built on ubuntu-24.04 ("GLIBC_2.39 not found"; Box's rc.2 gates hit it). Every
  reusable Link workflow and `link-ref` is pinned to `v1.0.12`, which keys the cache by runner image. The crate and the
  vendored files stay at `v1.0.11`: 1.0.12 changes no Rust code or design file.

- D15. Schema versions and safe downgrade (Round 5, Link FS-2; the model is Arcade Box's D19). Every data file Lens
  reads back has a version: no `schema` field means 1, the shape every release so far wrote, and the field is written
  only above 1, so a v1 file stays exactly what 0.4.x writes and reads (`lens_core::schema`). One helper,
  `crates/arcade-lens/src/datafile.rs`, loads and guards them: a file made by a newer Arcade Lens (schema above
  `CURRENT`) is read as far as possible, logged and never written over (a save is refused); a file that can't be parsed
  is moved to `<name>.unreadable-<unix seconds>` and Lens starts on the defaults, where before an unparseable
  `settings.toml` stopped the start and unreadable chains or usage were replaced by the next save. Fields this build
  doesn't know survive a save in `settings.toml` (each table keeps a flattened `extra`), chains, steps, usage and window
  geometry. Shapes: `chains.json` is a bare list in v1, so a newer one is an object `{"schema": N, "chains": [...]}`;
  `window-geometry.json` is a map of maps that 0.4.x would discard whole with a `schema` key, so a newer layout takes
  a new file name rather than a key. Chain steps keep `capability` and `action` required. History entries (one JSON
  file each, never rewritten; an unreadable one is skipped and left in place) and the append-only `collections/`
  lines need no code change and have tests. Fixtures in the 0.4.2 shape, with non-default values, are in
  `crates/arcade-lens/tests/data/v0.4.2/`; 14 of the new tests failed on the old code.

- D16. Round 5: clean uninstall, and 1.0.0. `scripts/verify-appimage.py` checks a clean uninstall the way Box (D19)
  and Look (D20) do: it takes an inventory of the disposable profile before installing; after `--uninstall --silent`
  nothing may be new except Lens's own folders (`ARCADE_LENS_HOME`) and no unpacked copy (`arcade/runtime/lens-*`,
  U1, removed by Link 1.0.11's uninstall) may remain; it then installs again and after `--uninstall --silent
  --remove-data` nothing new may remain at all (Link's sockets and locks under `run` and `handoff` are not counted).
  The profile's XDG data and config roots now sit under `ARCADE_HOME`, as in Look's check, so Link's environment and
  its unpacked copies use the same folders, as in a real profile. The "keeps user data" sentinel moved from the top
  of `ARCADE_LENS_HOME`, which `--remove-data` never deletes, into Lens's settings folder (`config`). Version 1.0.0
  for the first release candidate, `v1.0.0-rc.1`. README screenshots stay open, as for Box and Look.
- D17. Link conformance before rc.2. The release of `v1.0.0-rc.1` was cancelled before the Gates ran: the static check
  of Arcade Link, run locally (`arcade-link conformance .`), found 4 FAILs. ID-3: no `dev.arcade.Lens.svg` and no
  symbolic icon in the repository; `packaging/linux/` now holds both (the app icon from Link's `design/app-icons/lens`,
  the glyph `arcade.lens.svg` unchanged) and `build-appimage.sh` installs them under `hicolor/scalable/apps` and
  `hicolor/symbolic/apps`. REPO-2: no update bot; `.github/dependabot.yml` (Arcade tools' file, cargo and
  github-actions, grouped weekly, no rebase). REL-1: five steps of `ci.yml` that download or install had no
  `timeout-minutes`; each has one (10, 15 or 20), commands unchanged. LINK-2: a false positive on prose, because the
  line with the Link URL also held the tag; `VENDORED` and the text `scripts/sync-design.py` writes now put "tag
  `v1.x.y` (commit" on its own line, which the notices parser (`\s+`) still reads. The check now shows 0 failed.

## D18 - rc.2's publish, and Link 1.0.13

rc.2's Release built and tested every platform, then its publish failed: `gh release create` was refused on
`keyring.log` ("HTTP 400: Bad Content-Length"), and Link's publish removed the half-made release. The publish took
every artifact matching `"*-*"`, which also matched `desktop-lens`, Tier 1's logs (an empty `keyring.log` among them).
`release.yml` and `nightly.yml` now name the three installer artifacts.

Look's rc.2 gates found a Link bug that Lens shares: the unpacked copy (U1) inherited the launcher's references to
the AppImage's mount, and the GTK hook's `gsettings` crashed on the released mount and hung, so a start or Restart
never opened (Look D22). Lens moves to Link 1.0.13 before rc.3.

