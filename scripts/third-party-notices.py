#!/usr/bin/env python3
"""Writes THIRD_PARTY_NOTICES.md: every crate Arcade Lens can ship (all
platforms), its license expression, and the license texts it includes.

    python3 scripts/third-party-notices.py > THIRD_PARTY_NOTICES.md

Uses `cargo metadata` (run `cargo fetch` first so every platform's sources
are present). Identical license texts are printed once.
"""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

LICENSE_NAMES = ("license", "licence", "copying", "notice", "copyright")


def link_tag():
    """The Arcade Link tag the workspace pins."""
    cargo = (Path(__file__).resolve().parents[1] / "Cargo.toml").read_text()
    return cargo.split('Arcade-Link", tag = "', 1)[1].split('"', 1)[0]


def main():
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], text=True))
    pkgs = {p["id"]: p for p in meta["packages"]}
    workspace = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    # Everything reachable from the workspace through normal dependencies.
    seen, stack = set(), list(workspace)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for d in nodes[pid]["deps"]:
            if any(k["kind"] in (None, "normal") for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    crates = sorted((pkgs[p] for p in seen if p not in workspace), key=lambda p: (p["name"].lower(), p["version"]))
    texts = {}
    rows = []
    for p in crates:
        d = os.path.dirname(p["manifest_path"])
        refs = []
        try:
            entries = sorted(os.listdir(d))
        except OSError:
            entries = []
        for f in entries:
            if f.lower().startswith(LICENSE_NAMES) and os.path.isfile(os.path.join(d, f)):
                with open(os.path.join(d, f), encoding="utf-8", errors="replace") as fh:
                    t = fh.read().strip()
                h = hashlib.sha256(t.encode()).hexdigest()[:12]
                texts.setdefault(h, (t, []))[1].append(f"{p['name']} {p['version']}")
                refs.append(h)
        rows.append((p["name"], p["version"], p.get("license") or "see files", p.get("repository") or "", refs))
    out = sys.stdout
    out.write("# Third-party notices\n\n")
    out.write("Arcade Lens is licensed MIT OR Apache-2.0. It is built from the Rust crates below ")
    out.write("(all platforms; each build includes only its platform's subset). License texts follow the table.\n\n")
    out.write("Also included: the Arcade Link " + link_tag() + " design kit, glyphs and integration tokens (MIT OR Apache-2.0), Inter fonts (SIL Open Font License 1.1), ")
    out.write("and Arcade Link release, shortcut and Windows integration scripts (`tools/`, `scripts/`, `packaging/windows/`, MIT OR Apache-2.0); see `VENDORED`. ")
    out.write("Text recognition uses the Tesseract program installed on the computer; it is not bundled.\n\n")
    out.write("| Crate | Version | License | Source |\n|---|---|---|---|\n")
    for name, ver, lic, repo, _ in rows:
        out.write(f"| {name} | {ver} | {lic} | {repo} |\n")
    out.write("\n## Inter font licence\n\n```text\n" + (Path(__file__).resolve().parents[1] / "assets/design-licenses/Inter-OFL.txt").read_text().strip() + "\n```\n")
    out.write("\n## License texts\n")
    for h, (t, users) in sorted(texts.items(), key=lambda kv: kv[1][1][0].lower()):
        shown = ", ".join(users[:12]) + (f" and {len(users) - 12} more" if len(users) > 12 else "")
        out.write(f"\n### {shown}\n\n```text\n{t}\n```\n")
    missing = [f"{n} {v}" for n, v, _, _, refs in rows if not refs]
    if missing:
        out.write("\n## Crates without a bundled license file\n\nTheir license is the SPDX expression in the table above: ")
        out.write(", ".join(missing) + ".\n")


if __name__ == "__main__":
    main()
