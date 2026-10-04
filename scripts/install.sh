#!/usr/bin/env bash
# Builds Arcade Lens and installs it for the current user in ~/.local/bin.
# On its first start Lens adds itself to the applications menu and to the
# apps started at login. Run this again after pulling changes to update: a
# running instance is restarted on the new build.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p arcade-lens

bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
mkdir -p "$bin"
# Replace the file rather than writing into it: a running instance keeps its copy.
install -m 755 target/release/arcade-lens "$bin/.arcade-lens.new"
mv -f "$bin/.arcade-lens.new" "$bin/arcade-lens"

# Restart a running instance on the new build, or start one in the background.
setsid -f "$bin/arcade-lens" --restart >/dev/null 2>&1 < /dev/null

echo "Installed $bin/arcade-lens. Arcade Lens is running in the tray."
case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "Note: $bin is not on your PATH." ;;
esac
