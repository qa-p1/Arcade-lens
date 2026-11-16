# Lens 1.0 progress (newest first)

## 2026-11-16 - Link conformance before rc.2 (branch `local/lens-conformance`)
- D17: `v1.0.0-rc.1` was cancelled before the Gates: `arcade-link conformance .` showed 4 FAILs (ID-3 icons, REPO-2
  update bot, REL-1 steps without a timeout, LINK-2 prose). Fixed with scalable and symbolic icons in the AppImage,
  `.github/dependabot.yml`, `timeout-minutes` on five CI steps and a reflow of `VENDORED`; now 0 failed.
- Not run here (no builds on this disk): the Rust tests and the AppImage steps; they run in CI.

## 2026-11-16 - Round 5: clean uninstall; 1.0.0
- D16: the AppImage check compares the profile before install with what a plain uninstall and one with
  `--remove-data` leave (Box's and Look's check). Version 1.0.0 for `v1.0.0-rc.1`. The check runs only in CI.

## 2026-11-16 - Round 5: schema versions and safe downgrade (branch `local/lens-schema`)
- D15: `settings.toml`, `chains.json`, `usage.json` and `window-geometry.json` carry a schema version (none means 1),
  keep fields Lens doesn't know through a save, are never written over when a newer Lens made them, and are moved
  aside, not replaced, when they can't be parsed. History and collections were already safe; they now have tests.
- Tests first, from 0.4.2-shaped fixtures: 14 failed on the old code, all pass now.

## 2026-11-16 - Security hardening (branch `local/lens-hardening`)
- D13: three rows of `SECURITY.md` that were documented as open are fixed, each with a test written first and seen to
  fail on the old code: the Wayland hand-over folder (now a verified private per-user folder, `private_dir.rs`), the
  KDE Connect image share (mode 0600, unique name, shares over an hour old deleted) and chains (an outbound chain is
  refused or not offered when the selection holds a secret, confirmed or not).
- Moved to `BACKLOG.md` with reasons: plugins that declare `read-text` receive secrets; settings, chains, history and
  logs take the umask.

## 2026-11-15 - Round 5: security, licences, notices (cloud session, branch `local/lens-security`)
- D12: `SECURITY.md` (what Lens guards and how, each row with its test or "not covered"), `deny.toml` and a `deny` CI job
  (Box's, `--workspace --all-features`), and `THIRD_PARTY_NOTICES.md` from `scripts/third-party-notices.py`
  (`--check` runs in `scripts/check.py`; `--check-appimage` runs in CI and the gates). The notices list the crates of
  each shipped platform (`cargo tree --target`), the fonts (Inter, and the four egui ships, one under the Ubuntu Font
  Licence), the vendored Link files at the recorded tag, and the AppImage runtime's six components. The AppImage bundles
  no native library, so there is no native generator. Every package now carries the licences and the notices.
- Advisories: updated within range rustls, rustls-webpki, webpki-roots, webpki-root-certs, tokio, rustix,
  async-executor, clipboard-win; seven accepted with reasons (D12).
- New tests: the endpoint file is `0600`, saved names stay in their folder, the defaults keep history, plugins and image
  search off, Download only for http(s), terminal commands carry only a validated host, a secret copy asks, a dangerous
  chain does not run unconfirmed, reverse image search never uploads, and the notices script.
- Found, not changed (listed in `SECURITY.md`; both fixed in the 2026-11-16 entry above): Send to Phone leaves `arcade-lens-share-<pid>.png` in the temporary folder
  with default permissions, and chains are not covered by the secret guard.
- Tested: `python3 scripts/check.py` (version, docs, vendored, notices, 23 script tests, fmt, `clippy --workspace
  --all-targets --locked -D warnings`, `cargo test --workspace --locked` in the isolated profile): 224 passed, 0 failed,
  2 ignored. `cargo deny --workspace --all-features check --disable-fetch` against a local advisory database (the
  sandbox cannot reach GitHub): ok. `--check-appimage` passed on an AppImage built here with `packaging/linux/build-appimage.sh` (a stand-in
  binary). Windows and macOS packaging was not built here.

## 2026-11-15 - Link 1.0.11 (cloud session, branch `claude/sharp-cori-29w5eg`)
- D10: Link 1.0.7 → 1.0.11 (crate, every reusable workflow, vendored record, notices).
- CHANGELOG in Keep a Changelog format with every user-visible v1 change under Unreleased.
- D11: Tier 1 on headless Hyprland (`tests/desktop/lens_desktop.py`) in CI and the gates; written for the harness,
  which runs only in CI, so its first run is this PR's. LN-10 and RD-LENS-4 (P2) moved to `BACKLOG.md`.

## 2026-11-15 - step 7: release gates (cloud session, branch `claude/sharp-cori-29w5eg`)
- D9: `gates.yml` on Link `app-gates.yml@v1.0.9` for the published rc; `scripts/verify-upgrade.py`, the upgrade gate
  Lens did not have, run by CI from the published 0.4.2 and by the gates from the previous stable release.
- Tested: `scripts/verify-upgrade.py` in Link's isolated runner (`tools/e2e.py run`) on the real published AppImages,
  0.4.1 → 0.4.2 (the repository is still 0.4.2): passed. The workflow files parse. The 0.4.2 → candidate run is this
  PR's CI.

## 2026-11-15 - step 4 (cloud session, branch `claude/sharp-cori-29w5eg`)
- Step 4, dead code (D7). Removed 11 public items nothing used (lens-core `ActionRef`,
  `Analysis::cancel_token`, `Engine::settings`, `Registry::plugins`, `DateTimeValue::is_ambiguous`,
  `Host::share_text`, `Rect::is_empty`, `geometry::monitor_for`; lens-recognizers `register`, `find_unit`;
  lens-platform `Shortcuts::current_id`), swapped `allow(dead_code)` on `write_if_changed` for its real `cfg`, and
  dropped two repeated dev-dependencies. The release fixture test that discovery never found now runs. `cargo machete` found no unused dependency. Items kept on purpose, and
  scripts/examples nothing references (listed, not deleted), are in D7.
- Tested: `python3 scripts/check.py` (version, docs, vendored, 12 script tests, fmt, `clippy --workspace
  --all-targets --locked -D warnings`, `cargo test --workspace --locked` in the isolated profile): 216 passed,
  0 failed, 2 ignored (the two `#[ignore]`d tests in `lens-actions/tests/arcade.rs`, which need Link's e2e harness or real apps). Windows and macOS code was not compiled here.

## 2026-11-14 - steps 2 and 6 (cloud session, branch `claude/sharp-cori-29w5eg`)
- Step 6 (`343a0cc`): releases are rc-only through Link `app-publish` on the hidden `test` channel; `promote.yml`
  makes stable from a verified rc byte for byte; `nightly.yml` republishes main CI's packages. Inno kept.
- Step 2 (`0b39213`, merged in): one PATH lookup. `host::which` also finds `<cmd>.exe`, and the ghost host's copy
  delegates to it, so the GUI and the host agree on what Lens can offer.
- Tested: `cargo +1.88.0 clippy --workspace --all-targets -D warnings`, `cargo +1.88.0 test --workspace`. The PR
  opens once Link v1.0.8 is tagged (the release workflows pin it).

## 2026-11-13 - steps 1 and 3: every P0 and P1 fixed (PR #17)
- `11a9332` behaviour locks; each fix below flips its lock or adds a test that failed before.
- P0: `34e97e3` + `0a8d14b` LN-1 (`--status`/`--hide` skip the install dialog; its fonts load before the first
  frame), LN-8 (a secret's history thumbnail is a blank tile), RD-LENS-1 (Ctrl+C arrives as `Event::Copy` on
  Wayland and now copies in the overlay and the annotator).
- P1: LN-3/BP-6 (window processes reaped on every command and a 2 s tick, D4), `671b5a9` LN-4 (focus loss never
  closes the overlay, D5), `a195404` LN-5 + RD-LENS-2/3 (Escape ladders), `8fd66d0` LN-2 (a copy that would vanish
  is an error naming xclip/wl-clipboard), `cadf34b` LN-6 (ffmpeg stopped on drop; no Record Window on Wayland),
  `707a5c8` LN-7 (solo pins keep their source area, so X11 pins offer Live).
- Tested: `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`. Not tested by hand: the
  fresh-profile AppImage dialog (LN-1) and the X11 recorder/pin paths; the owner checks those.
- P2: `3037cc3` LN-13 (IBANs are not phones), `e2aac59` LN-9 (colour codes in text), `ae30ff4` LN-12 (OCR lines
  under 0.2 confidence dropped), `ff19f8e` RD-LENS-6 (a table needs aligned columns), `1243784` LN-11 (`--status`
  counts Link-opened overlays). Each with a unit test that fails without it.
- Open: LN-10 (the loupe reads the same frozen image as the click, so the wrong colour is likely a hover-coordinate
  mismatch; needs the harness to reproduce) and RD-LENS-4 (pin menu clipped by the pin window); then steps 2, 4-8.

## 2026-11-11 - main builds carry the plain version (D6)
- `ci.yml`: builds from main and pull requests are labelled with the Cargo version, not `-dev.<sha>`, so Link's
  ecosystem e2e can publish them as the stable release Tools installs (three Lens checks failed on this). Checked by
  reading the workflow; CI on the PR runs it.
- Resolved 2026-11-13: nightlies were a rolling release tagged `nightly`, which Tools' nightly channel never lists
  (it reads `vX.Y.Z-nightly.YYYYMMDD.N`, Tools `source.rs`). `release.yml` now publishes
  `v<version>-nightly.<YYYYMMDD>.<run>` prereleases; the binary keeps the plain version, which Tools matches to a
  `-nightly` manifest (`release.rs` `versions_match`).

## Step 0 - brief
- docs/v1/SPEC.md, docs/v1/audit/ (text only) and DECISIONS.md added. Not code; nothing to run.

Open: everything in steps 1-7 (see the git log for what has landed). Link needs: none yet.
