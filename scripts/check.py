#!/usr/bin/env python3
"""The checks CI's `test` job runs on every OS: docs, vendored files, script
tests, formatting, clippy with no warnings, and the workspace tests in a
temporary profile. Packaging and native checks run in CI's per-OS package
jobs (see docs/dev/testing.md)."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)


def run(*args):
    print('+', ' '.join(args), flush=True)
    subprocess.run(args, check=True)


def main():
    run(sys.executable, 'scripts/check-version.py')
    run(sys.executable, 'scripts/check-docs.py')
    run(sys.executable, 'scripts/check-vendored.py')
    run(sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_*.py')
    run('cargo', 'fmt', '--all', '--check')
    run('cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
    run(sys.executable, 'scripts/isolate.py', 'cargo', 'test', '--workspace', '--locked')


if __name__ == '__main__':
    main()
