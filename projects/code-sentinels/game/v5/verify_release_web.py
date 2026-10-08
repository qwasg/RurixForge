"""Verify that every file in the final client build is shipped and served."""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT.parents[1] / 'packages/client/dist'
PACK = ROOT / 'dist/CodeSentinels-V5-Windows'


def verify(base_url):
    def check(source):
        relative = source.relative_to(SOURCE)
        packaged = PACK / 'Web' / relative
        data = source.read_bytes()
        if not packaged.is_file() or packaged.read_bytes() != data:
            raise RuntimeError(f'Package mismatch: {relative}')
        request = urllib.request.Request(base_url + '/' + urllib.parse.quote(relative.as_posix()), method='HEAD')
        with urllib.request.urlopen(request, timeout=10) as response:
            if response.status != 200 or int(response.headers['Content-Length']) != len(data):
                raise RuntimeError(f'HTTP mismatch: {relative}')
        return {'path': relative.as_posix(), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        files = list(pool.map(check, [path for path in SOURCE.rglob('*') if path.is_file()]))
    report = {'passed': True, 'origin': base_url, 'checkedFiles': len(files),
              'method': 'Exact current production file equality plus real HTTP HEAD/Content-Length', 'files': files}
    (ROOT / 'game/v5/web-release-acceptance.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
    print(json.dumps({'passed': True, 'files': len(files)}))


if __name__ == '__main__':
    import sys
    verify(sys.argv[1])
