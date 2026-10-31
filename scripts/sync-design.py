#!/usr/bin/env python3
"""Copy the Arcade design kit files Lens ships from Arcade Link at the pinned tag.

    python3 scripts/sync-design.py --link ../Arcade-Link   # an existing checkout
    python3 scripts/sync-design.py                         # clones the tag

The tag is the one the workspace Cargo.toml pins arcade-link to; the
checkout's HEAD must be that tag's commit. Colours, sizes, the icon table,
Inter and the format helpers come from the arcade-link crate itself (features
`design` and `design-egui`), so only files the build or the packages read
from disk are copied: the app glyphs (assets/glyphs/), Lens's launcher icon set
(packaging/icons/, the names packaging uses) and Inter's licence. Their hashes
go into the "Arcade design kit" section of VENDORED, which
scripts/check-vendored.py checks.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
LINK_URL = "https://github.com/qa-p1/Arcade-Link"
HEADER = "Arcade design kit"
ICONS = "design/app-icons/lens/"
# (source in Link, destination here); a trailing slash copies the folder's files.
FILES = [
    ("design/fonts/OFL.txt", "assets/design-licenses/Inter-OFL.txt"),
    ("design/glyphs/", "assets/glyphs/"),
    (ICONS + "icon.svg", "packaging/icons/icon.svg"),
    (ICONS + "lens.ico", "packaging/icons/arcade-lens.ico"),
    (ICONS + "macos/icon.icns", "packaging/icons/arcade-lens.icns"),
    (ICONS + "png/256.png", "packaging/icons/arcade-lens.png"),
    (ICONS + "png/", "packaging/icons/png/"),
]
# Folders that hold only synced files, emptied first so removed files go too.
OWNED = ["assets/glyphs", "packaging/icons", "assets/design-licenses"]


def pinned_tag() -> str:
    match = re.search(r'arcade-link = \{[^}]*tag = "([^"]+)"', (ROOT / "Cargo.toml").read_text())
    if not match:
        raise SystemExit("Cargo.toml doesn't pin arcade-link to a tag")
    return match.group(1)


def git(link: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(link), *args], check=True, capture_output=True, text=True).stdout.strip()


def pairs(link: Path) -> list[tuple[Path, str]]:
    out = []
    for source, target in FILES:
        if source.endswith("/"):
            files = sorted(p for p in (link / source).iterdir() if p.is_file())
            if not files:
                raise SystemExit(f"{source} is empty in the Link checkout")
            out += [(p, f"{target}{p.name}") for p in files]
        else:
            out.append((link / source, target))
    return out


def sync(link: Path, tag: str) -> None:
    commit = git(link, "rev-parse", "HEAD")
    tagged = git(link, "rev-parse", f"{tag}^{{commit}}")
    if commit != tagged:
        raise SystemExit(f"{link} is at {commit[:12]}, not {tag} ({tagged[:12]})")
    for folder in OWNED:
        shutil.rmtree(ROOT / folder, ignore_errors=True)
    lines = []
    for path, target in pairs(link):
        destination = ROOT / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        lines.append(f"{hashlib.sha256(destination.read_bytes()).hexdigest()}  {target}")
    section = "\n".join([
        f"## {HEADER}",
        "",
        f"Copied unchanged from [Arcade Link]({LINK_URL}/tree/{tag}) tag `{tag}` (commit",
        f"`{commit}`) by `scripts/sync-design`; never edit these files here.",
        "License: MIT OR Apache-2.0.",
        "",
        "```text",
        *lines,
        "```",
    ])
    vendored = ROOT / "VENDORED"
    text = vendored.read_text()
    marker = f"## {HEADER}\n"
    if marker in text:
        start = text.index(marker)
        end = text.index("```\n", text.index("```text", start)) + len("```\n")
        text = text[:start] + section + "\n" + text[end:]
    else:
        text = text.rstrip("\n") + "\n\n" + section + "\n"
    vendored.write_text(text)
    print(f"Synced {len(lines)} design kit files from Link {tag} ({commit[:12]})")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--link", type=Path, help="an Arcade Link checkout at the pinned tag")
    args = parser.parse_args(argv)
    tag = pinned_tag()
    if args.link:
        sync(args.link.resolve(), tag)
        return 0
    with tempfile.TemporaryDirectory() as temp:
        checkout = Path(temp) / "Arcade-Link"
        subprocess.run(["git", "clone", "--quiet", "--depth", "1", "--branch", tag, LINK_URL, str(checkout)], check=True)
        sync(checkout, tag)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
