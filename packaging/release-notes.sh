#!/usr/bin/env bash
# Writes the release notes for a GitHub release to stdout:
#
#   packaging/release-notes.sh <tag> <package label> [<previous tag>]
#
# <tag> is v1.2.3 or nightly; <package label> is the version part of the
# package file names. Changes are the commits since <previous tag> (all
# commits when there is none).
set -euo pipefail

tag=$1 label=$2 prev=${3:-}
repo=${GITHUB_REPOSITORY:-qa-p1/Arcade-lens}
url="https://github.com/$repo/releases/download/$tag"
f() { echo "[\`ArcadeLens-$label-$1\`]($url/ArcadeLens-$label-$1)"; }

if [ "$tag" = nightly ]; then
  echo "> [!WARNING]"
  echo "> Built automatically from the latest commit on \`main\` ($(git rev-parse --short HEAD)). It may be unstable; the [latest release](https://github.com/$repo/releases/latest) is the stable version."
  echo
fi

cat <<NOTES
## Downloads

| System | Download |
|---|---|
| **Windows** 10/11 (x64) | $(f windows-x64-setup.exe) · or the portable $(f windows-x64-portable.exe) |
| **Linux** (x86_64) | $(f linux-x86_64.AppImage) |
| **macOS** 12.3+ (Apple silicon and Intel) | $(f macos-universal.dmg) |

## Installing

- **Windows:** run the setup. It installs for your user only (no administrator rights), adds Arcade Lens to the Start menu and, if you keep the option, starts it when you sign in. The installer isn't code-signed yet, so SmartScreen may warn: choose **More info → Run anyway**.
- **Linux:** make the AppImage executable (\`chmod +x ArcadeLens-*.AppImage\`) and open it. On first start it adds itself to the applications menu and to login startup. Text recognition models are included.
- **macOS:** open the disk image and drag **Arcade Lens** to Applications. The app isn't notarized yet, so the first time, right-click it and choose **Open**. Screen capture needs the Screen Recording permission (System Settings → Privacy & Security).

Arcade Lens then runs in the tray (menu bar on macOS). Press **Ctrl+Alt+Shift+L** (Ctrl+Option+Shift+L on macOS) to select anything on screen.

Verify a download with [\`SHA256SUMS.txt\`]($url/SHA256SUMS.txt): \`sha256sum -c SHA256SUMS.txt --ignore-missing\`.

## Changes

NOTES
range=HEAD
if [ -n "$prev" ]; then range="$prev..HEAD"; fi
git log --no-merges --pretty='- %s (%h)' "$range"
