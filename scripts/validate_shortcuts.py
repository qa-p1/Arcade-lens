#!/usr/bin/env python3
"""Validate app shortcuts or catalog sheets; emit Markdown. Stdlib only.

Vendor this file verbatim, pin its SHA-256, and run:
  python3 validate_shortcuts.py docs/user/shortcuts.json --manifest manifest.json
  python3 validate_shortcuts.py docs/user/shortcuts.json --markdown
Rules mirror arcade_link::{accelerator,shortcuts}; shared vectors pin parity.
"""
from __future__ import annotations
import argparse
import json
import re
import sys
from pathlib import Path

MODIFIERS = ("Ctrl", "Alt", "Shift", "Super")
KEYS = """Space Enter Tab Escape Backspace Delete Insert Home End PageUp PageDown Up Down Left Right Minus Equal BracketLeft BracketRight Backslash Semicolon Quote Backquote Comma Period Slash Print Pause ScrollLock CapsLock NumLock Menu NumpadAdd NumpadSubtract NumpadMultiply NumpadDivide NumpadDecimal NumpadEnter VolumeUp VolumeDown VolumeMute MicMute MediaPlayPause MediaNext MediaPrevious MediaStop BrightnessUp BrightnessDown MouseLeft MouseRight MouseMiddle MouseBack MouseForward WheelUp WheelDown WheelLeft WheelRight""".split()
MOD = {s.lower(): v for v, names in zip(MODIFIERS, (
    "Ctrl Control Ctl Control_L Control_R", "Alt Option Opt Alt_L Alt_R", "Shift Shift_L Shift_R", "Super Cmd Command Win Windows Meta Logo Mod4 Super_L Super_R")) for s in names.split()}
KEY = {s.lower(): s for s in KEYS}
ALIASES = {
    "/": "Slash", "-": "Minus", "=": "Equal", "plus": "Equal", ",": "Comma", ".": "Period", ";": "Semicolon",
    "'": "Quote", "apostrophe": "Quote", "`": "Backquote", "grave": "Backquote", "[": "BracketLeft", "]": "BracketRight", "\\": "Backslash",
    "return": "Enter", "esc": "Escape", "del": "Delete", "ins": "Insert", "pgup": "PageUp", "prior": "PageUp", "pgdn": "PageDown", "next": "PageDown",
    "arrowup": "Up", "arrowdown": "Down", "arrowleft": "Left", "arrowright": "Right",
    "xf86audioraisevolume": "VolumeUp", "xf86audiolowervolume": "VolumeDown", "xf86audiomute": "VolumeMute", "xf86audioplay": "MediaPlayPause",
    "xf86audionext": "MediaNext", "xf86audioprev": "MediaPrevious", "xf86audiostop": "MediaStop", "xf86monbrightnessup": "BrightnessUp", "xf86monbrightnessdown": "BrightnessDown",
    "mouse:272": "MouseLeft", "mouse:273": "MouseRight", "mouse:274": "MouseMiddle", "mouse:275": "MouseBack", "mouse:276": "MouseForward", "mouse_up": "WheelUp", "mouse_down": "WheelDown",
    "mouse_left": "WheelLeft", "mouse_right": "WheelRight", "xf86audiomicmute": "MicMute", "page_up": "PageUp", "page_down": "PageDown", "iso_left_tab": "Tab",
    **{f"kp_{n}": f"Numpad{n}" for n in range(10)},
    **{f"kp_{name.lower()}": f"Numpad{name}" for name in ("Add", "Subtract", "Multiply", "Divide", "Decimal", "Enter")},
}
FRIENDLY = {"Slash": "/", "Minus": "-", "Equal": "=", "Comma": ",", "Period": ".", "Semicolon": ";", "Quote": "'", "Backquote": "`", "BracketLeft": "[", "BracketRight": "]", "Backslash": "\\", "Up": "↑", "Down": "↓", "Left": "←", "Right": "→"}
MAC = {"Ctrl": "⌃", "Alt": "⌥", "Shift": "⇧", "Super": "⌘", "Enter": "↩", "Tab": "⇥", "Escape": "⎋", "Backspace": "⌫", "Delete": "⌦"}
OS = ("linux", "windows", "macos")

def normalize(value: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError("empty accelerator")
    compact = "+".join(p.strip() for p in value.strip().split("+"))
    out = []
    for chord in compact.split():
        mods, main = set(), None
        parts = chord.split("+")
        for index, part in enumerate(parts):
            lower = part.lower()
            if lower in MOD:
                if MOD[lower] in mods:
                    if not (index + 1 == len(parts) and main is None and lower.endswith(("_l", "_r"))):
                        raise ValueError("repeated modifier")
                mods.add(MOD[lower])
            else:
                if main is not None:
                    raise ValueError("more than one key")
                main = ALIASES.get(lower, KEY.get(lower))
                if re.fullmatch(r"[a-zA-Z0-9]", part):
                    main = part.upper()
                elif re.fullmatch(r"f[0-9]+", lower) and 1 <= int(lower[1:]) <= 24:
                    main = f"F{int(lower[1:])}"
                elif re.fullmatch(r"numpad[0-9]", lower):
                    main = "Numpad" + lower[-1]
                elif re.fullmatch(r"code:?[0-9]+", lower):
                    raw = lower.removeprefix("code").removeprefix(":").lstrip("0")
                    main = "Code" + (raw or "0")
                if main is None:
                    raise ValueError(f"unknown key {part!r}")
        if main is None and len(mods) != 1:
            raise ValueError("a chord needs a key")
        out.append("+".join([m for m in MODIFIERS if m in mods] + ([] if main is None else [main])))
    return " ".join(out)

def display(value: str, os: str) -> str:
    if os not in OS:
        raise ValueError("unknown OS")
    names = {**FRIENDLY, **(MAC if os == "macos" else {"Super": "Win"} if os == "windows" else {})}
    return " ".join(("" if os == "macos" else "+").join(names.get(k, k) for k in chord.split("+")) for chord in normalize(value).split(" "))

def conflicts(a: str, b: str) -> bool:
    a, b = normalize(a), normalize(b)
    return a == b or a.startswith(b + " ") or b.startswith(a + " ")

def _string(value, label, limit=None):
    if not isinstance(value, str) or not value.strip() or (limit is not None and len(value) > limit):
        raise ValueError(f"{label} must be a nonempty string" + (f" of at most {limit} characters" if limit else ""))

def _bindings(value):
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    if isinstance(value, list) and value and all(isinstance(v, str) for v in value):
        return value
    raise ValueError("keys must be a canonical string, nonempty string array, or null")

def effective(shortcut: dict, os: str) -> list[str]:
    keys = shortcut["keys"]
    return _bindings(keys.get(os, keys.get("default")))

def validate(doc: dict, manifest: dict | None = None) -> None:
    if not isinstance(doc, dict) or type(doc.get("schema")) is not int or doc["schema"] != 1:
        raise ValueError("schema must be 1")
    sheet = "app" not in doc
    if sheet:
        if not isinstance(doc.get("id"), str) or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", doc["id"]):
            raise ValueError("sheet id must be kebab-case")
        _string(doc.get("name"), "name")
        _string(doc.get("checkedVersion"), "checkedVersion")
        sources = doc.get("sources")
        if not isinstance(sources, list) or not sources or any(not isinstance(s, str) or not re.fullmatch(r"https://[^/\s]+[^\s]*", s) for s in sources):
            raise ValueError("sources must contain https URLs")
        matches = doc.get("match")
        if not isinstance(matches, dict) or not any(os in matches for os in OS):
            raise ValueError("match needs at least one OS")
        for os, field in zip(OS, ("class", "exe", "bundle")):
            if os in matches:
                value = matches[os]
                names = value.get(field) if isinstance(value, dict) else None
                if not isinstance(names, list) or not names or any(not isinstance(n, str) or not n.strip() for n in names):
                    raise ValueError(f"match.{os}.{field} needs nonempty strings")
    else:
        if not isinstance(doc.get("app"), str) or not re.fullmatch(r"arcade\.[a-z0-9][a-z0-9.-]*", doc["app"]):
            raise ValueError("invalid Arcade id")
        _string(doc.get("version"), "version")
        if manifest is not None and manifest.get("id") != doc["app"]:
            raise ValueError("manifest belongs to another app")
    groups = doc.get("groups")
    if not isinstance(groups, list):
        raise ValueError("groups must be an array")
    ids, used = set(), {}
    for group in groups:
        if not isinstance(group, dict):
            raise ValueError("invalid group")
        _string(group.get("title"), "group title")
        context = group.get("context")
        if not isinstance(context, str) or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", context):
            raise ValueError("invalid context")
        if sheet and context == "global" and doc["id"] not in ("windows-system", "macos-system"):
            raise ValueError("sheet global context is reserved for system sheets; use app for app-wide groups")
        if not isinstance(group.get("shortcuts"), list):
            raise ValueError("shortcuts must be an array")
        for shortcut in group["shortcuts"]:
            if not isinstance(shortcut, dict):
                raise ValueError("invalid shortcut")
            id_ = shortcut.get("id")
            if not isinstance(id_, str) or not re.fullmatch(r"[a-z0-9][a-z0-9.-]*", id_) or id_ in ids:
                raise ValueError("invalid or duplicate shortcut id")
            ids.add(id_)
            _string(shortcut.get("title"), "shortcut title", 80)
            if shortcut.get("description") is not None and not isinstance(shortcut["description"], str):
                raise ValueError("description must be a string")
            if "rebindable" in shortcut:
                if type(shortcut["rebindable"]) is not bool or sheet or context != "global":
                    raise ValueError("rebindable is only allowed in app global groups")
                if shortcut["rebindable"] and manifest is not None and not any(s.get("id") == id_ for s in manifest.get("shortcuts", [])):
                    raise ValueError("rebindable id absent from manifest")
            keys = shortcut.get("keys")
            if not isinstance(keys, dict):
                raise ValueError("keys must be an object")
            nonnull = False
            for name in ("default",) + OS:
                if name in keys:
                    alternatives = _bindings(keys[name])
                    for index, key in enumerate(alternatives):
                        nonnull = True
                        if normalize(key) != key:
                            raise ValueError(f"noncanonical key {key!r}")
                        if any(conflicts(key, previous) for previous in alternatives[:index]):
                            raise ValueError(f"{id_}: conflicting alternatives in keys.{name}")
            if not nonnull:
                raise ValueError("keys need at least one non-null binding")
            for os in OS:
                seen = used.setdefault((context, os), [])
                for key in effective(shortcut, os):
                    for other_id, other_key in seen:
                        if conflicts(key, other_key):
                            raise ValueError(f"{id_}: {os} conflict with {other_id} in {context}")
                    seen.append((id_, key))

def _escape(text):
    return text.replace("\\", "\\\\").replace("|", "\\|").replace("\r", " ").replace("\n", " ")

def markdown(doc: dict) -> str:
    validate(doc)
    result = ""
    for group in doc["groups"]:
        result += f"## {_escape(group['title'])}\n\n| Action | Linux | Windows | macOS |\n| --- | --- | --- | --- |\n"
        for shortcut in group["shortcuts"]:
            cells = [_escape(shortcut["title"])] + ["<br>".join(_escape(display(key, os)) for key in effective(shortcut, os)) or "—" for os in OS]
            result += "| " + " | ".join(cells) + " |\n"
        result += "\n"
    return result

def main(argv=None) -> int:
    # Canonical display includes Unicode glyphs even through a Windows pipe.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", newline="\n")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--markdown", action="store_true")
    args = parser.parse_args(argv)
    try:
        doc = json.loads(args.file.read_text(encoding="utf-8"))
        manifest = json.loads(args.manifest.read_text(encoding="utf-8")) if args.manifest else None
        validate(doc, manifest)
        if args.markdown:
            print(markdown(doc), end="")
        else:
            print(f"{args.file}: valid")
        return 0
    except (ValueError, OSError) as error:
        print(f"shortcuts: {error}", file=sys.stderr)
        return 1

if __name__ == "__main__":
    sys.exit(main())
