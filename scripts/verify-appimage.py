#!/usr/bin/env python3
"""Smoke-test a real AppImage outside the checkout with a disposable profile."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def inventory(root):
    """Every file and symlink under `root`, relative to it (directories are not counted: Link leaves its empty
    parents behind, and empty folders hold nothing of the user's)."""
    found = set()
    for folder, dirs, files in os.walk(root):
        base = Path(folder)
        found.update((base / name).relative_to(root) for name in files)
        found.update((base / name).relative_to(root) for name in dirs if (base / name).is_symlink())
    return found


def leftovers(root, before, env, keep_user_data):
    """What an uninstall left that was not there before the install. `keep_user_data` allows Lens's own folders
    (`ARCADE_LENS_HOME`), which a plain uninstall keeps on purpose. Not counted: Link's runtime state (sockets and
    locks in `run`, `arcade/run` and `arcade/handoff`). Adapted from Arcade Box (D19) and Look (D20)."""
    lens_home = Path(env["ARCADE_LENS_HOME"]).relative_to(root)
    runtime_state = {Path("run"), Path("arcade/run"), Path("arcade/handoff")}
    return [path for path in sorted(inventory(root) - before)
            if not any(path.is_relative_to(state) for state in runtime_state)
            and not (keep_user_data and path.is_relative_to(lens_home))]


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
        # Mirror a real profile: Link's environment and its unpacked copies (U1) must resolve the same XDG roots
        # even with ARCADE_HOME isolation, as in Look's check.
        env["XDG_DATA_HOME"] = str(install_home / ".local/share")
        env["XDG_CONFIG_HOME"] = str(install_home / ".config")
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
        # An unknown option is a usage error: exit 2 with a hint on stderr (Link conformance CLI-2).
        unknown = subprocess.run([str(download), "--arcade-unknown-option"], env=env, text=True, encoding="utf-8",
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
        assert unknown.returncode == 2 and "unknown option" in unknown.stderr, (unknown.returncode, unknown.stderr)
        before = inventory(root)
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
        # Lens's settings folder (config.rs: `config` under ARCADE_LENS_HOME), which --remove-data deletes.
        app_home = Path(env["ARCADE_LENS_HOME"]) / "config"
        app_home.mkdir(parents=True, exist_ok=True)
        sentinel = app_home / "keep-user-data.txt"
        sentinel.write_text("preserve", encoding="utf-8")
        run(installed, "--uninstall", "--silent")
        for path in [receipt_path, *entries]:
            assert not path.exists() and not path.is_symlink(), f"Uninstall retained {path}"
        assert sentinel.read_text(encoding="utf-8") == "preserve", "Uninstall removed user data"
        for name in (".profile", ".bash_profile", ".bashrc", ".zprofile", ".zshrc"):
            assert not (root / "home" / name).exists(), f"Installer created {name}"
        # Clean uninstall (Round 5, D16): nothing but Lens's kept data is new, and no unpacked copy (U1) remains.
        fails = leftovers(root, before, env, keep_user_data=True)
        assert not fails, "Uninstall left files behind:\n" + "\n".join(map(str, fails))
        copies = list(root.glob("**/arcade/runtime/lens-*")) + list(root.glob("**/arcade/runtime/.lens-*"))
        assert not copies, f"Uninstall left unpacked copies: {copies}"
        # "Remove my data": install again, run Lens once, then nothing of Lens's remains, its settings included.
        run(download, "--install", "--silent", "--no-launch")
        assert receipt_path.is_file(), "Second install wrote no receipt"
        run(installed, "--version")
        sentinel.write_text("remove", encoding="utf-8")
        run(installed, "--uninstall", "--silent", "--remove-data")
        for path in [receipt_path, *entries]:
            assert not path.exists() and not path.is_symlink(), f"Uninstall --remove-data retained {path}"
        assert not sentinel.exists(), "Uninstall --remove-data kept Lens's settings"
        fails = leftovers(root, before, env, keep_user_data=False)
        assert not fails, "Uninstall --remove-data left files behind:\n" + "\n".join(map(str, fails))
        copies = list(root.glob("**/arcade/runtime/lens-*")) + list(root.glob("**/arcade/runtime/.lens-*"))
        assert not copies, f"Uninstall --remove-data left unpacked copies: {copies}"
        print(f"{app_id}: AppImage install, receipt, launcher, icons, CLI, autostart, repair, uninstall and "
              "uninstall --remove-data passed")


if __name__ == "__main__":
    main()
