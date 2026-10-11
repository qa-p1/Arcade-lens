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

mkdir -p "$mount" "$home/Applications"
export HOME="$home"
hdiutil attach "$image" -readonly -nobrowse -mountpoint "$mount" >/dev/null
mounted=1

source_app="$mount/Arcade Lens.app"
copied_app="$home/Applications/Arcade Lens.app"
[[ -L "$mount/Applications" ]]
[[ "$(readlink "$mount/Applications")" == /Applications ]]
[[ -x "$mount/Install Arcade Lens CLI.command" ]]
[[ -x "$source_app/Contents/MacOS/arcade-lens" ]]
plutil -lint "$source_app/Contents/Info.plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$source_app/Contents/Info.plist")" == "$version" ]]

ditto "$source_app" "$copied_app"
[[ -x "$copied_app/Contents/MacOS/arcade-lens" ]]
cmp "$source_app/Contents/MacOS/arcade-lens" "$copied_app/Contents/MacOS/arcade-lens"
plutil -lint "$copied_app/Contents/Info.plist"
[[ ! -e "$home/.local/bin/arcade-lens" ]]
[[ ! -e "$home/.zprofile" && ! -e "$home/.bash_profile" && ! -e "$home/.profile" ]]
echo 'macOS CI DMG mount and temporary Applications copy passed; app remains unsigned.'
