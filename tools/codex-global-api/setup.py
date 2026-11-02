"""Publish local API configuration using Forge's existing Codex home, without copying OAuth credentials."""
import argparse
import csv
import ctypes
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import urllib.request

PRIVATE = Path.home() / '.config' / 'rurixforge'
CLIENT = PRIVATE / 'codex-api.json'


def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def save_private(value):
    PRIVATE.mkdir(parents=True, exist_ok=True)
    if os.name == 'nt':
        who = subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], text=True)
        sid = next(csv.reader([who.strip()]))[1]
        r = subprocess.run(['icacls', str(PRIVATE), '/inheritance:r', '/grant:r',
                            f'*{sid}:(OI)(CI)F', '*S-1-5-18:(OI)(CI)F'], capture_output=True)
        if r.returncode:
            raise RuntimeError('Cannot restrict the private configuration directory ACL')
    else:
        PRIVATE.chmod(0o700)
    tmp = CLIENT.with_suffix('.json.tmp')
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    if os.name != 'nt':
        tmp.chmod(0o600)
    os.replace(tmp, CLIENT)


def configure(rotate=False):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open('http://127.0.0.1:8103/api/forge/codex/status', timeout=30) as r:
        status = json.load(r)
    if status.get('authSource') != 'chatgpt' or not status.get('account', {}).get('authMode'):
        raise RuntimeError('RurixForge must have an existing authenticated ChatGPT backend')
    models = [m for m in status.get('models', []) if not m.get('hidden')]
    if not models:
        raise RuntimeError('RurixForge returned no available models')
    codex_home = Path(status['config']['codexHome']).resolve(strict=True)
    codex_bin = Path(status['command']).resolve(strict=True)
    if not (codex_home / 'auth.json').is_file():
        raise RuntimeError('The existing backend does not use auth.json credential storage')
    existing = load(CLIENT) if CLIENT.exists() else {}
    key = existing.get('apiKey') if existing.get('kind') == 'codex-local-api' and not rotate else None
    key = key or ('sk-rf-local-' + secrets.token_urlsafe(32))
    default = next((m['id'] for m in models if m.get('isDefault')), models[0]['id'])
    config = {'kind': 'codex-local-api', 'baseUrl': 'http://127.0.0.1:8111/v1',
              'apiKey': key, 'model': 'codex/' + default,
              'models': ['codex/' + m['id'] for m in models],
              'codexHome': str(codex_home), 'codexBin': str(codex_bin)}
    if 'requestTimeoutSeconds' in existing:
        timeout = existing['requestTimeoutSeconds']
        if not isinstance(timeout, int) or isinstance(timeout, bool) or not 1 <= timeout <= 3600:
            raise RuntimeError('requestTimeoutSeconds must be an integer from 1 to 3600')
        config['requestTimeoutSeconds'] = timeout
    save_private(config)
    if os.name == 'nt':
        import winreg
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, 'Environment') as env:
            for name, value in {'RURIXFORGE_API_KEY': key, 'RURIXFORGE_BASE_URL': config['baseUrl'],
                                'RURIXFORGE_MODEL': config['model']}.items():
                winreg.SetValueEx(env, name, 0, winreg.REG_SZ, value)
        result = ctypes.c_size_t()
        ctypes.windll.user32.SendMessageTimeoutW(0xFFFF, 0x001A, 0, 'Environment', 2, 5000, ctypes.byref(result))
    print(json.dumps({'configured': True, 'baseUrl': config['baseUrl'], 'models': config['models'],
                      'clientConfig': str(CLIENT), 'auth': 'existing Forge Codex home'}, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rotate-key', action='store_true', help='Generate a replacement key; restart the gateway afterward')
    args = parser.parse_args()
    try:
        configure(args.rotate_key)
    except (OSError, ValueError, KeyError, RuntimeError):
        print('Setup failed. Check Forge status, Codex paths, and private configuration permissions.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
