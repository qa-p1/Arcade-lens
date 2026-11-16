#!/usr/bin/env python3
"""THIRD_PARTY_NOTICES.md: every third-party part that ships inside Arcade Lens, with its version, licence, source
and the licence text.

    python3 scripts/third-party-notices.py > THIRD_PARTY_NOTICES.md      # write it (standard output)
    python3 scripts/third-party-notices.py --check                       # fail if the file is out of date
    python3 scripts/third-party-notices.py --check-appimage Lens.AppImage
                                                                         # fail if the AppImage bundles more than
                                                                         # this file says it does

Rust: the normal (non-dev, non-build) dependency graph of the `arcade-lens` binary for each platform Lens ships on
(Linux x86_64, Windows x64, macOS Apple silicon and Intel), from `cargo tree --locked`; the package data (licence
expression, source, licence files) from `cargo metadata --locked`. A crate is listed once, with the platforms whose
build contains it. Fonts: Inter (carried by Arcade Link) and the four fonts egui ships. Arcade Link: the vendored
files in `VENDORED`, at the version recorded there (which must be the tag Cargo.toml pins). Packaging: the AppImage
runtime. Identical licence texts are printed once, with every component that uses them.

The Linux AppImage bundles no library of its own: `packaging/linux/build-appimage.sh` packs the program, its launcher,
an icon, the licences and this file, and nothing for Tesseract. `--check-appimage` unpacks a built image and fails on
any other program or library in it, on licence files that differ from the committed ones, or when its runtime reports
a component this file does not list.

Needs `cargo fetch` first (cargo metadata lists the sources of every platform).
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_NOTICES.md"
SHIPPED = "arcade-lens"
# What each release ships, and the Rust target(s) it is built for (Windows and macOS builds are one file each:
# the macOS dmg is a universal binary of both architectures).
PLATFORMS = {
    "Linux": ["x86_64-unknown-linux-gnu"],
    "Windows": ["x86_64-pc-windows-msvc"],
    "macOS": ["aarch64-apple-darwin", "x86_64-apple-darwin"],
}
STANDARD = ROOT / "scripts" / "licences"
RUNTIME_LICENCES = ROOT / "packaging" / "linux" / "runtime-licences"
PREFERRED = ("MIT", "Apache-2.0", "Zlib", "BSD-3-Clause", "BSL-1.0", "MPL-2.0", "Unlicense")
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright)([-._].*)?$", re.I)

# egui ships four fonts inside epaint_default_fonts and keeps each one's licence next to it.
EGUI_FONTS_CRATE = "epaint_default_fonts"
EGUI_FONT_TEXTS = ("fonts/Hack-Regular.txt", "fonts/UFL.txt", "fonts/OFL.txt", "fonts/emoji-icon-font-mit-license.txt")

# What the AppImage runtime (the program at the front of the .AppImage file) says it contains
# (`--appimage-help`), and where each licence text is kept.
RUNTIME_COMPONENTS = {
    "runtime": ("AppImage type2-runtime", "MIT", "https://github.com/AppImage/type2-runtime", "type2-runtime.txt"),
    "musl libc": ("musl libc", "MIT", "https://musl.libc.org/", "musl.txt"),
    "libfuse": ("libfuse", "LGPL-2.1-only", "https://github.com/libfuse/libfuse", "libfuse.txt"),
    "squashfuse": ("squashfuse", "BSD-2-Clause", "https://github.com/vasi/squashfuse", "squashfuse.txt"),
    "libzstd": ("zstd", "BSD-3-Clause", "https://github.com/facebook/zstd", "zstd.txt"),
    "zlib": ("zlib", "Zlib", "https://zlib.net/", "zlib.txt"),
}
# The only program in the AppImage that is not the runtime's; everything else must be a script, desktop file or icon.
OWN_PROGRAMS = ("usr/bin/arcade-lens",)
# The licences and this file travel inside every package (packaging/linux/build-appimage.sh, macos/build-dmg.sh,
# windows/arcade-lens.iss); the AppImage's copy must be the committed one.
SHIPPED_DOCS = ("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md")
APPIMAGE_DOCS = "usr/share/doc/arcade-lens/"


def normalized(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def read(path):
    return path.read_text(encoding="utf-8", errors="replace")


def licence_texts(folder, declared=None):
    files = []
    if declared:
        files.append(folder / declared)
    if folder.is_dir():
        files += sorted(p for p in folder.iterdir() if p.is_file() and LICENCE_FILE.match(p.name))
        sub = folder / "LICENSES"
        if sub.is_dir():
            files += sorted(p for p in sub.iterdir() if p.is_file())
    seen, texts = set(), []
    for path in files:
        if path.resolve() in seen or not path.is_file():
            continue
        seen.add(path.resolve())
        text = normalized(read(path))
        if text:
            texts.append(text)
    return texts


def checkout_root(folder):
    """The repository root of a git dependency (licence files usually live there, not in the crate's folder)."""
    for parent in folder.parents:
        if (parent / ".git").exists() or parent.parent.name == "checkouts":
            return parent
    return None


def standard_text(expression, authors, name):
    """The SPDX text of the first preferred licence the crate offers, with its authors as copyright holders."""
    offered = re.split(r"\s+OR\s+|/", expression or "")
    # "Apache-2.0 WITH LLVM-exception": the licence text, then the exception's.
    exceptions = dict(term.split(" WITH ", 1) for term in offered if " WITH " in term)
    offered += list(exceptions)
    for spdx in PREFERRED:
        if spdx in offered:
            holders = ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in authors) or f"the {name} developers"
            text = read(STANDARD / f"{spdx}.txt")
            text = re.sub(r"Copyright \(c\) <year> <(copyright holders|owner)>\.?", f"Copyright (c) {holders}", text)
            if spdx in exceptions:
                text += "\n\n" + read(STANDARD / f"{exceptions[spdx]}.txt")
            return [normalized(f"{name} is offered under {expression}; it publishes no licence file, so the standard "
                               f"{spdx} text follows.\n\n{text}")]
    return []


def cargo(*args):
    return subprocess.run(["cargo", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def shipped_crates(target):
    """{(name, version)} in the normal dependency graph of the shipped binary for one Rust target."""
    listing = cargo("tree", "--locked", "-p", SHIPPED, "-e", "normal", "--target", target, "--prefix", "none", "--format", "{p}")
    found = set()
    for line in listing.splitlines():
        m = re.match(r"^(\S+) v(\S+)", line)
        if m:
            found.add((m.group(1), m.group(2)))
    if not any(name == SHIPPED for name, _ in found):
        sys.exit(f"notices: cargo tree for {target} did not list {SHIPPED}")
    return found


def rust_crates():
    meta = json.loads(cargo("metadata", "--locked", "--format-version", "1"))
    members = {p["name"] for p in meta["packages"] if p["id"] in set(meta["workspace_members"])}
    by_key = {}
    for p in meta["packages"]:
        by_key.setdefault((p["name"], p["version"]), []).append(p)
    ships = {}
    for platform, targets in PLATFORMS.items():
        for target in targets:
            for key in shipped_crates(target):
                if key[0] not in members:
                    ships.setdefault(key, set()).add(platform)
    crates = []
    for key in sorted(ships, key=lambda k: (k[0].lower(), k[1])):
        candidates = by_key.get(key)
        if not candidates:
            sys.exit(f"notices: cargo tree lists {key[0]} {key[1]}, which cargo metadata does not")
        for p in candidates:
            folder = Path(p["manifest_path"]).parent
            texts = licence_texts(folder, p.get("license_file"))
            if p["name"] == EGUI_FONTS_CRATE:
                texts = [normalized(read(folder / f)) for f in EGUI_FONT_TEXTS]
            if not texts and p["source"] and p["source"].startswith("git+"):
                root = checkout_root(folder)
                texts = licence_texts(root) if root else []
            if not texts:
                texts = standard_text(p.get("license"), p.get("authors") or [], p["name"])
            crates.append({
                "name": p["name"],
                "version": p["version"],
                "label": f'{p["name"]} {p["version"]}',
                "licence": p.get("license") or ("see licence file" if p.get("license_file") else "unspecified"),
                "home": p.get("repository") or p.get("homepage") or "",
                "ships": ships[key],
                "texts": texts,
                "folder": folder,
            })
    return crates


def pinned_link_tag():
    """The Arcade Link tag the workspace pins."""
    return read(ROOT / "Cargo.toml").split('Arcade-Link", tag = "', 1)[1].split('"', 1)[0]


def vendored_link():
    """(tag, commit) recorded in VENDORED, checked against Cargo.toml and Cargo.lock."""
    m = re.search(r"tag `(v[0-9][^`]*)` \(commit\s+`([0-9a-f]{40})`\)", read(ROOT / "VENDORED"))
    if not m:
        sys.exit("notices: VENDORED does not record the Arcade Link tag and commit")
    tag, commit = m.groups()
    if tag != pinned_link_tag():
        sys.exit(f"notices: VENDORED records Arcade Link {tag}, but Cargo.toml pins {pinned_link_tag()}")
    if f"?tag={tag}#{commit}" not in read(ROOT / "Cargo.lock"):
        sys.exit(f"notices: Cargo.lock does not resolve Arcade Link {tag} to the commit {commit} VENDORED records")
    return tag, commit


def vendored_files():
    """The files VENDORED records, grouped by what they are."""
    paths = re.findall(r"^[0-9a-f]{64}  (\S+)$", read(ROOT / "VENDORED"), re.M)
    kinds = {
        "the Arcade design kit (glyphs, icons, the Inter licence)": ("assets/glyphs/", "packaging/icons/", "assets/design-licenses/"),
        "shared installer text": ("assets/strings/",),
        "release, shortcut and Windows installer tooling": ("scripts/", "tools/", "packaging/windows/"),
        "schemas, test vectors and fixtures": ("spec/", "crates/lens-actions/tests/fixtures/"),
    }
    groups = {k: [] for k in kinds}
    for path in paths:
        for kind, prefixes in kinds.items():
            if path.startswith(prefixes):
                groups[kind].append(path)
                break
        else:
            sys.exit(f"notices: VENDORED lists {path}, which this script does not know how to describe")
    return groups


def fonts(crates):
    link = next(c for c in crates if c["name"] == "arcade-link")
    egui = next(c for c in crates if c["name"] == EGUI_FONTS_CRATE)
    hack, ubuntu, noto, emoji = [normalized(read(egui["folder"] / f)) for f in EGUI_FONT_TEXTS]
    inter = normalized(read(ROOT / "assets/design-licenses/Inter-OFL.txt"))
    return [
        {"label": "Inter 4 (typeface, in Arcade Link " + link["version"] + ")", "licence": "OFL-1.1", "home": "https://rsms.me/inter/", "texts": [inter]},
        {"label": "Hack (typeface, in egui)", "licence": "MIT, Bitstream Vera and DejaVu licences", "home": "https://github.com/source-foundry/Hack", "texts": [hack]},
        {"label": "Ubuntu Light (typeface, in egui)", "licence": "Ubuntu-font-1.0", "home": "https://design.ubuntu.com/font", "texts": [ubuntu]},
        {"label": "Noto Emoji (typeface, in egui)", "licence": "OFL-1.1", "home": "https://fonts.google.com/noto/specimen/Noto+Emoji", "texts": [noto]},
        {"label": "emoji-icon-font (typeface, in egui)", "licence": "MIT", "home": "https://github.com/jslegers/emoji-icon-font", "texts": [emoji]},
    ]


def arcade_link(crates):
    tag, commit = vendored_link()
    link = next(c for c in crates if c["name"] == "arcade-link")
    groups = vendored_files()
    what = "; ".join(f"{kind} ({len(paths)} file{'s' if len(paths) != 1 else ''})" for kind, paths in groups.items() if paths)
    return {
        "label": f"Arcade Link {tag} (vendored files)",
        "licence": "MIT OR Apache-2.0",
        "home": "https://github.com/qa-p1/Arcade-Link",
        "detail": f"Copied unchanged from tag {tag}, commit {commit}: {what}. The list and checksums are in `VENDORED`.",
        "texts": link["texts"],
    }


def runtime_components():
    out = []
    for key, (name, licence, home, text_file) in RUNTIME_COMPONENTS.items():
        out.append({"label": name, "licence": licence, "home": home,
                    "texts": [normalized(read(RUNTIME_LICENCES / text_file))]})
    return out


def table_licence(value):
    return value.replace("|", "\\|")


def render():
    crates = rust_crates()
    link = arcade_link(crates)
    font_list = fonts(crates)
    runtime = runtime_components()
    lens_licences = {"MIT": normalized(read(ROOT / "LICENSE-MIT")), "Apache-2.0": normalized(read(ROOT / "LICENSE-APACHE"))}
    groups = [("Fonts", font_list), ("Arcade Link", [link]), ("Packaging", runtime), ("Rust crates", crates)]
    missing = [c["label"] for _, group in groups for c in group if not c["texts"]]
    if missing:
        sys.exit("notices: no licence text found for " + ", ".join(sorted(missing)))

    everywhere = set(PLATFORMS)
    lines = [
        "# Third-party notices",
        "",
        "Arcade Lens is licensed MIT OR Apache-2.0 (`LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE`). It includes the parts below,",
        "each under its own licence, whose text is printed in full at the end. This file is generated by",
        "`scripts/third-party-notices.py`, and CI fails when it is out of date; do not edit it by hand.",
        "",
        "Lens ships as a Linux x86_64 AppImage, a Windows x64 installer and portable program, and a macOS universal",
        "(Apple silicon and Intel) disk image. Each build contains only its platform's share of the Rust crates; the",
        "table says which.",
        "",
        "## Not included",
        "",
        "- **Text recognition.** On Linux, Lens reads text with the Tesseract program the user has installed. If there is",
        "  none, Settings can download one when the user asks (through Arcade Link, from a pinned address and checksum).",
        "  Tesseract is not part of the Lens packages and is not covered by this file. Windows and macOS builds use the",
        "  system's own text recognition.",
        "- **Native libraries on Linux.** The AppImage bundles no library of its own: it holds the `arcade-lens` program,",
        "  its launcher script, a desktop file, an icon, and the licences and this file. The program links the system's C library (`libc`, `libm`,",
        "  `libgcc_s`) and loads X11, Wayland, OpenGL and D-Bus libraries from the user's system when it runs; none is",
        "  distributed with Lens. CI unpacks every AppImage it builds and fails if a library or another program appears",
        "  (`scripts/third-party-notices.py --check-appimage`). The runtime at the front of the file is listed under",
        "  Packaging.",
        "- **Windows and macOS** builds link the operating system's libraries and bundle none.",
        "",
        "## Fonts",
        "",
    ]
    by_text = {}

    def add_texts(c):
        for text in c["texts"]:
            by_text.setdefault(text, []).append(c["label"])

    for c in font_list:
        lines.append(f"- {c['label']} ({c['licence']}) — {c['home']}")
        add_texts(c)
    lines += [
        "",
        "## Arcade Link",
        "",
        f"- {link['label']} ({link['licence']}) — {link['home']}",
        f"  {link['detail']}",
        "  The Arcade Link crate that Lens compiles in is in the Rust crates table.",
        "",
    ]
    add_texts(link)
    lines += [
        "## Packaging",
        "",
        "The Linux AppImage starts with the AppImage runtime that appimagetool puts in front of the image, built from",
        "[AppImage/type2-runtime](https://github.com/AppImage/type2-runtime) (its source and build instructions are",
        "there). It statically contains the components below, and `--appimage-help` prints the same list. The runtime is",
        "a separate program in front of the image: to use a modified libfuse, build a runtime from that source and put it",
        "in front of the image, which starts at the offset `--appimage-offset` prints.",
        "",
    ]
    for c in runtime:
        lines.append(f"- {c['label']} ({c['licence']}) — {c['home']}")
        add_texts(c)
    lines += [
        "",
        "## Rust crates",
        "",
        "| Crate | Version | Licence | Ships on | Source |",
        "|---|---|---|---|---|",
    ]
    for c in crates:
        ships = "all" if c["ships"] == everywhere else ", ".join(p for p in PLATFORMS if p in c["ships"])
        lines.append(f"| {c['name']} | {c['version']} | {table_licence(c['licence'])} | {ships} | {c['home']} |")
        add_texts(c)
    lines += ["", "## Licence texts", ""]
    for text, label in (("MIT", "Arcade Lens"), ("Apache-2.0", "Arcade Lens")):
        by_text.setdefault(lens_licences[text], []).append(f"{label} ({text} option)")
    for n, (text, users) in enumerate(sorted(by_text.items(), key=lambda kv: (sorted(kv[1], key=str.lower)[0].lower(), kv[0])), 1):
        users = sorted(set(users), key=str.lower)
        lines += [f"### {n}. Used by {', '.join(users)}", "", "```text", text.replace("```", "'''"), "```", ""]
    return "\n".join(lines)


# --------------------------------------------------------------------------------------------- the AppImage

def is_elf(path):
    try:
        with open(path, "rb") as f:
            return f.read(4) == b"\x7fELF"
    except OSError:
        return False


def appimage_problems(files, runtime_components_found, docs):
    """What is wrong with an unpacked AppImage: `files` is {relative path: is an ELF file}, `docs` the normalized text
    of the files under usr/share/doc/arcade-lens/ as {file name: text}."""
    problems = []
    for rel, elf in sorted(files.items()):
        if rel in OWN_PROGRAMS:
            continue
        if elf or rel.startswith(("usr/lib", "usr/lib64", "lib/", "lib64/")) or re.search(r"\.so(\.|$)", rel):
            problems.append(f"the AppImage bundles {rel}, a program or library this file does not list")
    if not any(rel in OWN_PROGRAMS for rel in files):
        problems.append(f"the AppImage does not contain {OWN_PROGRAMS[0]}")
    for name in SHIPPED_DOCS:
        if name not in docs:
            problems.append(f"the AppImage does not carry {APPIMAGE_DOCS}{name}")
        elif docs[name] != normalized(read(ROOT / name)):
            problems.append(f"{APPIMAGE_DOCS}{name} differs from the committed {name}")
    unknown = sorted(set(runtime_components_found) - set(RUNTIME_COMPONENTS))
    gone = sorted(set(RUNTIME_COMPONENTS) - set(runtime_components_found))
    if unknown:
        problems.append("the AppImage runtime now contains " + ", ".join(unknown) + " (add its licence text to packaging/linux/runtime-licences/)")
    if gone:
        problems.append("the AppImage runtime no longer reports " + ", ".join(gone) + " (update RUNTIME_COMPONENTS)")
    return problems


def runtime_report(help_text):
    """The component names in the `License:` block of `--appimage-help`."""
    block = help_text.split("License:", 1)[-1]
    return [m.group(1).strip() for m in re.finditer(r"^\s+\* (.+?)(?:, licensed under.*)?$", block, re.M)]


def check_appimage(appimage):
    with tempfile.TemporaryDirectory() as work:
        image = Path(work) / "image.AppImage"
        shutil.copy2(appimage, image)
        image.chmod(0o755)
        env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="")
        helped = subprocess.run([str(image), "--appimage-help"], cwd=work, check=True, capture_output=True, text=True, env=env)
        help_text = helped.stdout + helped.stderr
        subprocess.run([str(image), "--appimage-extract"], cwd=work, check=True, stdout=subprocess.DEVNULL, env=env)
        root = Path(work) / "squashfs-root"
        files = {p.relative_to(root).as_posix(): is_elf(p) for p in root.rglob("*") if p.is_file() and not p.is_symlink()}
        docs = {name: normalized(read(root / APPIMAGE_DOCS / name)) for name in SHIPPED_DOCS if (root / APPIMAGE_DOCS / name).is_file()}
    problems = appimage_problems(files, runtime_report(help_text), docs)
    if problems:
        sys.exit("\n".join(f"{appimage.name}: {p}" for p in problems) + "\nUpdate scripts/third-party-notices.py and THIRD_PARTY_NOTICES.md.")
    print(f"{appimage.name}: {len(files)} files, one program (arcade-lens), no library, the current licences and notices; the runtime holds the {len(RUNTIME_COMPONENTS)} components listed")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail if THIRD_PARTY_NOTICES.md is out of date")
    ap.add_argument("--check-appimage", type=Path, metavar="APPIMAGE", help="fail if the AppImage bundles a library or program not listed")
    args = ap.parse_args()
    if args.check_appimage:
        check_appimage(args.check_appimage.resolve())
        return
    text = render()
    if args.check:
        current = normalized(read(OUT)) if OUT.exists() else ""
        if current != normalized(text):
            sys.exit("THIRD_PARTY_NOTICES.md is out of date: run `python3 scripts/third-party-notices.py > THIRD_PARTY_NOTICES.md`")
        print("THIRD_PARTY_NOTICES.md is current")
        return
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    sys.stdout.write(text + "\n")


if __name__ == "__main__":
    main()
