# Decisions

Short records of choices that are not obvious from the code. Newest first.

## 2026-11: release flow

- Releases moved from CI auto-tagging (a new `Cargo.toml` version on `main` was tagged and
  published by `ci.yml`) to the shared manual flow: `tag.yml` → `release.yml` on the tag.
  `ci.yml` only checks and packages. See [packaging.md](packaging.md).
- The rolling `nightly` prerelease stays, because Tools' nightly channel reads it. It is
  refreshed by running Release on `main` with channel `nightly`, no longer on every push.
- Beta rehearsals are private drafts. Link's `arcade-release.py` knows only `stable` and
  `nightly`, so a beta's manifest says `nightly`; it is never public.
- The portable Windows exe stays a download for people who want it, and is listed as
  `portable`, which Tools never installs.

## 2026-10: overhaul to the Arcade standard

- **Identifiers:** the OS id is `dev.arcade.Lens` everywhere (desktop file, window class,
  bundle id, AppUserModelID, Hyprland rules). Config and data move from `arcadelens` to
  Link's `app_dirs`, copy-verify-remove. Until the new settings file exists, Lens keeps
  reading the old folders, so a failed migration never loses settings.
- **Tray:** the overhaul renames and reorders items but adds and removes none (STANDARD
  §5.1). LENS.md's guide mentions History, but Lens's tray never had it, so the menu is
  Open Arcade Lens, Capture text, Pick color, Settings, Restart, Quit.
- **Settings apply at once.** The Save button is gone. Fields that need validation (the
  shortcut recorder, provider keys, the chain editor) apply on Enter or blur and show an
  inline error, and leave the rest of the settings alone.
- **Windows on Wayland:** the background instance has no window. Each overlay, pin,
  editor and the settings window is its own short-lived process (`--window <kind>`), so an
  idle Lens holds no GL context. `ARCADE_LENS_RESIDENT_UI=1` keeps the older in-process
  mode on X11. See [architecture.md](architecture.md#process-model).
- **Text recognition** uses the user's Tesseract. It is not bundled, to keep the
  installers small; Settings › Text recognition downloads it where the platform allows.
