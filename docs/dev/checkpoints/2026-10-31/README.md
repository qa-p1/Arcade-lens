# Paused Lens overhaul: 2026-10-31

The owner stopped development and requested a GitHub checkpoint. **Lens is not
complete, not released, and not ready to merge.** The released application remains
[v0.3.4](https://github.com/qa-p1/Arcade-lens/releases/tag/v0.3.4), and main remains
`c862102c19a9839f40ef371953f7bb922ce055a6`.

## Preservation incident — read before continuing

The active scratch workspace disappeared between the implementation turn and the
owner's stop message. A filesystem check found the active directory empty except
for runtime metadata. The older Find/Link checkouts survived; they were not
modified or mistaken for the latest Lens work. GitHub confirmed that the local
Lens `overhaul/standard` branch had **never been uploaded**. No Lens implementation
PR or release was created before the reset.

Consequently, this checkpoint is **documentation and recovered edit records**, not
a byte-for-byte snapshot of the final local Lens tree. The final Cargo.lock,
formatted files and local test logs are unavailable. The files in `recovered/`
are code copied from visible edit commands in the chat, not a freshly implemented
or verified replacement. They intentionally are not wired into the build. They
are incomplete: in particular the large Settings rewrite and subsequent edits
have only the detailed inventory below here. Do not describe this branch as the
completed implementation or carry its earlier intermediate Clippy result onto
this archive. Recover the original tool-call text from the conversation if an
exact restoration is required; otherwise reimplement and reverify against LENS.md.

## Scope and sequence followed

The owner explicitly paused Find and requested the next app. Read, in order,
Arcade-catalog `dev/overhaul/README.md`, `STANDARD.md`, `LENS.md`, then both referenced
audits (`keep.md`, `hygiene-and-dev-standard.md`). Read Link's design README and
existing egui component APIs. Started a clean local `overhaul/standard` from the
main SHA above. No subagents, remote branch deletion, force push, manual tag push,
merge or release was performed for Lens.

The Keep inventory remained binding: frozen multi-monitor capture, window/region
selection, ranked spatial/progressive palette, keyboard selection/resizing,
coexisting local findings, chains, providers, recognizer and action preferences,
opt-in plugins and explicit permissions, external/dangerous action confirmations,
secret filtering, Tesseract and language choices, annotation including pixelation,
floating pins including move/zoom/opacity/live refresh, measure, recording, and
opt-in masked history. macOS capability limitations must stay graceful.

## What was written locally before the reset

### Dependencies, assets and project files

- Workspace version 0.3.4 → **0.4.0**, for the required minor release; not released.
- Replaced Link revision `4c75f3a7c806ae78f96127c556708e65eaeeb476` with immutable
  **v0.4.11**, commit `047b686d9b02de6dcf7bdfa77c85bf7eefbf7264`, with existing
  watch/engines/install/trayhost features plus design/design-egui.
- Added Rust **1.88.0** toolchain file, rustfmt and Clippy components, `.editorconfig`,
  LICENSE plus MIT and Apache texts copied from Link.
- Added `resvg` 0.45, default features off, to render canonical kit SVGs.
- Added `scripts/sync-design` and Python implementation adapted from Find. It
  checked HEAD against the pinned tag and rejected dirty design inputs. It copied
  Lens SVG/ICO/ICNS/PNG files, all app glyphs and Inter OFL unchanged, preserving
  old files rather than deleting uncertain assets. It added a separate design
  section to VENDORED and retained old helper provenance.
- Synced **22 design files**, yielding **50 total verified hashes**. Final binary
  assets can be regenerated from Link v0.4.11; final local VENDORED is unavailable.
- Updated offline shortcut JSON version and regenerated shortcuts Markdown.
- Added an offline docs check rejecting relative links outside docs/user. It
  correctly exposed four existing violations; they were **not fixed**.

### Settings model and persistence behavior

- Added serializable System/Light/Dark Theme enum; default System. Added appearance
  to Settings and the portable settings allowlist, using serde default so old
  backups remain readable. Existing shortcut remains unchanged.
- Replaced Settings' Save/Revert screen with a Template A kit shell, fixed page
  headings and per-page scrolling. Default 760×560, minimum 720×520, title
  `Settings — Arcade Lens`, app_id `dev.arcade.Lens` for this window.
- Section order: General, Shortcuts, Actions, Text recognition, Chains, Services,
  Plugins, Privacy, Connected apps, About. Each has its own one-line description.
- Shared kit sidebar/brand, rows/lists, segmented choices, selects, switches,
  checkboxes, buttons, notices, badges, paths, shortcut recorder and dialogs.
- Immediate settings requests, with local per-field buffers committed on
  Enter/blur. Inline validators for service URLs, currency, language codes,
  action keys and chain names. Invalid chain drafts use saved valid chains for
  other preference changes rather than overwriting saved chains.
- Shared shortcut recording with Link registry and known OS conflict lookup;
  clear/reset behavior. Existing read-only palette key list retained.
- Preserved save folder, format, browser, terminal, editor, date reading order,
  home currency and primary action count. Primary count became a kit select.
- Text recognition page holds engine status/download, language input and all
  recognizer toggles. Download still uses the existing worker.
- Action list retains enable/disable, favorites, custom keys and safety badges;
  filter and no-match state added with kit controls.
- Chain list/editor retains type-compatible action selection, parameters,
  reordering and removal; avoids duplicate generated IDs; confirms Delete chain.
- Provider templates and reverse image search retained. Unknown existing reverse
  image search URLs remain selectable as a custom service; nothing uploads itself.
- Plugin permissions/errors and enable toggles retained with kit controls.
- Privacy retains secret guard, local learning reset, history opt-in/location and
  confirmed Clear history. Kit count formatting used for the history count.
- Connected apps retains master/per-peer opt-outs; uses Connected / Not running /
  Not installed and Open / Install. Later separated OpenApp from GetApp so opening
  an installed app would not incorrectly request Tools installation.
- About adds version/channel, Tools, guide, logs, diagnostics, licences and Quit.
  Added Ctrl/Command+Q, F1 guide and Ctrl/Command+K → action filter page.
- Added SettingsRequest handling for URL opening, installed-app opening and Quit.
  Worker-spawned installed app processes are reaped. Suppressed success “Saved”
  notices; save failures remain visible with Retry.
- Added focused regression cases for invalid chains preserving saved chains and
  invalid service URLs being rejected. Their execution result was **not observed**.

### Visual and native surfaces

- Replaced dark-only theme constants with per-frame Link palette/ink mapping.
  Installed Inter and applied shared egui styling on startup and settings reload.
- Added `ARCADE_LENS_UI_SCALE` support from 1.0 to 1.25 for screenshot/accessibility
  checking. A screenshot harness was planned but **not created or run**.
- Converted existing canvas text colors in overlay/panels/annotate/pins to dynamic
  kit mapping; snapped several out-of-scale fonts and sentence-cased Chains.
- Theme helpers delegate action controls, canvas key chips and safety badges to
  kit controls. Spatial placement functions remain. This is an intermediate
  refactor: several custom canvas rows and annotation/pin controls still need work.
- Replaced hand-drawn app glyphs and violet launcher artwork with kit SVG rendering,
  cached white glyph textures and monochrome tray rendering. Added macOS template
  icon flag. Settings brand/peer rows use canonical glyph textures.
- Reworked install/uninstall prompt to kit dialog/glyph/checkbox/actions, correct
  title/app_id, 720×520 resizable minimum, existing worker and options preserved.
  Button roles/order revised; no install operation was executed for Lens here.
- Palette filtering, confirmation buttons, result Copy/Close controls, skeleton
  and missing-Tesseract notice moved to kit helpers. Custom palette layout still
  needs a full audit and real screenshots.
- Tray labels changed to Open Arcade Lens / Settings, reordered existing actions,
  left click changed to capture, macOS icon set as template. Capture text, Pick
  color, Restart and Quit were preserved. No tray runtime verification was done.

## Checks actually observed

| Check | Observation |
|---|---|
| Initial dependency refresh | Completed; six workspace crates became 0.4.0, Link became 0.4.11. |
| resvg dependency refresh | Completed; resvg/usvg 0.45.1 and transitive dependencies resolved. Final lockfile lost. |
| rustfmt | Ran successfully during edits. Later formatted file bytes are unavailable. |
| Intermediate all-workspace/all-target Clippy, locked, -D warnings | **Observed success**, 10.81 s, before the final OpenApp/scale/docs edits. This is not a final-head or archive pass. |
| Earlier cargo check | Failed on a count cast (`u64`, helper expects `i64`) and the concurrent resvg addition. Those were changed before the successful intermediate Clippy. |
| Vendored hashes | **Observed 50 files verified** after sync. |
| Strict offline docs | **Failed**: features.md → ../STATUS.md; troubleshooting.md → ../STATUS.md; with-other-apps.md → ../ARCADE_LINK.md and ../STATUS.md. |
| Final Clippy command | Started; no result observed before reset. |
| Settings unit-test command | Started after final Clippy in the shell sequence; no completed result observed. |
| Native screenshots / UI review | **Not run** for Lens. |
| Full workspace tests, three-OS CI, native peers, packages, releases, Tools update test | **Not run** for this Lens work. |

The environment denied private Unix socket binding (EPERM) during Find work.
Full native/IPC checks require CI's isolated runner; never substitute compilation.
No final local log file survived the reset. Observations above come from actual
visible tool output in this chat, not reconstructed test results.

## What is still left

1. Restore/review the incomplete local work. Resolve the source-recovery gap first;
   this checkpoint does not change the app's built implementation.
2. Run the full LENS.md and STANDARD line-by-line review. Do not infer acceptance
   from the substantial Settings edit inventory above.
3. Finish every shared-control conversion in all surfaces. Review the ten-section
   sidebar at 720×520/125%: potential overflow was identified by source reasoning,
   **not a reproduced screenshot bug**. Fix shared kit defects in Link, not locally.
4. Implement History's Template A view and all required loading/empty/error/Retry
   states. Finish results actions, annotation toolbar, pin toolbar/readouts,
   selection halos/handles, focus-loss grace, first-frame visibility, per-monitor
   geometry, welcome/notifications and close-to-background behavior.
5. Finish exact wording/tagline, shared format helpers, app-side color audit and
   computed contrast checks, all appropriate identifiers (not just Settings).
6. Implement verified copy-verify-remove folder migration to Link paths, JSON
   settings compatibility, desktop/autostart rewrite and previous-release upgrade
   checks. This was discussed but **not implemented**.
7. Complete CLI status/show/hide/toggle/help behavior, logging/diagnostics and
   restart/single-instance review. Current restart uses an IPC ping wait, not the
   known Linux `/proc/<pid>` zombie test; still verify actual behavior.
8. Complete scripts/dev/check/package, three-OS CI, real UI captures, regression
   tests, real capture/action/chain/OCR/recording/peer checks. Missing shared kit
   components need Link PR/test/version/tag before app adoption.
9. Fix offline docs, the two Windows README links, add .gitattributes for binary
   and vendored hashes on Windows, move developer docs to lowercase docs/dev,
   preserve finished plans in history. Leave duplicate pinned helpers and isolated
   integration tests intact. List merged branches; do not delete them.
10. Add manual tag.yml/release.yml preserving Windows portable exclusion, prove
    draft beta before removing old publisher, then final green PR, merge commit
    with expected full SHA, workflow tag and explicit stable release dispatch.
11. Refresh README/user screenshots, dated status/changelog, then REST-download
    and verify every asset checksum/manifest size/digest, latest/catalog, and real
    Tools 0.4.1 FakeRelease/Scheduler/release::verify on all four platform choices.
12. Real desktop exceptions remain: actual Hyprland capture/shortcuts/focus/tray,
    Windows/macOS login/permissions/AppUserModelID, and an actual owner-machine
    update through Tools. Preserve documented macOS unsupported-capability handling.

## Other repos

Find remains unfinished in PR #11, with two current failing gates. Shelf and Wheel
have not been started. Box/Tools patch follow-ups have not been started. The full
cross-repo handoff lives in Arcade-catalog:
[HANDOFF-2026-10-31.md](https://github.com/qa-p1/Arcade-catalog/blob/docs/overhaul-checkpoint-2026-10-31/dev/overhaul/HANDOFF-2026-10-31.md).
