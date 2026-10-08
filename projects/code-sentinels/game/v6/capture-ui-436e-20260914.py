"""Collect this actual GUI run, immutable native receipts and JPEG-to-PNG evidence."""
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json
import re

from PIL import Image

PROJECT = Path(__file__).resolve().parents[2]
PACK = PROJECT / 'dist/final-candidate-6828c5b7-20260914/CodeSentinels-V6-Windows'
RUN = PROJECT / 'qa/v6/client/436e-20260914'
TESTS = PROJECT / 'game/v6/final-contracts-6828c5b7-20260913-a'
COLD = PROJECT / 'game/v6/cold-start-runs/final-436e6e0e-20260914'
UI_OUTPUT = RUN / 'native-ui-acceptance.json'
FUNCTION_OUTPUT = PROJECT / 'game/v6/functionality-final-436e6e0e-20260914.json'


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def ref(path):
    path = path.resolve()
    assert path.is_relative_to(PROJECT.resolve())
    return {'path': path.relative_to(PROJECT).as_posix(), 'sha256': sha(path)}


def verified(reference):
    path = Path(reference['path'])
    path = path if path.is_absolute() else PROJECT / path
    assert sha(path) == reference['sha256'].lower(), path
    return ref(path)


def write_new(path, value):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def own(state, collection):
    return sorted((v for v in state[collection] if v.get('owner') == 1), key=lambda v: v.get('id', v.get('owner')))


def glm(state):
    unit = next(v for v in state['units'] if v['id'] == 425)
    assert unit['kind'] == 'glm' and unit['owner'] == 1
    return unit


def native_command(command):
    command = json.loads(json.dumps(command))
    if command['op'] == 'room':
        command.setdefault('branch', None)
    if command['op'] == 'wire':
        command.setdefault('unitEndpoints', [])
    return command


assert not UI_OUTPUT.exists() and not FUNCTION_OUTPUT.exists(), 'Preserve previous acceptance documents'
target = read(PACK / 'v6-candidate.json')['target']
assert target['engineSha256'] == '436e6e0ee01b021f69f2a177a8deb0364932cd4909f89602da51d243c6ce5dff'
assert target['rulesFingerprint'] == '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'
assert target['payloadSha256'] == '1c9d74fa66a58973371ea25d5379aa9a8234bc924a7acdb318a1d1272b1763c5'
stream = read(RUN / 'native-stream.json')
assert stream['actualNativeExecution'] and stream['passed'] and stream['rgbaFrames'] == 30 and stream['errors'] == []
assert all(stream[key] == value for key, value in target.items())
cleanup = read(RUN / 'gui-process-cleanup.json')
assert cleanup['normalApplicationShutdown'] and cleanup['activeOwnedProcesses'] == []
assert cleanup['nativePid'] == stream['nativePid'] == 45820
assert cleanup['bridgePid'] == stream['bridgePid'] == 46828

save_path = PACK / '.forge/save/v6/4ecc9963-0cf8-4b53-a4c6-734b17a055a8.json'
saved = read(save_path)
stored = saved['save']['snapshot']
for key in ('rulesVersion', 'rulesFingerprint', 'engineSha256'):
    assert saved[key] == target[key]
assert saved['save']['rulesFingerprint'] == target['rulesFingerprint']
assert stored['seed'] == 6026 and stored['tick'] == 18065
states = {name: read(RUN / (name + '.json')) for name in (
    'skill-offline-state', 'after-compute-wire-state', 'final-saved-state', 'gui-loaded-state',
    'replay-before-skill', 'replay-ended-state', 'replay-unexpected-rewind-state')}
offline = states['skill-offline-state']['snapshot']
connected = states['after-compute-wire-state']['snapshot']
before = states['replay-before-skill']['snapshot']
end = states['replay-ended-state']['snapshot']
loaded = states['gui-loaded-state']['snapshot']
assert offline['tick'] == 15671 and glm(offline)['lastCastTick'] == 15668
assert glm(offline)['battery'] == 35 and glm(offline)['covered'] is False and glm(offline)['wired'] is False
assert connected['tick'] == 16380 and abs(glm(connected)['battery'] - 173.2) < 1e-6
assert glm(connected)['covered'] is True and glm(connected)['wired'] is False
assert before['tick'] == 17465 and glm(before)['lastCastTick'] == 15668
assert before['playback']['paused'] and before['playback']['speed'] == 8
assert end['tick'] == stored['tick'] and end['playback']['paused'] and end['playback']['speed'] == 8
assert glm(end)['lastCastTick'] == 18062 and glm(end)['attackCount'] == 0
assert glm(end)['battery'] == 300 and glm(end)['covered'] is True and glm(end)['wired'] is False
assert loaded['tick'] == 18075 and states['gui-loaded-state']['session']['lastSequence'] == 25
assert glm(loaded)['lastCastTick'] == 18062 and glm(loaded)['battery'] == 300
comparisons = []
for collection, count in [('players', 1), ('buildings', 6), ('rooms', 3), ('units', 3), ('links', 7)]:
    assert own(end, collection) == own(stored, collection)
    assert own(states['final-saved-state']['snapshot'], collection) == own(stored, collection)
    assert len(own(end, collection)) == count
    comparisons.append({'collection': collection, 'owner': 1, 'count': count, 'equal': True})
assert states['replay-unexpected-rewind-state'] == states['replay-before-skill']

orders = [row for row in saved['save']['orders'] if row['order']['owner'] == 1]
assert [row['order']['sequence'] for row in orders] == list(range(1, 26))
assert all(row['receipt']['accepted'] and row['receipt']['sequence'] == row['order']['sequence'] for row in orders)
assert saved['save']['sequences'][0] == 25
browser_orders = orders[15:]
assert len(browser_orders) == 10
commands = {row['order']['sequence']: row['order']['command'] for row in orders}
assert commands[16] == {'kind': 'stairs', 'op': 'entrance', 'pos': {'x': 15, 'y': 48, 'z': 1}, 'toLevel': 0, 'width': 1}
assert commands[17]['op'] == 'room' and commands[17]['shell'] == 214 and commands[17]['rect'] == {'x': 13, 'y': 46, 'z': 1, 'w': 4, 'h': 2}
assert commands[18]['op'] == 'build' and commands[18]['kind'] == 'wind-power'
for sequence, kind in [(19, 'power'), (20, 'compute')]:
    assert commands[sequence]['op'] == 'wire' and commands[sequence]['kind'] == kind
    assert {p['z'] for p in commands[sequence]['path']} == {0, 1}
assert commands[21] == {'op': 'install-gpu', 'room': 265, 'model': 'rtx-5060'}
assert commands[22] == {'op': 'deploy', 'room': 67, 'kind': 'glm', 'pos': {'x': 12, 'y': 50, 'z': 0}}
for sequence, tick in [(23, 15668), (25, 18062)]:
    assert commands[sequence]['op'] == 'skill' and commands[sequence]['id'] == 425
    assert commands[sequence]['pos'] == {'x': 12, 'y': 52, 'z': 0}
    assert orders[sequence - 1]['tick'] == tick
assert commands[24]['op'] == 'wire' and commands[24]['kind'] == 'compute'
assert commands[24]['path'][-1] == {'x': 12, 'y': 50, 'z': 0}
assert next(r for r in stored['rooms'] if r['id'] == 265)['gpus'] == ['rtx-5060']
assert glm(stored)['sourceFacility'] == 67 and glm(stored)['invested'] == 660
skill_events = [e for e in stored['events'] if e['subject'] == 425 and e['kind'] == 'compute-spent']
assert [(e['tick'], e['magnitude']) for e in skill_events] == [(15668, 85), (18062, 85)]
assert own(offline, 'players')[0]['totals']['compute-spent'] == 85
assert own(end, 'players')[0]['totals']['compute-spent'] == 170

actions_path = RUN / 'cua-actions.json'
actions = read(actions_path)
completed = ['new-solo', 'build-room', 'power-wire', 'compute-wire', 'install-gpu', 'deploy-unit', 'skill-target', 'change-layer', 'save-load', 'replay']
assert set(completed) <= {row['action'] for row in actions}
setup_path = RUN / 'paid-layer-setup.json'
setup = read(setup_path)
opening = next(row for row in setup if row['event'] == 'paid opening verified')
setup_orders = [row for row in setup if row['event'] == 'order']
assert len(setup_orders) == 15 and sum(row['event'] == 'order' for row in setup[:setup.index(opening)]) == 13
assert all(row['receipt']['accepted'] for row in setup_orders)
for i, row in enumerate(setup_orders):
    assert native_command(row['command']) == commands[i + 1]
    for key in ('accepted', 'sequence', 'tick'):
        assert row['receipt'][key] == orders[i]['receipt'][key]
assert opening['tick'] == 3436 and opening['player']['credits'] >= 500
ready = next(row for row in setup if row['event'] == 'ready for browser stairs test')
assert ready['dc'] == 64 and ready['lab'] == 67 and ready['shell'] == 34
checkpoint_path = PACK / ('.forge/save/v6/' + ready['saved']['id'] + '.json')
checkpoint_doc = read(checkpoint_path)
checkpoint = checkpoint_doc['save']['snapshot']
assert checkpoint_doc['rulesFingerprint'] == target['rulesFingerprint'] and checkpoint['tick'] == 7679
assert checkpoint_doc['save']['sequences'][0] == 15
for rid, kind in [(64, 'data-center'), (67, 'research-lab')]:
    room = next(r for r in checkpoint['rooms'] if r['id'] == rid)
    assert room['kind'] == kind and room['progress'] == 1 and room['powered'] and room['connected']
assert next(r for r in checkpoint['rooms'] if r['id'] == 64)['gpus'] == ['rtx-5060']
assert all(b['progress'] == 1 for b in checkpoint['buildings'] if b['owner'] == 1)
assert {u['kind'] for u in checkpoint['units'] if u['owner'] == 1} == {'vscode', 'pycharm'}
built = {'extractor': 1, 'power': 1, 'dataCenter': 1, 'gpu': 1, 'lab': 1, 'starterTurrets': 2, 'powerLines': 1, 'computeLines': 3}
assert sum(b['kind'] == 'extractor' for b in own(checkpoint, 'buildings')) == 1
assert sum(b['kind'] == 'wind-power' for b in own(checkpoint, 'buildings')) == 1
assert sum(l['kind'] == 'power' and l['active'] for l in own(checkpoint, 'links')) == 1
assert sum(l['kind'] == 'compute' and l['active'] for l in own(checkpoint, 'links')) == 3

contracts_path = TESTS / 'contract-execution-retained.json'
contracts = read(contracts_path)
assert contracts['exitCode'] == 0 and contracts['sourceUnchanged'] and contracts['historicalBaselinesRestored']
assert contracts['rulesFingerprint'] == contracts['currentFingerprint'] == target['rulesFingerprint']
assert len(contracts['targets']) == 19 and contracts['missingTargets'] == contracts['unexecutedTargets'] == []
assert contracts['passedTests'] == sum(t['passed'] for t in contracts['targets']) == 267
assert contracts['failedTests'] == sum(t['failed'] for t in contracts['targets']) == 0
assert contracts['ignoredTests'] == sum(t['ignored'] for t in contracts['targets']) == 2
assert all(t['status'] == 'executed' for t in contracts['targets'])
test_refs = [ref(contracts_path)] + [verified(r) for r in contracts['evidence']]
test_refs += [verified(t['artifact']) for t in contracts['targets']]
test_log = (TESTS / 'cargo-tests.log').read_text(encoding='utf-8-sig')
coverage = read(PROJECT / 'game/v6/functionality-final-fa31b360-20260913.json')['nativeTests']['coverageExamplesFromActualPassingLog']
for names in coverage.values():
    for name in names:
        assert re.search(r'test ' + re.escape(name) + r' \.\.\. ok', test_log), name

cold_path = COLD / 'cold-start-acceptance.json'
cold = read(cold_path)
assert cold['passed'] and cold['normalShutdown'] and cold['errors'] == []
assert all(cold[key] == value for key, value in target.items())
cold_pngs = [verified(item) for item in cold['nativeFrameEvidence']]
assert len(cold_pngs) == 2
for reference in cold_pngs:
    with Image.open(PROJECT / reference['path']) as image:
        assert image.format == 'PNG' and image.size == (1280, 720)

png_dir = RUN / 'png-evidence'
png_dir.mkdir(exist_ok=False)
images = []
originals = sorted(RUN.glob('*-original.jpg'))
assert len(originals) == 9
for source in originals:
    source_sha = sha(source)
    with Image.open(source) as image:
        assert image.format == 'JPEG'
        rgba = image.convert('RGBA')
        pixels = rgba.tobytes()
        destination = png_dir / (source.stem.removesuffix('-original') + '.png')
        rgba.save(destination, format='PNG')
    with Image.open(destination) as converted:
        assert converted.format == 'PNG' and converted.convert('RGBA').tobytes() == pixels
    assert sha(source) == source_sha
    images.append({'original': ref(source), 'originalMime': 'image/jpeg', 'png': ref(destination),
                   'size': list(rgba.size), 'decodedRgbaSha256': hashlib.sha256(pixels).hexdigest(),
                   'conversionLosslessForDecodedSource': True})
conversion_path = png_dir / 'conversion-manifest.json'
write_new(conversion_path, {'scope': 'Original CUA JPEG bytes are preserved. PNG conversion exactly preserves decoded source RGBA, not the pre-JPEG/native framebuffer.', 'images': images})
order_path = RUN / 'verified-native-save-orders.json'
write_new(order_path, {'sourceSave': ref(save_path), 'sourceCheckpoint': ref(checkpoint_path),
                      'scope': 'Read-back extraction of actual saved native order receipts; original runtime Save files stay in local receipts.',
                      'helperOrders': orders[:15], 'browserOrders': browser_orders, 'openingPlayerObservation': opening,
                      'openingInfrastructureCheckpoint': {k: own(checkpoint, k) for k in ('players', 'buildings', 'rooms', 'units', 'links')},
                      'skillPaymentEvents': skill_events, 'replayOwnCollections': comparisons})
json_inputs = [actions_path, RUN / 'native-stream.json', setup_path, RUN / 'gui-process-cleanup.json', order_path, conversion_path, cold_path]
json_inputs += [RUN / (name + '.json') for name in states]
ui_evidence = [ref(p) for p in json_inputs] + [item['png'] for item in images] + cold_pngs
assert sum(r['path'].endswith('.png') for r in ui_evidence) == 11
ui = {
    'kind': 'actual-native-ui-acceptance', 'schemaVersion': 2, **target,
    'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'actualNativeExecution': True,
    'rgbaFrames': stream['rgbaFrames'], 'errors': [], 'completedActions': completed,
    'nativePid': stream['nativePid'], 'bridgePid': stream['bridgePid'],
    'scope': 'Actual CUA actions plus native Save/order/state readback. Fifteen API setup orders and ten subsequent GUI orders are separated. Functional UI scope only, not balance, sustained FPS or45-minute LAN.',
    'browserOrdersFromSavedGame': browser_orders,
    'browserOrderBoundary': {'helperSequences': [1, 15], 'browserSequences': [16, 25], 'acceptedBrowserOrders': 10},
    'ordinaryPaidOpening': {'startingCredits': 2000, 'standardOrdinarySoloSession': True, 'seed': 6026,
        'injectedResources': False, 'acceptedOrders': 13, 'creditsRemaining': opening['player']['credits'], 'observedTick': opening['tick'],
        'startingCreditsBasis': 'CUA new-solo recorded displayed2000 and standard native starting credits; helper first order is at tick1386, so real passive income before/during setup remains included.',
        'built': built, 'checkpointTick': checkpoint['tick'], 'dataCenter': 64, 'lab': 67},
    'skill': {'unit': 425, 'kind': 'glm', 'attackCount': 0, 'costPerCast': 85,
        'offline': {'lastCastTick': 15668, 'observedTick': offline['tick'], 'initialCacheFromDeploymentAndCua': 120, 'afterCastCache': 35, 'covered': False, 'wired': False},
        'endpointRecharge': {'observedTick': connected['tick'], 'observedCache': glm(connected)['battery'], 'laterObservedCache': 300, 'covered': True, 'wired': False},
        'online': {'lastCastTick': 18062, 'savedTick': 18065, 'cacheAfterCast': 300, 'nativeComputeSpentTotal': 170, 'paymentEvents': skill_events},
        'scope': 'Two actual85-compute support casts. The offline cache fell120 to35; connected cache refilled173.2 then300 and stayed300 after the network-paid second cast. No GLM ordinary weapon attack was observed.'},
    'saveLoad': {'saveId': saved['id'], 'savedTick': 18065, 'loadedPausedTick': 18075, 'loadedSequence': 25, 'saveSha256': sha(save_path),
                 'scope': 'GUI load resumed the saved state, advanced10 actual ticks before pause, and retained the actor and sequence. Exact equality is assessed separately at the replay saved tick.'},
    'replayComparison': {'rewoundTick': 17465, 'beforeLastCastTick': 15668, 'exactEndedTick': 18065, 'endLastCastTick': 18062,
        'speed': 8, 'collections': comparisons, 'scope': 'Exact equality of five complete own-player collections; full private authority-world equality is not claimed.'},
    'retainedObservations': {'seekState': ref(RUN / 'replay-unexpected-rewind-state.json'), 'recordedNativeSeekTick': 17465,
        'sameAsBeforeSkillState': True, 'note': 'Root observed a temporary low-tick reconstruction display during seek. The retained native sample already records the correct17465 target; replay subsequently reaches exact18065. A checkbox-role lookup for Archive matched no control; DOM confirmed a button and the interaction succeeded. These are not fabricated gameplay failures or omitted native samples.'},
    'screenshots': images, 'additionalColdStartPngEvidence': cold_pngs,
    'additionalColdStartNativeRgbaFrames': cold['nativeRgbaFrames'], 'normalApplicationShutdown': True,
    'evidence': ui_evidence,
}
write_new(UI_OUTPUT, ui)
function_ui_evidence = [ref(UI_OUTPUT)] + ui_evidence
assert sum(r['path'].endswith('.png') for r in function_ui_evidence) == 11
functionality = {
    'schemaVersion': 2, 'kind': 'native-functional-acceptance', **target,
    'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'finalEligible': True, 'status': 'functional-scope-accepted',
    'scope': 'Completed functional observations only.267 tests ran in19 separately retained native executables with matching Game fingerprint; the target host was independently catalogued. No full balance, pressure, LAN or overall release approval.',
    'nativeTests': {'actualNativeExecution': True, 'passedTests': 267, 'failedTests': 0, 'unexecutedTests': 0,
        'ignoredComponentBenchmarks': 2, 'executedTargets': 19, 'coverage': sorted(coverage),
        'coverageExamplesFromActualPassingLog': coverage, 'evidence': test_refs},
    'ordinaryOpening': {'actualNativeExecution': True, 'ordinaryPaidCommands': True, 'injectedResources': False,
        'startingCredits': 2000, 'startingCreditsBasis': ui['ordinaryPaidOpening']['startingCreditsBasis'],
        'creditsRemaining': opening['player']['credits'], 'acceptedOrders': 13, 'verifiedTick': 3436, 'built': built,
        'observedPower': opening['player']['power'], 'observedLoad': opening['player']['demand'],
        'observedComputeProduction': opening['player']['production'], 'observedOreDelivered': opening['player']['totals']['ore-delivered'],
        'scope': 'First13 accepted paid opening orders; the observed939.0433333335104 reserve includes real elapsed passive income and delivered ore. Research/upper shell and GUI sequences16–25 are separate.',
        'evidence': [ref(UI_OUTPUT), ref(setup_path), ref(order_path)]},
    'nativeUi': {'actualNativeExecution': True, 'rgbaFrames': 30, 'errors': [], 'completedActions': completed,
        'acceptedBrowserOrders': 10, 'directRegisteredGuiPngs': 9, 'directRegisteredColdStartPngs': 2,
        'evidence': function_ui_evidence},
}
write_new(FUNCTION_OUTPUT, functionality)
print(json.dumps({'ui': ref(UI_OUTPUT), 'functionality': ref(FUNCTION_OUTPUT), 'guiOrders': 10, 'nativeTests': 267,
                  'rgbaFrames': 30, 'directPngEvidence': 11, 'openingCredits': opening['player']['credits']}))
