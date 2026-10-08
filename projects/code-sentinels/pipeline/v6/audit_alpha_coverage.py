"""Small CPU-only alpha footprint audit; no asset edits or GPU work."""
from pathlib import Path
from collections import defaultdict
import json
import hashlib
import sys
import numpy as np
from PIL import Image
from derive_runtime_frames_v1 import layout, preserved_rust_raster_crop

PROJECT = Path(__file__).resolve().parents[2]
MANIFEST = PROJECT / 'Content/UI/v6/resource-manifest.json'


def sha(file):
    with file.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def main():
    manifest = json.loads(MANIFEST.read_text(encoding='utf-8-sig'))
    index_path = PROJECT / 'Content/Animations/v6/runtime-frames/index.json'
    indexed = json.loads(index_path.read_text(encoding='utf-8-sig'))['atlases']
    dirs = ['s', 'sw', 'w', 'nw', 'n', 'ne', 'e', 'se']
    cases = []
    for asset in ('wall', 'physical-wall'):
        for direction in (1, 3, 5, 7):
            cases.append((asset, 'idle', dirs[direction], 'first'))
    cases += [('door', 'closed', None, 'first'), ('door', 'open', None, 'first'), ('data-center', 'idle', None, 'first')]
    for asset in ('claude', 'deepseek'):
        for direction in ('s', 'ne'):
            cases.append((asset, 'walk', direction, 'middle'))
    for asset in ('heavy-impact', 'kinetic-hit', 'power-arc'):
        for frame in ('first', 'middle', 'last'):
            cases.append((asset, 'oneshot', None, frame))
    sources = {}
    results = []
    for asset, action, direction, selected in cases:
        section, item = next((k, manifest[k][asset]) for k in ('models', 'characters', 'effects') if asset in manifest.get(k, {}))
        category = 'buildings' if section == 'models' else section
        atlas = item.get('nativeAtlas', f'Content/Animations/v6/{category}/{asset}.png')
        meta = item.get('nativeMetadata', f'Content/Animations/v6/{category}/{asset}.json')
        doc = json.loads((PROJECT / meta).read_text(encoding='utf-8-sig'))
        direction = direction or doc.get('defaultDirection', 'n')
        desired = doc['clips'][action]
        clip = desired.get(direction, desired)
        start, end = clip['start'], clip['endExclusive']
        frame = {'first': start, 'middle': start + (end - start) // 2, 'last': end - 1}[selected]
        box = doc['boxes'][frame]
        tile, sample = layout(box, doc.get('preservePixelDensity') is True)
        if atlas in indexed:
            page = indexed[atlas]['frames'][frame]
            assert page['bbox'] == box and page['tileSize'] == tile and page['sampleSize'] == sample
            image_path = PROJECT / page['path']
            assert sha(image_path) == page['sha256']
            with Image.open(image_path) as image:
                pixels = np.asarray(image.convert('RGBA'))
            source_ref = page['path']
        else:
            if atlas not in sources:
                with Image.open(PROJECT / atlas) as image:
                    sources[atlas] = np.array(image.convert('RGBA'))
            pixels = preserved_rust_raster_crop(sources[atlas], box, tile, sample)
            source_ref = atlas
        sw, sh = sample
        alpha = pixels[:sh, :sw, 3]
        ys, xs = np.nonzero(alpha > 0)
        bounds = [int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1] if xs.size else None
        guard = [max(0, bounds[0] - 2), max(0, bounds[1] - 2), min(sw, bounds[2] + 2), min(sh, bounds[3] + 2)] if bounds else None
        fraction = ((guard[2] - guard[0]) * (guard[3] - guard[1]) / (sw * sh)) if guard else 0.
        results.append({'asset': asset, 'action': action, 'direction': direction, 'frame': frame,
                        'sampleSize': sample, 'tileSize': tile, 'alphaPositiveBboxExclusive': bounds,
                        'guardTwoPixelBboxExclusive': guard, 'guardAreaFraction': fraction,
                        'alphaPositivePixelFraction': int(xs.size) / (sw * sh), 'source': source_ref})
    groups = defaultdict(list)
    for result in results:
        groups[result['asset']].append(result)
    summary = {asset: {'samples': len(rows), 'guardAreaMin': min(r['guardAreaFraction'] for r in rows),
                       'guardAreaMax': max(r['guardAreaFraction'] for r in rows),
                       'nonzeroMin': min(r['alphaPositivePixelFraction'] for r in rows),
                       'nonzeroMax': max(r['alphaPositivePixelFraction'] for r in rows)} for asset, rows in groups.items()}
    report = {'scope': 'CPU-only representative raster alpha audit. Area is measured inside sampleSize, excluding existing512 GPU padding. Alpha>0 includes even alpha1; two-pixel guard is clamped to original sample. No image edits, no FPS estimate or GPU execution.',
              'manifestSha256': sha(MANIFEST), 'runtimeIndexSha256': sha(index_path), 'cases': results, 'summary': summary}
    out = PROJECT / 'pipeline/v6/alpha-coverage-audit-20260912.json'
    assert not out.exists()
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'report': str(out), 'cases': len(results), 'summary': summary}))


if __name__ == '__main__':
    main()
