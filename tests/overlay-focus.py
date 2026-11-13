#!/usr/bin/env python3
"""The capture overlay stays open when another app takes focus (the fleet popup
rule, Lens D5 / LN-4: only Esc, an action, the shortcut or --hide close it), and
Esc closes it. Runs the real binary in a throwaway profile on an X server
(xvfb-run), in the default windowless mode and with ARCADE_LENS_RESIDENT_UI=1.

    xvfb-run -a dbus-run-session -- python3 tests/overlay-focus.py target/release/arcade-lens

Needs xdotool and xmessage (x11-utils).
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def wait_for(check, timeout, what):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if check():
            return
        time.sleep(0.1)
    raise SystemExit(f"FAIL: {what}")


def focused_name():
    return subprocess.run(["xdotool", "getwindowfocus", "getwindowname"], capture_output=True, text=True).stdout.strip()


def took_focus(label, what):
    """Waits for the capture to have focus (a slow runner maps it later)."""
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if focused_name() == "Arcade Lens":
            return
        time.sleep(0.1)
    wid = subprocess.run(["xdotool", "getwindowfocus"], capture_output=True, text=True).stdout.strip()
    props = subprocess.run(["xprop", "-id", wid, "WM_CLASS", "WM_NAME", "_NET_WM_NAME"], capture_output=True, text=True).stdout.strip() if wid else ""
    geometry = subprocess.run(["xdotool", "getwindowgeometry", wid], capture_output=True, text=True).stdout.strip() if wid else ""
    raise SystemExit(f"FAIL: {label}: {what} never took focus (focused: {wid} {props!r} {geometry!r})")


def run(binary, resident):
    with tempfile.TemporaryDirectory(prefix="lens-overlay-focus-") as tmp:
        root = Path(tmp)
        env = dict(os.environ, HOME=str(root / "home"), ARCADE_LENS_HOME=str(root / "lens"),
                   ARCADE_HOME=str(root / "arcade"), ARCADE_NO_INSTALL_PROMPT="1")
        if resident:
            env["ARCADE_LENS_RESIDENT_UI"] = "1"
        for d in ("home", "lens", "arcade"):
            (root / d).mkdir()

        def lens(*args):
            return subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=20)

        def overlay_open():
            out = lens("--status").stdout
            return bool(out.strip()) and json.loads(out).get("overlay") is True

        daemon = subprocess.Popen([binary, "--background"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        other = subprocess.Popen(["xmessage", "-geometry", "300x100+40+40", "-title", "lens-focus-other", "other app"])
        try:
            wait_for(lambda: lens("--status").stdout.strip().startswith("{") and json.loads(lens("--status").stdout)["running"], 20, "Lens did not start")
            label = "resident" if resident else "windowless"
            lens("--capture")
            wait_for(overlay_open, 15, f"{label}: the capture did not open")
            took_focus(label, "the capture")
            time.sleep(1.0)
            if not overlay_open():
                raise SystemExit(f"FAIL: {label}: the capture closed while it had focus")
            wid = subprocess.run(["xdotool", "search", "--name", "^lens-focus-other$"], capture_output=True, text=True).stdout.split()
            if not wid:
                raise SystemExit("FAIL: the other window is missing")
            subprocess.run(["xdotool", "windowfocus", wid[0]], check=True)
            time.sleep(1.5)
            if not overlay_open():
                raise SystemExit(f"FAIL: {label}: the capture closed when another app took focus")
            print(f"ok: {label}: losing focus keeps the capture open")
            # Esc goes to the focused window: give the capture focus back, as a click would.
            lens_wid = subprocess.run(["xdotool", "search", "--name", "^Arcade Lens$"], capture_output=True, text=True).stdout.split()
            if not lens_wid:
                raise SystemExit(f"FAIL: {label}: the capture window is missing")
            subprocess.run(["xdotool", "windowfocus", lens_wid[-1]], check=True)
            took_focus(label, "the capture, refocused")
            subprocess.run(["xdotool", "key", "Escape"], check=True)
            wait_for(lambda: not overlay_open(), 3, f"{label}: Esc did not close the capture")
            print(f"ok: {label}: Esc closes the capture")
        finally:
            lens("--quit")
            for p in (other, daemon):
                p.terminate()
                try:
                    p.wait(5)
                except subprocess.TimeoutExpired:
                    p.kill()


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    run(binary, resident=False)
    run(binary, resident=True)


if __name__ == "__main__":
    main()
