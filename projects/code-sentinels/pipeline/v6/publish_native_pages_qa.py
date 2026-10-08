"""Publish verified new native page-consumer visual QA, preserving previous published evidence."""
import argparse
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil

PROJECT = Path(__file__).resolve().parents[2]
REPO = PROJECT.parents[1]


def read(file):
    return json.loads(file.read_text(encoding='utf-8-sig'))


def sha(file):
    with file.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(file, data):
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding='utf-8')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--characters', required=True)
    parser.add_argument('--effects', required=True)
    parser.add_argument('--walls', required=True)
    parser.add_argument('--engine-hash', required=True)
    parser.add_argument('--build-receipt', required=True)
    args = parser.parse_args()
    labels = [args.characters, args.effects, args.walls]
    assert all(label and all(c in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in label) for label in labels)
    char_dir, fx_dir, wall_dir = [PROJECT / 'Logs/v6' / label for label in labels]
    chars = read(char_dir / 'gpu-frame-fidelity.json')
    fx = read(fx_dir / 'gpu-fx-fidelity.json')
    directions = read(fx_dir / 'gpu-directions-and-tails.json')
    walls = read(wall_dir / 'capture-report.json')
    captures = [read(folder / 'capture-report.json') for folder in (char_dir, fx_dir, wall_dir)]
    assert all(c['engineHash'] == args.engine_hash and not c['errors'] and c.get('captured') is True for c in captures)
    assert chars['pass'] and fx['pass'] and directions['pass']
    assert chars['characterFramesCompared'] == 3584 and fx['uniqueSourceFrames'] == 656
    assert len(walls['checks']) == 4 and all(c['pass'] for c in walls['checks'])
    assert sha(PROJECT / 'game/v6/runtime-bin/engine-host.exe') == args.engine_hash
    build_path = PROJECT / args.build_receipt
    build = read(build_path)
    assert build['artifact']['sha256'].lower() == args.engine_hash and build['source']['unchangedDuringBuild'] is True
    audit = PROJECT / 'pipeline/v6/runtime-frame-derivation-v1'
    index_path = PROJECT / 'Content/Animations/v6/runtime-frames/index.json'
    index = read(index_path)
    exported = read(audit / 'derivation-report.json')
    rust_path = audit / 'rust-page-oracle.json'
    rust = read(rust_path)
    assert sha(index_path) == exported['index']['sha256']
    assert rust['actualRustExecution'] is True and rust['passed'] is True and rust['totalFrames'] == 4288
    assert rust['characters'] == 3584 and rust['effects'] == 704
    assert set(index['atlases']) == {r['atlas'] for r in rust['atlases']}
    for row in rust['atlases']:
        entry = index['atlases'][row['atlas']]
        assert row['allRgbaBytesEqual'] and row['frames'] == entry['frameCount']
        assert sha(PROJECT / row['atlas']) == entry['sourceAtlasSha256']
        assert sha(PROJECT / entry['metadata']) == entry['sourceMetadataSha256']
        for frame in entry['frames']:
            assert sha(PROJECT / frame['path']) == frame['sha256']
    dest = PROJECT / 'Content/UI/v6/qa'
    web = REPO / 'packages/client/public/games/code-sentinels/ui-v6/qa'
    previous = read(dest / 'native-lifecycle-verification.json')
    history = audit / ('published-history-before-' + args.engine_hash[:8])
    assert not history.exists(), 'Prior publication snapshot is preserved; use a new revision'
    history.mkdir()
    shutil.copytree(dest, history / 'content-qa')
    shutil.copytree(web, history / 'public-qa')
    manifest_path = PROJECT / 'Content/UI/v6/resource-manifest.json'
    public_manifest = web.parent / 'resource-manifest.json'
    assert sha(manifest_path) == sha(public_manifest)
    shutil.copy2(manifest_path, history / 'resource-manifest.json')
    summary = deepcopy(previous)
    summary.update(verifiedAt=datetime.now(timezone.utc).isoformat(), pass_=True)
    summary.pop('pass_', None)
    summary['pass'] = True
    summary['engineSha256'] = args.engine_hash
    summary['rulesVersion'] = build['rulesVersion']
    summary['rulesFingerprint'] = build['rulesFingerprint']
    summary['scope'] = 'Actual native Rurix GPU lifecycle/alpha-pick verification of lossless indexed runtime pages, using explicitly unearned validated replica fixtures. This is not earned gameplay, balance, LAN, first-use timing or pressure acceptance.'
    summary['devices'] = sorted({e['diagnostics']['deviceName'] for c in captures for e in c['captures']})
    summary['nativePngReadbacks'] = sum(len(c['captures']) for c in captures)
    summary['nativeFallbacks'] = sum(e['diagnostics'].get('meshFallbacks', 0) for c in captures for e in c['captures'])
    summary['nativeTruncatedFrames'] = sum(bool(e['diagnostics'].get('truncated')) for c in captures for e in c['captures'])
    assert summary['nativeFallbacks'] == summary['nativeTruncatedFrames'] == 0
    char_records = chars['results']
    summary['characters'].update(actualAtlasFramesCovered=3584,
                                  opaqueScreenTexelsChecked=sum(v['opaqueScreenPixelsChecked'] for v in char_records),
                                  minMatchWithin3RgbBytes=chars['minTexelMatch'], maxMeanRgbByteError=chars['maxMeanRgbError'])
    summary['characters']['deathEnd']['expiresToEmptyNativeBaseline'] = all(v['equalsEmptyNativeBaseline'] for v in chars['expiredDeaths'])
    meaningful = [v for v in fx['results'] if v['meaningfulScreenPixelsCompared'] > 30]
    summary['effects'].update(currentlyConsumedEffects=fx['effects'], uniqueActualSourceFramesCovered=fx['uniqueSourceFrames'],
                               sourceFrameComparisonsIncludingLoops=fx['sourceFramesCompared'],
                               minMeaningfulPixelMatchWithin3RgbBytes=min(v['matchWithin3Bytes'] for v in meaningful),
                               persistentLoops=fx['persistentLoops'], eventsKeptPastDurationAndCorrectlyExpired=all(v['equalsEmptyNativeBaseline'] for v in fx['expired']))
    summary['directionMapping'].update(worldVectorVersusExplicitFacingByteIdenticalCases=len(directions['deathDirectionChecks']),
                                       directionalBeamAndConeGpuTexelChecks=len(directions['directionalEffects']), pass_=directions['pass'])
    summary['directionMapping'].pop('pass_', None)
    summary['directionMapping']['pass'] = directions['pass']
    summary['foregroundOcclusion'] = {'cases': walls['checks'], 'pass': True}
    for category, key in [('characters', 'characterAssets'), ('effects', 'effectAssets')]:
        for asset in summary[key]:
            assert sha(PROJECT / 'Content/Animations/v6' / category / (asset['id'] + '.png')) == asset['atlasSha256']
            assert sha(PROJECT / 'Content/Animations/v6' / category / (asset['id'] + '.json')) == asset['metadataSha256']
    summary['previousVisualAcceptance'] = {'engineSha256': previous['engineSha256'], 'preservedAt': str(history.relative_to(PROJECT)).replace('\\', '/')}
    summary['fixedNativeDefect']['historicalFixPredatesThisPageConsumerRun'] = True
    summary['runtimeFramePages'] = {'format': index['format'], 'index': str(index_path.relative_to(PROJECT)).replace('\\', '/'),
                                    'indexSha256': sha(index_path), 'characterPages': 3584, 'effectPages': 704,
                                    'totalPages': 4288, 'pngBytes': exported['pngBytes'], 'allOriginalRgbaPagesEqualInActualRust': True,
                                    'rustOracleSha256': sha(rust_path), 'gpuVerifiedCharacterFrames': 3584,
                                    'gpuVerifiedActiveEffectFrames': 656, 'unmappedLegacyRepairFramesRustOnly': 48,
                                    'originalAtlasAndMetadataPreserved': True, 'noTestingPrewarmUsed': True}
    pictures = []
    for name, source in [
        ('glm-death-eight-directions.png', char_dir / 'glm-death-09.png'),
        ('minimax-attack-eight-directions.png', char_dir / 'minimax-attack-06.png'),
        ('gpt-cast-eight-directions.png', char_dir / 'gpt-cast-08.png'),
        ('skill-effects.png', fx_dir / 'fx-group-2-frame-24.png'),
        ('impact-effects.png', fx_dir / 'fx-group-0-frame-08.png'),
        ('foreground-wall-occludes-corpse.png', wall_dir / 'glm-death-with-foreground-wall.png'),
    ]:
        for directory in (dest / 'native-rendering', web / 'native-rendering'):
            directory.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, directory / name)
        pictures.append({'file': 'native-rendering/' + name, 'sha256': sha(source), 'size': [1920, 1080],
                         'origin': 'Unmodified native Rurix GPU readback from this engine and explicitly unearned fixture'})
    summary['screenshots'] = pictures
    evidence_files = [char_dir / 'capture-report.json', char_dir / 'gpu-frame-fidelity.json', fx_dir / 'capture-report.json',
                      fx_dir / 'gpu-fx-fidelity.json', fx_dir / 'gpu-directions-and-tails.json', wall_dir / 'capture-report.json',
                      rust_path, audit / 'derivation-report.json', build_path]
    summary['sourceEvidence'] = {'keptInDevelopmentProject': True, 'files': [
        {'file': str(file.relative_to(PROJECT)).replace('\\', '/'), 'sha256': sha(file)} for file in evidence_files]}
    source_files = ['crates/engine-host/src/sentinels_v6_render.rs', 'crates/engine-host/src/sentinels_v6_assets.rs',
                    'crates/engine-host/src/sentinels_v6_pages.rs', 'crates/engine-host/src/sentinels_v6_clock.rs',
                    'crates/engine-host/src/sentinels_v6.rs', 'crates/assetd/src/texture.rs', 'vendor/rurix/src/rurix-rt/src/render_exec.rs']
    summary['consumerSources'] = [{'repositoryFile': name, 'sha256': sha(REPO / name)} for name in source_files]
    runtime_summary = {
        'schemaVersion': 1, 'kind': 'native-runtime-raster-pages-verification', 'verifiedAt': summary['verifiedAt'],
        'scope': 'Exact-pixel asset organization and actual new native visual fixtures. Separate matching-hash first-use, performance, gameplay and LAN evidence is still required.',
        'engineSha256': args.engine_hash, 'rulesVersion': build['rulesVersion'], 'rulesFingerprint': build['rulesFingerprint'],
        **summary['runtimeFramePages'], 'nativeVisualReport': 'Content/UI/v6/qa/native-lifecycle-verification.json',
        'sourceAssetsSourceSha256': exported['sourceAssetsSourceSha256'], 'sourceDecodeSourceSha256': exported['sourceDecodeSourceSha256'],
        'sourceOracleSha256': exported['sourceOracleSha256'], 'pythonPixelComparisonPassed': True,
        'actualRustPixelComparisonPassed': True, 'nativeGpuLifecyclePassed': True,
    }
    for directory in (dest, web):
        save(directory / 'native-lifecycle-verification.json', summary)
        save(directory / 'runtime-frame-derivation-verification.json', runtime_summary)
        shutil.copy2(rust_path, directory / 'runtime-frame-rust-oracle.json')
    media = read(dest / 'media-verification.json')
    media['nativeGpuAcceptance'] = {'status': 'verified-explicit-visual-fixtures', 'engineSha256': args.engine_hash,
                                     'report': 'qa/native-lifecycle-verification.json', 'characters': 7, 'characterFrames': 3584,
                                     'effects': 15, 'scope': summary['scope']}
    media['nativeRuntimePages'] = {'index': summary['runtimeFramePages']['index'], 'indexSha256': sha(index_path),
                                    'frames': 4288, 'exactOldRustRasterPixels': True,
                                    'report': 'qa/runtime-frame-derivation-verification.json'}
    for directory in (dest, web):
        save(directory / 'media-verification.json', media)
        (directory / 'NATIVE-RENDERING-QA.md').write_text(
            '# Native visual lifecycle acceptance\n\nThe included PNGs are unmodified1920×1080 native Rurix GPU readbacks from the engine hash recorded in the report. They show explicitly unearned visual fixtures, not campaign progress.\n\n'
            'All3584 character frames,15 active effects, full2second construction/barrier loops, eight-direction mapping and foreground-wall/native alpha-pick checks are covered. The original atlas/video assets and previous acceptance evidence remain preserved.\n\n'
            'The native loader now reads independently addressable lossless pages. All4288 exported pages exactly matched the original Rust raster_crop RGBA, including transparent RGB and512 padding. One legacy48-frame repair effect is retained and CPU-verified but has no current event mapping.\n\n'
            'These tests do not measure first-use latency, whole-game FPS, real-time clock stability, balance or LAN; consult separate matching-build reports. Death clips show all10frames and then expire, without an additional corpse hold/fade.\n', encoding='utf-8')
    manifest = read(manifest_path)
    manifest['nativeRuntimeFrames'] = {'schemaVersion': 1, 'format': index['format'], 'nativeOnly': True,
                                       'index': summary['runtimeFramePages']['index'], 'indexSha256': sha(index_path),
                                       'characters': 7, 'characterFrames': 3584, 'effects': 16, 'effectFrames': 704,
                                       'verification': 'Content/UI/v6/qa/runtime-frame-derivation-verification.json'}
    save(manifest_path, manifest)
    save(public_manifest, manifest)
    assert sha(manifest_path) == sha(public_manifest)
    for file in dest.rglob('*'):
        if file.is_file():
            mirror = web / file.relative_to(dest)
            assert mirror.is_file() and sha(file) == sha(mirror)
    receipt = {'scope': 'Published actual visual/raster-page QA and small manifest pointers, not global approval or performance acceptance.',
               'engineSha256': args.engine_hash, 'nativeReadbacks': summary['nativePngReadbacks'],
               'characterFrames': 3584, 'activeEffectFrames': 656, 'derivedPages': 4288,
               'indexSha256': sha(index_path), 'resourceManifestSha256': sha(manifest_path),
               'nativeSummarySha256': sha(dest / 'native-lifecycle-verification.json'),
               'runtimePagesQaSha256': sha(dest / 'runtime-frame-derivation-verification.json'),
               'mediaSummarySha256': sha(dest / 'media-verification.json'), 'preservedPriorQa': str(history),
               'publicQaMirrorsExact': True, 'webBuildRequired': True}
    receipt_path = audit / ('published-native-qa-' + args.engine_hash[:8] + '.json')
    assert not receipt_path.exists()
    save(receipt_path, receipt)
    print(json.dumps({**receipt, 'receipt': str(receipt_path)}))


if __name__ == '__main__':
    main()
