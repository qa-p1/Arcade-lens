#!/usr/bin/env bash
# Mount the release DMG and copy its bundle into a temporary home on macOS CI.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <image.dmg> <version>" >&2
  exit 2
fi

image=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
version=$2
if [[ ! -f "$image" ]]; then
  echo "verify-dmg-copy: image not found: $image" >&2
  exit 1
fi

root=$(mktemp -d)
mount="$root/mount"
home="$root/home"
mounted=0
cleanup() {
  if [[ "$mounted" -eq 1 ]]; then
    hdiutil detach "$mount" >/dev/null
  fi
  rm -rf "$root"
}
trap cleanup EXIT

mkdir -p "$mount" "$home" "$root/config" "$root/data" "$root/cache" "$root/run"
chmod 700 "$root/run"
export HOME="$home" ARCADE_HOME="$root/arcade" ARCADE_LENS_HOME="$root/lens"
export XDG_CONFIG_HOME="$root/config" XDG_DATA_HOME="$root/data" XDG_CACHE_HOME="$root/cache" XDG_RUNTIME_DIR="$root/run"
export ARCADE_NO_INSTALL_PROMPT=1
hdiutil attach "$image" -readonly -nobrowse -mountpoint "$mount" >/dev/null
mounted=1

source_app="$mount/Arcade Lens.app"
copied_app="$ARCADE_HOME/Applications/Arcade Lens.app"
[[ -L "$mount/Applications" ]]
[[ "$(readlink "$mount/Applications")" == /Applications ]]
[[ -x "$mount/Install Arcade Lens CLI.command" ]]
[[ -x "$source_app/Contents/MacOS/arcade-lens" ]]
plutil -lint "$source_app/Contents/Info.plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$source_app/Contents/Info.plist")" == "$version" ]]

"$source_app/Contents/MacOS/arcade-lens" --install --silent --no-launch
[[ -x "$copied_app/Contents/MacOS/arcade-lens" ]]
cmp "$source_app/Contents/MacOS/arcade-lens" "$copied_app/Contents/MacOS/arcade-lens"
plutil -lint "$copied_app/Contents/Info.plist"
python3 - "$ARCADE_HOME/installs/arcade.lens.json" "$copied_app" "$version" <<'PY'
import json
from pathlib import Path
import sys

receipt = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
assert receipt["schema"] == 1 and receipt["id"] == "arcade.lens"
assert receipt["method"] == "macos-bundle" and receipt["managedBy"] == "self"
assert Path(receipt["path"]) == Path(sys.argv[2])
assert receipt["version"] == sys.argv[3]
PY
[[ ! -e "$ARCADE_HOME/.local/bin/arcade-lens" ]]
[[ ! -e "$home/.zprofile" && ! -e "$home/.bash_profile" && ! -e "$home/.profile" ]]
echo 'macOS CI DMG mount, silent Applications install and receipt passed; app remains unsigned.'
