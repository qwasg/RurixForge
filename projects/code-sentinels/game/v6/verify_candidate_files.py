"""Read-only candidate file verification; deliberately never launches native code."""
import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path


def read(file):
    return json.loads(file.read_text(encoding='utf-8-sig'))


def sha(file):
    with file.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    project = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, default=project / 'dist/CodeSentinels-V6-Windows')
    parser.add_argument('--web', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, default=project / 'game/v6/native-build-receipt.json')
    parser.add_argument('--output', type=Path, default=project / 'game/v6/candidate-file-verification.json')
    args = parser.parse_args()
    candidate, web = args.candidate.resolve(), args.web.resolve()
    marker = read(candidate / 'v6-candidate.json')
    receipt = read(args.receipt)
    issues, checked = [], []

    def expect(condition, message):
        if not condition:
            issues.append(message)

    def same(file, source):
        if not file.is_file() or not source.is_file():
            issues.append(f'Missing expected file: {file.name} / {source.name}')
            return
        actual, expected = sha(file), sha(source)
        expect(actual == expected, f'Content mismatch: {file.relative_to(candidate)}')
        checked.append({'file': file.relative_to(candidate).as_posix(), 'sha256': actual})

    expect(marker.get('version') == 6, 'Candidate version mismatch')
    expect(marker.get('candidate') is True, 'Input must remain a candidate until gameplay acceptance')
    expect(receipt.get('source', {}).get('unchangedDuringBuild') is True, 'Native source changed during build or lacks a stable-source receipt')
    for relative in marker.get('included', []):
        file = (candidate / relative).resolve()
        expect(file.is_relative_to(candidate), f'File outside candidate: {relative}')
        expect(file.is_file(), f'Missing manifest file: {relative}')
    same(candidate / 'bin/engine-host.exe', project / 'game/v6/runtime-bin/engine-host.exe')
    expect(sha(candidate / 'bin/engine-host.exe').upper() == receipt['artifact']['sha256'].upper(), 'Engine build receipt mismatch')
    expect(str(marker.get('engineSha256', '')).upper() == receipt['artifact']['sha256'].upper(), 'Candidate marker engine mismatch')
    source_web = {f.relative_to(web).as_posix(): f for f in web.rglob('*') if f.is_file()}
    candidate_web = {f.relative_to(candidate / 'Web').as_posix(): f for f in (candidate / 'Web').rglob('*') if f.is_file()}
    expect(set(source_web) == set(candidate_web), 'Stale or missing Web files')
    for relative, source in source_web.items():
        same(candidate / 'Web' / relative, source)
    for packaged, source in [('bridge.mjs', 'portable-bridge-v6.mjs'), ('multiplayer-v6.mjs', 'multiplayer-v6.mjs'),
                             ('v6/native-rpc.mjs', 'v6/native-rpc.mjs'), ('v6/session-controller.mjs', 'v6/session-controller.mjs')]:
        same(candidate / packaged, project / 'game' / source)
    native_manifest = project / 'Content/UI/v6/resource-manifest.json'
    same(candidate / 'Content/UI/v6/resource-manifest.json', native_manifest)
    same(candidate / 'Web/games/code-sentinels/ui-v6/resource-manifest.json', native_manifest)
    media = read(native_manifest)
    for relative in media.get('assetRoots', []):
        source_root = (project / relative).resolve()
        expect(source_root.is_relative_to((project / 'Content').resolve()), f'Asset root outside Content: {relative}')
        if not source_root.is_relative_to((project / 'Content').resolve()):
            continue
        for source in source_root.rglob('*'):
            if source.is_file():
                same(candidate / source.relative_to(project), source)
    characters = media.get('characters', {})
    expect(set(characters) == {'deepseek', 'gpt', 'claude', 'gemini', 'kimi', 'minimax', 'glm'}, 'Incomplete character roster')
    for name, character in characters.items():
        expect(character.get('ready') is True, f'{name} media not ready')
        for native, public in [('nativeAtlas', 'atlas'), ('nativeMetadata', 'metadata')]:
            relative = character[native]
            same(candidate / relative, project / relative)
            same(candidate / 'Web' / character[public].lstrip('/'), project / relative)
        meta = read(project / character['nativeMetadata'])
        expect(meta.get('frameCount') == 512 and len(meta.get('boxes', [])) == 512, f'{name} incomplete frame boxes')
        expect(meta.get('preservePixelDensity') is True, f'{name} density contract missing')
        expect(all(len(meta.get('clips', {}).get(action, {})) == 8 for action in ['idle', 'walk', 'attack', 'cast', 'hit', 'death']), f'{name} missing action direction')
    same(candidate / 'QA/media-verification.json', project / 'Content/UI/v6/qa/media-verification.json')
    same(candidate / 'Sources/references/v6/sources.json', project / 'references/v6/sources.json')
    report = {
        'generatedAtUtc': datetime.now(timezone.utc).isoformat(), 'version': 6,
        'scope': 'Candidate file presence, shipped mirror hashes and build/media provenance only; no executable invocation.',
        'fileVerificationPassed': not issues, 'issues': issues, 'manifestFiles': len(marker.get('included', [])),
        'hashComparisons': checked, 'candidate': str(candidate), 'webSource': str(web),
        'engineSha256': receipt['artifact']['sha256'],
        'buildReceipt': str(args.receipt.resolve()),
        'nativeExecution': 'Not assessed by this read-only file verifier; consult matching runtime acceptance.',
        'gameplayAcceptance': False, 'multiplayerAcceptance': False, 'performanceAcceptance': False,
    }
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'passed': not issues, 'issues': issues, 'hashComparisons': len(checked), 'report': str(output)}, ensure_ascii=False))
    if issues:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
