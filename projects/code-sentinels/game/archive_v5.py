"""Archive the accepted V5 candidate after runtime and browser checks finish."""
import json
import hashlib
import pathlib
import shutil
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / 'dist/CodeSentinels-V5-Windows'
REPORT = ROOT / 'game/v5/final-acceptance.json'


def main():
    acceptance = json.loads(REPORT.read_text(encoding='utf8'))
    if acceptance.get('passed') is not True or not OUT.is_dir():
        raise RuntimeError('Final acceptance and the complete candidate are required before release')
    qa = OUT / 'QA'; qa.mkdir(exist_ok=True)
    reports = [
        'QA.md', 'final-acceptance.json', 'cpu-acceptance.json', 'multiplayer-tests.json',
        'browser-acceptance.json', 'media-inventory.json', 'portable-acceptance-final.json',
        'CPU-ACCEPTANCE.md', 'PORTABLE-ACCEPTANCE.md', 'client-tests.log',
        'ui-final-tests.json', 'ui-final-tests.log', 'ui-final-typecheck.log',
        'anim-index-tests.log', 'cpu-abi-before-fix.json', 'package-content-review.json',
        'ui-interaction-tests.json', 'ui-interaction-tests.log', 'ui-interaction-typecheck.log',
        'web-release-acceptance.json', 'client-release-build.log',
        'ui-interaction-tests-final.json', 'ui-interaction-tests-final.log',
        'ui-interaction-typecheck-final.log', 'client-release-build-final.log',
        'runtime-dependencies-acceptance.json',
    ]
    reports += [path.name for path in (ROOT / 'game/v5').glob('cpu-*-results.json')]
    for name in reports:
        source = ROOT / 'game/v5' / name
        if source.is_file(): shutil.copy2(source, qa / name)
    preview_fix = ROOT / 'game/v5/hotfix-preview'
    if (preview_fix / 'acceptance.json').is_file():
        hotfix_qa = qa / 'hotfix-preview'; hotfix_qa.mkdir(exist_ok=True)
        for name in ['acceptance.json', 'tests.json', 'tests.log', 'typecheck.log', 'build.log', 'browser.json', 'baseline-tests.log']:
            if (preview_fix / name).is_file(): shutil.copy2(preview_fix / name, hotfix_qa / name)
    gpu_report = json.loads((ROOT / 'game/v5/portable-acceptance-final.json').read_text(encoding='utf8'))
    for mode in gpu_report['modes']:
        relative = pathlib.Path(mode['report']).parent
        destination = qa / relative
        destination.mkdir(parents=True, exist_ok=True)
        for source in (ROOT / 'game/v5' / relative).iterdir():
            if source.is_file() and source.suffix in ['.json', '.png']:
                shutil.copy2(source, destination / source.name)
    (OUT / 'QA.md').write_text('# V5 验收记录\n\n完整说明、原始数值与两模式真实画面见 [QA/QA.md](QA/QA.md)。\n', encoding='utf8')
    readme = (ROOT / 'game/V5-README.md').read_text(encoding='utf8').replace('(v5/', '(QA/')
    (OUT / 'README.md').write_text(readme, encoding='utf8')
    archive = OUT.with_suffix('.zip')
    pending = archive.with_suffix('.zip.pending')
    # Include exactly the current client build; old cached chunks can remain in
    # the running candidate without being distributed in the release archive.
    web_source = ROOT.parents[1] / 'packages/client/dist'
    current_web_files = {file.relative_to(web_source).as_posix() for file in web_source.rglob('*') if file.is_file()}
    if 'index.html' not in current_web_files:
        raise RuntimeError('The current production client build is required')
    native_files = {pathlib.Path(name).name for name in acceptance['filesSha256'] if name.startswith('.forge/cache/rxdll/')}
    native_files |= {pathlib.Path(name).with_suffix('.native.json').name for name in list(native_files)}
    if len(native_files) != 2 or any(not (OUT / '.forge/cache/rxdll' / name).is_file() for name in native_files):
        raise RuntimeError('The accepted native DLL and its manifest are required')
    with zipfile.ZipFile(pending, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as package:
        for file in OUT.rglob('*'):
            if not file.is_file(): continue
            relative = file.relative_to(OUT)
            if relative.parts[0] == 'Logs' or relative.parts[:2] in [('.forge', 'save'), ('.forge', 'multiplayer')]: continue
            if relative.parts[0] == 'Web' and pathlib.Path(*relative.parts[1:]).as_posix() not in current_web_files: continue
            if relative.parts[:3] == ('.forge', 'cache', 'rxdll') and relative.name not in native_files: continue
            package.write(file, OUT.name + '/' + relative.as_posix())
    with zipfile.ZipFile(pending) as package:
        broken = package.testzip()
        if broken:
            raise RuntimeError(f'Archive CRC failed: {broken}')
        archived_files = package.namelist()
        for name in current_web_files:
            member = OUT.name + '/Web/' + name
            if package.read(member) != (web_source / name).read_bytes():
                raise RuntimeError(f'Archived client mismatch: {name}')
    pending.replace(archive)
    with archive.open('rb') as stream:
        zip_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
    report_path = ROOT / 'game/v5/portable-build.json'
    report = json.loads(report_path.read_text(encoding='utf8'))
    report.update({'candidate': False, 'acceptanceStatus': 'passed', 'zip': str(archive), 'zipBytes': archive.stat().st_size,
                   'testsRun': True, 'finalAcceptance': str(REPORT), 'zipSha256': zip_sha,
                   'archiveFiles': len(archived_files), 'archiveCrcPassed': True})
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
    verification = {'passed': True, 'crcPassed': True, 'webFilesCompared': len(current_web_files),
                    'files': len(archived_files), 'sha256': zip_sha, 'zip': str(archive), 'bytes': archive.stat().st_size}
    (ROOT / 'game/v5/zip-verification.json').write_text(json.dumps(verification, ensure_ascii=False, indent=2), encoding='utf8')
    production_path = ROOT / 'game/v5/production-status.json'
    production = json.loads(production_path.read_text(encoding='utf8'))
    production.update({'status': 'complete', 'pending': [], 'v5Released': True,
                       'releaseArchive': str(archive), 'archiveVerification': 'game/v5/zip-verification.json'})
    production_path.write_text(json.dumps(production, ensure_ascii=False, indent=2), encoding='utf8')
    print(json.dumps({'zip': str(archive), 'bytes': archive.stat().st_size, 'acceptance': 'passed'}, ensure_ascii=False))


if __name__ == '__main__': main()
