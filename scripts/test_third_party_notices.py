#!/usr/bin/env python3
"""Tests for scripts/third-party-notices.py: what it accepts in an AppImage, and how it reads the runtime."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("notices", Path(__file__).with_name("third-party-notices.py"))
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)

RUNTIME_HELP = """AppImage options:

  --appimage-help                 Print this help

License:
  This executable contains code from
  * runtime, licensed under the terms of
    https://github.com/AppImage/type2-runtime/blob/main/LICENSE
  * musl libc, licensed under the terms of
    https://git.musl-libc.org/cgit/musl/tree/COPYRIGHT
  * libfuse, licensed under the terms of
    https://github.com/libfuse/libfuse/blob/master/LGPL2.txt
  * squashfuse, licensed under the terms of
    https://github.com/vasi/squashfuse/blob/master/LICENSE
  * libzstd, licensed under the terms of
    https://github.com/facebook/zstd/blob/dev/LICENSE
  * zlib, licensed under the terms of
    https://zlib.net/zlib_license.html
Please see https://github.com/AppImage/type2-runtime
"""

CLEAN = {
    "AppRun": False,
    "dev.arcade.Lens.desktop": False,
    "dev.arcade.Lens.png": False,
    "usr/bin/arcade-lens": True,
    "usr/share/applications/dev.arcade.Lens.desktop": False,
    "usr/share/icons/hicolor/256x256/apps/dev.arcade.Lens.png": False,
}
KNOWN = list(notices.RUNTIME_COMPONENTS)
DOCS = {name: notices.normalized((notices.ROOT / name).read_text(encoding="utf-8")) for name in notices.SHIPPED_DOCS}


class AppImageCheck(unittest.TestCase):
    def test_the_runtime_report_names_every_component(self):
        self.assertEqual(notices.runtime_report(RUNTIME_HELP), ["runtime", "musl libc", "libfuse", "squashfuse", "libzstd", "zlib"])
        self.assertEqual(sorted(notices.runtime_report(RUNTIME_HELP)), sorted(notices.RUNTIME_COMPONENTS))

    def test_the_image_as_built_today_passes(self):
        self.assertEqual(notices.appimage_problems(CLEAN, KNOWN, DOCS), [])

    def test_a_bundled_library_fails(self):
        for rel, elf in (("usr/lib/libtesseract.so.5", True), ("usr/lib/libfoo.so.1", False), ("usr/lib64/libfoo", False), ("lib/libc.so.6", True)):
            problems = notices.appimage_problems({**CLEAN, rel: elf}, KNOWN, DOCS)
            self.assertEqual(len(problems), 1, rel)
            self.assertIn(rel, problems[0])

    def test_another_program_fails(self):
        problems = notices.appimage_problems({**CLEAN, "usr/bin/tesseract": True}, KNOWN, DOCS)
        self.assertEqual(len(problems), 1)
        self.assertIn("usr/bin/tesseract", problems[0])

    def test_the_program_must_be_there(self):
        files = {k: v for k, v in CLEAN.items() if k != "usr/bin/arcade-lens"}
        self.assertTrue(notices.appimage_problems(files, KNOWN, DOCS))

    def test_a_new_runtime_component_fails(self):
        problems = notices.appimage_problems(CLEAN, KNOWN + ["libnew"], DOCS)
        self.assertEqual(len(problems), 1)
        self.assertIn("libnew", problems[0])
        self.assertTrue(notices.appimage_problems(CLEAN, KNOWN[:-1], DOCS))


    def test_missing_or_stale_licences_and_notices_fail(self):
        gone = {k: v for k, v in DOCS.items() if k != "THIRD_PARTY_NOTICES.md"}
        problems = notices.appimage_problems(CLEAN, KNOWN, gone)
        self.assertEqual(len(problems), 1)
        self.assertIn("THIRD_PARTY_NOTICES.md", problems[0])
        stale = {**DOCS, "LICENSE-MIT": "an older text"}
        problems = notices.appimage_problems(CLEAN, KNOWN, stale)
        self.assertEqual(len(problems), 1)
        self.assertIn("differs", problems[0])


class Sources(unittest.TestCase):
    def test_vendored_link_is_the_pinned_tag(self):
        tag, commit = notices.vendored_link()
        self.assertEqual(tag, notices.pinned_link_tag())
        self.assertRegex(commit, r"^[0-9a-f]{40}$")

    def test_every_runtime_licence_text_exists(self):
        for name, _, _, text_file in notices.RUNTIME_COMPONENTS.values():
            self.assertTrue((notices.RUNTIME_LICENCES / text_file).read_text(encoding="utf-8").strip(), name)

    def test_normalized_ignores_line_endings_and_trailing_space(self):
        self.assertEqual(notices.normalized("\r\n a \r\n\r\nb  \r\n\r\n"), " a\n\nb")


if __name__ == "__main__":
    unittest.main()
