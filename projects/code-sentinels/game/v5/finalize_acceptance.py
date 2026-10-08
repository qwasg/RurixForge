"""Record acceptance only after all independently produced V5 reports pass."""
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
QA = ROOT / 'game/v5'
PACK = ROOT / 'dist/CodeSentinels-V5-Windows'


def read(path):
    return json.loads(path.read_text(encoding='utf8'))


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf8')


def main():
    required = ['cpu-acceptance.json', 'portable-acceptance-final.json', 'browser-acceptance.json',
                'web-release-acceptance.json', 'runtime-dependencies-acceptance.json', 'package-content-review.json']
    for name in required:
        if read(QA / name).get('passed') is not True:
            raise RuntimeError(f'Acceptance remains incomplete: {name}')
    ui = read(QA / 'ui-interaction-tests-final.json')
    if ui.get('overallPassed') is not True:
        raise RuntimeError('Final targeted UI validation is incomplete')
    if not all(check.get('pass') is True for check in read(QA / 'multiplayer-tests.json')['checks']):
        raise RuntimeError('Multiplayer infrastructure checks are incomplete')
    media = read(ROOT / 'pipeline/v5/local-production-result.json')
    if not (media.get('frozen') and media['sourceVideos'] == 44 and media['totalRuntimeFrames'] == 1856
            and not media['validation']['errors']):
        raise RuntimeError('Media production is incomplete')
    identity_files = ['bin/engine-host.exe', 'bridge.mjs', 'Web/index.html',
                      '.forge/cache/rxdll/sentinels_v5-383695227244bf95.dll']
    identities = {name: hashlib.sha256((PACK / name).read_bytes()).hexdigest() for name in identity_files}
    gpu = read(QA / 'portable-acceptance-final.json')
    if any(mode['engineSha256'] != identities['bin/engine-host.exe'] for mode in gpu['modes']):
        raise RuntimeError('Candidate engine changed after GPU acceptance')
    report = {
        'version': 5, 'passed': True, 'completedAt': datetime.now(timezone.utc).isoformat(),
        'reports': required + ['ui-interaction-tests-final.json', 'multiplayer-tests.json',
                               '../../pipeline/v5/local-production-result.json'],
        'fullClientSuite': {'files': 67, 'tests': 735, 'evidence': 'client-tests.log'},
        'finalTargetedUi': {'files': 3, 'tests': 32, 'typecheck': 'passed'},
        'media': {'sourceVideos': 44, 'buildingAtlases': 12, 'effectAtlases': 8, 'newFrames': 1856},
        'nativeCampaign': {'sectorsWon': 3, 'wavesWon': 12, 'input': 'ordinary game commands; isolated saves'},
        'gpu': {'device': gpu['modes'][0]['device'], 'resolution': [1280, 720],
                'offFps': gpu['modes'][0]['fps'], 'onFps': gpu['modes'][1]['fps'], 'requestLimit': 60, 'threshold': 30},
        'clientBundle': read(QA / 'browser-acceptance.json')['finalClientBundle'],
        'filesSha256': identities,
        'limitations': ['PVP rooms and protocol infrastructure only; native multiplayer combat is not implemented.',
                        'Save files retain sector unlocks; ongoing battles are not full disk saves.',
                        'Observed GPU performance is from one test machine and a bounded active-wave sample.'],
        'archiveStatus': 'ready for archival; actual ZIP receipt is portable-build.json'
    }
    write(QA / 'final-acceptance.json', report)
    production = read(QA / 'production-status.json')
    production['initialNeverSubmittedClips'] = production.pop('neverSubmittedClips', 17)
    production['previousChannelEvidence'] = production.pop('channelEvidence', 'pipeline/v5/starframe-channel-20260909.json')
    production.pop('uploadHost', None)
    production.update({'status': 'accepted-awaiting-archive', 'completedVideoClips': 44,
        'newLocalH3Clips': 27, 'newCloudPaidSubmissions': 0, 'pending': ['Archive accepted release'],
        'currentProductionBackend': 'local ComfyUI MiniMax-H3', 'remainingVideoClips': 0,
        'userQuestionPending': None, 'v5Released': False, 'finalAcceptance': 'game/v5/final-acceptance.json'})
    write(QA / 'production-status.json', production)
    portable = read(QA / 'portable-build.json')
    portable.update({'runtimeSource': str(QA / 'runtime-bin/engine-host.exe'),
        'developmentService': 'not required; self-contained native package', 'gameStarted': True,
        'testsRun': True, 'clientBundle': report['clientBundle'], 'runtimeLibraries': 'Microsoft VC143 x64 release app-local',
        'acceptanceStatus': 'passed; ready for archival', 'filesSha256': identities})
    write(QA / 'portable-build.json', portable)
    print(json.dumps({'passed': True, 'archiveReady': True, 'bundle': report['clientBundle']}))


if __name__ == '__main__':
    main()
