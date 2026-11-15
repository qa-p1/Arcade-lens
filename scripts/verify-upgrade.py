#!/usr/bin/env python3
"""Upgrade a real released Lens with a user's data to this package (PLAN exit gate "Upgrade test").

    python3 <link>/tools/e2e.py run -- python3 scripts/verify-upgrade.py PREVIOUS.AppImage CANDIDATE.AppImage

Only inside Link's isolated runner (private HOME, XDG folders, D-Bus and Xvfb). Installs PREVIOUS from a download
folder, gives it the files a user of that version has (settings with non-default values, a saved chain, learned
action usage), runs it, then installs CANDIDATE over it and checks: every setting is kept and the new version reads
it, the chain and usage files are unchanged, there is exactly one launcher and one login item (fleet rule "One
launcher per app"), and an uninstall without --remove-data keeps the data. CI runs it from the previous stable
release with the candidate it built; the release gates run it on the published files.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']

SETTINGS = '''notifications = false
activation_shortcut = "Ctrl+Alt+Shift+K"
ocr_languages = ["en", "de"]
primary_action_count = 3
preferred_actions = ["web.search"]
disabled_actions = ["web.whois"]
home_currency = "EUR"

[privacy]
history_enabled = false

[link]
enabled = false
'''
CHAINS = [{'id': 'upgrade-check', 'name': 'Search the web', 'steps': [{'step': 'run', 'action': 'web.search'}]}]
USAGE = {'entries': {'text': {'web.search': {'weight': 3.0, 'at': 1763164800}}}}


def until(probe, message, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(.2)
    raise AssertionError(message)


def main():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    if os.environ.get('ARCADE_E2E_INNER') != '1' or root == Path('/'):
        raise SystemExit('Run through Link tools/e2e.py run --')
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_RUNTIME_DIR', 'ARCADE_HOME'):
        assert Path(os.environ[key]).resolve().is_relative_to(root), key
    previous_image, candidate_image = (Path(arg).resolve(strict=True) for arg in sys.argv[1:3])

    # Real downloads outside any checkout: Link refuses to install a file below a .git tree.
    downloads = root / 'Downloads'
    downloads.mkdir(exist_ok=True)
    previous, candidate = downloads / 'previous.AppImage', downloads / 'candidate.AppImage'
    for source, target in ((previous_image, previous), (candidate_image, candidate)):
        shutil.copy2(source, target)
        assert hashlib.sha256(source.read_bytes()).digest() == hashlib.sha256(target.read_bytes()).digest()
    env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN='1', ARCADE_NO_INSTALL_PROMPT='1')
    for key in ('ARCADE_LENS_HOME', 'APPIMAGE', 'APPDIR', 'ARCADE_DEV_BUILD', 'ARCADE_UNPACKED'):
        env.pop(key, None)
    config = Path(env['XDG_CONFIG_HOME']) / 'arcade/lens'
    data = Path(env['XDG_DATA_HOME']) / 'arcade/lens'
    log = (root / 'upgrade.log').open('w')

    def command(binary, *args, timeout=60):
        result = subprocess.run([str(binary), *args], env=env, text=True, capture_output=True, timeout=timeout)
        assert result.returncode == 0, (binary.name, args, result.returncode, result.stdout, result.stderr)
        return result.stdout

    def status(binary):
        result = subprocess.run([str(binary), '--status'], env=env, text=True, capture_output=True, timeout=20)
        try:
            return json.loads(result.stdout) if result.returncode == 0 else {}
        except json.JSONDecodeError:
            return {}

    def run_resident(binary, version):
        # The launcher may hand over to Lens's unpacked copy and exit 0 (U1); the resident is found by --status.
        launcher = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            state = until(lambda: (s := status(binary)).get('running') and s, f'{version} did not start in the background')
            assert state['version'] == version, state
            return state
        finally:
            command(binary, '--quit')
            until(lambda: not status(binary).get('running'), f'{version} did not quit')
            assert launcher.wait(timeout=20) == 0, launcher.returncode

    old_receipt = json.loads(command(previous, '--install', '--silent', '--no-launch', '--keep-download'))
    old_version = old_receipt['version']
    # The same version is allowed: before the release bump CI's build still carries the previous release's number.
    old_installed = Path(old_receipt['path'])
    assert old_installed.is_relative_to(root), old_installed

    # The files a user of the previous version has, where it keeps them (0.4 and later: arcade/lens).
    config.mkdir(parents=True, exist_ok=True)
    data.mkdir(parents=True, exist_ok=True)
    (config / 'settings.toml').write_text(SETTINGS, encoding='utf-8')
    (config / 'chains.json').write_text(json.dumps(CHAINS, indent=2) + '\n', encoding='utf-8')
    (data / 'usage.json').write_text(json.dumps(USAGE) + '\n', encoding='utf-8')
    state = run_resident(old_installed, old_version)
    assert state['link']['enabled'] is False, f'{old_version} did not read the seeded settings: {state}'
    kept = {path: path.read_bytes() for path in (config / 'settings.toml', config / 'chains.json', data / 'usage.json')}
    before = tomllib.loads(kept[config / 'settings.toml'].decode())

    new_receipt = json.loads(command(candidate, '--install', '--silent', '--no-launch', '--keep-download'))
    assert new_receipt['version'] == VERSION, new_receipt
    installed = Path(new_receipt['path'])
    assert installed.is_relative_to(root), installed
    state = run_resident(installed, VERSION)
    assert state['link']['enabled'] is False, f'{VERSION} did not read the kept settings: {state}'

    # Settings may be written again with new defaults; every value the user had stays.
    after = tomllib.loads((config / 'settings.toml').read_text(encoding='utf-8'))

    def same(old, new, where=''):
        for key, value in old.items():
            assert key in new, f'setting {where}{key} was dropped'
            if isinstance(value, dict):
                same(value, new[key], f'{where}{key}.')
            else:
                assert new[key] == value, f'setting {where}{key}: {value!r} became {new[key]!r}'
    same(before, after)
    for path in (config / 'chains.json', data / 'usage.json'):
        assert path.read_bytes() == kept[path], f'{path.name} changed in the upgrade'

    # One launcher and one login item, both the new installation's (fleet rule "One launcher per app").
    for key in ('desktopEntry', 'autostart'):
        entry = Path(new_receipt['integration'][key])
        assert entry.is_file() and entry.is_relative_to(root), (key, entry)
        assert str(installed) in entry.read_text(encoding='utf-8'), (key, entry)
        old_entry = Path(old_receipt['integration'][key])
        assert old_entry == entry or not old_entry.exists(), f'the old {key} {old_entry} is still there'
        entries = [p for p in entry.parent.glob('*.desktop') if 'arcade.lens' in p.read_text(encoding='utf-8', errors='replace')
                   or 'arcade-lens' in p.name.lower() or 'arcadelens' in p.name.lower()]
        assert entries == [entry], f'{key}: expected only {entry.name}, found {[p.name for p in entries]}'

    assert json.loads(command(installed, '--uninstall', '--silent'))['uninstalled']
    assert not installed.exists()
    for path in (config / 'chains.json', data / 'usage.json'):
        assert path.read_bytes() == kept[path], f'uninstall without --remove-data removed or changed {path.name}'
    assert (config / 'settings.toml').is_file(), 'uninstall without --remove-data removed the settings'
    print(f'Real AppImage upgrade {old_version} -> {VERSION}: settings read and kept, chain and usage unchanged, '
          'one launcher and one login item, uninstall kept the data.')


if __name__ == '__main__':
    main()
