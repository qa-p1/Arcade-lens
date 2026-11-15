#!/usr/bin/env python3
"""Tier 1 (headless Hyprland, Link's desktop harness) for Arcade Lens's windows.

    lens_desktop.py <AppImage>

In a private profile, from the AppImage the way a user starts it (it hands over to its unpacked copy, D8). The
harness has no screenshot portal, so the capture overlay is opened on a known image (`--window analyze`, the overlay
Arcade Link's `lens.analyze` opens): the same overlay, selection and keys as a capture.
- ID-1: every Lens window's class is `dev.arcade.Lens`, told apart by title.
- The overlay maps floating and full-screen (Lens's window rules, installed by the resident) and has keyboard focus.
- RD-LENS-1: Ctrl+C on a selection copies it on Wayland (an image on the clipboard).
- Escape closes the overlay and its process exits.
- RD-LENS-3: on a pin with its menu open, the first Escape closes only the menu, the second the pin.
- Settings opens as its own floating window and Ctrl+W closes it.
- --status reports the resident and --quit stops it.
Exit 0 when every check passes, 1 otherwise. LN-10 (loupe colour) and RD-LENS-4 (pin menu clipped) are not
checked here: the harness cannot read what egui draws (docs/v1/DECISIONS.md D11).
"""
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

sys.path.insert(0, ".arcade-link/tools/desktop")
from arcade_desktop import Desktop, DesktopError  # noqa: E402

CLASS = "dev.arcade.Lens"
OVERLAY, PIN, SETTINGS = "Arcade Lens", "Pin — Arcade Lens", "Settings — Arcade Lens"
d = Desktop()
home = tempfile.mkdtemp(prefix="lens-desktop-")
downloads = Path(home, "Downloads")
downloads.mkdir()
app = str(downloads / "Arcade-Lens.AppImage")
shutil.copy2(sys.argv[1], app)
os.chmod(app, 0o755)
env = {**os.environ, "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_DATA_HOME": f"{home}/.local/share",
       "XDG_CACHE_HOME": f"{home}/.cache", "ARCADE_HOME": f"{home}/arcade-link", "ARCADE_NO_INSTALL_PROMPT": "1"}
# The harness marks AppImages "already unpacked" so tests keep the process they start; Lens is followed through
# --status and the compositor instead, so it runs as a user's would.
env.pop("ARCADE_UNPACKED", None)
log = open(d.artifact("lens.log"), "wb")
results = []


def png(path, width, height, pixel):
    """A solid-colour RGB PNG (no imaging library on the runner)."""
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + bytes(pixel) * width for _ in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


image = Path(home, "red.png")
monitor = d.monitors()[0]
png(image, monitor["width"], monitor["height"], (255, 0, 0))
small = Path(home, "pin.png")
png(small, 320, 200, (0, 128, 255))


def run(*args, timeout=60):
    return subprocess.run([app, *args], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)


def window(*args):
    return subprocess.Popen([app, "--window", *args], env=env, stdout=log, stderr=subprocess.STDOUT)


def status():
    out = subprocess.run([app, "--status"], env=env, capture_output=True, text=True, timeout=30)
    try:
        return json.loads(out.stdout.strip().splitlines()[-1])
    except (IndexError, ValueError):
        return {}


def check(name, ok, detail=""):
    result = "PASS" if ok else "FAIL"
    if not ok:
        d.screenshot(name.replace(" ", "-").replace("/", "-").lower()[:60])
    results.append((result, name, detail))
    print(f"{result} {name}{': ' + detail if detail else ''}", flush=True)
    return ok


def find(title):
    return next((c for c in d.clients() if c.get("class") == CLASS and c.get("title") == title), None)


def shown(title, timeout=30):
    try:
        return d.wait(lambda: find(title), timeout, f"{title} shown")
    except DesktopError:
        return None


def gone(title, process=None, timeout=8):
    try:
        d.wait(lambda: find(title) is None and (process is None or process.poll() is not None), timeout,
               f"{title} closed")
        return True
    except DesktopError:
        return False


def active():
    try:
        return json.loads(d.hyprctl("-j", "activewindow")).get("address")
    except (DesktopError, ValueError):
        return None


def clipboard_types():
    r = subprocess.run(["wl-paste", "--list-types"], capture_output=True, text=True, timeout=10)
    return r.stdout.split() if r.returncode == 0 else []


resident = subprocess.Popen([app, "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)
try:
    try:
        d.wait(lambda: status().get("running"), 60, "Lens to report running")
        check("--status reports the resident", True)
    except DesktopError:
        check("--status reports the resident", False, json.dumps(status()))
        raise SystemExit(1)

    # --- the overlay ----------------------------------------------------------------------------------------------
    overlay_process = window("analyze", str(image))
    overlay = shown(OVERLAY)
    if not check("the overlay opens on an image", overlay is not None):
        raise SystemExit(1)
    check("ID-1: the overlay's class is Lens's desktop id", overlay.get("class") == CLASS, overlay.get("class", ""))
    check("the overlay maps floating and full-screen (window rules)",
          overlay.get("floating") is True and overlay.get("fullscreen") not in (0, False, None),
          f"floating={overlay.get('floating')} fullscreen={overlay.get('fullscreen')}")
    try:
        d.wait(lambda: active() == overlay["address"], 5, "overlay focused")
        check("the overlay has keyboard focus when it opens", True)
    except DesktopError:
        check("the overlay has keyboard focus when it opens", False, f"active {active()}")

    x0, y0 = monitor["x"] + monitor["width"] // 4, monitor["y"] + monitor["height"] // 4
    d.drag((x0, y0), (x0 + 300, y0 + 200))
    time.sleep(0.8)
    subprocess.run(["wl-copy", "--clear"], capture_output=True, timeout=10)
    d.send_shortcut(overlay["address"], "CTRL", "c")
    try:
        d.wait(lambda: "image/png" in clipboard_types(), 10, "an image on the clipboard")
        check("RD-LENS-1: Ctrl+C copies the selection on Wayland", True)
    except DesktopError:
        check("RD-LENS-1: Ctrl+C copies the selection on Wayland", False, f"types {clipboard_types()}")

    if find(OVERLAY):
        d.escape(find(OVERLAY)["address"])
        if find(OVERLAY):  # The first Escape may only drop the selection; the next one closes the overlay.
            time.sleep(0.5)
            if find(OVERLAY):
                d.escape(find(OVERLAY)["address"])
    check("Escape closes the overlay and its process exits", gone(OVERLAY, overlay_process))

    # --- a pin: Escape closes its menu first ----------------------------------------------------------------------
    pin_process = window("pin", str(small))
    pin = shown(PIN)
    if check("a pin opens", pin is not None):
        check("ID-1: the pin's class is Lens's desktop id", pin.get("class") == CLASS)
        check("the pin floats above other windows (window rules)", pin.get("floating") is True and pin.get("pinned") is True,
              f"floating={pin.get('floating')} pinned={pin.get('pinned')}")
        x, y = d.center_of(pin)
        d.pointer_move(x, y)
        d.click(x, y, button="right")
        time.sleep(0.8)
        d.escape(pin["address"])
        time.sleep(0.8)
        check("RD-LENS-3: Escape with the pin's menu open closes only the menu", find(PIN) is not None)
        if find(PIN):
            d.escape(find(PIN)["address"])
        check("the next Escape closes the pin", gone(PIN, pin_process))

    # --- Settings -------------------------------------------------------------------------------------------------
    settings_process = window("settings")
    settings = shown(SETTINGS)
    if check("Settings opens", settings is not None):
        check("Settings floats (window rules)", settings.get("floating") is True)
        d.send_shortcut(settings["address"], "CTRL", "w")
        check("Ctrl+W closes Settings", gone(SETTINGS, settings_process))

    run("--quit")
    try:
        d.wait(lambda: not status().get("running"), 20, "Lens to quit")
        check("--quit stops the resident", True)
    except DesktopError:
        check("--quit stops the resident", False, json.dumps(status()))
finally:
    subprocess.run([app, "--quit"], env=env, capture_output=True, timeout=30)
    for process in [resident]:
        if process.poll() is None:
            process.terminate()
    log.flush()
    failed = [r for r in results if r[0] == "FAIL"]
    print(f"\n{len(results) - len(failed)}/{len(results)} checks passed", flush=True)
    if failed:  # Also when a check stopped the run early.
        print(Path(d.artifact("lens.log")).read_text(errors="replace")[-6000:], flush=True)

sys.exit(1 if failed else 0)
