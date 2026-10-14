#!/usr/bin/env python3
"""Run non-GUI tests with a temporary profile. This is process-scoped only.
For GUI work use Link tools/e2e.py run or scripts/e2e-linux.sh instead.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

STRIP = ("HYPRLAND_INSTANCE_SIGNATURE", "WAYLAND_DISPLAY", "DISPLAY", "SWAYSOCK", "APPIMAGE", "APPDIR", "ARCADE_DEV_BUILD")
with tempfile.TemporaryDirectory(prefix="arcade.lens-test-home-") as temporary:
    root = Path(temporary)
    env = {k: v for k, v in os.environ.items() if k not in STRIP}
    # Preserve Cargo's download/build cache while HOME is isolated. Never write
    # this (or any generated environment) into a login/startup file.
    env.setdefault("CARGO_HOME", str(Path.home() / ".cargo"))
    env.setdefault("RUSTUP_HOME", str(Path.home() / ".rustup"))
    for key, name in (("HOME", "home"), ("USERPROFILE", "home"), ("XDG_CONFIG_HOME", "config"),
                      ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "run"),
                      ("ARCADE_HOME", "arcade"), ("ARCADE_LENS_HOME", "app"), ("APPDATA", "roaming"), ("LOCALAPPDATA", "local")):
        path = root / name
        path.mkdir(exist_ok=True)
        env[key] = str(path)
    os.chmod(root / "run", 0o700)
    env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={root}/no-bus"
    sys.exit(subprocess.call(sys.argv[1:], env=env))
