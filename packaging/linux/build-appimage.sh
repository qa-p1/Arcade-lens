#!/usr/bin/env bash
# Packages a release build as an AppImage:
#
#   packaging/linux/build-appimage.sh target/release/arcade-lens dist/ArcadeLens-x86_64.AppImage
#
# Nothing for OCR is bundled: Lens uses the user's Tesseract.
# On its first start the AppImage adds itself to the applications menu and
# to the apps started at login.
set -euo pipefail

bin=$(realpath "$1")
out=$(realpath -m "$2")
here=$(cd "$(dirname "$0")" && pwd)
icons=$here/../icons
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app=$work/AppDir
install -Dm755 "$bin" "$app/usr/bin/arcade-lens"
install -Dm644 "$here/arcade-lens.desktop" "$app/usr/share/applications/arcade-lens.desktop"
install -Dm644 "$icons/arcade-lens.png" "$app/usr/share/icons/hicolor/256x256/apps/arcade-lens.png"
cp "$here/arcade-lens.desktop" "$app/arcade-lens.desktop"
cp "$icons/arcade-lens.png" "$app/arcade-lens.png"
ln -s arcade-lens.png "$app/.DirIcon"
# A script, not a symlink, so the process is called arcade-lens.
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/arcade-lens" "$@"
RUN
chmod 755 "$app/AppRun"

tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool=$work/appimagetool
  curl -fsSL --retry 3 -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
mkdir -p "$(dirname "$out")"
# Extract-and-run: CI runners have no FUSE.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$out"
echo "Built $out"
