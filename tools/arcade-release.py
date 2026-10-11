#!/usr/bin/env python3
"""Writes `arcade-release.json` and `SHA256SUMS.txt` for a release (plan §11).

    python3 arcade-release.py --id arcade.look --version 0.4.0 --channel stable \\
        --notes https://github.com/qa-p1/Arcade-look/releases/tag/v0.4.0 dist/

Every file in the asset directory is listed in `SHA256SUMS.txt`
(`<sha256>  <name>`, the format `sha256sum -c` reads). Installable files are
also described in `arcade-release.json`, classified from their names:

| Name | os | kind |
|---|---|---|
| `*.AppImage` | linux | appimage |
| `*.deb` / `*.rpm` | linux | deb / rpm |
| `*linux*.tar.gz` | linux | tarball |
| `*.msi` | windows | msi |
| `*.exe` | windows | `--windows-installer` (nsis or inno) |
| `*.dmg` | macos | dmg |

The architecture comes from the name (`x64`/`amd64`/`x86_64`, `arm64`/
`aarch64`, `universal`); without one, `x64`. Anything else (signatures,
checksums, archives of symbols) is checksummed but not listed. Existing
asset names are kept; the manifest maps them. Standard library only, so every
release pipeline can vendor this file (see `VENDORED` in each repository).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

SCHEMA = 1
LINK_PROTOCOL = [1]
SILENT = {
    "nsis": ["/S"],
    "inno": ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER"],
    "msi": ["/qn"],
}
OUTPUTS = ("arcade-release.json", "SHA256SUMS.txt")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def arch_of(name: str) -> str:
    n = name.lower()
    if "universal" in n:
        return "universal"
    if re.search(r"(arm64|aarch64)", n):
        return "arm64"
    return "x64"


def classify(name: str, windows_installer: str, portable: bool = False) -> dict | None:
    """`{os, arch, kind[, silent]}` for an installable file, else None."""
    n = name.lower()
    if (portable or "portable" in n) and n.endswith((".exe", ".zip")):
        os_, kind = "windows", "portable"
    elif n.endswith(".zip") and any(s in n for s in ("wheel", "windows", "win32", "win64")):
        os_, kind = "windows", "portable"
    elif n.endswith(".appimage"):
        os_, kind = "linux", "appimage"
    elif n.endswith(".deb"):
        os_, kind = "linux", "deb"
    elif n.endswith(".rpm"):
        os_, kind = "linux", "rpm"
    elif n.endswith((".tar.gz", ".tgz")) and "linux" in n:
        os_, kind = "linux", "tarball"
    elif n.endswith(".msi"):
        os_, kind = "windows", "msi"
    elif n.endswith(".exe"):
        os_, kind = "windows", windows_installer
    elif n.endswith(".dmg"):
        os_, kind = "macos", "dmg"
    else:
        return None
    entry = {"os": os_, "arch": arch_of(name), "kind": kind}
    if kind in SILENT:
        entry["silent"] = SILENT[kind]
    if kind in ("appimage", "dmg"):
        entry["installArgs"] = ["--install", "--silent"]
    return entry


def build(app_id: str, version: str, channel: str, notes: str, assets: Path, windows_installer: str,
          portable_assets: set[str] | None = None) -> tuple[dict, str]:
    files = sorted(p for p in assets.iterdir() if p.is_file() and p.name not in OUTPUTS)
    if not files:
        raise SystemExit(f"arcade-release: no files in {assets}")
    portable_assets = portable_assets or set()
    if portable_assets - {p.name for p in files}:
        raise SystemExit("arcade-release: --portable names must exist in the assets directory")
    sums, listed = [], []
    for p in files:
        digest = sha256(p)
        sums.append(f"{digest}  {p.name}")
        entry = classify(p.name, windows_installer, p.name in portable_assets)
        if entry:
            listed.append({**entry, "file": p.name, "sha256": digest, "size": p.stat().st_size})
    if not listed:
        raise SystemExit(f"arcade-release: nothing installable in {assets}")
    manifest = {
        "schema": SCHEMA,
        "id": app_id,
        "version": version,
        "channel": channel,
        "linkProtocol": LINK_PROTOCOL,
        "notes": notes,
        "assets": listed,
    }
    return manifest, "\n".join(sums) + "\n"


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--id", required=True, help="canonical app ID, e.g. arcade.look")
    p.add_argument("--version", required=True, help="release version without the leading v")
    p.add_argument("--channel", default="stable", choices=["stable", "nightly"])
    p.add_argument("--notes", required=True, help="release notes URL")
    p.add_argument("--windows-installer", default="nsis", choices=["nsis", "inno"],
                   help="installer technology of *.exe assets")
    p.add_argument("--portable", action="append", default=[], metavar="FILE",
                   help="mark a Windows exe/zip as portable (repeatable); Tools ignores it")
    p.add_argument("assets", type=Path, help="directory holding the release files; outputs are written here")
    a = p.parse_args(argv)
    if not re.fullmatch(r"arcade\.[a-z]+", a.id):
        raise SystemExit(f"arcade-release: {a.id!r} is not a canonical app ID")
    if a.version.startswith("v"):
        raise SystemExit("arcade-release: pass the version without the leading v")
    manifest, sums = build(a.id, a.version, a.channel, a.notes, a.assets, a.windows_installer, set(a.portable))
    (a.assets / "arcade-release.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (a.assets / "SHA256SUMS.txt").write_text(sums)
    print(f"arcade-release: {len(manifest['assets'])} installable assets, {sums.count(chr(10))} checksums")
    return 0


if __name__ == "__main__":
    sys.exit(main())
