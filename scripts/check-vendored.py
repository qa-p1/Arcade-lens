#!/usr/bin/env python3
"""Verify the checksum inventory for unchanged Arcade Link sources."""
import hashlib
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
entries = re.findall(r"^([0-9a-f]{64})  (\S+)$", (root / "VENDORED").read_text(), re.M)
if not entries:
    sys.exit("Vendored inventory contains no checksums")
failed = [name for expected, name in entries
          if not (root / name).is_file() or hashlib.sha256((root / name).read_bytes()).hexdigest() != expected]
if failed:
    sys.exit("Vendored checksum drift: " + ", ".join(failed))
print(f"Verified {len(entries)} vendored Arcade Link files")
