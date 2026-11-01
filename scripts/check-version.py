#!/usr/bin/env python3
"""Read the single workspace version (also supports Python 3.10 on Jammy)."""
from pathlib import Path
import re
source = (Path(__file__).resolve().parents[1] / 'Cargo.toml').read_text()
section = source.split('[workspace.package]', 1)[1].split('\n[', 1)[0]
match = re.search(r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"$', section, re.M)
assert match, 'workspace.package.version is missing'
print(match[1])
