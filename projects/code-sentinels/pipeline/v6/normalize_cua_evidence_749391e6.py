"""Lossless-to-decoded-pixels format normalization of original CUA JPEG captures.

No image content, dimensions, crop, orientation or game state is changed.
Original mislabeled JPEG files and the original evidence report are preserved.
"""
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sys
from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT / 'game/v6'))
from release_gate import read_json, sha


def ref(file):
    return {'path': file.relative_to(PROJECT).as_posix(), 'sha256': sha(file)}


def main():
    original_report = PROJECT / 'qa/v6/client/native-ui-749391e6-20260911.json'
    original_hash = sha(original_report)
    original = read_json(original_report)
    outdir = PROJECT / 'qa/v6/client/png-evidence-749391e6-20260911'
    new_report = PROJECT / 'qa/v6/client/native-ui-749391e6-20260911-v2.json'
    assert not outdir.exists() and not new_report.exists(), 'Preserve prior derived evidence'
    outdir.mkdir()
    conversions = []
    for reference in original['evidence']:
        if not reference['path'].endswith('.png'):
            continue
        source = PROJECT / reference['path']
        assert sha(source) == reference['sha256']
        assert source.read_bytes()[:3] == b'\xff\xd8\xff'
        destination = outdir / source.name
        with Image.open(source) as image:
            assert image.format == 'JPEG'
            image.load()
            decoded, mode, size = image.tobytes(), image.mode, image.size
            image.save(destination, format='PNG', **({'icc_profile': image.info['icc_profile']} if 'icc_profile' in image.info else {}))
        assert destination.read_bytes()[:8] == b'\x89PNG\r\n\x1a\n'
        with Image.open(destination) as image:
            assert image.format == 'PNG'
            image.load()
            assert image.mode == mode and image.size == size and image.tobytes() == decoded
        assert sha(source) == reference['sha256'], 'Original capture was modified'
        conversions.append({
            'source': {**reference, 'actualMime': 'image/jpeg', 'originalExtension': '.png', 'originalBytesPreserved': True},
            'output': {**ref(destination), 'actualMime': 'image/png'},
            'width': size[0], 'height': size[1], 'mode': mode,
            'decodedPixelSha256': hashlib.sha256(decoded).hexdigest(), 'decodedPixelsEqual': True,
        })
    assert len(conversions) == 6
    manifest = outdir / 'conversion-manifest.json'
    manifest.write_text(json.dumps({
        'schemaVersion': 1, 'kind': 'capture-format-normalization', 'createdAtUtc': datetime.now(timezone.utc).isoformat(),
        'scope': 'The CUA screenshot tool supplied JPEG/JFIF bytes under .png filenames. These new PNGs preserve every decoded source pixel. Original compressed bytes/report remain unchanged; this corrects format labeling, not image content or recorded actions. No new GPU frame or browser action was generated.',
        'sourceUiReport': ref(original_report), 'conversions': conversions,
    }, ensure_ascii=False, indent=2), encoding='utf-8')
    revised = deepcopy(original)
    revised['evidenceRevision'] = 2
    revised['evidenceRevisionAtUtc'] = datetime.now(timezone.utc).isoformat()
    revised['sourceUiEvidence'] = ref(original_report)
    revised['screenshotFormatCorrection'] = {
        'originalActualMime': 'image/jpeg', 'originalMislabelledExtension': '.png',
        'primaryEvidenceActualMime': 'image/png', 'conversionManifest': ref(manifest),
        'decodedPixelsPreserved': True, 'newBrowserActions': 0, 'newNativeFrames': 0,
        'explanation': '原始CUA截图实际为JPEG字节，原文件和旧报告保留。新版六张PNG仅做格式编码，逐像素等于JPEG解码结果；不裁切、修饰、美化或新增测试结果。',
    }
    revised['evidence'] = [ref(original_report), *[r for r in original['evidence'] if not r['path'].endswith('.png')],
                           ref(manifest), *[r['output'] for r in conversions]]
    revised['observationNotes'].append('Screenshot format wording in revision1 is corrected: original .png-named files contain JPEG. Revision2 primary PNG evidence is verified against decoded original pixels; original captures and action/observer receipts are unchanged.')
    new_report.write_text(json.dumps(revised, ensure_ascii=False, indent=2), encoding='utf-8')
    assert sha(original_report) == original_hash
    print(json.dumps({'report': str(new_report), 'sha256': sha(new_report), 'conversionManifest': str(manifest),
                      'convertedImages': len(conversions), 'decodedPixelsEqual': True, 'originalFilesUnchanged': True,
                      'scope': 'Evidence format correction only; original UI-only result unchanged, no global approval.'}))


if __name__ == '__main__':
    main()
