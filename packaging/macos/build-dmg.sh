#!/usr/bin/env bash
# Packages a release build as "Arcade Lens.app" inside a drag-to-install DMG:
#
#   packaging/macos/build-dmg.sh target/universal/arcade-lens 0.1.0 dist/ArcadeLens-0.1.0-macos.dmg
#
# The app is ad-hoc signed (not notarized). On first launch from
# /Applications it adds itself to the apps opened at login.
set -euo pipefail

bin=$1 version=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app="$work/dmg/Arcade Lens.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$bin" "$app/Contents/MacOS/arcade-lens"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
cp "$here/../icons/arcade-lens.icns" "$app/Contents/Resources/ArcadeLens.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"
codesign --force --deep --sign - "$app"

ln -s /Applications "$work/dmg/Applications"
mkdir -p "$(dirname "$out")"
hdiutil create -volname "Arcade Lens" -srcfolder "$work/dmg" -ov -format UDZO "$out"
echo "Built $out"
