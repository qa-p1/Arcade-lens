#!/usr/bin/env python3
"""Smoke-test a real AppImage outside the checkout with a disposable profile."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    image, app_id, filename, cli_name = sys.argv[1:]
    image = Path(image).resolve(strict=True)
    # Link treats executables anywhere under a .git tree as development builds.
    with tempfile.TemporaryDirectory(prefix=app_id + "-", dir=os.environ.get("RUNNER_TEMP")) as temporary:
        root = Path(temporary).resolve()
        for parent in (root, *root.parents):
            if (parent / ".git").exists():
                raise RuntimeError("AppImage verification must run outside the checkout")
        env = os.environ.copy()
        for key in ("APPIMAGE", "APPDIR", "ARCADE_DEV_BUILD"):
            env.pop(key, None)
        for key, directory in (
            ("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "run"), ("ARCADE_HOME", "arcade"),
            ("ARCADE_LENS_HOME", "lens"), ("ARCADE_LOOK_HOME", "look"),
        ):
            path = root / directory
            path.mkdir()
            env[key] = str(path)
        (root / "run").chmod(0o700)
        env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
        env["ARCADE_NO_INSTALL_PROMPT"] = "1"
        # ARCADE_HOME redirects Link's Applications, integration and CLI roots.
        install_home = Path(env["ARCADE_HOME"])
        env["PATH"] = str(install_home / ".local/bin") + os.pathsep + env.get("PATH", "")
        download = root / "Downloaded.AppImage"
        shutil.copy2(image, download)
        download.chmod(0o755)

        def run(executable, *args):
            result = subprocess.run(
                [str(executable), *args], env=env, text=True, encoding="utf-8",
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180,
            )
            if result.returncode:
                raise RuntimeError(f"{executable} {' '.join(args)} exited {result.returncode}\n{result.stdout}\n{result.stderr}")
            return result.stdout

        version = run(download, "--version").strip().split()[-1]
        run(download, "--install", "--silent", "--no-launch")
        receipt_path = root / "arcade/installs" / (app_id + ".json")
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        assert receipt["schema"] == 1 and receipt["id"] == app_id, receipt
        assert receipt["method"] == "appimage" and receipt["managedBy"] == "self", receipt
        assert receipt["version"] == version and receipt["channel"] == "stable", receipt
        installed = install_home / "Applications/Arcade" / filename
        assert Path(receipt["path"]) == installed and os.access(installed, os.X_OK), receipt
        assert installed.read_bytes() == image.read_bytes(), "Installed package differs from the download"
        integration = receipt["integration"]
        desktop = Path(integration["desktopEntry"])
        cli = Path(integration["cli"])
        autostart = Path(integration["autostart"])
        icons = [Path(path) for path in integration["icons"]]
        entries = [installed, desktop, cli, autostart, *icons]
        assert icons, "No installed icons"
        for path in entries:
            assert path.is_relative_to(root) and path.exists(), path
        assert cli == install_home / ".local/bin" / cli_name and cli.is_symlink(), cli
        assert cli.resolve() == installed, cli
        assert f"X-Arcade-Id={app_id}" in desktop.read_text(encoding="utf-8"), desktop
        assert f"X-Arcade-Id={app_id}" in autostart.read_text(encoding="utf-8"), autostart
        assert run(cli_name, "--version").strip().split()[-1] == version
        status = json.loads(run(cli_name, "--integration-status"))
        assert status["receipt"]["path"] == str(installed), status
        assert status["entries"] and all(entry["exists"] for entry in status["entries"]), status

        # A missing launcher must be repaired from the installed package.
        desktop.unlink()
        run(installed, "--repair")
        assert desktop.is_file(), "Repair did not restore the launcher"
        status = json.loads(run(installed, "--integration-status"))
        assert all(entry["exists"] for entry in status["entries"]), status
        app_home = Path(env["ARCADE_" + app_id.split(".")[-1].upper() + "_HOME"])
        sentinel = app_home / "keep-user-data.txt"
        sentinel.write_text("preserve", encoding="utf-8")
        run(installed, "--uninstall", "--silent")
        for path in [receipt_path, *entries]:
            assert not path.exists() and not path.is_symlink(), f"Uninstall retained {path}"
        assert sentinel.read_text(encoding="utf-8") == "preserve", "Uninstall removed user data"
        for name in (".profile", ".bash_profile", ".bashrc", ".zprofile", ".zshrc"):
            assert not (root / "home" / name).exists(), f"Installer created {name}"
        print(f"{app_id}: AppImage install, receipt, launcher, icons, CLI, autostart, repair and uninstall passed")


if __name__ == "__main__":
    main()
