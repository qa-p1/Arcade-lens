#!/usr/bin/env python3
"""Verify the normal Linux release UI and OCR inside Arcade Link's runner.

Run with tools/e2e.py run -- python3 scripts/verify-desktop.py
and repeat with --peers. Never launches against the owner's desktop/profile.
Requires ImageMagick, Tesseract and the built siblings.
"""

import argparse
import importlib.util
import json
import os
import signal
import subprocess
from pathlib import Path


def load_module(name, path, **globals):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    module.__dict__.update(globals)
    spec.loader.exec_module(module)
    return module


def verify(peers):
    assert os.environ.get("ARCADE_E2E_INNER") == "1", "use Arcade-link/tools/e2e.py run --"
    root = Path(os.environ["ARCADE_E2E_ROOT"])
    for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "ARCADE_HOME", "ARCADE_LENS_HOME"):
        assert Path(os.environ[key]).is_relative_to(root), f"{key} is outside the isolated session"
    assert not os.environ.get("WAYLAND_DISPLAY") and not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    repo = Path(__file__).resolve().parents[1]
    link = repo.parent / "Arcade-link"
    harness = load_module("arcade_e2e", link / "tools/e2e.py")
    checks = load_module("lens_checks", link / "tools/e2e_checks/lens.py",
                         check=lambda _: lambda f: f, APPS=harness.APPS, CLI=harness.CLI)
    # The outer runner already owns Xvfb, the private bus and keyring. Adopt
    # its environment and track only the processes this command starts.
    s = harness.Session.__new__(harness.Session)
    s.root, s.env, s.procs = root, dict(os.environ), {}
    label = "peers" if peers else "alone"
    assert json.loads(s.cli("ls", "--json", check=True).stdout) == [], "session has existing peers"
    try:
        if peers:
            checks._clipboard_driver(s, checks._clipboard_init(s),
                                     {"op": "create_mesh", "device_name": "Lens desktop verification"},
                                     {"op": "shutdown"})
            s.start("arcade.clipboard")
            checks._box_start_with_pipelines(s)
            s.env["ALOOK_E2E_MAP_EARLY"] = "1"
            s.start("arcade.look")
            s.start("arcade.wheel")
        checks._start(s)
        registered = json.loads(s.cli("ls", "--json", check=True).stdout)
        wanted = set(harness.APPS) if peers else {"arcade.lens"}
        assert {row["id"] for row in registered} == wanted, registered
        assert all(row["state"].startswith("running") for row in registered), registered

        fixture = root / "lens-ocr-fixture.png"
        subprocess.run(["magick", "-size", "840x210", "xc:white", "-font", "DejaVu-Sans",
                        "-pointsize", "38", "-fill", "black",
                        "-annotate", "+25+75", "Arcade Lens reads this text",
                        "-annotate", "+25+145", "Capture and copy locally", str(fixture)],
                       env=s.env, check=True)
        display = subprocess.Popen(["display", "-title", "Lens OCR fixture", "-geometry", "+100+100", str(fixture)],
                                   env=s.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   start_new_session=True)
        s.procs["lens-ocr-fixture"] = display
        win = s.wait_window("^Lens OCR fixture$")
        s.xdotool("windowraise", win)
        geometry = dict(line.split("=", 1) for line in s.xdotool("getwindowgeometry", "--shell", win).splitlines())
        assert (geometry["WIDTH"], geometry["HEIGHT"]) == ("840", "210"), geometry
        x, y = int(geometry["X"]), int(geometry["Y"])
        # Drive Lens's existing single-instance capture command, independent
        # of any peer or the Link invocation path.
        exe = harness.APPS["arcade.lens"]["dir"] / harness.APPS["arcade.lens"]["bin"]
        s.lens_palette_offset = len(checks._lens_log(s))
        subprocess.run([str(exe), "--capture"], env=s.env, check=True, timeout=15)
        checks._focus(s)
        s.xdotool("mousemove", str(x), str(y), "sleep", "0.2", "mousedown", "1",
                  "sleep", "0.2", "mousemove", str(x + 840), str(y + 210), "sleep", "0.2")
        s.screenshot(f"lens-final-{label}-capture")
        s.xdotool("mouseup", "1")
        checks._palette(s, lambda es: any(e["action"] == "core.region.copy" for e in es))
        s.screenshot(f"lens-final-{label}-palette")
        entries = checks._palette(s, lambda es: any(e["action"] == "core.text.copy" for e in es))
        if peers:
            assert any(e["action"] == "arcade.clipboard.add" for e in entries), entries
        else:
            assert all(not e["action"].startswith("arcade.") for e in entries), entries
        checks._filter(s, "Copy Text")
        s.screenshot(f"lens-final-{label}-ocr")
        # Verify the real OCR engine and output as well as its native palette.
        code, result = s.invoke("lens", "lens.recognize", "--file", str(fixture),
                                "--option", "ocrOnly=true", timeout=120)
        assert code == 0 and result["data"]["ocrEngine"] == "Tesseract", result
        text = "\n".join(o["text"] for o in result["outputs"] if o["type"] == "text/plain")
        assert "arcade lens reads this text" in text.lower(), text
        assert "capture and copy locally" in text.lower(), text
        s.xdotool("key", "Return")
        checks._wait(lambda: "completed action core.text.copy" in checks._lens_log(s)[s.lens_palette_offset:],
                     "native Copy Text did not complete")
        checks._closed(s)
        assert {row["id"] for row in json.loads(s.cli("ls", "--json", check=True).stdout)} == wanted
        print(json.dumps({"scenario": label, "registered": sorted(wanted), "ocrEngine": "Tesseract",
                          "text": text, "capture": "840x210", "nativeCopyText": "passed"}), flush=True)
    except Exception:
        s.screenshot(f"lens-final-{label}-failed")
        print(s.log("arcade.lens"), flush=True)
        raise
    finally:
        for app in list(s.procs):
            s.kill(app, signal.SIGTERM)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--peers", action="store_true")
    args = parser.parse_args()
    verify(args.peers)
