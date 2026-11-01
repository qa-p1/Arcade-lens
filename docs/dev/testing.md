# Testing

## Before a pull request

`scripts/check` runs what CI's `test` job runs on Linux, Windows and macOS:

- `scripts/check-version.py`, `scripts/check-docs.py` (local links and fragments, user-guide
  links stay inside `docs/user`, the shortcut sheet matches the build and `shortcuts.md`
  is regenerated) and `scripts/check-vendored.py` (every vendored Link file matches its
  SHA-256);
- the Python tests for the vendored release and shortcut scripts;
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace` in a temporary profile (`scripts/isolate.py`), so tests never
  touch real settings, history, login entries or the Link registry.

`scripts/dev` runs Lens from source with its own profile in `target/dev-profile`.

## In CI after the checks

Each OS then packages Lens (`scripts/package`) and verifies the installer for real:

- **Linux:** `scripts/verify-appimage.py` installs, repairs and uninstalls the AppImage in a
  disposable profile under Xvfb and D-Bus. `tests/tray-registration.py` registers the tray
  icon with a real StatusNotifierWatcher (through Link's `tools/e2e.py` runner, at the
  Link tag `Cargo.toml` pins). `tests/overlay-focus.py` checks, in both window modes, that a
  capture stays open while it has focus, closes when another app takes focus, and closes on Esc.
- **Windows:** `packaging/windows/verify.ps1` installs silently, checks the receipt,
  shortcuts and login entry, and uninstalls, on the ephemeral runner account.
- **macOS:** every slice of the universal binary must support macOS 12.3, and
  `packaging/macos/verify-dmg-copy.sh` copies the app out of the dmg into a temporary home
  and runs it.
- `release-manifest` builds `arcade-release.json` and `SHA256SUMS.txt` from those
  installers and checks the asset kinds.

## Peer tests

`crates/lens-actions/tests/arcade.rs` has isolated Link tests (always run) and real-peer
tests that start the released Find, Box, Shelf, Clipboard or Look binaries in a private
`ARCADE_HOME` (ignored unless their binaries are provided).

## Manual checks

Interactive desktop behaviour that CI cannot reach (real Hyprland and GNOME shortcuts,
Windows and macOS permissions and login items, Tools updating an installed Lens) is listed
with its last result in [`docs/STATUS.md`](../STATUS.md).
