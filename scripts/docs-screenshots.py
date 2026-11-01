#!/usr/bin/env python3
"""Retake the README and docs/user/images screenshots from a release build.

    dbus-run-session -- python3 scripts/docs-screenshots.py target/release/arcade-lens docs/user/images <work dir>

Runs Lens in a throwaway profile on a private Xvfb (:77) with xcompmgr, over a
generated page, and takes the action palette over a capture, the annotate
editor and Settings, in light and in dark. Needs Xvfb, xcompmgr, xdotool,
ImageMagick 6 (convert, display, import) and Tesseract. Never touches the
desktop you are using or your Lens profile.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

W, H = 1440, 900
CONVERT = "convert"


def sh(*args, **kw):
    return subprocess.run([str(a) for a in args], check=True, **kw)


def backdrop(path, dark):
    bg1, bg2 = ("#1d2733", "#121820") if dark else ("#dfe7ef", "#c9d4e0")
    page, ink, muted, bar, rule = (
        ("#1b1f24", "#e6e9ee", "#9aa3ad", "#252a31", "#323943") if dark
        else ("#ffffff", "#1d232b", "#5d6773", "#eef1f4", "#dde2e8"))
    x0, y0, x1, y1 = 150, 80, 1290, 820
    font = "DejaVu-Sans"
    mono = "DejaVu-Sans-Mono"
    cmd = [CONVERT, "-size", f"{W}x{H}", f"gradient:{bg1}-{bg2}",
           "-fill", page, "-draw", f"roundrectangle {x0},{y0} {x1},{y1} 10,10",
           "-fill", bar, "-draw", f"roundrectangle {x0},{y0} {x1},{y0 + 44} 10,10",
           "-draw", f"rectangle {x0},{y0 + 30} {x1},{y0 + 44}",
           "-fill", "#e5534b", "-draw", f"circle {x0 + 24},{y0 + 22} {x0 + 30},{y0 + 22}",
           "-fill", "#e0a63a", "-draw", f"circle {x0 + 46},{y0 + 22} {x0 + 52},{y0 + 22}",
           "-fill", "#57b65f", "-draw", f"circle {x0 + 68},{y0 + 22} {x0 + 74},{y0 + 22}",
           "-font", font, "-fill", muted, "-pointsize", "15",
           "-annotate", f"+{x0 + 520}+{y0 + 28}", "Release notes — draft",
           "-fill", ink, "-pointsize", "34", "-annotate", f"+{x0 + 70}+{y0 + 120}", "Shipping the new Arcade look",
           "-fill", muted, "-pointsize", "17", "-annotate", f"+{x0 + 70}+{y0 + 156}", "Updated 1 November 2026 · 4 min read",
           "-fill", rule, "-draw", f"rectangle {x0 + 70},{y0 + 180} {x1 - 70},{y0 + 181}",
           "-fill", ink, "-pointsize", "21"]
    lines = [
        "Every Arcade app now shares one design kit: the lime accent,",
        "the Inter typeface and light and dark themes that switch live.",
        "Read the full guide at https://github.com/qa-p1/Arcade-lens",
        "or write to the team at hello@arcade.dev with feedback.",
    ]
    for i, line in enumerate(lines):
        cmd += ["-annotate", f"+{x0 + 70}+{y0 + 232 + i * 36}", line]
    cmd += ["-fill", muted, "-pointsize", "17", "-annotate", f"+{x0 + 70}+{y0 + 410}", "Accent swatch"]
    cmd += ["-fill", "#9bd34a", "-draw", f"roundrectangle {x0 + 70},{y0 + 426} {x0 + 230},{y0 + 506} 8,8",
            "-fill", ink, "-font", mono, "-pointsize", "19", "-annotate", f"+{x0 + 250}+{y0 + 474}", "#9BD34A   rgb(155, 211, 74)"]
    cmd += ["-fill", bar, "-draw", f"roundrectangle {x0 + 70},{y0 + 540} {x1 - 70},{y0 + 680} 8,8",
            "-fill", muted, "-font", mono, "-pointsize", "17"]
    code = ["$ arcade-lens --status", '{"running":true,"version":"0.4.0","overlay":false}',
            "$ arcade-lens --capture"]
    for i, line in enumerate(code):
        cmd += ["-annotate", f"+{x0 + 96}+{y0 + 582 + i * 32}", line]
    cmd.append(str(path))
    sh(*cmd)


def main():
    binary, out, work = Path(sys.argv[1]).resolve(), Path(sys.argv[2]), Path(sys.argv[3])
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    disp = ":77"
    xvfb = subprocess.Popen(["Xvfb", disp, "-screen", "0", f"{W}x{H}x24", "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.0)
    comp = subprocess.Popen(["xcompmgr", "-n"], env=dict(os.environ, DISPLAY=disp),
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    try:
        for theme in ("light", "dark"):
            shoot(binary, out, work / theme, disp, theme)
    finally:
        comp.terminate()
        xvfb.terminate()


def shoot(binary, out, root, disp, theme):
    for d in ("home", "lens/config", "arcade", "xdg-config", "xdg-data", "runtime"):
        (root / d).mkdir(parents=True, exist_ok=True)
    os.chmod(root / "runtime", 0o700)
    (root / "lens/config/settings.toml").write_text(f'appearance = "{theme}"\n')
    env = dict(os.environ, DISPLAY=disp, HOME=str(root / "home"), ARCADE_LENS_HOME=str(root / "lens"),
               ARCADE_HOME=str(root / "arcade"), XDG_CONFIG_HOME=str(root / "xdg-config"),
               XDG_DATA_HOME=str(root / "xdg-data"), XDG_RUNTIME_DIR=str(root / "runtime"),
               ARCADE_NO_INSTALL_PROMPT="1")
    env.pop("WAYLAND_DISPLAY", None)

    def x(*args):
        return subprocess.run(["xdotool", *map(str, args)], env=env, capture_output=True, text=True).stdout

    def lens(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=20)

    def status():
        o = lens("--status").stdout.strip()
        return json.loads(o) if o.startswith("{") else {}

    def wait(check, what, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if check():
                return
            time.sleep(0.1)
        raise SystemExit(f"FAIL {theme}: {what}")

    def grab(name, window="root"):
        sh("import", "-window", window, str(out / f"{name}-{theme}.png"), env=env)

    bg = root / "backdrop.png"
    backdrop(bg, theme == "dark")
    procs = [subprocess.Popen(["display", "-geometry", "+0+0", "-borderwidth", "0", "-title", "backdrop", str(bg)],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
    time.sleep(1.0)
    log = open(root / "lens.log", "w")
    procs.append(subprocess.Popen([str(binary), "--background"], env=env, stdout=log, stderr=log))
    try:
        wait(lambda: status().get("running"), "Lens did not start")
        # Palette over a capture: select the paragraph with the link and the address.
        lens("--capture")
        wait(lambda: status().get("overlay") is True, "capture did not open")
        time.sleep(1.2)
        x("mousemove", 205, 285, "sleep", 0.2, "mousedown", 1, "sleep", 0.2,
          "mousemove", 600, 330, "sleep", 0.1, "mousemove", 900, 400, "sleep", 0.1, "mousemove", 1000, 445, "sleep", 0.3, "mouseup", 1)
        time.sleep(3.5)  # text recognition, then the palette settles
        grab("palette")
        # Annotate from the palette.
        x("key", "space")
        time.sleep(0.4)
        x("type", "--delay", 30, "Annotate")
        time.sleep(0.6)
        x("key", "Return")
        wait(lambda: x("search", "--onlyvisible", "--name", "^Annotate").strip(), "annotate did not open")
        time.sleep(1.5)
        win = x("search", "--onlyvisible", "--name", "^Annotate").split()[0]
        geo = dict(l.split("=", 1) for l in x("getwindowgeometry", "--shell", win).splitlines())
        ax, ay, aw, ah = (int(geo[k]) for k in ("X", "Y", "WIDTH", "HEIGHT"))
        x("windowfocus", win)
        time.sleep(0.3)
        # The capture (795 x 160) sits centred under the one-row toolbar.
        top = int(os.environ.get("TOOLBAR", "72"))
        ox, oy = ax + (aw - 795) // 2, ay + top + (ah - top - 160) // 2
        x("key", 5)  # a rectangle around the link
        x("mousemove", ox + 238, oy + 72, "mousedown", 1, "sleep", 0.1, "mousemove", ox + 240, oy + 73, "sleep", 0.1, "mousemove", ox + 400, oy + 90,
          "sleep", 0.1, "mousemove", ox + 650, oy + 110, "sleep", 0.2, "mouseup", 1)
        x("key", 4)  # an arrow pointing at it
        x("mousemove", ox + 110, oy + 150, "mousedown", 1, "sleep", 0.1, "mousemove", ox + 112, oy + 149, "sleep", 0.1, "mousemove", ox + 170, oy + 125,
          "sleep", 0.1, "mousemove", ox + 228, oy + 100, "sleep", 0.2, "mouseup", 1)
        time.sleep(0.6)
        grab("annotate", win)
        x("windowkill", win)
        time.sleep(0.8)
        # Settings window alone.
        lens("--settings")
        wait(lambda: x("search", "--onlyvisible", "--name", "^Settings").strip(), "settings did not open")
        time.sleep(2.0)
        win = x("search", "--onlyvisible", "--name", "^Settings").split()[0]
        grab("settings", win)
    finally:
        lens("--quit")
        time.sleep(0.5)
        for p in procs:
            p.terminate()
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()
        log.close()


if __name__ == "__main__":
    main()
