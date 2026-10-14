"""Tests for tools/arcade-release.py (run: python3 -m unittest discover -s tools -p 'test_*.py')."""

import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).parent
spec = importlib.util.spec_from_file_location("arcade_release", HERE / "arcade-release.py")
ar = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ar)


class ArcadeRelease(unittest.TestCase):
    def test_classifies_each_platforms_assets(self):
        c = lambda n, w="nsis": ar.classify(n, w)
        self.assertEqual(c("Arcade-Look_0.4.0_amd64.AppImage"), {"os": "linux", "arch": "x64", "kind": "appimage", "installArgs": ["--install", "--silent"]})
        self.assertEqual(c("Arcade-Look_0.4.0_x64-setup.exe")["silent"], ["/S"])
        self.assertEqual(c("ArcadeWheel-Setup.exe", "inno")["silent"][0], "/VERYSILENT")
        self.assertEqual(c("Arcade-Look_0.4.0_universal.dmg"), {"os": "macos", "arch": "universal", "kind": "dmg", "installArgs": ["--install", "--silent"]})
        self.assertEqual(c("Arcade-Clipboard-linux-x64.tar.gz")["kind"], "tarball")
        self.assertEqual(c("arcade-lens-aarch64.AppImage")["arch"], "arm64")
        self.assertIsNone(c("Arcade-Look_0.4.0_x64-setup.exe.sig"))
        self.assertIsNone(c("symbols.tar.gz"))
        self.assertEqual(c("arcade-lens-portable-x64.exe")["kind"], "portable")
        self.assertNotIn("silent", c("ArcadeWheel-x64.zip"))
        self.assertEqual(ar.classify("Lens.exe", "inno", True)["kind"], "portable")

    def test_portable_override_and_missing_asset(self):
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory)
            (assets / "lens.exe").write_bytes(b"exe")
            ar.main(["--id", "arcade.lens", "--version", "1", "--notes", "n", "--portable", "lens.exe", str(assets)])
            m = json.loads((assets / "arcade-release.json").read_text())
            self.assertEqual(m["assets"][0]["kind"], "portable")
            self.assertNotIn("installArgs", m["assets"][0])
            with self.assertRaises(SystemExit):
                ar.main(["--id", "arcade.lens", "--version", "1", "--notes", "n", "--portable", "missing.exe", str(assets)])

    def test_writes_manifest_and_checksums_that_verify(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "App_1.0.0_amd64.AppImage").write_bytes(b"appimage")
            (d / "App_1.0.0_x64-setup.exe").write_bytes(b"setup")
            (d / "App_1.0.0_x64-setup.exe.sig").write_bytes(b"sig")
            rc = ar.main(["--id", "arcade.look", "--version", "1.0.0", "--notes", "https://example.invalid/n", str(d)])
            self.assertEqual(rc, 0)
            m = json.loads((d / "arcade-release.json").read_text())
            self.assertEqual((m["schema"], m["id"], m["channel"], m["linkProtocol"]), (1, "arcade.look", "stable", [1]))
            self.assertEqual(len(m["assets"]), 2)
            for a in m["assets"]:
                self.assertEqual(a["sha256"], hashlib.sha256((d / a["file"]).read_bytes()).hexdigest())
            sums = (d / "SHA256SUMS.txt").read_text().splitlines()
            self.assertEqual(len(sums), 3)  # the signature is checksummed but not installable
            if sys.platform.startswith("linux"):
                subprocess.run(["sha256sum", "-c", "SHA256SUMS.txt"], cwd=d, check=True, capture_output=True)
            # Re-running replaces the outputs instead of listing them.
            ar.main(["--id", "arcade.look", "--version", "1.0.0", "--notes", "n", str(d)])
            self.assertEqual(len((d / "SHA256SUMS.txt").read_text().splitlines()), 3)

    def test_matches_the_schema(self):
        schema = json.loads((HERE.parent / "spec/arcade-release.schema.json").read_text())
        try:
            import jsonschema  # optional
        except ImportError:
            self.skipTest("jsonschema not installed")
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "x.dmg").write_bytes(b"x")
            ar.main(["--id", "arcade.box", "--version", "0.1.0", "--channel", "nightly", "--notes", "n", str(d)])
            jsonschema.validate(json.loads((d / "arcade-release.json").read_text()), schema)

    def test_rejects_bad_arguments(self):
        with tempfile.TemporaryDirectory() as d:
            for args in (["--id", "look"], ["--id", "arcade.look", "--version", "v1"]):
                full = args + (["--version", "1"] if "--version" not in args else []) + ["--notes", "n", d]
                with self.assertRaises(SystemExit):
                    ar.main(full)


if __name__ == "__main__":
    unittest.main()
