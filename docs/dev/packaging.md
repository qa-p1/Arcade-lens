# Packaging and releases

## Installers

`scripts/package [--label LABEL]` builds this OS's installers into `dist/` from the one
workspace version (`[workspace.package] version` in `Cargo.toml`, read by
`scripts/check-version.py`). LABEL names the files and is the version the binary reports
(`ARCADE_RELEASE_VERSION`, see `crates/arcade-lens/src/link.rs`); it defaults to the version.

| OS | File | Built by |
|---|---|---|
| Linux | `ArcadeLens-<label>-linux-x86_64.AppImage` | `packaging/linux/build-appimage.sh` (on Ubuntu 22.04 in CI, for older glibc) |
| Windows | `ArcadeLens-<label>-windows-x64-setup.exe` (per-user Inno Setup) | `packaging/windows/arcade-lens.iss`, which includes Link's `arcade.iss` |
| Windows | `ArcadeLens-<label>-windows-x64-portable.exe` | a copy of the release exe; `arcade-release.json` lists it as `portable`, which Tools never installs |
| macOS | `ArcadeLens-<label>-macos-universal.dmg` (Apple silicon and Intel, macOS 12.3+) | `packaging/macos/build-dmg.sh` |

Nothing for text recognition is bundled: Lens uses the Tesseract installed on the computer
(Settings › Text recognition can download it).

The desktop id, icon name, window class, bundle id and Windows AppUserModelID are all
`dev.arcade.Lens`; the Link id is `arcade.lens`.

## Release metadata

`scripts/arcade-release.py` (vendored from Link) writes `arcade-release.json` and
`SHA256SUMS.txt` next to the installers. CI's `release-manifest` job builds both from each
run's installers, so naming problems show up in pull requests, not at release time.

## Releasing

Stable is never built: it is a verified release candidate promoted byte for byte, the same way in every Arcade app.

1. Merge to `main` with the new version in `Cargo.toml`, `docs/user/shortcuts.json` and
   `CHANGELOG.md` (`scripts/check` verifies the shortcut file's version).
2. **Release candidate:** Actions → Tag a release, with `v<version>-rc.<n>` and the full merge
   SHA. `tag.yml` refuses a commit that isn't on `main` or whose version differs, creates the
   tag and starts Release on it: the full CI, then Link's `app-publish` puts the installers on
   the hidden `test` channel as a prerelease (`arcade-release.json` keeps the Inno switches).
3. **Exit gates** run against those published files.
4. **Promote:** Actions → Promote, with the rc tag. Link's `app-promote` republishes the same
   files as `v<version>` with a signed `manifest.json` and checks every hash again.

Nightly (`nightly.yml`, daily) republishes main CI's installers as a signed
`v<version>-nightly.*` prerelease when `main` has changed and its CI is green. Nothing is
published from `ci.yml`.
