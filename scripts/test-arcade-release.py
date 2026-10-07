#!/usr/bin/env python3
"""Check release metadata against Lens's actual package names."""

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class ReleaseFixture(unittest.TestCase):
    def test_packages_checksums_and_repeat_generation(self):
        (ROOT / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="lens-release-fixture-", dir=ROOT / "target") as tmp:
            dist = Path(tmp)
            files = {
                "ArcadeLens-0.1.0-linux-x86_64.AppImage": b"Linux package",
                "ArcadeLens-0.1.0-windows-x64-setup.exe": b"Inno installer",
                "ArcadeLens-0.1.0-windows-x64-portable.exe": b"Portable executable",
                "ArcadeLens-0.1.0-macos-universal.dmg": b"Universal disk image",
                "symbols.zip": b"Debug symbols",
            }
            for name, data in files.items():
                (dist / name).write_bytes(data)
            command = [
                "python3", str(ROOT / "scripts/arcade-release.py"), "--id", "arcade.lens",
                "--version", "0.1.0", "--channel", "stable", "--windows-installer", "inno",
                "--notes", "https://github.com/qa-p1/Arcade-lens/releases/tag/v0.1.0", str(dist),
            ]
            subprocess.run(command, check=True, capture_output=True)
            manifest = json.loads((dist / "arcade-release.json").read_text())
            self.assertEqual(manifest["schema"], 1)
            self.assertEqual(manifest["id"], "arcade.lens")
            self.assertEqual(manifest["version"], "0.1.0")
            self.assertEqual(manifest["channel"], "stable")
            self.assertEqual(manifest["linkProtocol"], [1])
            self.assertEqual(manifest["notes"], command[-2])
            assets = {a["os"]: a for a in manifest["assets"]}
            self.assertEqual(len(manifest["assets"]), 3)
            self.assertEqual((assets["linux"]["kind"], assets["linux"]["arch"]), ("appimage", "x64"))
            self.assertEqual((assets["macos"]["kind"], assets["macos"]["arch"]), ("dmg", "universal"))
            self.assertEqual((assets["windows"]["kind"], assets["windows"]["arch"]), ("inno", "x64"))
            self.assertEqual(assets["windows"]["silent"],
                             ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER"])
            for asset in assets.values():
                self.assertEqual(asset["sha256"], hashlib.sha256(files[asset["file"]]).hexdigest())
                self.assertEqual(asset["size"], len(files[asset["file"]]))
            expected_sums = "".join(f"{hashlib.sha256(files[name]).hexdigest()}  {name}\n"
                                    for name in sorted(files))
            self.assertEqual((dist / "SHA256SUMS.txt").read_text(), expected_sums)
            before = (dist / "arcade-release.json").read_bytes()
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual((dist / "arcade-release.json").read_bytes(), before)
            self.assertEqual((dist / "SHA256SUMS.txt").read_text(), expected_sums)
            command[command.index("stable")] = "nightly"
            command[command.index("0.1.0")] = "0.1.0-dev.fixture"
            subprocess.run(command, check=True, capture_output=True)
            nightly = json.loads((dist / "arcade-release.json").read_text())
            self.assertEqual((nightly["channel"], nightly["version"]), ("nightly", "0.1.0-dev.fixture"))


if __name__ == "__main__":
    unittest.main()
