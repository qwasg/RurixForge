"""Read-only review of actual evidence; writes an explicitly NONFINAL candidate wrapper."""
from pathlib import Path
from datetime import datetime, timezone
import json
import re
import struct
import sys

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT / 'game/v6'))
import release_gate as gate
import portable_release as package


def ref(file):
    return {'path': file.relative_to(PROJECT).as_posix(), 'sha256': gate.sha(file)}


def unique(refs):
    result, seen = [], set()
    for item in refs:
        file = Path(item['path'])
        file = (file if file.is_absolute() else PROJECT / file).resolve()
        value = ref(file)
        assert value['sha256'] == item['sha256'].lower()
        if value['path'] not in seen:
            seen.add(value['path'])
            result.append(value)
    return result


def main():
    target = gate.read_json(PROJECT / 'dist/final-candidate-749391e6-20260911/CodeSentinels-V6-Windows/v6-candidate.json')['target']
    paths = {
        'ui': PROJECT / 'qa/v6/client/native-ui-749391e6-20260911-v2.json',
        'stream': PROJECT / 'qa/v6/client/native-stream-749391e6-20260911.json',
        'opening': PROJECT / 'game/v6/ui-final-paid-opening-749391e6-20260911.json',
        'tests': PROJECT / 'game/v6/final-contracts-749391e6-20260911-a/contract-execution.json',
        'cold': PROJECT / 'game/v6/cold-start-runs/final-749391e6-20260911/cold-start-acceptance.json',
    }
    inputs = {name: gate.read_json(file) for name, file in paths.items()}
    hashes = {file.relative_to(PROJECT).as_posix(): gate.sha(file) for file in paths.values()}
    frozen = [PROJECT / 'game/build_portable_v6.py'] + [PROJECT / 'game/v6' / name for name in (
        'portable_release.py', 'release_gate.py', 'strategy-plan.json', 'README.md', 'RELEASE-ACCEPTANCE-SCHEMA.md')]
    frozen_hashes = {file.relative_to(PROJECT).as_posix(): gate.sha(file) for file in frozen}
    ui, stream, opening, tests, cold = (inputs[key] for key in ('ui', 'stream', 'opening', 'tests', 'cold'))
    for name in ('ui', 'stream', 'tests', 'cold'):
        gate.identity(inputs[name], target, name, payload=name != 'tests')
    assert ui['actualNativeExecution'] is True and ui['errors'] == []
    assert gate.UI_ACTIONS <= set(ui['completedActions'])
    assert stream['passed'] is True and stream['rgbaFrames'] == ui['rgbaFrames'] == 30 and stream['errors'] == []
    assert all(s['truncated'] is False and s['meshFallbacks'] is None and s['meshFallbacksObserved'] is False for s in stream['statusObservations'])
    assert len(ui['browserOrders']) == 10 and sum(r['result']['accepted'] is True for r in ui['browserOrders']) == 9
    skill, replay = ui['offlineSkill'], ui['saveLoadReplay']
    assert (skill['batteryBefore'], skill['batteryAfter'], skill['spent'], skill['covered']) == (120, 35, 85, False)
    assert (replay['savedTick'], replay['rewindTick'], replay['rewindBattery'], replay['replayFinalBattery']) == (23345, 22745, 120, 35)
    assert [(r['collection'], r['count'], r['owner'], r['equal']) for r in replay['ownerStateComparisons']] == [
        ('players', 1, 1, True), ('buildings', 6, 1, True), ('rooms', 3, 1, True), ('units', 3, 1, True), ('links', 7, 1, True)]
    assert replay['wholeAuthorityStateCompared'] is False
    assert (tests['passedTests'], tests['failedTests'], tests['unexecutedTests']) == (138, 0, 84)
    assert sum(r.get('passed', 0) for r in tests['records']) == 138
    store = gate.EvidenceStore(PROJECT)
    log_lines = store.file(tests['evidence'][0], 'contract-log').read_text(encoding='utf-8-sig').splitlines()
    for record in tests['records']:
        if record['status'] == 'executed':
            assert re.search(r'test result: ok\. ' + str(record['passed']) + r' passed; 0 failed;', log_lines[record['resultLogLine'] - 1])
        elif record['status'] == 'executed-on-one-original-path-retry':
            log = store.file(record['retry']['evidence'], 'retry-log').read_text(encoding='utf-8-sig')
            assert re.search(r'test result: ok\. ' + str(record['passed']) + r' passed; 0 failed;', log)
        else:
            assert record['status'] == 'os-blocked-unexecuted' and record['unexecutedTests'] == 84 and record['error'] == 4551
    boundary = next(i for i, row in enumerate(opening) if row['event'] == 'paid opening verified')
    paid, checkpoint = opening[:boundary], opening[boundary]
    player = checkpoint['player']
    assert len(paid) == 13 and all(r['event'] == 'order' and r['receipt']['accepted'] is True for r in paid)
    assert player['credits'] >= 500 and player['owner'] == 1 and player['production'] == 12 and player['power'] == 150
    commands = [r['command'] for r in paid]
    count = lambda predicate: sum(predicate(c) for c in commands)
    built = {
        'extractor': count(lambda c: c['op'] == 'build' and c.get('kind') == 'extractor'),
        'power': count(lambda c: c['op'] == 'build' and c.get('kind') == 'wind-power'),
        'dataCenter': count(lambda c: c['op'] == 'room' and c.get('kind') == 'data-center'),
        'gpu': count(lambda c: c['op'] == 'install-gpu'),
        'lab': count(lambda c: c['op'] == 'room' and c.get('kind') == 'research-lab'),
        'starterTurrets': count(lambda c: c['op'] == 'deploy' and c['kind'] in ('vscode', 'pycharm')),
        'powerLines': count(lambda c: c['op'] == 'wire' and c.get('kind') == 'power'),
        'computeLines': count(lambda c: c['op'] == 'wire' and c.get('kind') == 'compute'),
    }
    assert built == {'extractor': 1, 'power': 1, 'dataCenter': 1, 'gpu': 1, 'lab': 1, 'starterTurrets': 2, 'powerLines': 1, 'computeLines': 3}
    assert 'credits: 2000.' in (PROJECT / 'native-v6/src/world.rs').read_text(encoding='utf-8-sig')
    ui_refs = unique([ref(paths['ui']), *ui['evidence'], ref(paths['cold']), *cold['nativeFrameEvidence'],
                      ref(paths['cold'].parent / 'artifact-verification.json'), ref(paths['cold'].parent / 'shutdown-process-verification.json')])
    images = [r for r in ui_refs if r['path'].endswith('.png')]
    assert len(images) == 8
    image_metadata = []
    for item in images:
        data = (PROJECT / item['path']).read_bytes()
        assert data[:8] == b'\x89PNG\r\n\x1a\n'
        width, height = struct.unpack_from('>II', data, 16)
        assert width >= 1280 and height >= 720
        image_metadata.append({**item, 'width': width, 'height': height})
    candidate = {
        'schemaVersion': 2, 'kind': 'native-functional-acceptance', **target,
        'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'finalEligible': False,
        'status': 'candidate-awaiting-signing-and-execution-of-84-native-library-tests',
        'statusDetail': '功能候选汇总：等待签名方案后实际执行仍被 Windows 4551 阻止的84项原生库测试。138项已执行通过，0项断言失败。后续另建最终汇总，不覆盖本记录。',
        'scope': 'Read-only integration of current-rule native receipts, paid opening, actual CUA evidence, independently counted RGBA and isolated cold-start artifacts. This review starts no new gameplay and grants no global approval. Rule fixtures are distinct from earned gameplay, balance, performance and45-minute LAN.',
        'nativeTests': {
            'actualNativeExecution': True, 'passedTests': 138, 'failedTests': 0, 'unexecutedTests': 84,
            'ignoredComponentBenchmarks': tests['ignoredComponentBenchmarks'],
            'coverage': sorted(gate.FUNCTION_TAGS - {'ground-and-air-logistics'}),
            'coverageMeaning': 'Executed integration-contract coverage by topic, not a claim that related unit cases are complete. All84 blocked library cases remain required.',
            'pendingCoverage': ['ground-and-air-logistics'],
            'partialCoverage': {'ground-and-air-logistics': 'Executed cases cover ground depot capacity, field resupply and aircraft runway/fuel constraints. Dedicated cargo, routing and unloading library cases are among the84 unexecuted cases; the full tag is not claimed.'},
            'executedTargets': [{'target': r['target'], 'passed': r['passed'], 'status': r['status']} for r in tests['records'] if 'passed' in r],
            'blockedTargets': [r for r in tests['records'] if r['status'] == 'os-blocked-unexecuted'],
            'evidence': unique([ref(paths['tests']), *tests['evidence']]),
        },
        'ordinaryOpening': {
            'actualNativeExecution': True, 'ordinaryPaidCommands': True, 'injectedResources': False,
            'startingCredits': 2000,
            'startingCreditsBasis': 'Standard compiled Game::new initialization in world.rs and normal solo from CUA lobby; this preparation array does not itself contain an initial-tick snapshot.',
            'creditsRemaining': player['credits'], 'acceptedOrders': len(paid), 'verifiedTick': checkpoint['tick'], 'built': built,
            'observedPower': player['power'], 'observedLoad': player['demand'], 'observedComputeProduction': player['production'],
            'observedOreDelivered': player['totals']['ore-delivered'],
            'scope': 'The13 accepted paid setup orders before the checkpoint. Later research/upper-floor preparation and9 successful browser orders are counted separately.',
            'evidence': [ref(paths['opening'])],
        },
        'nativeUi': {
            'actualNativeExecution': True, 'rgbaFrames': 30, 'errors': [], 'completedActions': ui['completedActions'],
            'acceptedBrowserOrders': 9, 'rejectedBrowserOrders': 1,
            'rejectedOrderScope': 'First stairs click used stale viewport coordinates and was correctly rejected; retained as an operator input mistake and excluded from successful order count.',
            'nativeFrameCountingSource': ref(paths['stream']),
            'offlineSkill': {k: skill[k] for k in ('unit', 'kind', 'batteryBefore', 'batteryAfter', 'spent', 'covered', 'tick')},
            'saveLoadReplay': replay, 'evidence': ui_refs,
            'coldStartFrameCountKeptSeparate': cold['nativeRgbaFrames'],
            'scope': 'Ten recorded CUA action categories plus30 separately observed local RGBA frames. Cold-start PNGs support native display only, not browser actions or these30 frames. Screenshot FPS is not a performance result; stream meshFallbacks is unobserved, not zero.',
        },
        'coldStartScope': {'finalEligible': cold['finalEligible'], 'actualNativeExecution': True,
                           'nativeRgbaFrames': cold['nativeRgbaFrames'], 'normalShutdown': cold['normalShutdown'],
                           'completedFlows': cold['completedFlows'], 'evidence': ref(paths['cold'])},
        'review': {
            'reviewerScope': 'Verified reference hashes and actual test result lines; independently inspected all6 new CUA PNGs and the2 cold-start PNGs previously viewed during the actual run.',
            'screenshotObservations': {
                'upper-room-network': 'Upper floor selected; rack1/2, power300/170, compute599/600 visible.',
                'offline-glm-skill': 'Offline GLM35/300 after the85-cost skill.',
                'loaded-battle': 'Running loaded battle06:43; not relabelled as exact saved-tick equality.',
                'replay-upper-floor': 'Replay06:29/8x and isolated upper floor with real building modules.',
                'replay-rewind-cache': 'Rewind06:19 shows GLM120/300 and ready skill.',
                'replay-final-state': 'Endpoint06:29 shows GLM35/300 and cooldown.',
                'cold-start-pngs': 'Both own starting map views preserve the original raw RGBA; these are viewport pixels, not browser operations.',
            },
            'explicitPngReferences': image_metadata,
            'screenshotFormatCorrection': ui['screenshotFormatCorrection'],
            'limits': 'This review did not repeat the live owner-state comparison or execute the84 blocked cases. Owner1 equality is recorded current CUA/native evidence, not whole hidden-authority equality.',
        },
    }
    output = PROJECT / 'game/v6/functionality-candidate-749391e6-20260911.json'
    assert not output.exists(), 'Preserve prior candidate wrapper'
    output.write_text(json.dumps(candidate, ensure_ascii=False, indent=2), encoding='utf-8')
    package.check_data_privacy(output)
    for part in ('nativeTests', 'ordinaryOpening', 'nativeUi'):
        gate.EvidenceStore(PROJECT).supporting(candidate[part]['evidence'], 'candidate-' + part)
    try:
        gate.validate_functionality(candidate, target, gate.EvidenceStore(PROJECT))
    except gate.ReleaseError as error:
        rejection = str(error)
    else:
        raise AssertionError('Candidate was incorrectly accepted as final')
    assert all(gate.sha(PROJECT / name) == value for name, value in hashes.items())
    assert all(gate.sha(PROJECT / name) == value for name, value in frozen_hashes.items())
    audit = {
        'scope': 'Independent evidence review and explicitly nonfinal wrapper; no source, original evidence, final approval or ZIP changes.',
        'candidateSummary': ref(output), 'candidateGateRejection': rejection,
        'explicitPngCount': 8, 'allReferencesExistAndMatchSha256': True,
        'inputReceiptsUnchanged': hashes, 'frozenPackagingSourcesUnchanged': frozen_hashes,
        'actualPassedTests': 138, 'assertionFailures': 0, 'unexecutedLibraryTests': 84,
        'openingAcceptedOrders': 13, 'openingCreditsRemaining': player['credits'],
        'uiAcceptedOrders': 9, 'uiRejectedOrders': 1, 'uiNativeFrames': 30, 'coldStartFramesSeparate': 305,
        'reviewHarnessNote': 'First inline attempt stopped before writing because world.rs was read using default GBK. After using explicit UTF-8, PNG signature validation independently discovered all6 original CUA files were JPEG bytes despite their extensions. Original files/report remain preserved; authorized format-normalization output preserves every decoded pixel. This wrapper uses the corrected v2 evidence; no game tests were rerun or fabricated.',
    }
    audit_path = PROJECT / 'pipeline/v6/functionality-candidate-review-749391e6-20260911.json'
    assert not audit_path.exists()
    audit_path.write_text(json.dumps(audit, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'summary': str(output), 'sha256': gate.sha(output), 'review': str(audit_path), 'passed': 138, 'unexecuted': 84, 'explicitPngs': 8, 'gateRejection': rejection}))


if __name__ == '__main__':
    main()
