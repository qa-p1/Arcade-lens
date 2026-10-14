#!/usr/bin/env bash
# Build an unsigned Arcade Lens.app in a drag-to-Applications DMG (macOS only).
#
#   packaging/macos/build-dmg.sh target/universal/arcade-lens 0.2.0 dist/ArcadeLens-0.2.0-macos-universal.dmg
#
# This preserves Gatekeeper quarantine behavior. It does not sign, notarize,
# remove quarantine attributes, install a CLI shim, or edit shell profiles.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <universal-binary> <version> <output-dmg>" >&2
  exit 2
fi

bin=$1
version=$2
out=$3
here=$(cd "$(dirname "$0")" && pwd)

if [[ ! -x "$bin" ]]; then
  echo "build-dmg: executable not found: $bin" >&2
  exit 1
fi
if [[ ! "$version" =~ ^[A-Za-z0-9.+-]+$ ]]; then
  echo "build-dmg: invalid version: $version" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/dmg/Arcade Lens.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$bin" "$app/Contents/MacOS/arcade-lens"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
cp "$here/../icons/arcade-lens.icns" "$app/Contents/Resources/ArcadeLens.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"

ln -s /Applications "$work/dmg/Applications"
install -m 755 "$here/install-cli.command" "$work/dmg/Install Arcade Lens CLI.command"
mkdir -p "$(dirname "$out")"
hdiutil create -volname "Arcade Lens" -srcfolder "$work/dmg" -ov -format UDZO "$out"
echo "Built unsigned bundle DMG: $out"
