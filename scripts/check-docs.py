#!/usr/bin/env python3
"""Check local Markdown targets/fragments, shortcut data, and generated tables."""
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent

def anchors(path):
    seen = {}
    result = set()
    for heading in re.findall(r"^#{1,6}\s+(.+)$", path.read_text(), re.M):
        slug = re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
        n = seen.get(slug, 0)
        seen[slug] = n + 1
        result.add(slug if n == 0 else f"{slug}-{n}")
    return result

def main():
    errors = []
    checked = 0
    user_docs = ROOT / "docs/user"
    files =[ROOT / "README.md", ROOT / "CHANGELOG.md", ROOT / "VENDORED", *sorted((ROOT / "docs").rglob("*.md"))]
    for file in files:
        if not file.is_file():
            continue
        for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)", file.read_text()):
            target = target.strip().split(' "', 1)[0].strip("<>")
            url = urlsplit(target)
            if url.scheme or url.netloc:
                continue  # External URLs are not fetched by this offline check.
            path = (file.parent / unquote(url.path)).resolve() if url.path else file
            checked += 1
            if user_docs in file.parents and user_docs not in path.parents and path != user_docs:
                # Tools shows the user guide offline: it may only link inside docs/user or to URLs.
                errors.append(f"{file.relative_to(ROOT)}: {target} leaves docs/user; use an absolute URL")
            elif not path.exists():
                errors.append(f"{file.relative_to(ROOT)}: missing {target}")
            elif url.fragment and path.suffix == ".md" and unquote(url.fragment) not in anchors(path):
                errors.append(f"{file.relative_to(ROOT)}: missing fragment {target}")
    source = ROOT / "docs/user/shortcuts.json"
    doc = json.loads(source.read_text())
    version = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
    if doc["app"] != 'arcade.lens' or doc["version"] != version:
        errors.append("shortcuts app/version differs from the build")
    generated = subprocess.check_output([sys.executable, str(ROOT / "scripts/validate_shortcuts.py"), str(source), "--markdown"], text=True)
    if generated != (ROOT / "docs/user/shortcuts.md").read_text():
        errors.append("shortcuts.md is stale; regenerate it with scripts/validate_shortcuts.py")
    if errors:
        sys.exit("\n".join(errors))
    print(f"docs: {checked} local links, shortcut validation and generated Markdown passed")

if __name__ == "__main__":
    main()
