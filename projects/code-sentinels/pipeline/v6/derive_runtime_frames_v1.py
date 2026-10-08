"""Derive lossless addressable native raster pages from the APPROVED final atlases.

This does not use frames-native, rerun two-thirds sampling, change clips, create
new motion, or touch original media/receipts. Page bytes implement the preserved
old Rust raster_crop contract, including all transparent RGB and zero padding.
The native consumer independently compares these pages with the old Rust oracle.
"""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import sys
import time

import numpy as np
from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
AUDIT = PROJECT / 'pipeline/v6/runtime-frame-derivation-v1'
OUTPUT = PROJECT / 'Content/Animations/v6/runtime-frames'
INDEX = OUTPUT / 'index.json'
FORMAT = 'native-raster-pages-png-v1'


def sha(file):
    with Path(file).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def rel(file):
    file = Path(file).resolve()
    assert file.is_relative_to(PROJECT)
    return file.relative_to(PROJECT).as_posix()


def read(file):
    return json.loads(Path(file).read_text(encoding='utf-8-sig'))


def write_new(file, value):
    assert not file.exists(), f'Existing receipt preserved: {file}'
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf-8')


def emit(value):
    print(json.dumps(value, ensure_ascii=True), flush=True)


def layout(box, preserve):
    x, y, width, height = box
    assert min(x, y) >= 0 and 0 < width <= 4096 and 0 < height <= 4096
    if preserve:
        assert width <= 512 and height <= 512, 'Do not downsample a dense frame'
        sample = [width, height]
    else:
        sample = [256, 256]
    tile = 512 if max(sample) > 256 else 256
    return tile, sample


def preserved_rust_raster_crop(source, box, tile, sample):
    """Integer indices exactly match the sealed original function, no PNG resampling."""
    x, y, width, height = box
    sw, sh = sample
    assert x + width <= source.shape[1] and y + height <= source.shape[0]
    rows = y + np.arange(sh, dtype=np.int64) * height // sh
    columns = x + np.arange(sw, dtype=np.int64) * width // sw
    output = np.zeros((tile, tile, 4), dtype=np.uint8)
    output[:sh, :sw] = source[rows[:, None], columns[None, :], :]
    return output


def main():
    started = time.perf_counter()
    assert AUDIT.is_dir() and (AUDIT / 'source-oracle/sentinels_v6_assets.rs').is_file()
    assert not OUTPUT.exists() and not INDEX.exists(), 'Previous/partial runtime output preserved; choose another revision'
    old_source = AUDIT / 'source-oracle/sentinels_v6_assets.rs'
    old_texture = AUDIT / 'source-oracle/texture.rs'
    text = old_source.read_text(encoding='utf-8-sig')
    function = text[text.index('fn raster_crop('):text.index('pub fn direction(', text.index('fn raster_crop('))]
    (AUDIT / 'source-oracle/raster_crop-function.txt').write_text(function, encoding='utf-8')
    descriptors = []
    originals = {}
    for category in ('characters', 'effects'):
        folder = PROJECT / 'Content/Animations/v6' / category
        metadata_files = sorted(folder.glob('*.json'))
        assert len(metadata_files) == (7 if category == 'characters' else 16)
        for file in metadata_files:
            doc = read(file)
            atlas = file.with_suffix('.png')
            assert atlas.is_file()
            boxes = doc.get('boxes', doc.get('frames'))
            assert isinstance(boxes, list) and boxes
            assert all(isinstance(box, list) and len(box) == 4 and all(type(v) is int for v in box) for box in boxes)
            if category == 'characters':
                assert len(boxes) == 512 and doc['preservePixelDensity'] is True
                assert doc['samplingContract'] == 'uniform-source-density-v1'
                assert all(len(doc['clips'][action]) == 8 for action in ('idle', 'walk', 'attack', 'cast', 'hit', 'death'))
            record = {'category': category, 'id': file.stem, 'metadata': rel(file), 'atlas': rel(atlas),
                      'metadataSha256': sha(file), 'atlasSha256': sha(atlas), 'atlasBytes': atlas.stat().st_size,
                      'metadataBytes': file.stat().st_size, 'boxes': boxes,
                      'preservePixelDensity': doc.get('preservePixelDensity') is True,
                      'metadataWidth': doc.get('width'), 'metadataHeight': doc.get('height')}
            descriptors.append(record)
            for source in (file, atlas):
                originals[rel(source)] = sha(source)
            snapshot = AUDIT / 'source-oracle/metadata' / category / file.name
            assert not snapshot.exists()
            snapshot.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(file, snapshot)
    assert sum(len(d['boxes']) for d in descriptors if d['category'] == 'characters') == 3584
    assert sum(len(d['boxes']) for d in descriptors if d['category'] == 'effects') == 704
    write_new(AUDIT / 'layout-oracle.json', {
        'schemaVersion': 1, 'scope': 'Sealed original Rust source and approved final atlas layouts before runtime derivation.',
        'originalAssetsSourceSha256': sha(old_source), 'originalDecodeSourceSha256': sha(old_texture),
        'originalRasterFunctionSha256': hashlib.sha256(function.encode()).hexdigest(),
        'equation': 'dst[(row*tileSize+col)*4+c] = src[((bboxY+floor(row*bboxH/sampleH))*atlasWidth+bboxX+floor(col*bboxW/sampleW))*4+c]; all unfilled page bytes zero',
        'rgbaSemantics': 'Straight RGBA8, all four channels preserved even when alpha is zero; no mask compositing or premultiplication.',
        'assets': descriptors, 'originalFileSha256': originals,
    })
    OUTPUT.mkdir(parents=True)
    atlases = {}
    page_hashes = []
    total_png_bytes = 0
    for asset in descriptors:
        atlas_path = PROJECT / asset['atlas']
        with Image.open(atlas_path) as decoded:
            assert decoded.format == 'PNG'
            width, height = decoded.size
            # The original decoder normalizes to RGBA8 too. No sizing/filtering is performed here.
            rgba = decoded if decoded.mode == 'RGBA' else decoded.convert('RGBA')
            rgba.load()
            source = np.array(rgba, dtype=np.uint8)
        entry = {'metadata': asset['metadata'], 'sourceAtlasSha256': asset['atlasSha256'],
                 'sourceMetadataSha256': asset['metadataSha256'], 'sourceAtlasBytes': asset['atlasBytes'],
                 'sourceMetadataBytes': asset['metadataBytes'], 'width': width, 'height': height,
                 'frameCount': len(asset['boxes']), 'frames': []}
        destination = OUTPUT / asset['category'] / asset['id']
        destination.mkdir(parents=True)
        for index, box in enumerate(asset['boxes']):
            tile, sample = layout(box, asset['preservePixelDensity'])
            page = preserved_rust_raster_crop(source, box, tile, sample)
            expected = page.tobytes(order='C')
            image_path = destination / f'{index:04}.png'
            Image.fromarray(page).save(image_path, format='PNG', compress_level=1, optimize=False)
            with Image.open(image_path) as actual:
                assert actual.format == 'PNG' and actual.mode == 'RGBA' and actual.size == (tile, tile)
                actual.load()
                assert actual.tobytes() == expected, f'Pixel mismatch: {asset["id"]}/{index}'
            record = {'index': index, 'bbox': box, 'tileSize': tile, 'sampleSize': sample,
                      'path': rel(image_path), 'sha256': sha(image_path),
                      'rgbaSha256': hashlib.sha256(expected).hexdigest()}
            entry['frames'].append(record)
            page_hashes.append({'atlas': asset['atlas'], **record})
            total_png_bytes += image_path.stat().st_size
        atlases[asset['atlas']] = entry
        emit({'asset': asset['id'], 'category': asset['category'], 'pages': len(entry['frames']),
              'cumulativePages': len(page_hashes), 'elapsedSeconds': round(time.perf_counter() - started, 3)})
        del source
    assert len(page_hashes) == 4288
    for name, expected in originals.items():
        assert sha(PROJECT / name) == expected, f'Approved original changed during derivation: {name}'
    index = {'schemaVersion': 1, 'format': FORMAT, 'atlases': atlases,
             'provenance': {'method': 'Exact old native raster_crop pages derived from approved final atlas pixels; no new source sampling or animation.',
                            'sourceOracle': 'pipeline/v6/runtime-frame-derivation-v1/layout-oracle.json',
                            'sourceOracleSha256': sha(AUDIT / 'layout-oracle.json'),
                            'characterFrames': 3584, 'effectFrames': 704,
                            'pythonDecodedPngComparedToPreservedIntegerIndexOracle': True,
                            'independentRustOracle': 'Pending native test execution; not claimed by this export receipt.'}}
    write_new(INDEX, index)
    write_new(AUDIT / 'frame-oracle-digests.json', {'schemaVersion': 1, 'format': FORMAT, 'pages': page_hashes})
    report = {'schemaVersion': 1, 'kind': 'native-runtime-frame-derivation', 'createdAtUtc': datetime.now(timezone.utc).isoformat(),
              'scope': 'Offline exact-pixel runtime organization only. Python oracle/PNG codec equality verified; independent actual Rust oracle and new native GPU first-use/lifecycle/pressure acceptance remain separate.',
              'index': {'path': rel(INDEX), 'sha256': sha(INDEX)}, 'sourceOracleSha256': sha(AUDIT / 'layout-oracle.json'),
              'sourceAssetsSourceSha256': sha(old_source), 'sourceDecodeSourceSha256': sha(old_texture),
              'characters': 7, 'characterFrames': 3584, 'effects': 16, 'effectFrames': 704, 'totalFrames': 4288,
              'pageFormats': sorted({r['tileSize'] for r in page_hashes}),
              'fullPageRgbaBytes': sum(r['tileSize'] ** 2 * 4 for r in page_hashes), 'pngBytes': total_png_bytes,
              'pythonPixelComparisonPassed': True, 'transparentRgbAndPaddingPreserved': True,
              'originalAtlasAndMetadataUnchanged': True, 'sourceVideosOrAttemptReceiptsModified': False,
              'rustOracle': {'status': 'pending'}, 'nativeGpuValidation': {'status': 'pending'},
              'elapsedSeconds': time.perf_counter() - started}
    write_new(AUDIT / 'derivation-report.json', report)
    emit(report)


if __name__ == '__main__':
    main()
