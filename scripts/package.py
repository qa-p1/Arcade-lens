#!/usr/bin/env python3
"""Build this OS's installers into dist/ from the one workspace version.

    scripts/package [--label LABEL]

LABEL names the files and is the version the binary reports (default: the
version). Linux: the AppImage. macOS: the
universal dmg. Windows: the Inno Setup installer and the portable exe (Tools
ignores the portable one). The macOS bundle and Windows installer always carry
the plain version.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)


def run(*args):
    print('+', ' '.join(str(a) for a in args), flush=True)
    subprocess.run([str(a) for a in args], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--label')
    args = parser.parse_args()
    version = subprocess.check_output([sys.executable, 'scripts/check-version.py'], text=True).strip()
    label = args.label or version
    # The binary reports this version (link.rs VERSION, Windows file info), so it
    # matches the manifest Tools compares against, nightly suffix included.
    os.environ['ARCADE_RELEASE_VERSION'] = label
    target = Path(os.environ.get('CARGO_TARGET_DIR', 'target')).resolve()
    dist = ROOT / 'dist'
    dist.mkdir(exist_ok=True)
    if sys.platform == 'darwin':
        arches = ('aarch64-apple-darwin', 'x86_64-apple-darwin')
        for arch in arches:
            run('cargo', 'build', '--release', '--locked', '-p', 'arcade-lens', '--target', arch)
        universal = target / 'universal'
        universal.mkdir(exist_ok=True)
        run('lipo', '-create', '-output', universal / 'arcade-lens', *(target / arch / 'release/arcade-lens' for arch in arches))
        run('packaging/macos/build-dmg.sh', universal / 'arcade-lens', version, dist / f'ArcadeLens-{label}-macos-universal.dmg')
    elif sys.platform == 'win32':
        run('cargo', 'build', '--release', '--locked', '-p', 'arcade-lens')
        exe = target / 'release/arcade-lens.exe'
        iscc = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Inno Setup 6/ISCC.exe'
        if not iscc.is_file():
            iscc = shutil.which('ISCC') or sys.exit('Inno Setup 6 (ISCC.exe) is required.')
        run(iscc, f'/DAppVersion={label}', f'/DSourceExe={exe}', f'/DOutputDir={dist}', 'packaging\\windows\\arcade-lens.iss')
        shutil.copy2(exe, dist / f'ArcadeLens-{label}-windows-x64-portable.exe')
    else:
        run('cargo', 'build', '--release', '--locked', '-p', 'arcade-lens')
        run('packaging/linux/build-appimage.sh', target / 'release/arcade-lens', dist / f'ArcadeLens-{label}-linux-x86_64.AppImage')


if __name__ == '__main__':
    main()
