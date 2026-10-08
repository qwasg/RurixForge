"""Save fixed visual inputs from the ORIGINAL pressure snapshots; no native execution."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path


def sha(file):
    with file.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def frames(file):
    decoder = json.JSONDecoder()
    with file.open(encoding='utf-8-sig') as stream:
        buffer = stream.read(4 * 1024 * 1024).lstrip()
        assert buffer[0] == '['
        buffer = buffer[1:]
        while True:
            buffer = buffer.lstrip()
            if buffer.startswith(','):
                buffer = buffer[1:].lstrip()
            if buffer.startswith(']'):
                return
            try:
                value, end = decoder.raw_decode(buffer)
            except json.JSONDecodeError:
                more = stream.read(4 * 1024 * 1024)
                if not more:
                    raise
                buffer += more
                continue
            yield value
            buffer = buffer[end:]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    root, out = args.fixture.resolve(), args.out.resolve()
    assert not out.exists(), 'Preserve original fixed input corpus'
    base = json.loads((root / 'pressure-save.json').read_text(encoding='utf-8-sig'))['snapshot']
    selected = {i: frame for i, frame in enumerate(frames(root / 'render-frames.json')) if i in (0, 79, 119)}
    assert len(selected) == 3
    out.mkdir(parents=True)
    cases = []
    for index, frame in selected.items():
        snapshot = {**base, **frame}
        snapshot['winner'] = None
        snapshot['winReason'] = ''
        snapshot['playback'] = {'paused': True, 'speed': 1, 'currentTick': snapshot['tick'], 'totalTicks': 10000000}
        raw = json.dumps(snapshot, ensure_ascii=False, separators=(',', ':')).encode()
        compressed = gzip.compress(raw, compresslevel=1, mtime=0)
        file = out / f'pressure-frame-{index:03}.snapshot.json.gz'
        file.write_bytes(compressed)
        views = [(f'layer-{layer}', {'centerX': 63, 'centerY': 46, 'zoom': .65, 'layer': layer, 'cutaway': True, 'localPlayer': 1}) for layer in range(-2, 6)]
        if index == 79:
            views += [(f'detail-{layer}', {'centerX': 53.125, 'centerY': 36.375, 'zoom': 4, 'layer': layer, 'cutaway': True, 'localPlayer': 1}) for layer in (0, 5)]
            views += [(f'edge-{layer}', {'centerX': 42.013, 'centerY': 38.211, 'zoom': 1.8, 'layer': layer, 'cutaway': True, 'localPlayer': 1}) for layer in (0, 5)]
        for label, view in views:
            cases.append({'id': f'pressure-{index:03}-{label}', 'tick': snapshot['tick'], 'width': 1280, 'height': 720,
                          'view': view, 'snapshotPath': str(file), 'snapshotSha256': hashlib.sha256(raw).hexdigest(),
                          'compressedSnapshotSha256': hashlib.sha256(compressed).hexdigest(),
                          'expected': [{'kind': 'full-pressure-render-input', 'originalFrameIndex': index}]})
    manifest = {'schemaVersion': 1, 'kind': 'fixed-native-snapshot-inputs',
                'scope': 'Same original full workload snapshots with only explicit visual pause/winner-null flags. No normal gameplay, balance or FPS claim. Three original ticks times eight original views plus four detail/edge views.',
                'source': {name: {'path': str(root / name), 'sha256': sha(root / name)} for name in ('pressure-save.json', 'render-frames.json')},
                'cases': cases}
    (out / 'cases.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'manifest': str(out / 'cases.json'), 'cases': len(cases), 'ticks': [f['tick'] for f in selected.values()]}))


if __name__ == '__main__':
    main()
