#!/usr/bin/env python3
"""The capture overlay closes when another app takes focus (STANDARD: overlays
close on Esc and when they lose focus), stays open while it has focus, and Esc
still closes it. Runs the real binary in a throwaway profile on an X server
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
            time.sleep(1.5)
            if not overlay_open():
                raise SystemExit(f"FAIL: {label}: the capture closed while it had focus")
            wid = subprocess.run(["xdotool", "search", "--name", "^lens-focus-other$"], capture_output=True, text=True).stdout.split()
            if not wid:
                raise SystemExit("FAIL: the other window is missing")
            subprocess.run(["xdotool", "windowfocus", wid[0]], check=True)
            wait_for(lambda: not overlay_open(), 3, f"{label}: the capture stayed open after another app took focus")
            print(f"ok: {label}: losing focus closes the capture")
            lens("--capture")
            wait_for(overlay_open, 15, f"{label}: the second capture did not open")
            time.sleep(1.0)
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
